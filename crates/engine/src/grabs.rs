//! The direct grabs after all admitted commands, by their entry of the source table
//! ([`GRABS`]). Each light property is held by one grab at most: a new grab takes it from
//! the one before, which then offers on the lights it still holds, or ends with none.
//! What the tick needs comes out as [`Step`]s: a grab's offers, made here from the patch,
//! so the tick only swaps a box.
//!
//! ```text
//! grab --Start--> live --Update (value, lights taken, patch moved)--> live
//!                   |--end with Keep--> kept --UntilStop rules--> free
//!                   |--end with Remove, last light taken, release--> free
//! any --ReleaseAll--> free
//! ```

use engine_core::{
    End, GRABS, GrabId, HeldLight, LightNumber, LightRun, Lights, MAX_LIGHTS, Offer, Property,
    Refusal, Value,
};
use rig::Patch;
use std::sync::Arc;

/// No grab holds the property.
const FREE: u16 = u16::MAX;

/// What the tick does to one grab entry, in order. `Start` and `Update` bring the
/// entry's offers.
pub enum Step {
    Start(u16, Box<[Offer]>),
    Update(u16, Box<[Offer]>),
    Keep(u16),
    Remove(u16),
}

impl Step {
    /// The entry this step brings a box of offers to.
    pub const fn brings(&self) -> Option<u16> {
        match self {
            Self::Start(entry, _) | Self::Update(entry, _) => Some(*entry),
            Self::Keep(_) | Self::Remove(_) => None,
        }
    }
}

/// One grab: who can still reach it (`None` once its end kept it), its value, the runs it
/// was given, and how many offers it makes on the lights it still holds.
#[derive(Clone)]
struct Held {
    id: Option<GrabId>,
    value: Value,
    runs: Arc<[LightRun]>,
    offers: usize,
}

#[derive(Clone)]
pub struct Grabs {
    /// Per light and property, the entry that holds it, or [`FREE`].
    holders: Box<[[u16; Property::ALL.len()]]>,
    entries: Box<[Option<Held>]>,
    /// Each light's held intensity and colour, by light number, for the picture.
    held: Arc<[HeldLight]>,
}

/// The light indices of `runs`.
fn lights(runs: &[LightRun]) -> impl Iterator<Item = usize> + '_ {
    runs.iter().flat_map(|run| {
        let first = usize::from(run.first().index());
        first..first + usize::from(run.count())
    })
}

impl Grabs {
    pub fn new() -> Self {
        Self {
            holders: vec![[FREE; Property::ALL.len()]; usize::from(MAX_LIGHTS)].into_boxed_slice(),
            entries: vec![None; GRABS].into_boxed_slice(),
            held: Arc::from([]),
        }
    }

    /// The offers every grab makes, all together: what the frame visits.
    pub fn offers(&self) -> usize {
        self.entries.iter().flatten().map(|held| held.offers).sum()
    }

    /// The lights that hold intensity or colour, by light number: what the picture shows.
    pub fn held(&self) -> Arc<[HeldLight]> {
        Arc::clone(&self.held)
    }

    /// The book was queued: the picture's held lights follow it.
    pub fn sent(&mut self) {
        self.held = self.lights_held();
    }

    /// A new grab `id` of `value` on `runs`, the latest: the grabs it takes the lights
    /// from offer on the rest. `GrabFull` with no entry free.
    pub fn grab(
        &mut self,
        patch: &Patch,
        id: GrabId,
        value: Value,
        runs: Vec<LightRun>,
    ) -> Result<Vec<Step>, Refusal> {
        let entry = self
            .entries
            .iter()
            .position(Option::is_none)
            .and_then(|e| u16::try_from(e).ok())
            .ok_or(Refusal::GrabFull { most: GRABS as u32 })?;
        let property = value.property();
        let mut touched = Vec::new();
        for light in lights(&runs) {
            let old = self.set_holder(light, property, entry);
            if old != FREE && !touched.contains(&old) {
                touched.push(old);
            }
        }
        let runs: Arc<[LightRun]> = runs.into();
        let offers = self.offers_of(patch, entry, property, value, &runs);
        if let Some(slot) = self.entries.get_mut(usize::from(entry)) {
            *slot = Some(Held {
                id: Some(id),
                value,
                runs,
                offers: offers.len(),
            });
        }
        let mut steps = self.rebuild(patch, &touched);
        steps.push(Step::Start(entry, offers));
        Ok(steps)
    }

