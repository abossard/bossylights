//! Compile only the reachable graph. Shared references reuse a node, but each use
//! contributes its full expansion to admission and keeps its own timing and spread.

use crate::Looks;
use crate::compile::{Leaf, Target as SampleTarget, Timed, WORK_LIMIT, sample_work};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use core::mem::size_of_val;
use engine_core::{
    Binding, Definition, Item, LightNumber, LightRun, Lights, MAX_DEPTH, Member, Node, Refusal,
    Sequence, Target, Writer,
};
use rig::Patch;

const SIXTEENTH: u64 = 1 << 28;

/// Counts compose bottom-up. A memoized shared child must fit even when reached by a
/// longer path. Expansion charges repeated uses without materialising the tree.
#[derive(Clone, Copy)]
struct Size {
    depth: usize,
    expansion: usize,
    visits: usize,
}

struct Built {
    node: Node,
    ordered: Vec<u16>,
    size: Size,
}

struct Builder<'a> {
    looks: &'a Looks,
    patch: &'a Patch,
    nodes: Vec<Built>,
    items: Vec<Item>,
    leaves: Vec<Leaf>,
    indexed: BTreeMap<u16, usize>,
    path: Vec<u16>,
    bindings: usize,
}

impl Builder<'_> {
    fn visit(&mut self, look: u16) -> Result<usize, Refusal> {
        if self.path.contains(&look) {
            return Err(Refusal::LookCycle);
        }
        if let Some(&index) = self.indexed.get(&look) {
            if self.path.len() + self.built(index)?.size.depth > MAX_DEPTH {
                return Err(Refusal::WorkLimit);
            }
            return Ok(index);
        }
        if self.path.len() == MAX_DEPTH {
            return Err(Refusal::WorkLimit);
        }
        self.path.push(look);
        let saved = self.looks.get(look).ok_or(Refusal::NoSuchLook)?;
        let built = match &saved.definition {
            Definition::Step(step) => {
                let leaf = Leaf::new(*step, self.patch)?;
                let ordered = ordered(&leaf, self.patch)?;
                let expansion = ordered.len();
                self.leaves.push(leaf);
                Built {
                    node: Node::Step {
                        values: step.values,
                    },
                    ordered,
                    size: Size {
                        depth: 1,
                        expansion,
                        visits: 1,
                    },
                }
            }
            Definition::Sequence(sequence) => self.sequence(sequence)?,
        };
        self.path.pop();
        self.bindings = limited(self.bindings.checked_add(built.ordered.len()))?;
        let index = self.nodes.len();
        self.nodes.push(built);
        self.indexed.insert(look, index);
        Ok(index)
    }

    fn built(&self, index: usize) -> Result<&Built, Refusal> {
        self.nodes.get(index).ok_or(Refusal::NoSuchLook)
    }

    fn sequence(&mut self, sequence: &Sequence) -> Result<Built, Refusal> {
        if sequence.too_many() {
            return Err(Refusal::WorkLimit);
        }
        let references = sequence.as_slice();
        if references.is_empty() {
            return Err(Refusal::InvalidTiming);
        }
        let mut items = Vec::with_capacity(references.len());
        let mut end = 0u64;
        let mut size = Size {
            depth: 1,
            expansion: 1,
            visits: 0,
        };
        let mut seen = BTreeSet::new();
        let mut ordered = Vec::new();
        for item in references {
            if item.width == 0 || item.transition > 100 || item.usage.spread > 100 {
                return Err(Refusal::InvalidTiming);
            }
            let child = self.visit(item.look)?;
            let built = self.built(child)?;
            size.expansion = limited(size.expansion.checked_add(built.size.expansion))?;
            size.depth = size.depth.max(1 + built.size.depth);
            ordered.extend(
                built
                    .ordered
                    .iter()
                    .copied()
                    .filter(|light| seen.insert(*light)),
            );
            end = end
                .checked_add(u64::from(item.width) * SIXTEENTH)
                .ok_or(Refusal::InvalidTiming)?;
            items.push(Item {
                child,
                end,
                settings: item.usage,
                transition: item.transition,
            });
        }

        // Only the adjacent outgoing child can join the current one. A delayed step
        // can show its predecessor before entry, including across loop wrap.
        let mut previous = items.last().ok_or(Refusal::InvalidTiming)?;
        for item in &items {
            let child = self.built(item.child)?;
            let before = self.built(previous.child)?;
            let outgoing = if item.transition > 0
                || (item.settings.spread > 0 && matches!(child.node, Node::Step { .. }))
            {
                before.size.visits
            } else {
                0
            };
            let visits = 1 + comparisons(items.len()) + child.size.visits + outgoing;
            size.visits = size.visits.max(visits);
            previous = item;
        }
        let node = Node::Sequence {
            first: self.items.len(),
            count: items.len(),
            measure: end,
        };
        self.items.extend(items);
        Ok(Built {
            node,
            ordered,
            size,
        })
    }
}

