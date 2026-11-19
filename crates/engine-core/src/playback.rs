//! Saved looks and the plays of them, as the tick thread keeps them: fixed in size.
//!
//! ```text
//! off --StartLook--> Playing --StopLook, the Mixer keeps it--> Kept --ReleaseHold--> off
//!                       |  \--StopLook, the Mixer reveals; EndFlash (Manual); ReleaseHold--> off
//!                       \--StartLook again (same look, same rank)--> Playing, a new play in the same slot
//! Kept --StartLook again--> Playing
//! off, Kept --StartLook on a beat or bar--> Waiting --the grid reaches it--> Playing
//! Waiting --StopLook, the Mixer would keep it--> Cancelled --ReleaseHold--> off
//! Waiting --StopLook, the Mixer reveals; EndFlash (Manual); ReleaseHold--> off
//! ```
//!
//! A cancelled play shows nothing and is in no picture. It keeps its slot and plan until
//! it is released, as a kept play does: the front cannot tell whether the stop came
//! before the launch, so it counts it as kept either way.
//!
//! A play's values come as a plan the front compiled off the tick thread: its step's
//! values on the runs of lights its target names ([`Entry`]). Who wins a light's
//! property between plays and direct holds, and what a stop keeps: `mix.rs`.

use crate::clock::{Clock, When, beats};
use crate::library::Text;
use crate::mix::{GrabId, MixRules};
use crate::patch::Target;
use crate::property::{Light, Value};
use crate::resolve::LightRun;
use core::time::Duration;
use serde::{Deserialize, Serialize};

/// The most plays at once.
pub const PLAYS: usize = 32;

/// The most saved looks.
pub const LOOKS: usize = 1024;

/// A step: sparse values for the lights a target names. An absent property lets
/// another source through; zero and black claim it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Step {
    pub target: Target,
    pub values: Light,
}

/// A saved look, as the picture and the show file show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Saved {
    pub look: u16,
    #[cfg_attr(test, ts(type = "string"))]
    pub name: Text,
    pub step: Step,
}

/// What a client asks of looks and plays. The front checks it against the saved looks
/// and the patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a client's command has no heap: `PutStep` carries its name and step in place. Moved once per request, off the tick thread"
)]
pub enum LookCommand {
    PutStep {
        look: u16,
        name: Text,
        step: Step,
    },
    RemoveLook {
        look: u16,
    },
    StartLook {
        look: u16,
        rank: Rank,
        when: When,
    },
    StopLook {
        play: PlayId,
    },
    EndFlash {
        play: PlayId,
    },
    ReleaseHold {
        play: PlayId,
    },
    /// Every Manual play, every direct hold and every raw channel let go.
    ReleaseAll,
    UpdateGrab {
        grab: GrabId,
        value: Value,
    },
    EndGrab {
        grab: GrabId,
    },
    SetMixRules {
        rules: MixRules,
    },
}

/// Who a play plays for. A higher rank wins a light's property over a lower one; a
/// direct hold is Manual.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Rank {
    Autopilot,
    Show,
    #[default]
    Manual,
}

/// Where a play is: it waits for beat `beat` of the grid, it plays, or it was stopped
/// and the Mixer keeps its last values until it is released or started again.
/// `Cancelled`: stopped while it waited, kept by nothing; never in a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum PlayState {
    #[cfg_attr(test, ts(type = "{ beat: number }"))]
    Waiting {
        beat: u64,
    },
    Playing,
    Kept,
    Cancelled,
}

/// One play: a slot and the generation of the play in it. An ended generation names
/// nothing: a late finger-up cannot end the play that took its slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PlayId {
    pub slot: u8,
    pub generation: u32,
}

impl PlayId {
    /// `None` past [`PLAYS`].
    pub fn new(slot: usize, generation: u32) -> Option<Self> {
        (slot < PLAYS).then_some(Self {
            slot: u8::try_from(slot).ok()?,
            generation,
        })
    }

