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
mod output;

pub use api::{Refusal, Reply, Request};
pub use channel::{
    ADDRESSES, Channel, ChannelLevel, ChannelNumbers, Levels, MAX_UNIVERSES, Universes, changes,
};
pub use output::{
    Color, Counters, GIVE_UP, MAX_OUTPUTS, OFF_BEAT, OUTPUT_WORDS, Outcome, Output, OutputKind,
    OutputNumber, Rate, Run, RunNumbers, Status, from_words, to_words,
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
    /// The outputs, by number. `None` where no output is set.
    #[cfg_attr(test, ts(type = "Array<Output | null>"))]
    pub outputs: [Option<Output>; MAX_OUTPUTS],
    /// When the engine last woke, since it started.
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub now: Duration,
    /// When it wakes next if no output is set: the state stays alive on this beat.
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub beat: Duration,
}

impl State {
    /// A new engine's state: nothing held, no outputs.
    pub const fn new(priority: Priority) -> Self {
        Self {
            frame: 0,
            priority,
            blackout: false,
            outputs: [None; MAX_OUTPUTS],
            now: Duration::ZERO,
            beat: BEAT,
        }
    }
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

/// What changes the state: the closed list. `C` and `R` are a [`Channel`] and a [`Run`]
/// inside the engine, and [`ChannelNumbers`] and [`RunNumbers`] as a client sends them,
/// before [`Command::check`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(
    test,
    derive(ts_rs::TS),
    ts(concrete(C = ChannelNumbers, R = RunNumbers))
)]
pub enum Command<C = Channel, R = Run> {
    /// The engine woke at `now`, since it started. Never from a client.
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    Tick { now: Duration },
    /// A frame was handed to an output. Never from a client.
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    Handed {
        output: OutputNumber,
        outcome: Outcome,
    },
    /// Hold a channel at a level.
    SetChannel { channel: C, value: u8 },
    /// Let a channel go back to zero.
    ReleaseChannel { channel: C },
    /// Let every channel go back to zero.
    ReleaseAll,
    /// Every channel out at zero while on. The held levels stay and come back when off.
    Blackout { on: bool },
    /// Add an output, or replace the one with this number. Its counters start at zero.
    /// Without a rate, the kind's default.
    SetOutput {
        output: OutputNumber,
        kind: OutputKind,
        universes: R,
        rate: Option<Rate>,
    },
    /// Remove an output. Nothing happens if there is none.
    RemoveOutput { output: OutputNumber },
}

