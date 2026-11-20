//! Immutable plans cross to the tick by shared ownership. Only the sample buffer
//! changes there. A standalone step goes straight to the Mixer as runs of lights.

use crate::{Looks, sequence};
use alloc::boxed::Box;
use core::mem::size_of_val;
use engine_core::{
    Binding, Definition, Entry, Item, Light, LightRun, Node, Program, Refusal, Step, Use,
};
use rig::Patch;

/// Maximum variable visits booked for one wake.
pub const WORK_LIMIT: usize = 65_536;

/// A play reserves its frame sample and a possible terminal sample in the same wake.
pub const fn play_work(sample: usize) -> usize {
    sample.saturating_mul(2)
}

pub(crate) fn sample_work(lights: usize, visits_per_light: usize) -> Result<usize, Refusal> {
    lights
        .checked_mul(visits_per_light)
        .filter(|&sample| play_work(sample) <= WORK_LIMIT)
        .ok_or(Refusal::WorkLimit)
}

/// Maximum compiled storage in a saved revision. Queued and retired plan references
/// share the same immutable objects until their last owner lets go.
pub const PLAN_BYTES: usize = 64 * 1024 * 1024;

/// A step's values on its checked target runs, shared by static playback and nested
/// sequence compilation.
pub fn compile(step: &Step, patch: &Patch) -> Result<Box<[Entry]>, Refusal> {
    let runs = patch.runs(step.target)?;
    if runs.is_empty() {
        return Err(Refusal::EmptyGroup);
    }
    rig::supports(patch.writers(), &runs, &step.values)?;
    Ok(runs
        .into_iter()
        .map(|lights| Entry {
            lights,
            values: step.values,
        })
        .collect())
}

/// The original leaf and its bound lights, for validating changes against held samples.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Leaf {
    pub step: Step,
    pub entries: Box<[Entry]>,
}

impl Leaf {
    pub fn new(step: Step, patch: &Patch) -> Result<Self, Refusal> {
        Ok(Self {
            step,
            entries: compile(&step, patch)?,
        })
    }

    fn matches(&self, patch: &Patch) -> bool {
        compile(&self.step, patch).is_ok_and(|now| now == self.entries)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub light: LightRun,
    pub bindings: Box<[Binding]>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Timed {
    pub root: usize,
    /// Stable authored IDs, sorted for checking a retained generation under the sender lock.
    pub saved: Box<[u16]>,
    pub nodes: Box<[Node]>,
    pub items: Box<[Item]>,
    pub targets: Box<[Target]>,
    pub leaves: Box<[Leaf]>,
}

impl Timed {
    fn program(&self) -> Program<'_> {
        Program {
            root: self.root,
            nodes: &self.nodes,
            items: &self.items,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PlanKind {
    Step(Leaf),
    Sequence(Timed),
}

impl PlanKind {
    fn storage(&self) -> usize {
        match self {
            PlanKind::Step(leaf) => size_of_val(leaf.entries.as_ref()),
            PlanKind::Sequence(timed) => {
                size_of_val(timed.saved.as_ref())
                    + size_of_val(timed.nodes.as_ref())
                    + size_of_val(timed.items.as_ref())
                    + size_of_val(timed.targets.as_ref())
                    + size_of_val(timed.leaves.as_ref())
                    + timed
                        .targets
                        .iter()
                        .map(|target| size_of_val(target.bindings.as_ref()))
                        .sum::<usize>()
                    + timed
                        .leaves
                        .iter()
                        .map(|leaf| size_of_val(leaf.entries.as_ref()))
                        .sum::<usize>()
            }
        }
    }
}

/// One root and every child it reaches in a particular revision.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    look: u16,
    kind: PlanKind,
    work: usize,
    bytes: usize,
}

impl Plan {
    pub(crate) fn build(
        looks: &Looks,
        look: u16,
        patch: &Patch,
        room: usize,
    ) -> Result<Self, Refusal> {
        let room = room
            .checked_sub(size_of::<Self>())
            .ok_or(Refusal::WorkLimit)?;
        let (kind, work) = match &looks.get(look).ok_or(Refusal::NoSuchLook)?.definition {
            Definition::Step(step) => {
                let leaf = Leaf::new(*step, patch)?;
                if size_of_val(leaf.entries.as_ref()) > room {
                    return Err(Refusal::WorkLimit);
                }
                let lights = leaf
                    .entries
                    .iter()
                    .map(|entry| usize::from(entry.lights.count()))
                    .sum();
                (PlanKind::Step(leaf), sample_work(lights, 1)?)
            }
            Definition::Sequence(_) => {
                let (program, work) = sequence::compile(looks, look, patch, room)?;
                (PlanKind::Sequence(program), work)
            }
        };
        let bytes = core::mem::size_of::<Self>() + kind.storage();
        Ok(Self {
            look,
            kind,
            work,
            bytes,
        })
    }

    /// A frozen play retains its original linked IDs even when the saved graph changes.
    pub fn uses(&self, look: u16) -> bool {
        self.look == look
            || matches!(&self.kind, PlanKind::Sequence(timed) if timed.saved.binary_search(&look).is_ok())
    }

    pub fn is_step(&self) -> bool {
        matches!(self.kind, PlanKind::Step(_))
    }

    /// Scratch entries needed, one per light of a timed root.
    pub fn lights(&self) -> usize {
        match &self.kind {
            PlanKind::Step(_) => 0,
            PlanKind::Sequence(timed) => timed.targets.len(),
        }
    }

    /// Worst-case visits of a sample and its mix.
    pub const fn work(&self) -> usize {
        self.work
    }

    /// Storage booked at admission; measured once, off the sender lock.
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn bindings_match(&self, patch: &Patch) -> bool {
        match &self.kind {
            PlanKind::Step(leaf) => leaf.matches(patch),
            PlanKind::Sequence(timed) => timed.leaves.iter().all(|leaf| leaf.matches(patch)),
        }
    }

    pub fn finished(&self, elapsed: u64, usage: Use) -> bool {
        match &self.kind {
            PlanKind::Step(_) => false,
            PlanKind::Sequence(timed) => timed.program().finished(elapsed, usage),
        }
    }

    /// Fill caller-owned scratch. A sample with no properties has nothing to keep.
    pub fn sample(&self, elapsed: u64, usage: Use, scratch: &mut [Entry]) -> bool {
        match &self.kind {
            PlanKind::Step(leaf) => leaf.step.values != Light::DARK,
            PlanKind::Sequence(timed) => {
                let mut supplied = false;
                let program = timed.program();
                for (entry, target) in scratch.iter_mut().zip(&timed.targets) {
                    let values = program.sample(&target.bindings, elapsed, usage);
                    *entry = Entry {
                        lights: target.light,
                        values,
                    };
                    supplied |= values != Light::DARK;
                }
                supplied
            }
        }
    }

    /// The last sample for the Mixer, static runs or the timed root's scratch.
    pub fn entries<'a>(&'a self, scratch: &'a [Entry]) -> &'a [Entry] {
        match &self.kind {
            PlanKind::Step(leaf) => &leaf.entries,
            PlanKind::Sequence(timed) => scratch.get(..timed.targets.len()).unwrap_or_default(),
        }
    }
}
