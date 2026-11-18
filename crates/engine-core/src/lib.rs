//! The real-time core: everything that runs per tick, as calculations.
//!
//! No `std` and no `alloc`. The clock, files, the network and the heap do not exist
//! here, so code that would use them does not compile. Time comes in as a value: the
//! time since the engine started. `crates/engine` is the thin action around this crate:
//! it reads the clock, sleeps, calls [`update`] and publishes the state.

#![cfg_attr(not(test), no_std)] // tests may use std
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod api;
mod channel;

pub use api::{Refusal, Reply, Request};
pub use channel::{
    ADDRESSES, Channel, ChannelLevel, ChannelNumbers, Levels, MAX_UNIVERSES, Universes, changes,
};

use core::time::Duration;
use serde::{Deserialize, Serialize};

/// What the engine is doing now, without the levels. Sent to the UI as JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct State {
    /// Frames computed since the engine started.
    #[cfg_attr(test, ts(type = "number"))]
    pub frame: u64,
    /// What the system gave the output thread. Asked once, when the thread starts.
    pub priority: Priority,
    /// While on, every channel goes out at zero. The held levels stay.
    pub blackout: bool,
}

/// The output thread's priority, as the system answered when the thread started.
///
/// ```text
/// start the thread -> ask for real-time -> granted ----------> RealTime
///                                       -> refused ----------> BestEffort { Refused }
///                                       -> no such policy ---> BestEffort { NotSupported }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Priority {
    /// The system's real-time class: the thread wakes on time.
    RealTime,
    /// The default class: the thread may wake late.
    BestEffort { reason: BestEffort },
}

/// Why the output thread runs best effort.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum BestEffort {
    /// The system said no. `code` is its answer: a Mach `kern_return_t` or an `errno`.
    Refused { code: i32 },
    /// This system has no real-time policy we know how to ask for.
    NotSupported,
}

/// What changes the state: the closed list. `C` is a [`Channel`] inside the engine
/// and [`ChannelNumbers`] as a client sends it, before [`Command::check`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(concrete(C = ChannelNumbers)))]
pub enum Command<C = Channel> {
    /// The output thread computes a frame. Never from a client.
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    Tick,
    /// Hold a channel at a level.
    SetChannel { channel: C, value: u8 },
    /// Let a channel go back to zero.
    ReleaseChannel { channel: C },
    /// Let every channel go back to zero.
    ReleaseAll,
    /// Every channel out at zero while on. The held levels stay and come back when off.
    Blackout { on: bool },
}

impl Command<ChannelNumbers> {
    /// The command for this engine, or why it cannot be one. Done once, at the Engine.
    pub fn check(self, universes: Universes) -> Result<Command, Refusal> {
        Ok(match self {
            Self::Tick => return Err(Refusal::NotACommand),
            Self::SetChannel { channel, value } => Command::SetChannel {
                channel: channel.check(universes)?,
                value,
            },
            Self::ReleaseChannel { channel } => Command::ReleaseChannel {
                channel: channel.check(universes)?,
            },
            Self::ReleaseAll => Command::ReleaseAll,
            Self::Blackout { on } => Command::Blackout { on },
        })
    }
}

/// What must happen outside after an update, as data. None exist yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {}

/// The only place state changes: state and command in, new state and effects out.
/// `universes` are the held levels, a buffer the caller owns; a command changes them
/// in place. At most one effect, so nothing allocates. A command that needs more
/// widens this to a fixed-size array, never to a `Vec`.
pub fn update(state: State, universes: &mut [Levels], command: Command) -> (State, Option<Effect>) {
    match command {
        Command::Tick => (
            State {
                frame: state.frame.wrapping_add(1),
                ..state
            },
            None,
        ),
        Command::SetChannel { channel, value } => {
            if let Some(level) = channel.level(universes) {
                *level = value;
            }
            (state, None)
        }
        Command::ReleaseChannel { channel } => {
            if let Some(level) = channel.level(universes) {
                *level = 0;
            }
            (state, None)
        }
        Command::ReleaseAll => {
            universes.fill(Levels::ZERO);
            (state, None)
        }
        Command::Blackout { on } => (
            State {
                blackout: on,
                ..state
            },
            None,
        ),
    }
}

/// The levels that go out, written into `out`, a buffer the caller owns: the held
/// levels, or zero in a blackout.
pub fn frame(state: &State, universes: &[Levels], out: &mut [Levels]) {
    for (out, held) in out.iter_mut().zip(universes) {
        *out = if state.blackout { Levels::ZERO } else { *held };
    }
}