fn limited(value: Option<usize>) -> Result<usize, Refusal> {
    value
        .filter(|&value| value <= WORK_LIMIT)
        .ok_or(Refusal::WorkLimit)
}

/// Comparisons for a binary search over a nonempty slice.
fn comparisons(len: usize) -> usize {
    (usize::BITS - len.leading_zeros()) as usize
}

pub(crate) fn compile(
    looks: &Looks,
    root: u16,
    patch: &Patch,
    room: usize,
) -> Result<(Timed, usize), Refusal> {
    let mut builder = Builder {
        looks,
        patch,
        nodes: Vec::new(),
        items: Vec::new(),
        leaves: Vec::new(),
        indexed: BTreeMap::new(),
        path: Vec::new(),
        bindings: 0,
    };
    let root = builder.visit(root)?;
    let built = builder.built(root)?;
    // Charge the binding search at every visited node, and the sample write and mix
    // for each target light. The front also books event samples for the FIFO prefix.
    let per_light = built.size.visits * (1 + comparisons(builder.nodes.len())) + 2;
    let work = sample_work(built.ordered.len(), per_light)?;

    // The graph, count of bindings and union of targets determine the complete
    // flattened plan size before allocating its per-light binding tables.
    let storage = builder.nodes.len() * (size_of::<Node>() + size_of::<u16>())
        + builder.items.len() * size_of::<Item>()
        + built.ordered.len() * size_of::<SampleTarget>()
        + builder.bindings * size_of::<Binding>()
        + builder.leaves.len() * size_of::<Leaf>()
        + builder
            .leaves
            .iter()
            .map(|leaf| size_of_val(leaf.entries.as_ref()))
            .sum::<usize>();
    if storage > room {
        return Err(Refusal::WorkLimit);
    }
    let wheels = discrete_colours(patch);
    let mut by_light: BTreeMap<u16, Vec<Binding>> = BTreeMap::new();
    let mut nodes = Vec::with_capacity(builder.nodes.len());
    for (node, built) in builder.nodes.into_iter().enumerate() {
        nodes.push(built.node);
        let count = u16::try_from(built.ordered.len()).map_err(|_| Refusal::WorkLimit)?;
        for (index, light) in (0..count).zip(built.ordered) {
            by_light.entry(light).or_default().push(Binding {
                node,
                index,
                count,
                discrete_colour: wheels.contains(&light),
            });
        }
    }
    let targets = by_light
        .into_iter()
        .map(|(index, bindings)| {
            let light = LightNumber::new(index, Lights::MOST)
                .and_then(|light| LightRun::new(light, 1, Lights::MOST))
                .ok_or(Refusal::NoSuchLight)?;
            Ok(SampleTarget {
                light,
                bindings: bindings.into_boxed_slice(),
            })
        })
        .collect::<Result<_, Refusal>>()?;
    Ok((
        Timed {
            root,
            saved: builder.indexed.into_keys().collect(),
            nodes: nodes.into_boxed_slice(),
            items: builder.items.into_boxed_slice(),
            targets,
            leaves: builder.leaves.into_boxed_slice(),
        },
        work,
    ))
}

/// A group's set of lights in saved member order. A fixture contributes its own light
/// and its heads in definition order. The first occurrence of a repeated light wins.
fn ordered(leaf: &Leaf, patch: &Patch) -> Result<Vec<u16>, Refusal> {
    let numbers = |run: LightRun| run.first().index()..run.first().index() + run.count();
    let Target::Group { group } = leaf.step.target else {
        return Ok(leaf
            .entries
            .iter()
            .flat_map(|entry| numbers(entry.lights))
            .collect());
    };
    let groups = patch.groups();
    let group = groups
        .iter()
        .find(|row| row.name == group.as_str())
        .ok_or(Refusal::NoSuchGroup)?;
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    for &member in group.members {
        let target = match member {
            Member::Fixture { fixture } => Target::Fixture { fixture },
            Member::Light { light } => Target::Light { light },
        };
        ordered.extend(
            patch
                .runs(target)?
                .into_iter()
                .flat_map(numbers)
                .filter(|light| seen.insert(*light)),
        );
    }
    Ok(ordered)
}

fn discrete_colours(patch: &Patch) -> BTreeSet<u16> {
    let mut wheel = None;
    let mut colours = BTreeSet::new();
    for writer in patch.writers() {
        match writer {
            Writer::Picks { light, .. } => wheel = Some(light.index()),
            Writer::ColourSlot { .. } => colours.extend(wheel),
            Writer::GoboSlot { .. } => {}
            Writer::Takes { .. } | Writer::Fixed { .. } => wheel = None,
        }
    }
    colours
}