    pub const fn slot(self) -> usize {
        self.slot as usize
    }
}

/// A play on the tick thread: which look, its rank, where it is, its start order and
/// the elapsed count of the clock at its start: its own beat is the clock's less this.
/// A waiting play takes its order and start at its launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Play {
    pub play: PlayId,
    pub look: u16,
    pub rank: Rank,
    pub state: PlayState,
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub order: u64,
    /// In beats, as the picture shows it.
    #[serde(serialize_with = "in_beats")]
    #[cfg_attr(test, ts(type = "number"))]
    pub start: u64,
}

fn in_beats<S: serde::Serializer>(count: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_f64(beats(*count))
}

/// Every play, by slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plays([Option<Play>; PLAYS]);

impl Plays {
    pub const NONE: Self = Self([None; PLAYS]);

    /// `play` starts in its slot at `rank`, in place of whatever played there: now,
    /// as the latest start, or waiting for the beat or bar `when` names, its request
    /// order kept to launch plays on one beat in. Then every waiting play whose beat the
    /// grid reached by now launches. The next start order.
    pub fn start(
        &mut self,
        (play, look, rank): (PlayId, u16, Rank),
        when: When,
        clock: &Clock,
        now: Duration,
        order: u64,
    ) -> u64 {
        let state = clock
            .next(when)
            .map_or(PlayState::Playing, |beat| PlayState::Waiting { beat });
        if let Some(slot) = self.0.get_mut(play.slot()) {
            *slot = Some(Play {
                play,
                look,
                rank,
                state,
                order,
                start: clock.elapsed_at(now),
            });
        }
        self.launch(clock, order.saturating_add(1))
    }

    /// Every waiting play whose beat the grid reached by now plays, in the order of their
    /// beats, then of their requests: each takes the next start order and starts at its
    /// crossing. The next start order.
    pub fn launch(&mut self, clock: &Clock, mut order: u64) -> u64 {
        while let Some(play) = self
            .0
            .iter_mut()
            .flatten()
            .filter_map(|play| match play.state {
                PlayState::Waiting { beat } if clock.reached(beat) => Some((beat, play)),
                PlayState::Waiting { .. }
                | PlayState::Playing
                | PlayState::Kept
                | PlayState::Cancelled => None,
            })
            .min_by_key(|(beat, play)| (*beat, play.order))
            .map(|(beat, play)| {
                play.start = clock.elapsed_at(clock.crossing(beat));
                play
            })
        {
            play.state = PlayState::Playing;
            play.order = order;
            order = order.saturating_add(1);
        }
        order
    }

    /// `play` is kept, if it still plays: its values stay at its rank and order. If it
    /// waits, it is cancelled.
    pub fn keep(&mut self, play: PlayId) {
        if let Some(Some(now)) = self.0.get_mut(play.slot())
            && now.play == play
        {
            now.state = match now.state {
                PlayState::Waiting { .. } | PlayState::Cancelled => PlayState::Cancelled,
                PlayState::Playing | PlayState::Kept => PlayState::Kept,
            };
        }
    }

    /// `play` ends, if it is still there. Whether it did.
    pub fn release(&mut self, play: PlayId) -> bool {
        match self.0.get_mut(play.slot()) {
            Some(slot) if slot.is_some_and(|now| now.play == play) => {
                *slot = None;
                true
            }
            _ => false,
        }
    }

    /// Every play for which `ends` holds ends.
    pub fn release_where(&mut self, ends: impl Fn(&Play) -> bool) {
        for slot in &mut self.0 {
            if slot.is_some_and(|play| ends(&play)) {
                *slot = None;
            }
        }
    }

    /// Whether a play is in `slot`.
    pub fn has(&self, slot: usize) -> bool {
        self.0.get(slot).is_some_and(Option::is_some)
    }