/// How many frames late the tick may fall before it gives up catching up.
pub const MAX_BEHIND: u32 = 10;

/// When the frame after `due` is due. A late frame is made up at once, so the rate
/// holds when the OS oversleeps. Far behind (the machine slept), it starts over from now.
/// All times are since the engine started.
pub fn next_frame(due: Duration, now: Duration, period: Duration) -> Duration {
    let next = due.saturating_add(period);
    if now > next.saturating_add(period.saturating_mul(MAX_BEHIND)) {
        now.saturating_add(period)
    } else {
        next
    }
}

/// The output thread sleeps until this long before a frame is due, then spins: a
/// sleep can wake late, a spin cannot. Measured in spike #23.
pub const SPIN: Duration = Duration::from_millis(1);

/// How long to sleep at `now` before spinning until `due`. All times since the engine started.
pub fn sleep_before(due: Duration, now: Duration) -> Duration {
    due.saturating_sub(now).saturating_sub(SPIN)
}

/// Times are counted in buckets this wide.
pub const BUCKET: Duration = Duration::from_micros(100);
/// Buckets for tick lateness: 25 ms, about one frame. The last one holds everything later.
pub const LATENESS_BUCKETS: usize = 251;
/// Buckets for the time from a send to its frame: 50 ms, about two frames.
pub const LATENCY_BUCKETS: usize = 501;
/// How many times fell into each bucket of [`BUCKET`].
pub type Histogram<const N: usize> = [u64; N];

/// The bucket for `time`. The last bucket holds everything later.
pub fn bucket<const N: usize>(time: Duration) -> usize {
    let bucket = time.as_micros() / BUCKET.as_micros();
    usize::try_from(bucket)
        .unwrap_or(usize::MAX)
        .min(N.saturating_sub(1))
}

/// The time `percent` of 100 counts stay under: the upper edge of the bucket that
/// holds that percentile. `None` before the first count.
pub fn percentile<const N: usize>(histogram: &Histogram<N>, percent: u64) -> Option<Duration> {
    let total = histogram
        .iter()
        .fold(0_u64, |sum, n| sum.saturating_add(*n));
    let above = 100_u64.saturating_sub(percent);
    // ceil(percent / 100 × total), without overflow
    let rank = total - (total / 100 * above + total % 100 * above / 100);
    let mut seen = 0_u64;
    for (bucket, count) in histogram.iter().enumerate() {
        seen = seen.saturating_add(*count);
        if total > 0 && seen >= rank {
            return Some(edge(bucket));
        }
    }
    None
}

/// The time no count went past: the upper edge of the latest bucket that holds a
/// count. `None` before the first count.
pub fn worst<const N: usize>(histogram: &Histogram<N>) -> Option<Duration> {
    histogram.iter().rposition(|count| *count > 0).map(edge)
}