impl Command<ChannelNumbers, RunNumbers> {
    /// The command for this engine, or why it cannot be one. Done once, at the Engine.
    pub fn check(self, universes: Universes) -> Result<Command, Refusal> {
        Ok(match self {
            Self::Tick { .. } | Self::Handed { .. } => return Err(Refusal::NotACommand),
            Self::SetChannel { channel, value } => Command::SetChannel {
                channel: channel.check(universes)?,
                value,
            },
            Self::ReleaseChannel { channel } => Command::ReleaseChannel {
                channel: channel.check(universes)?,
            },
            Self::ReleaseAll => Command::ReleaseAll,
            Self::Blackout { on } => Command::Blackout { on },
            Self::SetOutput {
                output,
                kind,
                universes: run,
                rate,
            } => Command::SetOutput {
                output,
                kind,
                universes: run.check(universes)?,
                rate,
            },
            Self::RemoveOutput { output } => Command::RemoveOutput { output },
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
        Command::Tick { now } => (
            State {
                frame: state.frame.wrapping_add(1),
                now,
                beat: next_frame(state.beat, now, BEAT),
                ..state
            },
            None,
        ),
        Command::Handed { output, outcome } => {
            let mut outputs = state.outputs;
            if let Some(Some(handed)) = outputs.get_mut(output.slot()) {
                *handed = handed.handed(outcome, state.now);
            }
            (State { outputs, ..state }, None)
        }
        Command::SetOutput {
            output,
            kind,
            universes: run,
            rate,
        } => {
            let mut outputs = state.outputs;
            if let Some(slot) = outputs.get_mut(output.slot()) {
                let rate = rate.unwrap_or(kind.default_rate());
                *slot = Some(Output::new(kind, run, rate, state.now));
            }
            (State { outputs, ..state }, None)
        }
        Command::RemoveOutput { output } => {
            let mut outputs = state.outputs;
            if let Some(slot) = outputs.get_mut(output.slot()) {
                *slot = None;
            }
            (State { outputs, ..state }, None)
        }
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

/// The levels that go out at `_at` (since the engine started), written into `out`, a
/// buffer the caller owns: the held levels, or zero in a blackout. No level depends on
/// time yet; effects will.
pub fn frame(state: &State, universes: &[Levels], _at: Duration, out: &mut [Levels]) {
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

/// With no output set, the engine still wakes 44 times a second, so the state stays alive.
pub const BEAT: Duration = Duration::from_nanos(1_000_000_000 / 44);

/// When the engine wakes next: when the first output is due, or on the beat if none is set.
pub fn next_wake(state: &State) -> Duration {
    state
        .outputs
        .iter()
        .flatten()
        .map(|output| output.due)
        .min()
        .unwrap_or(state.beat)
}

/// The outputs due at `now`, by number.
pub fn due(state: &State, now: Duration) -> impl Iterator<Item = OutputNumber> + '_ {
    (1..)
        .zip(&state.outputs)
        .filter(move |(_, output)| output.is_some_and(|output| output.due <= now))
        .filter_map(|(number, _)| OutputNumber::try_from(number).ok())
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
        ..State::new(Priority::RealTime)
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

    fn tick(ms: u64) -> Command {
        Command::Tick {
            now: Duration::from_millis(ms),
        }
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
                tick(0),
                State {
                    frame: 8,
                    beat: BEAT * 2,
                    ..CALM
                },
                held(&[]),
            ),
            (
                max,
                held(&[]),
                tick(0),
                State {
                    frame: 0,
                    beat: BEAT * 2,
                    ..CALM
                },
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
            frame(&state, &universes, Duration::ZERO, &mut out);
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
            (
                Command::Tick {
                    now: Duration::ZERO,
                },
                Err(Refusal::NotACommand),
            ),
            (
                Command::SetOutput {
                    output: OutputNumber::try_from(1).unwrap(),
                    kind: OutputKind::Mock,
                    universes: RunNumbers { first: 1, count: 2 },
                    rate: None,
                },
                Err(Refusal::NoSuchUniverse { universes: 1 }),
            ),
        ];
        for (sent, expected) in cases {
            assert_eq!(sent.check(one), expected, "{sent:?}");
        }
    }

    fn output(number: u8) -> OutputNumber {
        OutputNumber::try_from(number).unwrap()
    }

    fn set_output(number: u8, rate: Rate) -> Command {
        Command::SetOutput {
            output: output(number),
            kind: OutputKind::Mock,
            universes: RunNumbers { first: 1, count: 1 }
                .check(Universes::ONE)
                .unwrap(),
            rate: Some(rate),
        }
    }

    /// Per wake: the time in ms and the outputs served.
    type Wakes = Vec<(u64, Vec<u8>)>;

    /// Runs the schedule like the output thread does, on time: wake, hand every due
    /// output a frame. Returns, per wake, the time in ms and the outputs served.
    fn run(mut state: State, until_ms: u64, mut at: impl FnMut(u64) -> Vec<Command>) -> Wakes {
        let mut wakes = vec![];
        let mut universes = [Levels::ZERO];
        loop {
            let now = next_wake(&state);
            let ms = u64::try_from(now.as_millis()).unwrap();
            if ms > until_ms {
                return wakes;
            }
            state = update(state, &mut universes, Command::Tick { now }).0;
            for command in at(ms) {
                state = update(state, &mut universes, command).0;
            }
            let served: Vec<u8> = due(&state, now).map(|n| n.slot() as u8 + 1).collect();
            for &number in &served {
                let handed = Command::Handed {
                    output: output(number),
                    outcome: Outcome::Accepted,
                };
                state = update(state, &mut universes, handed).0;
            }
            wakes.push((ms, served));
        }
    }

    #[test]
    fn the_engine_wakes_when_the_next_output_is_due() {
        let calm = State::new(Priority::RealTime);
        let (thirty, forty_four, sixty) = (Rate::Thirty, Rate::FortyFour, Rate::Sixty);
        // (outputs set on the first wake, until ms, wakes: ms and outputs served)
        // 60 and 30 a second meet at 56.06 ms, 1 ns apart: two wakes.
        let cases: [(Vec<Command>, u64, Wakes); 4] = [
            (vec![], 70, vec![(22, vec![]), (45, vec![]), (68, vec![])]), // the beat
            (
                vec![set_output(1, sixty)],
                40,
                vec![(22, vec![1]), (39, vec![1])],
            ),
            (
                vec![set_output(1, sixty), set_output(2, thirty)],
                60,
                vec![
                    (22, vec![1, 2]),
                    (39, vec![1]),
                    (56, vec![1]),
                    (56, vec![2]),
                ],
            ),
            (
                vec![
                    set_output(1, sixty),
                    set_output(2, thirty),
                    set_output(3, forty_four),
                ],
                60,
                vec![
                    (22, vec![1, 2, 3]),
                    (39, vec![1]),
                    (45, vec![3]),
                    (56, vec![1]),
                    (56, vec![2]),
                ],
            ),
        ];
        for (commands, until, expected) in cases {
            let mut first = Some(commands.clone());
            let wakes = run(calm, until, |_| first.take().unwrap_or_default());
            assert_eq!(wakes, expected, "{commands:?}");
        }
    }

    #[test]
    fn outputs_added_or_removed_while_running_change_the_wakes() {
        let calm = State::new(Priority::RealTime);
        let remove = Command::RemoveOutput { output: output(1) };
        let wakes = run(calm, 100, |ms| match ms {
            22 => vec![set_output(1, Rate::Sixty)],
            39 => vec![set_output(2, Rate::Thirty)],
            56 => vec![remove],
            _ => vec![],
        });
        let expected = vec![
            (22, vec![1]),    // added on the beat: served at once
            (39, vec![1, 2]), // the second one too
            (56, vec![]),     // 1 removed before it was served
            (72, vec![2]),    // only 2 is left, on its own rate; no beat while it is set
        ];
        assert_eq!(wakes, expected);
    }

    #[test]
    fn after_a_long_stall_each_output_starts_over_from_now() {
        let mut universes = [Levels::ZERO];
        let mut state = State::new(Priority::RealTime);
        state = update(state, &mut universes, set_output(1, Rate::Sixty)).0;
        state = update(state, &mut universes, set_output(2, Rate::Thirty)).0;
        // the machine slept for a second
        let now = Duration::from_secs(1);
        state = update(state, &mut universes, Command::Tick { now }).0;
        let served: Vec<_> = due(&state, now).collect();
        assert_eq!(served, vec![output(1), output(2)]);
        for number in served {
            let handed = Command::Handed {
                output: number,
                outcome: Outcome::Accepted,
            };
            state = update(state, &mut universes, handed).0;
        }
        assert_eq!(next_wake(&state), now + Rate::Sixty.period());
        let thirty = state.outputs[1].unwrap();
        assert_eq!(thirty.due, now + Rate::Thirty.period());
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
                Command::<ChannelNumbers, RunNumbers>::decl(&config),
                ChannelNumbers::decl(&config),
                RunNumbers::decl(&config),
                OutputNumber::decl(&config),
                OutputKind::decl(&config),
                Color::decl(&config),
                Rate::decl(&config),
                Reply::decl(&config),
                Refusal::decl(&config),
                State::decl(&config),
                Priority::decl(&config),
                BestEffort::decl(&config),
                Levels::decl(&config),
                ChannelLevel::decl(&config),
                Output::decl(&config),
                Run::decl(&config),
                Status::decl(&config),
                Counters::decl(&config),
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