    /// The plays, by slot: waiting, playing or kept.
    pub fn iter(&self) -> impl Iterator<Item = &Play> {
        self.0
            .iter()
            .flatten()
            .filter(|play| play.state != PlayState::Cancelled)
    }

    /// The plays the Mixer hears, by slot: playing or kept.
    pub fn sounding(&self) -> impl Iterator<Item = &Play> {
        self.iter()
            .filter(|play| matches!(play.state, PlayState::Playing | PlayState::Kept))
    }
}

impl Serialize for Plays {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}

/// A step's values on one run of lights: what a play holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub lights: LightRun,
    pub values: Light,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(slot: usize, generation: u32) -> PlayId {
        PlayId::new(slot, generation).unwrap()
    }

    /// Starts `play` at once, as the start order `order`.
    fn now(plays: &mut Plays, play: PlayId, rank: Rank, order: u64) {
        plays.start(
            (play, 7, rank),
            When::Now,
            &Clock::START,
            Duration::ZERO,
            order,
        );
    }

    #[test]
    fn a_play_ends_only_by_its_own_generation() {
        // (name, start, release, ends, plays after)
        let cases = [
            ("its own", id(3, 1), id(3, 1), true, 0),
            ("an ended generation", id(3, 2), id(3, 1), false, 1),
            ("another slot", id(3, 1), id(4, 1), false, 1),
        ];
        for (name, start, release, ends, after) in cases {
            for kept in [false, true] {
                let mut plays = Plays::NONE;
                now(&mut plays, start, Rank::Show, 1);
                if kept {
                    plays.keep(start);
                }
                assert_eq!(plays.release(release), ends, "{name}, kept {kept}");
                assert_eq!(plays.iter().count(), after, "{name}, kept {kept}");
            }
        }
    }

    #[test]
    fn only_its_own_generation_keeps_a_play() {
        // (name, start, keep, state after)
        let cases = [
            ("its own", id(3, 1), id(3, 1), PlayState::Kept),
            (
                "an ended generation",
                id(3, 2),
                id(3, 1),
                PlayState::Playing,
            ),
            ("another slot", id(3, 1), id(4, 1), PlayState::Playing),
        ];
        for (name, start, keep, state) in cases {
            let mut plays = Plays::NONE;
            now(&mut plays, start, Rank::Manual, 1);
            plays.keep(keep);
            let now: Vec<_> = plays.iter().map(|play| play.state).collect();
            assert_eq!(now, [state], "{name}");
        }
    }

    #[test]
    fn a_kept_play_started_again_plays() {
        let mut plays = Plays::NONE;
        now(&mut plays, id(0, 1), Rank::Show, 1);
        plays.keep(id(0, 1));
        now(&mut plays, id(0, 2), Rank::Show, 2);
        let now: Vec<_> = plays.iter().map(|play| (play.play, play.state)).collect();
        assert_eq!(now, [(id(0, 2), PlayState::Playing)]);
    }

    #[test]
    fn ranks_order_manual_over_show_over_autopilot() {
        assert!(Rank::Manual > Rank::Show && Rank::Show > Rank::Autopilot);
        assert_eq!(Rank::default(), Rank::Manual);
    }

    #[test]
    fn a_start_takes_the_place_of_the_play_in_its_slot() {
        let mut plays = Plays::NONE;
        now(&mut plays, id(0, 1), Rank::Manual, 1);
        now(&mut plays, id(0, 2), Rank::Manual, 2);
        let now: Vec<_> = plays.iter().copied().collect();
        assert_eq!(
            now,
            [Play {
                play: id(0, 2),
                look: 7,
                rank: Rank::Manual,
                state: PlayState::Playing,
                order: 2,
                start: 0,
            }]
        );
        assert!(!plays.release(id(0, 1)));
    }

    #[test]
    fn play_ids_stop_at_the_last_slot() {
        assert!(PlayId::new(PLAYS - 1, 0).is_some());
        assert!(PlayId::new(PLAYS, 0).is_none());
    }
}
