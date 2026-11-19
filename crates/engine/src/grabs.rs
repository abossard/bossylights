//! The live grabs as the front knows them, off the tick thread: which grab holds each
//! property of each light. A grab that holds nothing any more is forgotten, an update is
//! checked against the lights the grab still holds, and the runs kept stay within
//! [`RUNS`], whatever a client sends.
//!
//! ```text
//! grab --> live, holds its lights --a later grab or a release takes the last--> forgotten
//!                                  --EndGrab or ReleaseAll----------------------> forgotten
//! ```

use engine_core::{GrabId, LightNumber, LightRun, Lights, MAX_LIGHTS, Property, Refusal};
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

pub struct Grabs {
    /// Per light, per property: the live grab that holds it.
    holders: Box<[[Option<GrabId>; Property::ALL.len()]]>,
    live: HashMap<GrabId, Live>,
    /// The runs the live grabs keep, all together.
    runs: usize,
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
        }
    }

    /// The live grabs that hold `property` on a light of `runs`, and how many of those
    /// lights each holds.
    fn holding(&self, property: Property, runs: &[LightRun]) -> HashMap<GrabId, u32> {
        let mut holding = HashMap::new();
        for light in lights(runs) {
            if let Some(grab) = self.holder(light, property) {
                *holding.entry(grab).or_insert(0) += 1;
            }
        }
        holding
    }

    /// `GrabFull` unless a grab of `property` on `runs` fits within [`RUNS`], once the
    /// grabs it leaves holding nothing are forgotten.
    pub fn fits(&self, property: Property, runs: &[LightRun]) -> Result<(), Refusal> {
        let freed: usize = self
            .holding(property, runs)
            .into_iter()
            .filter_map(|(grab, taken)| self.live.get(&grab).filter(|live| live.holds == taken))
            .map(|live| live.runs.len())
            .sum();
        if self.runs - freed + runs.len() > RUNS {
            return Err(Refusal::GrabFull { most: RUNS as u32 });
        }
        Ok(())
    }

    /// `grab` holds `property` on the lights of `runs`, the latest: the grabs it takes the
    /// lights from no longer hold them.
    pub fn grab(&mut self, grab: GrabId, property: Property, runs: Vec<LightRun>) {
        self.let_go(Some(property), &runs);
        let mut holds = 0;
        for light in lights(&runs) {
            if let Some(holder) = self.holder_mut(light, property) {
                *holder = Some(grab);
                holds += 1;
            }
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
                if let Some(holder) = self.holder_mut(light, property) {
                    *holder = None;
                }
            }
        }
    }

    /// The property of `grab` and the runs it was given, while it is live.
    pub fn get(&self, grab: GrabId) -> Option<(Property, &[LightRun])> {
        self.live
            .get(&grab)
            .map(|live| (live.property, live.runs.as_slice()))
    }

    /// The lights `grab` still holds, as runs of one light each.
    pub fn held(&self, grab: GrabId) -> Vec<LightRun> {
        let Some(live) = self.live.get(&grab) else {
            return Vec::new();
        };
        lights(&live.runs)
            .filter(|&light| self.holder(light, live.property) == Some(grab))
            .filter_map(|light| {
                let light = LightNumber::new(u16::try_from(light).ok()?, Lights::MOST)?;
                LightRun::new(light, 1, Lights::MOST)
            })
            .collect()
    }

    /// Ends `grab`: its lights are held by no grab. Its property and runs.
    pub fn end(&mut self, grab: GrabId) -> Option<(Property, Vec<LightRun>)> {
        let live = self.live.get(&grab)?;
        let (property, runs) = (live.property, live.runs.clone());
        for light in lights(&runs) {
            if let Some(holder) = self.holder_mut(light, property)
                && *holder == Some(grab)
            {
                *holder = None;
            }
        }
        self.forget(grab);
        Some((property, runs))
    }

    /// Every grab ends.
    pub fn clear(&mut self) {
        self.holders.fill([None; Property::ALL.len()]);
        self.live.clear();
        self.runs = 0;
    }

    fn holder(&self, light: usize, property: Property) -> Option<GrabId> {
        self.holders
            .get(light)
            .and_then(|holders| holders.get(property.index()))
            .copied()
            .flatten()
    }

    fn holder_mut(&mut self, light: usize, property: Property) -> Option<&mut Option<GrabId>> {
        self.holders
            .get_mut(light)
            .and_then(|holders| holders.get_mut(property.index()))
    }

    fn forget(&mut self, grab: GrabId) {
        if let Some(live) = self.live.remove(&grab) {
            self.runs -= live.runs.len();
        }
    }
}
