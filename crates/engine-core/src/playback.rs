//! Saved looks and the plays of them, as the tick thread keeps them: fixed in size.
//!
//! ```text
//! off --StartLook--> Playing --StopLook, the Mixer keeps it--> Kept --ReleaseHold--> off
//!                       |  \--StopLook, the Mixer reveals; EndFlash (Manual); ReleaseHold--> off
//!                       \--StartLook again (same look, same rank)--> Playing, a new play in the same slot
//! Kept --StartLook again--> Playing
//! ```
//!
//! A play's values come as a plan the front compiled off the tick thread: its step's
//! values on the runs of lights its target names ([`Entry`]). Who wins a light's
//! property between plays and direct holds, and what a stop keeps: `mix.rs`.

use crate::library::Text;
use crate::mix::MixRules;
use crate::patch::Target;
use crate::property::Light;
use crate::resolve::LightRun;
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

/// Where a play is: it plays, or it was stopped and the Mixer keeps its last values
/// until it is released or started again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum PlayState {
    Playing,
    Kept,
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

/// A play on the tick thread: which look, its rank, where it is and its start order.
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
}

/// Every play, by slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plays([Option<Play>; PLAYS]);

impl Plays {
    pub const NONE: Self = Self([None; PLAYS]);

    /// `play` starts in its slot at `rank`, in place of whatever played there.
    pub fn start(&mut self, play: PlayId, look: u16, rank: Rank, order: u64) {
        if let Some(slot) = self.0.get_mut(play.slot()) {
            *slot = Some(Play {
                play,
                look,
                rank,
                state: PlayState::Playing,
                order,
            });
        }
    }

    /// `play` is kept, if it still plays: its values stay at its rank and order.
    pub fn keep(&mut self, play: PlayId) {
        if let Some(Some(now)) = self.0.get_mut(play.slot())
            && now.play == play
        {
            now.state = PlayState::Kept;
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

    /// The plays, by slot.
    pub fn iter(&self) -> impl Iterator<Item = &Play> {
        self.0.iter().flatten()
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
                plays.start(start, 7, Rank::Show, 1);
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
            plays.start(start, 7, Rank::Manual, 1);
            plays.keep(keep);
            let now: Vec<_> = plays.iter().map(|play| play.state).collect();
            assert_eq!(now, [state], "{name}");
        }
    }

    #[test]
    fn a_kept_play_started_again_plays() {
        let mut plays = Plays::NONE;
        plays.start(id(0, 1), 7, Rank::Show, 1);
        plays.keep(id(0, 1));
        plays.start(id(0, 2), 7, Rank::Show, 2);
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
        plays.start(id(0, 1), 7, Rank::Manual, 1);
        plays.start(id(0, 2), 7, Rank::Manual, 2);
        let now: Vec<_> = plays.iter().copied().collect();
        assert_eq!(
            now,
            [Play {
                play: id(0, 2),
                look: 7,
                rank: Rank::Manual,
                state: PlayState::Playing,
                order: 2
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
