//! Immutable plans cross to the tick by shared ownership. A plan names lights by index
//! and holds words per field of each light: the field table (`rig::Targets`) gives each
//! light's fields on the tick, so a Move swaps the table and the plan follows. A
//! standalone step offers its words as they are; a sequence or a wave is sampled per
//! light into the table's scratch, and each word goes to the Mixer.

use alloc::{boxed::Box, vec::Vec};
use core::mem::size_of_val;
use engine_core::{
    Binding, Elapsed, Field, Item, Light, LightNumber, Lights, Node, Program, Refusal, Sampled,
    Step, Swatches, Use,
};
use rig::{LightFields, Patch, Targets, WaveBinding, Words};

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

/// A step's words on its checked target runs, shared by static playback and nested
/// sequence compilation.
pub fn compile(step: &Step, patch: &Patch) -> Result<Words, Refusal> {
    patch.bind_step(step)
}

pub(crate) fn numbers(run: engine_core::LightRun) -> core::ops::Range<u16> {
    run.first().index()..run.first().index() + run.count()
}

/// A group's lights in saved member order. Each fixture contributes its light and heads
/// in definition order. Only the first occurrence of a repeated light counts.
pub(crate) fn ordered_group(group: engine_core::Text, patch: &Patch) -> Result<Vec<u16>, Refusal> {
    patch.ordered_group(group)
}

/// The original leaf and its bound words, for validating changes against held samples.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Leaf {
    pub step: Step,
    pub words: Words,
}

impl Leaf {
    pub fn new(step: Step, patch: &Patch) -> Result<Self, Refusal> {
        Ok(Self {
            step,
            words: compile(&step, patch)?,
        })
    }

    fn matches(&self, patch: &Patch) -> bool {
        compile(&self.step, patch).is_ok_and(|now| now == self.words)
    }
}

/// What a node shows: the words of each light bound to it come from it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Shows {
    Step(Step),
    Wave(Swatches),
    Sequence,
}

impl Shows {
    /// The words a light of `fields` takes from this node, after `into`: a step's value
    /// on each field, a wave's swatches one set after the other.
    pub fn words(&self, fields: &LightFields, into: &mut Vec<Option<u16>>) {
        match self {
            Shows::Step(step) => into.extend(fields.words(&step.values)),
            Shows::Wave(swatches) => {
                for colour in swatches.as_slice() {
                    let swatch = Light {
                        colour: Some(*colour),
                        ..Light::DARK
                    };
                    into.extend(fields.words(&swatch));
                }
            }
            Shows::Sequence => {}
        }
    }
}

/// One light of a timed plan, by index, and its bindings sorted by node.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub light: u16,
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
    /// What each node shows, to check a retained plan against a changed patch.
    pub shows: Box<[Shows]>,
    /// Sorted by light.
    pub targets: Box<[Target]>,
    /// The words of every binding. Equal words are kept once.
    pub words: Box<[Option<u16>]>,
    pub leaves: Box<[Leaf]>,
    pub waves: Box<[WaveBinding]>,
}

impl Timed {
    fn program(&self) -> Program<'_> {
        Program {
            root: self.root,
            nodes: &self.nodes,
            items: &self.items,
            words: &self.words,
        }
    }

    /// Each light still has the words of each of its bindings: a light whose fields
    /// changed (another mode, unpatched) does not.
    fn fits(&self, patch: &Patch) -> bool {
        let mut words = Vec::new();
        self.targets.iter().all(|target| {
            let Some(fields) =
                LightNumber::new(target.light, Lights::MOST).and_then(|light| patch.fields(light))
            else {
                return false;
            };
            target.bindings.iter().all(|binding| {
                words.clear();
                let Some(show) = self.shows.get(binding.node as usize) else {
                    return false;
                };
                show.words(fields, &mut words);
                let at = binding.words as usize;
                self.words.get(at..at + words.len()) == Some(words.as_slice())
            })
        })
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
            PlanKind::Step(leaf) => leaf.words.bytes(),
            PlanKind::Timed(timed) => {
                size_of_val(timed.saved.as_ref())
                    + size_of_val(timed.palettes.as_ref())
                    + size_of_val(timed.nodes.as_ref())
                    + size_of_val(timed.items.as_ref())
                    + size_of_val(timed.shows.as_ref())
                    + size_of_val(timed.targets.as_ref())
                    + size_of_val(timed.words.as_ref())
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
                        .map(|leaf| leaf.words.bytes())
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
        if leaf.words.bytes() > room {
            return Err(Refusal::WorkLimit);
        }
        let lights = leaf.words.lights();
        Ok(Self::new(
            look,
            PlanKind::Step(leaf),
            sample_work(lights, 1)?,
        ))
    }

    /// A timed plan whose storage fits `room`: the words are known only once built.
    pub(crate) fn timed(
        look: u16,
        timed: Timed,
        work: usize,
        room: usize,
    ) -> Result<Self, Refusal> {
        let kind = PlanKind::Timed(timed);
        if kind.storage() > room {
            return Err(Refusal::WorkLimit);
        }
        Ok(Self::new(look, kind, work))
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

    /// Worst-case visits of a sample and its mix.
    pub const fn work(&self) -> usize {
        self.work
    }

    /// Storage booked at admission; measured once, off the sender lock.
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    /// Whether the plan still means the same on `patch`: each step names the same lights
    /// with the same words, each wave the same lights, each light of a timed plan the
    /// same words. A Move keeps every word: only the table's fields move.
    pub fn bindings_match(&self, patch: &Patch) -> bool {
        match &self.kind {
            PlanKind::Step(leaf) => leaf.matches(patch),
            PlanKind::Timed(timed) => {
                timed.leaves.iter().all(|leaf| leaf.matches(patch))
                    && timed.waves.iter().all(|wave| wave.matches(patch))
                    && timed.fits(patch)
            }
        }
    }

    pub fn finished(&self, elapsed: impl Into<Elapsed>, usage: Use) -> bool {
        match &self.kind {
            PlanKind::Step(_) => false,
            PlanKind::Timed(timed) => timed.program().finished(elapsed, usage),
        }
    }

    /// Each word the plan offers at `elapsed` on its field, through `targets`: `emit`
    /// takes it. A timed plan samples each light into `scratch` (the table's, room for
    /// the light with the most fields at every depth). Whether it offered anything: a
    /// sample with nothing has nothing to keep. Allocates nothing.
    pub fn offer(
        &self,
        (elapsed, usage): (Elapsed, Use),
        (targets, scratch): (Targets<'_>, &mut [Option<u16>]),
        mut emit: impl FnMut(Field, u16),
    ) -> bool {
        let mut offered = false;
        let mut each = |field, word| {
            offered = true;
            emit(field, word);
        };
        match &self.kind {
            PlanKind::Step(leaf) => leaf.words.offer(targets, each),
            PlanKind::Timed(timed) => {
                let program = timed.program();
                for target in &timed.targets {
                    let (fields, fades) = targets.of(target.light);
                    let Some((out, spare)) = scratch.split_at_mut_checked(fields.len()) else {
                        continue;
                    };
                    let sampled = Sampled {
                        bindings: &target.bindings,
                        fades,
                    };
                    program.sample(sampled, elapsed, usage, out, spare);
                    for (field, word) in fields.iter().zip(out.iter()) {
                        if let Some(word) = word {
                            each(*field, *word);
                        }
                    }
                }
            }
        }
        offered
    }
}
