//! Immutable plans cross to the tick by shared ownership. Only the sample buffer
//! changes there. A standalone step goes straight to the Mixer as fixed offers; a
//! sequence or wave is sampled per target into words on the target's fields.

use crate::{Looks, pattern::WaveBinding, sequence};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::iter;
use core::mem::size_of_val;
use core::ops::Range;
use engine_core::{
    Binding, Definition, Elapsed, Field, Item, Light, LightNumber, Lights, MAX_DEPTH, Node, Offer,
    Offering, Program, Refusal, Sampled, Step, Swatches, Use,
};
use rig::{LightFields, Patch};

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

/// A step's offers on its checked target runs: each light's words on its fields.
pub fn compile(step: &Step, patch: &Patch) -> Result<Box<[Offer]>, Refusal> {
    let runs = patch.runs(step.target)?;
    if runs.is_empty() {
        return Err(Refusal::EmptyGroup);
    }
    rig::supports(patch.targets(), &runs, &step.values)?;
    let mut offers = Vec::new();
    for light in runs.iter().copied().flat_map(sequence::numbers) {
        let target = engine_core::LightNumber::new(light, engine_core::Lights::MOST)
            .and_then(|light| patch.target(light))
            .ok_or(Refusal::NoSuchLight)?;
        offers.extend(target.offers(&step.values));
    }
    Ok(offers.into_boxed_slice())
}

/// A standalone step and its offers, for validating changes against held samples.
#[derive(Debug, PartialEq, Eq)]
struct Leaf {
    step: Step,
    offers: Box<[Offer]>,
}

impl Leaf {
    pub fn new(step: Step, patch: &Patch) -> Result<Self, Refusal> {
        Ok(Self {
            step,
            offers: compile(&step, patch)?,
        })
    }

    fn matches(&self, patch: &Patch) -> bool {
        compile(&self.step, patch).is_ok_and(|now| now == self.offers)
    }
}

/// What a node shows: the words of each target bound to it come from it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Shows {
    Step(Step),
    Wave(Swatches),
    Sequence,
}

impl Shows {
    /// The words `target` takes from this node, after `into`: a step's value on each
    /// field, a wave's swatches one after the other.
    pub fn words(&self, target: &LightFields, into: &mut Vec<Option<u16>>) {
        match self {
            Shows::Step(step) => into.extend(target.words(&step.values)),
            Shows::Wave(swatches) => {
                for colour in swatches.as_slice() {
                    let swatch = Light {
                        colour: Some(*colour),
                        ..Light::DARK
                    };
                    into.extend(target.words(&swatch));
                }
            }
            Shows::Sequence => {}
        }
    }
}

