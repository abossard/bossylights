//! What the engine knows about every output, whatever its kind: a run of universes, a
//! rate, a state and counters. What kind it is, its device and its bytes belong to
//! `crates/outputs`; this file is the data and the calculations around it.
//!
//! ```text
//! [*] -> NotFound --accepted--> Connecting --accepted--> Running --failed--> Reconnecting --accepted--> Running
//!                               Connecting --failed----------------------->  Reconnecting --gave up--> NotFound
//! ```
//!
//! "Accepted" is the device's word: a device that must be found accepts only once it is.
//! A kind that gets no answer from its device accepts what it sent: for it, Running means
//! the frames were sent.

use crate::{Levels, Refusal, Universes};
use core::time::Duration;
use serde::{Deserialize, Serialize};

/// The most outputs an engine has at once.
pub const MAX_OUTPUTS: usize = 16;
/// An interval further than this from the output's period is off its beat.
pub const OFF_BEAT: Duration = Duration::from_millis(2);
/// Failures in a row while reconnecting before an output counts as not found.
pub const GIVE_UP: u16 = 100;

/// Which output, 1 to [`MAX_OUTPUTS`] as a client counts. Anything else does not parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u8")]
#[cfg_attr(test, derive(ts_rs::TS), ts(type = "number"))]
pub struct OutputNumber(u8);

impl OutputNumber {
    /// Counted from 0, below [`MAX_OUTPUTS`].
    pub const fn slot(self) -> usize {
        self.0 as usize
    }
}

impl TryFrom<u8> for OutputNumber {
    type Error = &'static str;
    fn try_from(number: u8) -> Result<Self, Self::Error> {
        if number >= 1 && usize::from(number) <= MAX_OUTPUTS {
            Ok(Self(number - 1))
        } else {
            Err("outputs are 1 to 16")
        }
    }
}

/// Frames a second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Rate {
    #[serde(rename = "30")]
    Thirty,
    #[serde(rename = "44")]
    FortyFour,
    #[serde(rename = "60")]
    Sixty,
}

impl Rate {
    pub const fn per_second(self) -> u32 {
        match self {
            Self::Thirty => 30,
            Self::FortyFour => 44,
            Self::Sixty => 60,
        }
    }

    pub const fn period(self) -> Duration {
        Duration::from_nanos(1_000_000_000 / self.per_second() as u64)
    }
}

/// A run of universes as a client sends it: `count` universes from `first`, counted from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RunNumbers {
    pub first: u16,
    pub count: u16,
}

/// A run of this engine's universes, sent as one stream of channels. Until the Rig
/// exists, this is how an output knows what to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Run {
    /// Counted from 1, as the client sent it.
    first: u16,
    count: u16,
}

impl RunNumbers {
    /// The run, if all of it is in this engine.
    pub fn check(self, universes: Universes) -> Result<Run, Refusal> {
        let last = self.first.saturating_add(self.count.saturating_sub(1));
        if self.first < 1 || self.count < 1 || last > universes.count() {
            return Err(Refusal::NoSuchUniverse {
                universes: universes.count(),
            });
        }
        Ok(Run {
            first: self.first,
            count: self.count,
        })
    }
}

impl Run {
    /// How many universes.
    pub const fn count(self) -> u16 {
        self.count
    }

    /// This run's universes of a frame. Empty if the frame is shorter.
    pub fn of(self, frame: &[Levels]) -> &[Levels] {
        let start = usize::from(self.first.saturating_sub(1));
        frame
            .get(start..start + usize::from(self.count))
            .unwrap_or_default()
    }
}

/// Where an output is. One type, one transition function: [`Status::after`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Status {
    NotFound,
    Connecting,
    Running,
    /// Sends failed; `failures` in a row so far.
    Reconnecting {
        failures: u16,
    },
}

/// What happened when a frame was handed to an output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The device took all of it.
    Accepted,
    /// The device was busy: the frame was dropped, nothing waited.
    Dropped,
    /// The device refused or is gone.
    Failed,
}

impl Status {
    /// Every transition. A dropped frame changes nothing: the device is there, only busy.
    pub const fn after(self, outcome: Outcome) -> Self {
        match (self, outcome) {
            (Self::NotFound, Outcome::Accepted) => Self::Connecting,
            (Self::Connecting | Self::Running | Self::Reconnecting { .. }, Outcome::Accepted) => {
                Self::Running
            }
            (Self::Connecting | Self::Running, Outcome::Failed) => {
                Self::Reconnecting { failures: 1 }
            }
            (Self::Reconnecting { failures }, Outcome::Failed) if failures + 1 >= GIVE_UP => {
                Self::NotFound
            }
            (Self::Reconnecting { failures }, Outcome::Failed) => Self::Reconnecting {
                failures: failures + 1,
            },
            (Self::NotFound, Outcome::Failed) => Self::NotFound,
            (state, Outcome::Dropped) => state,
        }
    }
}

/// Frames counted since the output was set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Counters {
    #[cfg_attr(test, ts(type = "number"))]
    pub accepted: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub dropped: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub failed: u64,
    /// Intervals more than [`OFF_BEAT`] away from the period.
    #[cfg_attr(test, ts(type = "number"))]
    pub off_beat: u64,
}

/// One output as the engine knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Output {
    pub universes: Run,
    pub rate: Rate,
    pub status: Status,
    pub counters: Counters,
    /// When its next frame is due, since the engine started.
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub due: Duration,
    /// When its last frame was handed over.
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub last: Option<Duration>,
}

impl Output {
    /// A new output, due at once. Not found until its device takes a frame.
    pub const fn new(universes: Run, rate: Rate, now: Duration) -> Self {
        Self {
            universes,
            rate,
            status: Status::NotFound,
            counters: Counters {
                accepted: 0,
                dropped: 0,
                failed: 0,
                off_beat: 0,
            },
            due: now,
            last: None,
        }
    }

