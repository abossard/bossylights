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

use core::time::Duration;
use serde::Serialize;

/// A snapshot of what the engine is doing now. Sent to the UI as JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct State {
    /// Frames computed since the engine started.
    #[cfg_attr(test, ts(type = "number"))]
    pub frame: u64,
}

/// What changes the state. Only the tick, for now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Command {
    Tick,
}

/// What must happen outside after an update, as data. None exist yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {}

/// The only place state changes: state and command in, new state and effects out.
/// At most one effect, so nothing allocates. A command that needs more widens this
/// to a fixed-size array, never to a `Vec`.
pub fn update(state: State, command: Command) -> (State, Option<Effect>) {
    match command {
        Command::Tick => (
            State {
                frame: state.frame.wrapping_add(1),
            },
            None,
        ),
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

/// Tick lateness is counted in buckets this wide.
pub const LATENESS_STEP: Duration = Duration::from_micros(100);
/// The number of buckets: 25 ms, about one frame. The last one holds everything later.
pub const LATENESS_BUCKETS: usize = 251;
/// How many ticks woke how late: one count per bucket of [`LATENESS_STEP`].
pub type Lateness = [u64; LATENESS_BUCKETS];

/// The bucket for a tick that woke `late` after it was due.
pub fn lateness_bucket(late: Duration) -> usize {
    let bucket = late.as_micros() / LATENESS_STEP.as_micros();
    usize::try_from(bucket)
        .unwrap_or(usize::MAX)
        .min(LATENESS_BUCKETS - 1)
}

/// The lateness 99 of 100 ticks stay under: the upper edge of the bucket that holds
/// the 99th percentile. `None` before the first tick.
pub fn p99(lateness: &Lateness) -> Option<Duration> {
    let total = lateness.iter().fold(0_u64, |sum, n| sum.saturating_add(*n));
    let rank = total - total / 100; // ceil(0.99 * total)
    let mut seen = 0_u64;
    for (bucket, count) in lateness.iter().enumerate() {
        seen = seen.saturating_add(*count);
        if total > 0 && seen >= rank {
            let edges = u32::try_from(bucket + 1).unwrap_or(u32::MAX);
            return Some(LATENESS_STEP.saturating_mul(edges));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ts_rs::{Config, TS};

    #[test]
    fn update_turns_a_command_into_a_new_state() {
        // (before, command, after, effect)
        let cases = [
            (0, Command::Tick, 1, None),
            (41, Command::Tick, 42, None),
            (u64::MAX, Command::Tick, 0, None), // wraps, never panics
        ];
        for (before, command, after, effect) in cases {
            let result = update(State { frame: before }, command);
            assert_eq!(
                result,
                (State { frame: after }, effect),
                "{before} + {command:?}"
            );
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
    fn lateness_bucket_counts_by_100_microseconds() {
        // (late in µs, bucket)
        let cases = [
            (0, 0),
            (99, 0),
            (100, 1),
            (250, 2),
            (9_999, 99),
            (24_999, 249),
            (25_000, 250),
            (u64::MAX, 250),
        ];
        for (micros, bucket) in cases {
            assert_eq!(
                lateness_bucket(Duration::from_micros(micros)),
                bucket,
                "{micros} µs"
            );
        }
    }

    #[test]
    fn p99_is_the_upper_edge_of_the_99th_percentile_bucket() {
        let ticks = |counts: &[(usize, u64)]| {
            let mut lateness = [0; LATENESS_BUCKETS];
            for (bucket, count) in counts {
                lateness[*bucket] = *count;
            }
            lateness
        };
        // (bucket and count, p99 in µs)
        let cases = [
            (ticks(&[]), None),
            (ticks(&[(0, 1)]), Some(100)),
            (ticks(&[(0, 99), (5, 1)]), Some(100)),
            (ticks(&[(0, 98), (5, 2)]), Some(600)),
            (ticks(&[(3, 1000), (100, 11)]), Some(10_100)),
            (ticks(&[(0, u64::MAX), (1, u64::MAX)]), Some(100)), // saturates, never panics
        ];
        for (lateness, expected) in cases {
            assert_eq!(
                p99(&lateness),
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
             export {}\n\nexport {}\n",
            State::decl(&config),
            Command::decl(&config),
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