/// One light of a timed plan: where its fields (with their fades) and its bindings
/// start in the plan's flat arrays. Each runs to where the next light's start.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub light: u16,
    pub fields: u32,
    pub bindings: u32,
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
    /// Every target's fields, one after the other, and whether each fades.
    pub fields: Box<[Field]>,
    pub fades: Box<[bool]>,
    /// Every target's bindings, one after the other, each target's sorted by node.
    pub bindings: Box<[Binding]>,
    /// The words of every binding. Equal words are kept once.
    pub words: Box<[Option<u16>]>,
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

    /// Each target's light, and its fields and bindings in the flat arrays.
    fn spans(&self) -> impl Iterator<Item = (u16, Range<usize>, Range<usize>)> + '_ {
        let ends = self
            .targets
            .iter()
            .skip(1)
            .map(|next| (next.fields as usize, next.bindings as usize))
            .chain(iter::once((self.fields.len(), self.bindings.len())));
        self.targets
            .iter()
            .zip(ends)
            .map(|(target, (fields, bindings))| {
                (
                    target.light,
                    target.fields as usize..fields,
                    target.bindings as usize..bindings,
                )
            })
    }

    /// A retained plan still fits `patch`: each step names the same lights and is still
    /// supported, each wave has the same runs, and each light has the same fields, fades
    /// and words.
    fn matches(&self, patch: &Patch) -> bool {
        let mut words = Vec::new();
        self.waves.iter().all(|wave| wave.matches(patch))
            && self
                .shows
                .iter()
                .enumerate()
                .all(|(node, show)| match show {
                    Shows::Step(step) => self.names(node, step, patch),
                    Shows::Wave(_) | Shows::Sequence => true,
                })
            && self.spans().all(|(light, fields, bindings)| {
                self.fits(light, fields, bindings, patch, &mut words)
            })
    }

    /// `step` still names exactly the lights bound to `node`, and they support it.
    fn names(&self, node: usize, step: &Step, patch: &Patch) -> bool {
        let Ok(runs) = patch.runs(step.target) else {
            return false;
        };
        let bound = self
            .spans()
            .filter(|(_, _, bindings)| {
                self.bindings
                    .get(bindings.clone())
                    .is_some_and(|bindings| bindings.iter().any(|b| b.node as usize == node))
            })
            .map(|(light, ..)| light);
        !runs.is_empty()
            && rig::supports(patch.targets(), &runs, &step.values).is_ok()
            && runs.iter().copied().flat_map(sequence::numbers).eq(bound)
    }

    /// `light` still has the plan's fields and fades, and the words of each binding.
    fn fits(
        &self,
        light: u16,
        fields: Range<usize>,
        bindings: Range<usize>,
        patch: &Patch,
        words: &mut Vec<Option<u16>>,
    ) -> bool {
        let Some(now) = LightNumber::new(light, Lights::MOST).and_then(|light| patch.target(light))
        else {
            return false;
        };
        Some(now.fields.as_slice()) == self.fields.get(fields.clone())
            && self
                .fades
                .get(fields)
                .is_some_and(|fades| now.fades().eq(fades.iter().copied()))
            && self.bindings.get(bindings).is_some_and(|bindings| {
                bindings.iter().all(|binding| {
                    words.clear();
                    let Some(show) = self.shows.get(binding.node as usize) else {
                        return false;
                    };
                    show.words(now, words);
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
            PlanKind::Step(leaf) => size_of_val(leaf.offers.as_ref()),
            PlanKind::Timed(timed) => {
                size_of_val(timed.saved.as_ref())
                    + size_of_val(timed.palettes.as_ref())
                    + size_of_val(timed.nodes.as_ref())
                    + size_of_val(timed.items.as_ref())
                    + size_of_val(timed.shows.as_ref())
                    + size_of_val(timed.targets.as_ref())
                    + size_of_val(timed.fields.as_ref())
                    + size_of_val(timed.fades.as_ref())
                    + size_of_val(timed.bindings.as_ref())
                    + size_of_val(timed.words.as_ref())
                    + WaveBinding::storage(&timed.waves)
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
                let leaf = Leaf::new(step.step(), patch)?;
                if size_of_val(leaf.offers.as_ref()) > room {
                    return Err(Refusal::WorkLimit);
                }
                let lights = patch
                    .runs(step.target)?
                    .iter()
                    .map(|run| usize::from(run.count()))
                    .sum();
                (PlanKind::Step(leaf), sample_work(lights, 1)?)
            }
            Definition::Sequence(_) | Definition::Pattern(_) => {
                let (program, work) = sequence::compile(looks, look, patch, room)?;
                (PlanKind::Timed(program), work)
            }
        };
        let mut plan = Self {
            look,
            kind,
            work,
            bytes: 0,
        };
        // the scratch the engine makes for its samples is booked with the plan
        plan.bytes = core::mem::size_of::<Self>()
            + plan.kind.storage()
            + (plan.words() + plan.spare()) * size_of::<Option<u16>>();
        Ok(plan)
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

    /// Scratch words a sample needs: one per field of every target of a timed root.
    pub fn words(&self) -> usize {
        match &self.kind {
            PlanKind::Step(_) => 0,
            PlanKind::Timed(timed) => timed.fields.len(),
        }
    }

    /// Spare words a sample blends with: the most fields of a target, for each depth.
    pub fn spare(&self) -> usize {
        match &self.kind {
            PlanKind::Step(_) => 0,
            PlanKind::Timed(timed) => {
                timed
                    .spans()
                    .map(|(_, fields, _)| fields.len())
                    .max()
                    .unwrap_or(0)
                    * MAX_DEPTH
            }
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
            PlanKind::Timed(timed) => timed.matches(patch),
        }
    }

    pub fn finished(&self, elapsed: impl Into<Elapsed>, usage: Use) -> bool {
        match &self.kind {
            PlanKind::Step(_) => false,
            PlanKind::Timed(timed) => timed.program().finished(elapsed, usage),
        }
    }

    /// Fill caller-owned scratch ([`Plan::words`]), blending in `spare`
    /// ([`Plan::spare`]). False when the sample offers nothing: nothing to keep.
    pub fn sample(
        &self,
        elapsed: impl Into<Elapsed>,
        usage: Use,
        scratch: &mut [Option<u16>],
        spare: &mut [Option<u16>],
    ) -> bool {
        match &self.kind {
            PlanKind::Step(leaf) => !leaf.offers.is_empty(),
            PlanKind::Timed(timed) => {
                let elapsed = elapsed.into();
                let program = timed.program();
                for (_, fields, bindings) in timed.spans() {
                    let (Some(out), Some(fades), Some(bindings)) = (
                        scratch.get_mut(fields.clone()),
                        timed.fades.get(fields),
                        timed.bindings.get(bindings),
                    ) else {
                        continue;
                    };
                    program.sample(Sampled { bindings, fades }, elapsed, usage, out, spare);
                }
                scratch.iter().any(Option::is_some)
            }
        }
    }

    /// What the play offers: a step's fixed offers, or the timed root's last sample in
    /// `scratch`.
    pub fn offering<'a>(&'a self, scratch: &'a [Option<u16>]) -> Offering<'a> {
        match &self.kind {
            PlanKind::Step(leaf) => Offering::List(&leaf.offers),
            PlanKind::Timed(timed) => Offering::Sampled {
                fields: &timed.fields,
                values: scratch.get(..timed.fields.len()).unwrap_or_default(),
            },
        }
    }
}