    /// After a frame was handed over at `now`: state, counters, and when the next is due.
    pub fn handed(self, outcome: Outcome, now: Duration) -> Self {
        let period = self.rate.period();
        let off = self
            .last
            .is_some_and(|last| now.saturating_sub(last).abs_diff(period) > OFF_BEAT);
        let counters = self.counters;
        Self {
            status: self.status.after(outcome),
            counters: Counters {
                accepted: counters.accepted + u64::from(outcome == Outcome::Accepted),
                dropped: counters.dropped + u64::from(outcome == Outcome::Dropped),
                failed: counters.failed + u64::from(outcome == Outcome::Failed),
                off_beat: counters.off_beat + u64::from(off),
            },
            due: crate::next_frame(self.due, now, period),
            last: Some(now),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: fn(u16) -> Status = |failures| Status::Reconnecting { failures };

    #[test]
    fn every_status_and_outcome() {
        use Outcome::{Accepted, Dropped, Failed};
        use Status::{Connecting, NotFound, Running};
        // (state, outcome, next state)
        let table = [
            (NotFound, Accepted, Connecting), // the device appeared
            (NotFound, Dropped, NotFound),
            (NotFound, Failed, NotFound),
            (Connecting, Accepted, Running), // first frame accepted
            (Connecting, Dropped, Connecting),
            (Connecting, Failed, R(1)),
            (Running, Accepted, Running),
            (Running, Dropped, Running),
            (Running, Failed, R(1)), // lost
            (R(1), Accepted, Running),
            (R(1), Dropped, R(1)),
            (R(1), Failed, R(2)),
            (R(GIVE_UP - 2), Failed, R(GIVE_UP - 1)),
            (R(GIVE_UP - 1), Failed, NotFound), // gave up
        ];
        for (state, outcome, next) in table {
            assert_eq!(state.after(outcome), next, "{state:?} + {outcome:?}");
        }
    }

    fn fresh(rate: Rate) -> Output {
        let run = RunNumbers { first: 1, count: 1 }
            .check(Universes::ONE)
            .unwrap();
        Output::new(run, rate, Duration::ZERO)
    }

    #[test]
    fn handed_counts_and_judges_the_interval() {
        let ms = Duration::from_millis;
        let us = Duration::from_micros;
        let at_16_667 = Output {
            last: Some(ms(100)),
            ..fresh(Rate::Sixty)
        };
        // (output, outcome, handed at, counters: accepted, dropped, failed, off beat)
        let cases = [
            (fresh(Rate::Sixty), Outcome::Accepted, ms(0), [1, 0, 0, 0]), // no interval yet
            (at_16_667, Outcome::Accepted, us(116_667), [1, 0, 0, 0]),
            (at_16_667, Outcome::Dropped, us(118_600), [0, 1, 0, 0]), // 1.9 ms late: on beat
            (at_16_667, Outcome::Failed, us(118_700), [0, 0, 1, 1]),  // 2.03 ms late
            (at_16_667, Outcome::Accepted, us(114_600), [1, 0, 0, 1]), // 2.07 ms early
        ];
        for (output, outcome, at, [accepted, dropped, failed, off_beat]) in cases {
            let handed = output.handed(outcome, at);
            let expected = Counters {
                accepted,
                dropped,
                failed,
                off_beat,
            };
            assert_eq!(handed.counters, expected, "{outcome:?} at {at:?}");
            assert_eq!(handed.last, Some(at));
            assert_eq!(
                handed.due,
                crate::next_frame(output.due, at, output.rate.period())
            );
        }
    }

    #[test]
    fn a_run_is_all_inside_this_engine() {
        let four = Universes::new(4).unwrap();
        // (first, count, ok)
        let cases = [
            (1, 1, true),
            (1, 4, true),
            (4, 1, true),
            (3, 2, true),
            (0, 1, false),
            (1, 0, false),
            (4, 2, false),
            (5, 1, false),
            (u16::MAX, u16::MAX, false),
        ];
        for (first, count, ok) in cases {
            let checked = RunNumbers { first, count }.check(four);
            let expected = if ok {
                Ok(Run { first, count })
            } else {
                Err(Refusal::NoSuchUniverse { universes: 4 })
            };
            assert_eq!(checked, expected, "{first}, {count}");
        }
    }

    #[test]
    fn a_run_takes_its_universes_of_the_frame() {
        let frame: Vec<Levels> = (0..4).map(|n| Levels([n; crate::ADDRESSES])).collect();
        let four = Universes::new(4).unwrap();
        // (first, count, the levels of each universe taken)
        let cases = [(1, 1, vec![0]), (2, 3, vec![1, 2, 3]), (4, 1, vec![3])];
        for (first, count, expected) in cases {
            let run = RunNumbers { first, count }.check(four).unwrap();
            let taken: Vec<u8> = run.of(&frame).iter().map(|u| u.0[0]).collect();
            assert_eq!(taken, expected, "{first}, {count}");
        }
        let run = RunNumbers { first: 4, count: 1 }.check(four).unwrap();
        assert!(
            run.of(&frame[..2]).is_empty(),
            "a shorter frame gives nothing"
        );
    }

    #[test]
    fn output_numbers_are_1_to_16() {
        // (number, slot)
        let cases = [
            (0, None),
            (1, Some(0)),
            (16, Some(15)),
            (17, None),
            (u8::MAX, None),
        ];
        for (number, slot) in cases {
            assert_eq!(
                OutputNumber::try_from(number).ok().map(OutputNumber::slot),
                slot
            );
        }
    }
}