    /// The live grab `id` takes `value` on the lights it still holds. `OtherProperty` for
    /// a value of another property; nothing for a grab no longer live.
    pub fn update(
        &mut self,
        patch: &Patch,
        id: GrabId,
        value: Value,
    ) -> Result<Vec<Step>, Refusal> {
        let Some((entry, held)) = self.live(id) else {
            return Ok(Vec::new());
        };
        let property = held.value.property();
        if property != value.property() {
            return Err(Refusal::OtherProperty { property });
        }
        patch.supports(&self.held_runs(entry), value)?;
        if let Some(Some(held)) = self.entries.get_mut(usize::from(entry)) {
            held.value = value;
        }
        Ok(self.rebuild(patch, &[entry]))
    }

    /// The live grab `id` ends as `end` says: kept, or gone.
    pub fn end(&mut self, id: GrabId, end: End) -> Vec<Step> {
        let Some((entry, _)) = self.live(id) else {
            return Vec::new();
        };
        match end {
            End::Keep => {
                if let Some(Some(held)) = self.entries.get_mut(usize::from(entry)) {
                    held.id = None;
                }
                vec![Step::Keep(entry)]
            }
            End::Remove => {
                self.free(entry);
                vec![Step::Remove(entry)]
            }
        }
    }

    /// The lights of `runs` let `property` go, or every property when `None` (an
    /// unpatched fixture's lights).
    pub fn let_go(
        &mut self,
        patch: &Patch,
        property: Option<Property>,
        runs: &[LightRun],
    ) -> Vec<Step> {
        let mut touched = Vec::new();
        for light in lights(runs) {
            for p in Property::ALL {
                if property.is_none_or(|property| property == p) {
                    let old = self.set_holder(light, p, FREE);
                    if old != FREE && !touched.contains(&old) {
                        touched.push(old);
                    }
                }
            }
        }
        self.rebuild(patch, &touched)
    }

    /// The lights of `runs` are at new channels: the grabs on them offer there.
    pub fn moved(&mut self, patch: &Patch, runs: &[LightRun]) -> Vec<Step> {
        let mut touched = Vec::new();
        for light in lights(runs) {
            for entry in self.holders.get(light).into_iter().flatten() {
                if *entry != FREE && !touched.contains(entry) {
                    touched.push(*entry);
                }
            }
        }
        self.rebuild(patch, &touched)
    }

    /// The Mixer dropped the kept grabs on a rules change, as the tick does.
    pub fn clear_kept(&mut self) {
        let kept: Vec<u16> = (0..GRABS as u16)
            .filter(|e| {
                self.entries
                    .get(usize::from(*e))
                    .is_some_and(|held| held.as_ref().is_some_and(|h| h.id.is_none()))
            })
            .collect();
        for entry in kept {
            self.free(entry);
        }
        self.held = self.lights_held();
    }

    /// Every grab ends, as the tick ends them. The boxes stay where they are.
    pub fn clear(&mut self) {
        self.holders.fill([FREE; Property::ALL.len()]);
        self.entries.fill(None);
        self.held = Arc::from([]);
    }

    fn live(&self, id: GrabId) -> Option<(u16, &Held)> {
        self.entries.iter().enumerate().find_map(|(entry, held)| {
            let held = held.as_ref().filter(|h| h.id == Some(id))?;
            Some((u16::try_from(entry).ok()?, held))
        })
    }