fn edge(bucket: usize) -> Duration {
    BUCKET.saturating_mul(u32::try_from(bucket + 1).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ts_rs::{Config, TS};

    const CALM: State = State {
        frame: 7,
        priority: Priority::RealTime,
        blackout: false,
    };
    const DARK: State = State {
        blackout: true,
        ..CALM
    };

    /// Two universes, with these (universe, address, level) held; counted from 1.
    fn held(levels: &[(u16, u16, u8)]) -> [Levels; 2] {
        let mut universes = [Levels::ZERO; 2];
        for (universe, address, level) in levels {
            universes[usize::from(*universe - 1)].0[usize::from(*address - 1)] = *level;
        }
        universes
    }

    fn channel(universe: u16, address: u16) -> Channel {
        ChannelNumbers { universe, address }
            .check(Universes::new(2).unwrap())
            .unwrap()
    }

    #[test]
    fn update_turns_a_command_into_a_new_state() {
        let set = |universe, address, value| Command::SetChannel {
            channel: channel(universe, address),
            value,
        };
        let release = |universe, address| Command::ReleaseChannel {
            channel: channel(universe, address),
        };
        let max = State {
            frame: u64::MAX,
            ..CALM
        };
        // (state, held before, command, state after, held after)
        let cases = [
            (
                CALM,
                held(&[]),
                Command::Tick,
                State { frame: 8, ..CALM },
                held(&[]),
            ),
            (
                max,
                held(&[]),
                Command::Tick,
                State { frame: 0, ..CALM },
                held(&[]),
            ), // wraps
            (CALM, held(&[]), set(1, 1, 255), CALM, held(&[(1, 1, 255)])),
            (
                CALM,
                held(&[(2, 512, 9)]),
                set(2, 512, 10),
                CALM,
                held(&[(2, 512, 10)]),
            ),
            (
                CALM,
                held(&[(1, 3, 9), (1, 4, 5)]),
                release(1, 3),
                CALM,
                held(&[(1, 4, 5)]),
            ),
            (CALM, held(&[]), release(1, 3), CALM, held(&[])), // nothing held: no change
            (
                CALM,
                held(&[(1, 1, 1), (2, 2, 2)]),
                Command::ReleaseAll,
                CALM,
                held(&[]),
            ),
            (
                CALM,
                held(&[(1, 1, 80)]),
                Command::Blackout { on: true },
                DARK,
                held(&[(1, 1, 80)]),
            ),
            (
                DARK,
                held(&[(1, 1, 80)]),
                Command::Blackout { on: false },
                CALM,
                held(&[(1, 1, 80)]),
            ),
            (DARK, held(&[]), set(1, 2, 50), DARK, held(&[(1, 2, 50)])), // held in a blackout
        ];
        for (state, mut universes, command, after, held_after) in cases {
            let result = update(state, &mut universes, command);
            assert_eq!(result, (after, None), "{command:?}");
            assert_eq!(universes, held_after, "{command:?}");
        }
    }

    #[test]
    fn a_channel_on_a_shorter_buffer_changes_nothing() {
        let mut one = [Levels::ZERO];
        let command = Command::SetChannel {
            channel: channel(2, 1),
            value: 9,
        };
        assert_eq!(update(CALM, &mut one, command), (CALM, None));
        assert_eq!(one, [Levels::ZERO]);
    }

    #[test]
    fn frame_sends_the_held_levels_or_zero_in_a_blackout() {
        // (state, held, out)
        let cases = [
            (CALM, held(&[]), held(&[])),
            (
                CALM,
                held(&[(1, 1, 255), (2, 512, 3)]),
                held(&[(1, 1, 255), (2, 512, 3)]),
            ),
            (DARK, held(&[(1, 1, 255), (2, 512, 3)]), held(&[])),
        ];
        for (state, universes, expected) in cases {
            let mut out = held(&[(1, 9, 9)]); // whatever was there before
            frame(&state, &universes, &mut out);
            assert_eq!(out, expected, "{state:?}");
        }
    }

    #[test]
    fn check_turns_a_sent_command_into_one_for_this_engine() {
        let one = Universes::ONE;
        let numbers = |universe, address| ChannelNumbers { universe, address };
        // (as sent, checked)
        let cases = [
            (
                Command::SetChannel {
                    channel: numbers(1, 512),
                    value: 7,
                },
                Ok(Command::SetChannel {
                    channel: numbers(1, 512).check(one).unwrap(),
                    value: 7,
                }),
            ),
            (
                Command::SetChannel {
                    channel: numbers(2, 1),
                    value: 7,
                },
                Err(Refusal::NoSuchUniverse { universes: 1 }),
            ),
            (
                Command::ReleaseChannel {
                    channel: numbers(1, 0),
                },
                Err(Refusal::NoSuchAddress),
            ),
            (Command::ReleaseAll, Ok(Command::ReleaseAll)),
            (
                Command::Blackout { on: true },
                Ok(Command::Blackout { on: true }),
            ),
            (Command::Tick, Err(Refusal::NotACommand)),
        ];
        for (sent, expected) in cases {
            assert_eq!(sent.check(one), expected, "{sent:?}");
        }
    }

    #[test]
    fn next_frame_catches_up_when_late_and_resyncs_after_a_stall() {
        let period = Duration::from_millis(10);
        let due = Duration::from_secs(5);
        // (frames late, when the next frame is due)
        let cases = [
            (0, due + period),              // on time
            (3, due + period),              // oversleep: make it up at once
            (MAX_BEHIND, due + period),     // still catching up
            (MAX_BEHIND + 1, due + period), // the last frame it still makes up
            (20, due + period * 21),        // machine slept: start over from now
        ];
        for (late, expected) in cases {
            let now = due + period * late;
            assert_eq!(next_frame(due, now, period), expected, "{late} frames late");
        }
        assert_eq!(
            next_frame(Duration::MAX, Duration::MAX, period),
            Duration::MAX
        );
    }

    #[test]
    fn sleep_before_leaves_the_last_millisecond_to_spin() {
        let due = Duration::from_secs(5);
        // (µs before due, µs to sleep)
        let cases = [
            (22_727, 21_727), // a whole frame ahead
            (1_001, 1),
            (1_000, 0), // spin all of it
            (400, 0),
            (0, 0),
        ];
        for (ahead, sleep) in cases {
            let now = due - Duration::from_micros(ahead);
            assert_eq!(
                sleep_before(due, now),
                Duration::from_micros(sleep),
                "{ahead} µs ahead"
            );
        }
        // past due: no sleep
        assert_eq!(sleep_before(due, due * 2), Duration::ZERO);
    }

    #[test]
    fn bucket_counts_by_100_microseconds() {
        // (time in µs, bucket of 251, bucket of 501)
        let cases = [
            (0, 0, 0),
            (99, 0, 0),
            (100, 1, 1),
            (250, 2, 2),
            (9_999, 99, 99),
            (24_999, 249, 249),
            (25_000, 250, 250),
            (49_999, 250, 499),
            (u64::MAX, 250, 500),
        ];
        for (micros, lateness, latency) in cases {
            let time = Duration::from_micros(micros);
            assert_eq!(bucket::<LATENESS_BUCKETS>(time), lateness, "{micros} µs");
            assert_eq!(bucket::<LATENCY_BUCKETS>(time), latency, "{micros} µs");
        }
    }

    fn counts(counts: &[(usize, u64)]) -> Histogram<LATENESS_BUCKETS> {
        let mut histogram = [0; LATENESS_BUCKETS];
        for (bucket, count) in counts {
            histogram[*bucket] = *count;
        }
        histogram
    }

    #[test]
    fn percentile_is_the_upper_edge_of_its_bucket() {
        // (bucket and count, percent, result in µs)
        let cases = [
            (counts(&[]), 99, None),
            (counts(&[(0, 1)]), 99, Some(100)),
            (counts(&[(0, 99), (5, 1)]), 99, Some(100)),
            (counts(&[(0, 98), (5, 2)]), 99, Some(600)),
            (counts(&[(3, 1000), (100, 11)]), 99, Some(10_100)),
            (counts(&[(0, 95), (5, 5)]), 95, Some(100)),
            (counts(&[(0, 94), (5, 6)]), 95, Some(600)),
            (counts(&[(0, 1), (9, 1)]), 95, Some(1_000)),
            (counts(&[(0, u64::MAX), (1, u64::MAX)]), 99, Some(100)), // saturates, never panics
        ];
        for (histogram, percent, expected) in cases {
            assert_eq!(
                percentile(&histogram, percent),
                expected.map(Duration::from_micros),
                "p{percent} {expected:?}"
            );
        }
    }

    #[test]
    fn worst_is_the_upper_edge_of_the_latest_bucket_with_a_count() {
        // (bucket and count, worst in µs)
        let cases = [
            (counts(&[]), None),
            (counts(&[(0, 1)]), Some(100)),
            (counts(&[(0, 1000), (7, 1)]), Some(800)),
            (counts(&[(2, 5), (9, 0)]), Some(300)),
            (counts(&[(LATENESS_BUCKETS - 1, 1)]), Some(25_100)),
        ];
        for (histogram, expected) in cases {
            assert_eq!(
                worst(&histogram),
                expected.map(Duration::from_micros),
                "{expected:?}"
            );
        }
    }

    /// The UI's types for the engine API, generated from the types above.
    const TYPESCRIPT: &str = "../../ui/src/engine-api.ts";

    #[test]
    fn typescript_types_match_the_rust_types() {
        let config = Config::default();
        let generated = format!(
            "// Generated from crates/engine-core. Do not edit.\n\
             // Update: UPDATE_TS=1 cargo test -p engine-core\n\n\
             {}",
            [
                Request::decl(&config),
                Command::<ChannelNumbers>::decl(&config),
                ChannelNumbers::decl(&config),
                Reply::decl(&config),
                Refusal::decl(&config),
                State::decl(&config),
                Priority::decl(&config),
                BestEffort::decl(&config),
                Levels::decl(&config),
                ChannelLevel::decl(&config),
            ]
            .map(|declaration| format!("export {declaration}\n"))
            .join("\n"),
        );
        if std::env::var_os("UPDATE_TS").is_some() {
            std::fs::write(TYPESCRIPT, &generated).expect("write the TypeScript types");
        }
        let committed = std::fs::read_to_string(TYPESCRIPT).unwrap_or_else(|_| String::new());
        assert_eq!(
            committed, generated,
            "{TYPESCRIPT} is out of date: UPDATE_TS=1 cargo test -p engine-core"
        );
    }
}
