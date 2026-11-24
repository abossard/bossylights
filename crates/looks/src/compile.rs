//! Immutable plans cross to the tick by shared ownership. Only the sample buffer
//! changes there. A standalone step goes straight to the Mixer as runs of lights.

use alloc::{boxed::Box, vec::Vec};
use core::mem::size_of_val;
use engine_core::{
    Binding, Elapsed, Entry, Item, Light, LightRun, Node, Program, Refusal, Step, Use,
};
use rig::{Patch, WaveBinding};

/// Maximum play visits booked for one wake. The engine shares unused visits
/// with light commands and guarantees room for one direct light grab.
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
    patch.bind_step(step)
}

pub(crate) fn numbers(run: LightRun) -> core::ops::Range<u16> {
    run.first().index()..run.first().index() + run.count()
}

/// A group's lights in saved member order. Each fixture contributes its light and heads
/// in definition order. Only the first occurrence of a repeated light counts.
pub(crate) fn ordered_group(group: engine_core::Text, patch: &Patch) -> Result<Vec<u16>, Refusal> {
    patch.ordered_group(group)
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
    pub palettes: Box<[u16]>,
    pub nodes: Box<[Node]>,
    pub items: Box<[Item]>,
    pub targets: Box<[Target]>,
    pub leaves: Box<[Leaf]>,
    pub waves: Box<[WaveBinding]>,
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
    Timed(Timed),
}

impl PlanKind {
    fn storage(&self) -> usize {
        match self {
            PlanKind::Step(leaf) => size_of_val(leaf.entries.as_ref()),
            PlanKind::Timed(timed) => {
                size_of_val(timed.saved.as_ref())
                    + size_of_val(timed.palettes.as_ref())
                    + size_of_val(timed.nodes.as_ref())
                    + size_of_val(timed.items.as_ref())
                    + size_of_val(timed.targets.as_ref())
                    + timed.waves.iter().map(WaveBinding::bytes).sum::<usize>()
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
    pub(crate) fn content_room(room: usize) -> Result<usize, Refusal> {
        room.checked_sub(size_of::<Self>())
            .ok_or(Refusal::WorkLimit)
    }

    pub(crate) fn step(look: u16, step: Step, patch: &Patch, room: usize) -> Result<Self, Refusal> {
        let leaf = Leaf::new(step, patch)?;
        if size_of_val(leaf.entries.as_ref()) > room {
            return Err(Refusal::WorkLimit);
        }
        let lights = leaf
            .entries
            .iter()
            .map(|entry| usize::from(entry.lights.count()))
            .sum();
        Ok(Self::new(
            look,
            PlanKind::Step(leaf),
            sample_work(lights, 1)?,
        ))
    }

    pub(crate) fn timed(look: u16, timed: Timed, work: usize) -> Self {
        Self::new(look, PlanKind::Timed(timed), work)
    }

    fn new(look: u16, kind: PlanKind, work: usize) -> Self {
        let bytes = size_of::<Self>() + kind.storage();
        Self {
            look,
            kind,
            work,
            bytes,
        }
    }

    /// A frozen play retains its original linked IDs even when the saved graph changes.
    pub fn uses(&self, look: u16) -> bool {
        self.look == look
            || self.uses_palette(look)
            || matches!(&self.kind, PlanKind::Timed(timed) if timed.saved.binary_search(&look).is_ok())
    }

    /// A retained Wave still needs its palette even if the currently saved Wave
    /// has switched to inline swatches. A sequence that just plays a step does not.
    pub fn uses_palette(&self, look: u16) -> bool {
        matches!(&self.kind, PlanKind::Timed(timed) if timed.palettes.binary_search(&look).is_ok())
    }

    pub fn is_step(&self) -> bool {
        matches!(self.kind, PlanKind::Step(_))
    }

    /// Scratch entries needed, one per light of a timed root.
    pub fn lights(&self) -> usize {
        match &self.kind {
            PlanKind::Step(_) => 0,
            PlanKind::Timed(timed) => timed.targets.len(),
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
            PlanKind::Timed(timed) => {
                timed.leaves.iter().all(|leaf| leaf.matches(patch))
                    && timed.waves.iter().all(|wave| wave.matches(patch))
            }
        }
    }

    pub fn finished(&self, elapsed: impl Into<Elapsed>, usage: Use) -> bool {
        match &self.kind {
            PlanKind::Step(_) => false,
            PlanKind::Timed(timed) => timed.program().finished(elapsed, usage),
        }
    }

    /// Fill caller-owned scratch. A sample with no properties has nothing to keep.
    pub fn sample(&self, elapsed: impl Into<Elapsed>, usage: Use, scratch: &mut [Entry]) -> bool {
        match &self.kind {
            PlanKind::Step(leaf) => leaf.step.values != Light::DARK,
            PlanKind::Timed(timed) => {
                let mut supplied = false;
                let elapsed = elapsed.into();
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
            PlanKind::Timed(timed) => scratch.get(..timed.targets.len()).unwrap_or_default(),
        }
    }
}