    /// Each entry of `touched` offers anew on the lights it still holds, or ends with none.
    fn rebuild(&mut self, patch: &Patch, touched: &[u16]) -> Vec<Step> {
        let mut steps = Vec::new();
        for &entry in touched {
            let Some(Some(held)) = self.entries.get(usize::from(entry)) else {
                continue;
            };
            let (value, runs) = (held.value, Arc::clone(&held.runs));
            if self.held_runs(entry).is_empty() {
                self.free(entry);
                steps.push(Step::Remove(entry));
            } else {
                let offers = self.offers_of(patch, entry, value.property(), value, &runs);
                if let Some(Some(held)) = self.entries.get_mut(usize::from(entry)) {
                    held.offers = offers.len();
                }
                steps.push(Step::Update(entry, offers));
            }
        }
        steps
    }

    /// What `value` offers on the lights of `runs` that `entry` holds.
    fn offers_of(
        &self,
        patch: &Patch,
        entry: u16,
        property: Property,
        value: Value,
        runs: &[LightRun],
    ) -> Box<[Offer]> {
        let light = value.light();
        lights(runs)
            .filter(|&at| self.holder(at, property) == entry)
            .filter_map(|at| {
                let number = LightNumber::new(u16::try_from(at).ok()?, Lights::MOST)?;
                patch.target(number)
            })
            .flat_map(|target| target.offers(&light).collect::<Vec<_>>())
            .collect()
    }

    /// The runs of the lights `entry` still holds.
    fn held_runs(&self, entry: u16) -> Vec<LightRun> {
        let Some(Some(held)) = self.entries.get(usize::from(entry)) else {
            return Vec::new();
        };
        let property = held.value.property();
        selected(&held.runs, |light| self.holder(light, property) == entry)
    }

    /// `entry` holds nothing more and is free.
    fn free(&mut self, entry: u16) {
        let Some(Some(held)) = self.entries.get(usize::from(entry)) else {
            return;
        };
        let (property, runs) = (held.value.property(), Arc::clone(&held.runs));
        for light in lights(&runs) {
            if self.holder(light, property) == entry {
                self.set_holder(light, property, FREE);
            }
        }
        if let Some(slot) = self.entries.get_mut(usize::from(entry)) {
            *slot = None;
        }
    }

    fn holder(&self, light: usize, property: Property) -> u16 {
        self.holders
            .get(light)
            .and_then(|holders| holders.get(property.index()))
            .copied()
            .unwrap_or(FREE)
    }

    /// `entry` holds `property` of `light` now; the entry that held it before.
    fn set_holder(&mut self, light: usize, property: Property, entry: u16) -> u16 {
        self.holders
            .get_mut(light)
            .and_then(|holders| holders.get_mut(property.index()))
            .map_or(FREE, |slot| core::mem::replace(slot, entry))
    }

    fn value_of(&self, light: usize, property: Property) -> Option<Value> {
        let entry = self.holder(light, property);
        self.entries
            .get(usize::from(entry))?
            .as_ref()
            .map(|held| held.value)
    }

    /// Every light that holds intensity or colour, by number, counted from 1.
    fn lights_held(&self) -> Arc<[HeldLight]> {
        (0..self.holders.len())
            .filter_map(|at| {
                let intensity = self.value_of(at, Property::Intensity);
                let colour = self.value_of(at, Property::Colour);
                (intensity.is_some() || colour.is_some()).then(|| HeldLight {
                    light: u16::try_from(at + 1).unwrap_or(u16::MAX),
                    intensity: intensity.and_then(|v| v.light().intensity),
                    colour: colour.and_then(|v| v.light().colour),
                })
            })
            .collect()
    }
}

/// Compact the matching lights of the Rig's sorted, disjoint runs.
fn selected(runs: &[LightRun], matches: impl Fn(usize) -> bool) -> Vec<LightRun> {
    let mut result = Vec::new();
    let mut indices = lights(runs).filter(|&light| matches(light)).peekable();
    while let Some(index) = indices.next() {
        let mut end = index + 1;
        while indices.next_if_eq(&end).is_some() {
            end += 1;
        }
        if let Some(first) = LightNumber::new(index as u16, Lights::MOST)
            && let Some(run) = LightRun::new(first, (end - index) as u16, Lights::MOST)
        {
            result.push(run);
        }
    }
    result
}
