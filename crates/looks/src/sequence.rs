//! Compile only the reachable graph. Shared references reuse a node, but each use
//! contributes its full expansion to admission and keeps its own timing and spread.

use crate::compile::{
    Shows, Target as SampleTarget, Timed, WORK_LIMIT, compile as offers, sample_work,
};
use crate::{
    Looks,
    pattern::{self, WaveBinding},
};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use engine_core::{
    Binding, Definition, Field, Item, LightNumber, LightRun, Lights, MAX_DEPTH, Member, Node,
    Refusal, Sequence, Target,
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
    shows: Shows,
    ordered: Vec<u16>,
    /// A matrix repeats the horizontal spread on every row. Other nodes use the whole target.
    columns: Option<u16>,
    size: Size,
}

struct Builder<'a> {
    looks: &'a Looks,
    patch: &'a Patch,
    nodes: Vec<Built>,
    items: Vec<Item>,
    waves: Vec<WaveBinding>,
    /// Steps whose swatches this plan reads, independently of their playable values.
    palettes: BTreeSet<u16>,
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
                // refused here as a standalone step would be
                offers(&step.step(), self.patch)?;
                let ordered = match step.target {
                    Target::Group { group } => ordered_group(group, self.patch)?,
                    Target::Light { .. } | Target::Fixture { .. } => self
                        .patch
                        .runs(step.target)?
                        .into_iter()
                        .flat_map(numbers)
                        .collect(),
                };
                let expansion = ordered.len();
                Built {
                    node: Node::Step,
                    shows: Shows::Step(step.step()),
                    ordered,
                    columns: None,
                    size: Size {
                        depth: 1,
                        expansion,
                        visits: 1,
                    },
                }
            }
            Definition::Sequence(sequence) => self.sequence(sequence)?,
            Definition::Pattern(pattern) => {
                let (node, swatches, wave) = pattern::compile(self.looks, pattern, self.patch)?;
                let ordered = wave.ordered(self.patch)?;
                let columns = wave.columns;
                let expansion = ordered.len();
                self.waves.push(wave);
                let palette = pattern::reference(pattern);
                self.palettes.extend(palette);
                Built {
                    node,
                    shows: Shows::Wave(swatches),
                    ordered,
                    columns,
                    size: Size {
                        // A linked palette is an authored leaf, resolved before playback.
                        depth: 1 + usize::from(palette.is_some()),
                        expansion,
                        // Each sample visits the Wave and its two neighbouring swatches.
                        visits: 3,
                    },
                }
            }
        };
        self.path.pop();
        if self.path.len() + built.size.depth > MAX_DEPTH {
            return Err(Refusal::WorkLimit);
        }
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
                || (item.settings.spread > 0 && matches!(child.node, Node::Step))
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
            shows: Shows::Sequence,
            ordered,
            columns: None,
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
        indexed: BTreeMap::new(),
        palettes: BTreeSet::new(),
        waves: Vec::new(),
        path: Vec::new(),
        bindings: 0,
    };
    let root = builder.visit(root)?;
    let built = builder.built(root)?;
    // Charge the binding search at every visited node, and the sample write and mix
    // for each target light. The front also books event samples for the FIFO prefix.
    let per_light = built.size.visits * (1 + comparisons(builder.nodes.len())) + 2;
    let work = sample_work(built.ordered.len(), per_light)?;

    // The graph, count of bindings and union of targets determine the plan's size
    // before its flat per-light arrays are made.
    let storage = builder.nodes.len() * (size_of::<Node>() + size_of::<Shows>())
        + (builder.indexed.len() + builder.palettes.len()) * size_of::<u16>()
        + WaveBinding::storage(&builder.waves)
        + builder.items.len() * size_of::<Item>()
        + built.ordered.len() * size_of::<SampleTarget>()
        + builder.bindings * size_of::<Binding>();
    if storage > room {
        return Err(Refusal::WorkLimit);
    }
    let mut by_light: BTreeMap<u16, Vec<(usize, u16, u16)>> = BTreeMap::new();
    let mut nodes = Vec::with_capacity(builder.nodes.len());
    let mut shows = Vec::with_capacity(builder.nodes.len());
    for (node, built) in builder.nodes.into_iter().enumerate() {
        nodes.push(built.node);
        shows.push(built.shows);
        let targets = u16::try_from(built.ordered.len()).map_err(|_| Refusal::WorkLimit)?;
        let count = built.columns.unwrap_or(targets);
        for (index, light) in (0..targets).zip(built.ordered) {
            by_light
                .entry(light)
                .or_default()
                .push((node, index % count, count));
        }
    }
    // each target's words: a step's value on its fields, a wave's swatches on them;
    // equal words are kept once
    let mut words: Vec<Option<u16>> = Vec::new();
    let mut kept: BTreeMap<Vec<Option<u16>>, u32> = BTreeMap::new();
    let mut mine = Vec::new();
    let mut fields: Vec<Field> = Vec::new();
    let mut fades = Vec::new();
    let mut bindings = Vec::with_capacity(builder.bindings);
    let mut targets = Vec::with_capacity(by_light.len());
    for (index, bound) in by_light {
        let light = LightNumber::new(index, Lights::MOST).ok_or(Refusal::NoSuchLight)?;
        let target = patch.target(light).ok_or(Refusal::NoSuchLight)?;
        targets.push(SampleTarget {
            light: index,
            fields: offset(fields.len())?,
            bindings: offset(bindings.len())?,
        });
        for (node, index, count) in bound {
            mine.clear();
            if let Some(show) = shows.get(node) {
                show.words(target, &mut mine);
            }
            let at = match kept.get(&mine) {
                Some(&at) => at,
                None => {
                    let at = offset(words.len())?;
                    words.extend_from_slice(&mine);
                    kept.insert(mine.clone(), at);
                    at
                }
            };
            bindings.push(Binding {
                node: offset(node)?,
                index,
                count,
                words: at,
            });
        }
        fields.extend(target.fields.iter().copied());
        fades.extend(target.fades());
    }
    let words_storage = words.len() * size_of::<Option<u16>>()
        + fields.len() * (size_of::<Field>() + size_of::<bool>());
    if storage + words_storage > room {
        return Err(Refusal::WorkLimit);
    }
    Ok((
        Timed {
            root,
            saved: builder.indexed.into_keys().collect(),
            palettes: builder.palettes.into_iter().collect(),
            nodes: nodes.into_boxed_slice(),
            items: builder.items.into_boxed_slice(),
            shows: shows.into_boxed_slice(),
            targets: targets.into_boxed_slice(),
            fields: fields.into_boxed_slice(),
            fades: fades.into_boxed_slice(),
            bindings: bindings.into_boxed_slice(),
            words: words.into_boxed_slice(),
            waves: builder.waves.into_boxed_slice(),
        },
        work,
    ))
}

/// A place in a plan's flat arrays.
fn offset(at: usize) -> Result<u32, Refusal> {
    u32::try_from(at).map_err(|_| Refusal::WorkLimit)
}

pub(crate) fn numbers(run: LightRun) -> core::ops::Range<u16> {
    run.first().index()..run.first().index() + run.count()
}

/// A group's lights in saved member order. Each fixture contributes its light and heads
/// in definition order. Only the first occurrence of a repeated light counts.
pub(crate) fn ordered_group(group: engine_core::Text, patch: &Patch) -> Result<Vec<u16>, Refusal> {
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
