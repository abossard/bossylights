//! Direct holds after all admitted commands. Each live light property reserves work
//! for an end and a release. A kept property needs only its final release.
//! A live grab keeps its original runs within [`RUNS`] until it loses its last light.
//!
//! ```text
//! grab --> live --end with Keep--> kept
//!            |                      |
//!            +--end with Remove     +--UntilStop rules
//!            +--release-------------+--release------> free
//! ```

use engine_core::{End, GrabId, LightNumber, LightRun, Lights, MAX_LIGHTS, Property, Refusal};
use std::collections::HashMap;

/// The most runs the live grabs keep: every property of every light held by a grab of its
/// own fits. A grab that would keep more is refused [`Refusal::GrabFull`].
pub const RUNS: usize = MAX_LIGHTS as usize * Property::ALL.len();

/// A live grab: its property, the runs it was given, and how many of their lights it
/// still holds (at least one).
struct Live {
    property: Property,
    runs: Vec<LightRun>,
    holds: u32,
}

/// A live grab can end and then be released. A kept property can only be released.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Holder {
    Live(GrabId),
    Kept,
}

fn release_work(holder: Option<Holder>) -> usize {
    match holder {
        Some(Holder::Live(_)) => 2,
        Some(Holder::Kept) => 1,
        None => 0,
    }
}

pub struct Grabs {
    /// Per light and property, who still holds it after all admitted commands.
    holders: Box<[[Option<Holder>; Property::ALL.len()]]>,
    live: HashMap<GrabId, Live>,
    /// The runs the live grabs keep, all together.
    runs: usize,
    /// Visits already promised to end and release these holds.
    reserved: usize,
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
            holders: vec![[None; Property::ALL.len()]; usize::from(MAX_LIGHTS)].into_boxed_slice(),
            live: HashMap::new(),
            runs: 0,
            reserved: 0,
        }
    }

    /// The live grabs that hold `property` on a light of `runs`, and how many of those
    /// lights each holds.
    fn holding(&self, property: Property, runs: &[LightRun]) -> HashMap<GrabId, u32> {
        let mut holding = HashMap::new();
        for light in lights(runs) {
            if let Some(Holder::Live(grab)) = self.holder(light, property) {
                *holding.entry(grab).or_insert(0) += 1;
            }
        }
        holding
    }

    pub fn reserved(&self) -> usize {
        self.reserved
    }

    /// Check the run limit after replacing earlier grabs and return the additional
    /// visits needed to end and release the new grab.
    pub fn fits(&self, property: Property, runs: &[LightRun]) -> Result<usize, Refusal> {
        let freed: usize = self
            .holding(property, runs)
            .into_iter()
            .filter_map(|(grab, taken)| self.live.get(&grab).filter(|live| live.holds == taken))
            .map(|live| live.runs.len())
            .sum();
        if self.runs - freed + runs.len() > RUNS {
            return Err(Refusal::GrabFull { most: RUNS as u32 });
        }
        Ok(lights(runs)
            .map(|light| 2 - release_work(self.holder(light, property)))
            .sum())
    }

    /// `grab` holds `property` on the lights of `runs`, the latest: the grabs it takes the
    /// lights from no longer hold them.
    pub fn grab(&mut self, grab: GrabId, property: Property, runs: Vec<LightRun>) {
        self.let_go(Some(property), &runs);
        let mut holds = 0;
        for light in lights(&runs) {
            self.set_holder(light, property, Some(Holder::Live(grab)));
            holds += 1;
        }
        self.runs += runs.len();
        self.live.insert(
            grab,
            Live {
                property,
                runs,
                holds,
            },
        );
    }

    /// The lights of `runs` let `property` go, or every property when `None` (an
    /// unpatched fixture's lights).
    pub fn let_go(&mut self, property: Option<Property>, runs: &[LightRun]) {
        let properties = Property::ALL
            .into_iter()
            .filter(|p| property.is_none_or(|property| property == *p));
        for property in properties {
            for (grab, taken) in self.holding(property, runs) {
                if let Some(live) = self.live.get_mut(&grab) {
                    live.holds = live.holds.saturating_sub(taken);
                    if live.holds == 0 {
                        self.forget(grab);
                    }
                }
            }
            for light in lights(runs) {
                self.set_holder(light, property, None);
            }
        }
    }

    pub fn property(&self, grab: GrabId) -> Option<Property> {
        self.live.get(&grab).map(|live| live.property)
    }

    /// The lights this grab can still update or end.
    pub fn held(&self, grab: GrabId) -> Vec<LightRun> {
        let Some(live) = self.live.get(&grab) else {
            return Vec::new();
        };
        selected(&live.runs, |light| {
            self.holder(light, live.property) == Some(Holder::Live(grab))
        })
    }

    /// A release visits only the lights on which this property is live or kept.
    pub fn releasing(&self, property: Property, runs: &[LightRun]) -> Vec<LightRun> {
        if self.reserved == 0 {
            return Vec::new();
        }
        selected(runs, |light| self.holder(light, property).is_some())
    }

    /// Ends a grab on the lights it still owns. Kept values keep one release visit.
    pub fn end(&mut self, grab: GrabId, property: Property, held: &[LightRun], end: End) {
        let holder = match end {
            End::Keep => Some(Holder::Kept),
            End::Remove => None,
        };
        for light in lights(held) {
            self.set_holder(light, property, holder);
        }
        self.forget(grab);
    }

    /// The Mixer discarded stopped Manual holds on a rules change.
    pub fn clear_kept(&mut self) {
        for slot in self.holders.iter_mut().flatten() {
            if *slot == Some(Holder::Kept) {
                *slot = None;
                self.reserved -= 1;
            }
        }
    }

    /// Every grab ends.
    pub fn clear(&mut self) {
        self.holders.fill([None; Property::ALL.len()]);
        self.live.clear();
        self.runs = 0;
        self.reserved = 0;
    }

    fn holder(&self, light: usize, property: Property) -> Option<Holder> {
        self.holders
            .get(light)
            .and_then(|holders| holders.get(property.index()))
            .copied()
            .flatten()
    }

    fn set_holder(&mut self, light: usize, property: Property, holder: Option<Holder>) {
        if let Some(slot) = self
            .holders
            .get_mut(light)
            .and_then(|holders| holders.get_mut(property.index()))
        {
            self.reserved = self.reserved - release_work(*slot) + release_work(holder);
            *slot = holder;
        }
    }

    fn forget(&mut self, grab: GrabId) {
        if let Some(live) = self.live.remove(&grab) {
            self.runs -= live.runs.len();
        }
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
