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
mod kinds;
mod library;
mod mix;
pub use mix::{Held, show, wins};
use mix::{Want, mix};
mod output;
mod patch;
mod playback;
mod project;
mod property;
mod resolve;
mod show;

pub use api::{API_VERSION, MATCHES, Refusal, Reply, Request};
pub use channel::{
    ADDRESSES, Channel, ChannelLevel, ChannelNumbers, ChannelSet, Levels, MAX_UNIVERSES, Universes,
    changes,
};
pub use kinds::{Adapter, Color, OutputKind};
pub use library::{
    ChannelFunction, Failure, FixtureKind, Library, Match, ModeChannels, ModeSize, NamedChannel,
    Text,
};
pub use output::{
    Counters, MAX_OUTPUTS, OFF_BEAT, Outcome, Output, OutputNumber, Rate, Run, RunNumbers, Status,
};
pub use patch::{
    Grouped, Layout, Member, MemberList, Members, PatchCommand, Patched, Pixels, Target,
};
pub use playback::{Entry, LOOKS, LookCommand, PLAYS, Play, PlayId, Plays, Saved, Step};
pub use project::{Project, ProjectEffect, ProjectEvent, ProjectOutput};
pub use property::{Gobo, Light, Position, Property, Rgb};
pub use resolve::{
    HeldLight, Less, LightNumber, LightRun, Lights, MAX_LIGHTS, Part, Place, Scale, Sense, Take,
    Writer, held_lights, light_changes, resolve,
};
pub use show::{ShowFile, Unopened};

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
    /// The looks playing, by slot.
    #[cfg_attr(test, ts(type = "Array<Play>"))]
    pub plays: Plays,
    /// The start order the next hold or play takes: a later start wins.
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub order: u64,
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
            plays: Plays::NONE,
            order: 1,
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

/// What a client may send: the closed list, numbers as sent. The engine front checks it
/// against the project ([`Project::update`]); only that makes an [`EngineCommand`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[expect(
    clippy::large_enum_variant,
    reason = "a client's command has no heap: `Patch` carries its four names in place, `SetGroup` its members. Moved once per request, off the tick thread"
)]
pub enum Command {
    /// Hold a channel at a level. It wins over the light that writes it.
    SetChannel { channel: ChannelNumbers, value: u8 },
    /// Let a channel go: the light that writes it shows again, else zero.
    ReleaseChannel { channel: ChannelNumbers },
    /// Let every channel go.
    ReleaseAll,
    /// Every channel out at zero while on. The held levels stay and come back when off.
    Blackout { on: bool },
    /// Add an output, or replace the one with this number. Its counters start at zero.
    /// Without a rate, the kind's default.
    SetOutput {
        output: OutputNumber,
        kind: OutputKind,
        universes: RunNumbers,
        rate: Option<Rate>,
    },
    /// Remove an output. Nothing happens if there is none.
    RemoveOutput { output: OutputNumber },
    /// Add a fixture of the library in one of its modes, from `at` on. The reply carries
    /// its number. Another fixture on the same channels is allowed: the patch shows it.
    Patch {
        #[cfg_attr(test, ts(type = "string"))]
        name: Text,
        #[cfg_attr(test, ts(type = "string"))]
        maker: Text,
        #[cfg_attr(test, ts(type = "string"))]
        model: Text,
        #[cfg_attr(test, ts(type = "string"))]
        mode: Text,
        at: ChannelNumbers,
    },
    /// Add a generic strip of `pixels` pixels, each red, green, blue (and white for
    /// `Rgbw`), from `at` on: one light per pixel, no definition file. As `Patch` otherwise.
    PatchStrip {
        #[cfg_attr(test, ts(type = "string"))]
        name: Text,
        pixels: Pixels,
        color: Color,
        at: ChannelNumbers,
    },
    /// Give a fixture a new address. Its lights keep their numbers and held values.
    Move { fixture: u16, at: ChannelNumbers },
    /// Remove a fixture. Its lights' held values go.
    Unpatch { fixture: u16 },
    Rename {
        fixture: u16,
        #[cfg_attr(test, ts(type = "string"))]
        name: Text,
    },
    /// Hold the intensity of a light, or of every light of a fixture or a group. 0 to
    /// 65 535 is 0 to 1.
    SetIntensity { target: Target, intensity: u16 },
    /// Hold the colour of a light, or of every light of a fixture or a group. 16 bits each.
    SetColour {
        target: Target,
        red: u16,
        green: u16,
        blue: u16,
    },
    /// Let the intensity of a light, or of every light of a fixture, go: its dimmer shows the
    /// definition's default again, else 0.
    ReleaseIntensity { target: Target },
    /// Let the colour of a light, or of every light of a fixture, go: its colour channels
    /// show the definition's defaults again, else black.
    ReleaseColour { target: Target },
    /// Make a group, or replace the one of this name. Its members are lights or fixtures,
    /// in the order given, each patched. Unpatching a fixture takes it out of its groups.
    SetGroup {
        #[cfg_attr(test, ts(type = "string"))]
        name: Text,
        #[cfg_attr(test, ts(type = "Array<Member>"))]
        members: MemberList,
    },
    /// Remove a group. Nothing happens if there is none.
    RemoveGroup {
        #[cfg_attr(test, ts(type = "string"))]
        name: Text,
    },
    /// Save `step` as look `look` named `name`, in place of the one saved there. A
    /// playing look shows the new step in the next frame and keeps its start.
    PutStep {
        look: u16,
        #[cfg_attr(test, ts(type = "string"))]
        name: Text,
        step: Step,
    },
    /// Remove a saved look that no play uses. Nothing happens if there is none.
    RemoveLook { look: u16 },
    /// Play a saved look at Manual rank: answered `Started` with its play. A look playing
    /// already starts afresh, as the latest start.
    StartLook { look: u16 },
    /// End a play: what it held shows what is underneath in the next frame. An ended
    /// play is a harmless no-op.
    ReleaseHold { play: PlayId },
}

/// What changes the state on the tick thread. The front makes these from a checked
/// [`Command`]; the tick thread makes `Tick`, `Handed` and `Sent` itself. No kind gets here: a
/// device is opened first and arrives with `Attach`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineCommand {
    /// The engine woke at `now`, since it started.
    Tick {
        now: Duration,
    },
    /// A frame went to an output's own thread at `at`, since the engine started: its
    /// next frame is due a period on.
    Handed {
        output: OutputNumber,
        at: Duration,
    },
    /// What came of a frame on an output's own thread: its device was handed it at `at`,
    /// or it was dropped then. Moves the output's state and counters.
    Sent {
        output: OutputNumber,
        outcome: Outcome,
        at: Duration,
    },
    SetChannel {
        channel: Channel,
        value: u8,
    },
    ReleaseChannel {
        channel: Channel,
    },
    ReleaseAll,
    Blackout {
        on: bool,
    },
    /// An opened device takes the place of `output`, or the device there stays and only
    /// its run and rate change. Its counters start at zero.
    Attach {
        output: OutputNumber,
        universes: Run,
        rate: Rate,
    },
    RemoveOutput {
        output: OutputNumber,
    },
    /// The writers of the patch arrive with this command and take the place of the ones
    /// there, which go back to be freed. No light value changes.
    Writers,
    SetIntensity {
        lights: LightRun,
        intensity: u16,
    },
    SetColour {
        lights: LightRun,
        red: u16,
        green: u16,
        blue: u16,
    },
    ReleaseIntensity {
        lights: LightRun,
    },
    ReleaseColour {
        lights: LightRun,
    },
    /// The lights of an unpatched fixture: their held values go.
    Darken {
        lights: LightRun,
    },
    /// The runs of a group arrive with this command: each takes `change`, all in the
    /// same wake, so one frame shows all of them or none. Alone it changes nothing.
    Runs {
        change: LightChange,
    },
    /// `play` of `look` starts in its slot, in place of the play there. Its plan arrives
    /// with this command and takes the place of the slot's plan.
    Start {
        play: PlayId,
        look: u16,
    },
    /// The plan of `play` arrives with this command: the play keeps its start order.
    Plan {
        play: PlayId,
    },
    /// `play` ends; its plan goes back to be freed.
    Release {
        play: PlayId,
    },
}

/// What a light command does to each light it names, without naming them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightChange {
    SetIntensity { intensity: u16 },
    SetColour { red: u16, green: u16, blue: u16 },
    ReleaseIntensity,
    ReleaseColour,
}

impl LightChange {
    /// The command that makes this change to the lights of `run`.
    pub const fn on(self, run: LightRun) -> EngineCommand {
        match self {
            Self::SetIntensity { intensity } => EngineCommand::SetIntensity {
                lights: run,
                intensity,
            },
            Self::SetColour { red, green, blue } => EngineCommand::SetColour {
                lights: run,
                red,
                green,
                blue,
            },
            Self::ReleaseIntensity => EngineCommand::ReleaseIntensity { lights: run },
            Self::ReleaseColour => EngineCommand::ReleaseColour { lights: run },
        }
    }
}

/// The only place state changes: state and command in, new state out. `universes` are
/// the levels of the raw channels, `holds` which of them are held (a channel not held is
/// at 0), `lights` the held values per light: buffers the caller owns; a command changes
/// them in place, so nothing allocates. No command needs anything done outside yet: the
/// first that does returns it as data beside the state, never in a `Vec`.
pub fn update(
    state: State,
    universes: &mut [Levels],
    holds: &mut [ChannelSet],
    lights: &mut [Held],
    command: EngineCommand,
) -> State {
    let held = State {
        order: state.order.saturating_add(1),
        ..state
    };
    match command {
        EngineCommand::Tick { now } => State {
            frame: state.frame.wrapping_add(1),
            now,
            // Only a due beat moves on: a wake for an output must not push it ahead.
            beat: if now < state.beat {
                state.beat
            } else {
                next_frame(state.beat, now, BEAT)
            },
            ..state
        },
        EngineCommand::Handed { output, at } => {
            let mut outputs = state.outputs;
            if let Some(Some(handed)) = outputs.get_mut(output.slot()) {
                *handed = handed.handed(at);
            }
            State { outputs, ..state }
        }
        EngineCommand::Sent {
            output,
            outcome,
            at,
        } => {
            let mut outputs = state.outputs;
            if let Some(Some(sent)) = outputs.get_mut(output.slot()) {
                *sent = sent.sent(outcome, at);
            }
            State { outputs, ..state }
        }
        EngineCommand::Attach {
            output,
            universes: run,
            rate,
        } => {
            let mut outputs = state.outputs;
            let due = first_due(&outputs, rate, state.now);
            if let Some(slot) = outputs.get_mut(output.slot()) {
                *slot = Some(Output::new(run, rate, due));
            }
            State { outputs, ..state }
        }
        EngineCommand::RemoveOutput { output } => {
            let mut outputs = state.outputs;
            if let Some(slot) = outputs.get_mut(output.slot()) {
                *slot = None;
            }
            State { outputs, ..state }
        }
        EngineCommand::SetChannel { channel, value } => {
            if let Some(level) = channel.level(universes) {
                *level = value;
                channel.mark(holds, true);
            }
            state
        }
        EngineCommand::ReleaseChannel { channel } => {
            if let Some(level) = channel.level(universes) {
                *level = 0;
            }
            channel.mark(holds, false);
            state
        }
        EngineCommand::ReleaseAll => {
            universes.fill(Levels::ZERO);
            holds.fill(ChannelSet::NONE);
            state
        }
        EngineCommand::Blackout { on } => State {
            blackout: on,
            ..state
        },
        EngineCommand::Writers | EngineCommand::Runs { .. } | EngineCommand::Plan { .. } => state,
        EngineCommand::SetIntensity {
            lights: run,
            intensity,
        } => {
            let value = Light {
                intensity: Some(intensity),
                ..Light::DARK
            };
            for light in run.of(lights) {
                light.hold(Property::Intensity, &value, state.order);
            }
            held
        }
        EngineCommand::SetColour {
            lights: run,
            red,
            green,
            blue,
        } => {
            let value = Light {
                colour: Some(Rgb { red, green, blue }),
                ..Light::DARK
            };
            for light in run.of(lights) {
                light.hold(Property::Colour, &value, state.order);
            }
            held
        }
        EngineCommand::ReleaseIntensity { lights: run } => {
            for light in run.of(lights) {
                light.release(Property::Intensity);
            }
            state
        }
        EngineCommand::ReleaseColour { lights: run } => {
            for light in run.of(lights) {
                light.release(Property::Colour);
            }
            state
        }
        EngineCommand::Darken { lights: run } => {
            run.of(lights).fill(Held::NONE);
            state
        }
        EngineCommand::Start { play, look } => {
            let mut plays = state.plays;
            plays.start(play, look, state.order);
            State { plays, ..held }
        }
        EngineCommand::Release { play } => {
            let mut plays = state.plays;
            plays.release(play);
            State { plays, ..state }
        }
    }
}

/// The levels that go out now, written into `out`, a buffer the caller owns. It
/// resolves and mixes, nothing else: what the `writers` make of the `lights`, the raw
/// channels held (`universes` at `holds`), the blackout. `mix` decides each channel. No
/// level depends on time yet: the first that does takes the time as an argument.
pub fn frame(
    state: &State,
    universes: &[Levels],
    holds: &[ChannelSet],
    lights: &[Light],
    writers: &[Writer],
    out: &mut [Levels],
) {
    let mut lit = [ChannelSet::NONE; MAX_UNIVERSES as usize];
    out.fill(Levels::ZERO);
    resolve::write(lights, writers, out, &mut lit);
    let raw = universes.iter().zip(holds);
    for ((out, lit), (held, holds)) in out.iter_mut().zip(&lit).zip(raw) {
        for (address, (out, held)) in out.0.iter_mut().zip(held.0).enumerate() {
            let mut want = if lit.has(address) {
                Want::Light(*out)
            } else {
                Want::Default(*out)
            };
            if holds.has(address) {
                want = mix(want, Want::Raw(held));
            }
            if state.blackout {
                want = mix(want, Want::Blackout);
            }
            *out = want.level();
        }
    }
}

/// How many frames late the tick may fall before it gives up catching up.
const MAX_BEHIND: u32 = 10;

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
fn next_wake(state: &State) -> Duration {
    state
        .outputs
        .iter()
        .flatten()
        .map(|output| output.due)
        .min()
        .unwrap_or(state.beat)
}

/// When a new output of `rate` is first due: at the next frame of an output that runs at
/// that rate already, so they go out in one wake. At `now` if none does.
fn first_due(outputs: &[Option<Output>], rate: Rate, now: Duration) -> Duration {
    outputs
        .iter()
        .flatten()
        .filter(|output| output.rate == rate)
        .map(|output| output.due)
        .min()
        .unwrap_or(now)
}

/// The outputs to serve at `now`, by number: those due, and those due within [`SPIN`]
/// after. A wake costs a spin, so outputs due that close together go out in one.
pub fn due(state: &State, now: Duration) -> impl Iterator<Item = OutputNumber> + '_ {
    let soon = now.saturating_add(SPIN);
    (1..)
        .zip(&state.outputs)
        .filter(move |(_, output)| output.is_some_and(|output| output.due <= soon))
        .filter_map(|(number, _)| OutputNumber::try_from(number).ok())
}

/// The output thread sleeps until this long before a frame is due, then spins: a
/// sleep can wake late, a spin cannot. Measured in spike #23.
const SPIN: Duration = Duration::from_millis(1);

/// How long to sleep at `now` before spinning until `due`. All times since the engine started.
pub fn sleep_before(due: Duration, now: Duration) -> Duration {
    due.saturating_sub(now).saturating_sub(SPIN)
}

/// How the tick thread waits at `now` for its next wake: it sleeps this long, then spins
/// until the time returned, if any. Only a wake that serves an output spins. On the beat
/// with no output set nothing goes out, so it only sleeps.
pub fn wait(state: &State, now: Duration) -> (Duration, Option<Duration>) {
    let wake = next_wake(state);
    if state.outputs.iter().all(Option::is_none) {
        return (wake.saturating_sub(now), None);
    }
    (sleep_before(wake, now), Some(wake))
}

/// Times are counted in buckets this wide.
const BUCKET: Duration = Duration::from_micros(100);
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
/// holds that percentile, never past `worst`, the latest time counted. In the last
/// bucket, which holds everything later, it is `worst`: the histogram cannot say more.
/// `None` before the first count.
pub fn percentile<const N: usize>(
    histogram: &Histogram<N>,
    percent: u64,
    worst: Duration,
) -> Option<Duration> {
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
            let last = bucket + 1 == N;
            return Some(if last { worst } else { edge(bucket).min(worst) });
        }
    }
    None
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

    /// Two universes, with these (universe, address, level) set; counted from 1.
    fn held(levels: &[(u16, u16, u8)]) -> [Levels; 2] {
        let mut universes = [Levels::ZERO; 2];
        for (universe, address, level) in levels {
            universes[usize::from(*universe - 1)].0[usize::from(*address - 1)] = *level;
        }
        universes
    }

    /// The raw channels of two universes, these (universe, address, level) held.
    fn raw(levels: &[(u16, u16, u8)]) -> ([Levels; 2], [ChannelSet; 2]) {
        let mut holds = [ChannelSet::NONE; 2];
        for (universe, address, _) in levels {
            channel(*universe, *address).mark(&mut holds, true);
        }
        (held(levels), holds)
    }

    fn channel(universe: u16, address: u16) -> Channel {
        ChannelNumbers { universe, address }
            .check(Universes::new(2).unwrap())
            .unwrap()
    }

    fn tick(ms: u64) -> EngineCommand {
        EngineCommand::Tick {
            now: Duration::from_millis(ms),
        }
    }

    #[test]
    fn update_turns_a_command_into_a_new_state() {
        let set = |universe, address, value| EngineCommand::SetChannel {
            channel: channel(universe, address),
            value,
        };
        let release = |universe, address| EngineCommand::ReleaseChannel {
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
                raw(&[]),
                tick(0),
                State { frame: 8, ..CALM },
                raw(&[]),
            ), // the beat is not due yet: it stays
            (
                CALM,
                raw(&[]),
                EngineCommand::Tick { now: BEAT },
                State {
                    frame: 8,
                    now: BEAT,
                    beat: BEAT * 2,
                    ..CALM
                },
                raw(&[]),
            ), // the beat is due: it moves on
            (max, raw(&[]), tick(0), State { frame: 0, ..CALM }, raw(&[])), // wraps
            (CALM, raw(&[]), set(1, 1, 255), CALM, raw(&[(1, 1, 255)])),
            (
                CALM,
                raw(&[(2, 512, 9)]),
                set(2, 512, 10),
                CALM,
                raw(&[(2, 512, 10)]),
            ),
            (
                CALM,
                raw(&[(1, 3, 9), (1, 4, 5)]),
                release(1, 3),
                CALM,
                raw(&[(1, 4, 5)]),
            ),
            (CALM, raw(&[]), release(1, 3), CALM, raw(&[])), // nothing held: no change
            (
                CALM,
                raw(&[(1, 1, 1), (2, 2, 2)]),
                EngineCommand::ReleaseAll,
                CALM,
                raw(&[]),
            ),
            (
                CALM,
                raw(&[(1, 1, 80)]),
                EngineCommand::Blackout { on: true },
                DARK,
                raw(&[(1, 1, 80)]),
            ),
            (
                DARK,
                raw(&[(1, 1, 80)]),
                EngineCommand::Blackout { on: false },
                CALM,
                raw(&[(1, 1, 80)]),
            ),
            (DARK, raw(&[]), set(1, 2, 50), DARK, raw(&[(1, 2, 50)])), // held in a blackout
            (CALM, raw(&[]), set(1, 2, 0), CALM, raw(&[(1, 2, 0)])),   // held at 0 is held
            (CALM, raw(&[(1, 2, 0)]), release(1, 2), CALM, raw(&[])),
            (
                CALM,
                raw(&[(1, 2, 0)]),
                release(1, 3),
                CALM,
                raw(&[(1, 2, 0)]),
            ),
        ];
        for (state, (mut universes, mut holds), command, after, raw_after) in cases {
            let result = update(state, &mut universes, &mut holds, &mut [], command);
            assert_eq!(result, after, "{command:?}");
            assert_eq!((universes, holds), raw_after, "{command:?}");
        }
    }

    #[test]
    fn a_light_command_changes_the_values_of_its_lights() {
        let all = Lights::new(4).unwrap();
        let run = |first, count| {
            LightRun::new(LightNumber::new(first, all).unwrap(), count, all).unwrap()
        };
        let red = Light {
            intensity: None,
            colour: Some(Rgb {
                red: 0xffff,
                green: 0,
                blue: 0,
            }),
            ..Light::DARK
        };
        let lit = Light {
            intensity: Some(0x8000),
            ..Light::DARK
        };
        let held_black = Light {
            colour: Some(Rgb {
                red: 0,
                green: 0,
                blue: 0,
            }),
            ..Light::DARK
        };
        let dark = Light::DARK;
        let set_red = |lights| EngineCommand::SetColour {
            lights,
            red: 0xffff,
            green: 0,
            blue: 0,
        };
        let half = |lights| EngineCommand::SetIntensity {
            lights,
            intensity: 0x8000,
        };
        let full_red = Light {
            intensity: Some(0x8000),
            ..red
        };
        let let_go = |lights| EngineCommand::ReleaseIntensity { lights };
        let black = |lights| EngineCommand::ReleaseColour { lights };
        // (lights before, command, lights after)
        let cases = [
            ([dark; 4], set_red(run(1, 2)), [dark, red, red, dark]),
            ([dark; 4], half(run(3, 1)), [dark, dark, dark, lit]),
            // black set is held black, not nothing held
            (
                [dark; 4],
                EngineCommand::SetColour {
                    lights: run(0, 1),
                    red: 0,
                    green: 0,
                    blue: 0,
                },
                [held_black, dark, dark, dark],
            ),
            // intensity and colour are held on their own
            ([red; 4], half(run(0, 1)), [full_red, red, red, red]),
            ([lit; 4], set_red(run(0, 4)), [full_red; 4]),
            (
                [full_red; 4],
                EngineCommand::Darken { lights: run(1, 3) },
                [full_red, dark, dark, dark],
            ),
            ([red; 4], EngineCommand::Writers, [red; 4]),
            // a release lets one property go back to its default, on its own
            (
                [full_red; 4],
                let_go(run(1, 2)),
                [full_red, red, red, full_red],
            ),
            (
                [full_red; 4],
                black(run(0, 1)),
                [lit, full_red, full_red, full_red],
            ),
            ([full_red; 4], black(run(0, 4)), [lit; 4]),
            // releasing what is not held does nothing
            ([red; 4], let_go(run(0, 4)), [red; 4]),
            ([lit; 4], black(run(0, 4)), [lit; 4]),
            ([dark; 4], let_go(run(0, 4)), [dark; 4]),
            ([dark; 4], black(run(0, 4)), [dark; 4]),
        ];
        for (before, command, after) in cases {
            let mut lights = before.map(|values| {
                let mut held = Held::NONE;
                for property in Property::ALL.into_iter().filter(|p| values.has(*p)) {
                    held.hold(property, &values, 1);
                }
                held
            });
            let (mut universes, mut holds) = raw(&[(1, 1, 5)]);
            let state = update(CALM, &mut universes, &mut holds, &mut lights, command);
            assert_eq!(
                State {
                    order: CALM.order,
                    ..state
                },
                CALM,
                "{command:?}"
            );
            assert_eq!(lights.map(|held| held.values), after, "{command:?}");
            assert_eq!((universes, holds), raw(&[(1, 1, 5)]), "{command:?}");
        }
    }

    #[test]
    fn a_light_run_stays_in_the_lights_of_the_engine() {
        let all = Lights::new(4).unwrap();
        let number = |n| LightNumber::new(n, all).unwrap();
        // (first, count, made)
        let cases = [
            (0, 4, true),
            (3, 1, true),
            (3, 2, false),
            (0, 0, false),
            (2, u16::MAX, false),
        ];
        for (first, count, made) in cases {
            assert_eq!(
                LightRun::new(number(first), count, all).is_some(),
                made,
                "{first} {count}"
            );
        }
        // a shorter buffer of values: only the lights it has
        let mut two = [Light::DARK; 2];
        assert_eq!(
            LightRun::new(number(1), 3, all).unwrap().of(&mut two).len(),
            1
        );
    }

    #[test]
    fn a_channel_on_a_shorter_buffer_changes_nothing() {
        let (mut one, mut holds) = ([Levels::ZERO], [ChannelSet::NONE]);
        let command = EngineCommand::SetChannel {
            channel: channel(2, 1),
            value: 9,
        };
        assert_eq!(update(CALM, &mut one, &mut holds, &mut [], command), CALM);
        assert_eq!((one, holds), ([Levels::ZERO], [ChannelSet::NONE]));
    }

    #[test]
    fn frame_resolves_the_lights_and_mix_decides_each_channel() {
        let all = Lights::new(1).unwrap();
        let light = LightNumber::new(0, all).unwrap();
        let takes = |of, address| Writer::Takes {
            light,
            take: Take {
                of,
                ..Take::INTENSITY
            },
            at: Place::Byte(channel(1, address)),
            unheld: 0,
        };
        let shutter = Writer::Fixed {
            at: channel(1, 4),
            value: 200,
        };
        // a PAR at 1: red, green, blue, a shutter open at 200
        let par = [
            takes(Part::Red, 1),
            takes(Part::Green, 2),
            takes(Part::Blue, 3),
            shutter,
        ];
        // two fixtures on channel 1: a light value and a default
        let light_then_default = [
            takes(Part::Red, 1),
            Writer::Fixed {
                at: channel(1, 1),
                value: 100,
            },
        ];
        let default_then_light = [
            Writer::Fixed {
                at: channel(1, 1),
                value: 100,
            },
            takes(Part::Red, 1),
        ];
        let red = [Light {
            colour: Some(Rgb {
                red: 0xffff,
                green: 0,
                blue: 0,
            }),
            ..Light::DARK
        }];
        let lit = |more: &[(u16, u16, u8)]| {
            let mut levels = vec![(1, 1, 255), (1, 4, 200)];
            levels.extend_from_slice(more);
            held(&levels)
        };
        // (row, state, raw held, writers, out)
        type Case<'a> = (
            &'a str,
            State,
            &'a [(u16, u16, u8)],
            &'a [Writer],
            [Levels; 2],
        );
        let cases: [Case; 13] = [
            ("nothing", CALM, &[], &[], held(&[])),
            (
                "raw only",
                CALM,
                &[(1, 1, 255), (2, 512, 3)],
                &[],
                held(&[(1, 1, 255), (2, 512, 3)]),
            ),
            ("the light", CALM, &[], &par, lit(&[])),
            (
                "2 raw over the light",
                CALM,
                &[(1, 2, 9)],
                &par,
                lit(&[(1, 2, 9)]),
            ),
            (
                "2 raw at 0 over the light",
                CALM,
                &[(1, 1, 0)],
                &par,
                held(&[(1, 4, 200)]),
            ),
            (
                "2 raw over a default",
                CALM,
                &[(1, 4, 7)],
                &par,
                held(&[(1, 1, 255), (1, 4, 7)]),
            ),
            (
                "2 raw beside the light",
                CALM,
                &[(2, 512, 3)],
                &par,
                lit(&[(2, 512, 3)]),
            ),
            ("1 blackout", DARK, &[], &par, held(&[])),
            (
                "1 blackout over raw",
                DARK,
                &[(1, 2, 9), (2, 512, 3)],
                &par,
                held(&[]),
            ),
            ("1 blackout, raw only", DARK, &[(1, 1, 255)], &[], held(&[])),
            (
                "3 a light over a default",
                CALM,
                &[],
                &light_then_default,
                held(&[(1, 1, 255)]),
            ),
            (
                "3 a light over a default",
                CALM,
                &[],
                &default_then_light,
                held(&[(1, 1, 255)]),
            ),
            (
                "2 raw over both",
                CALM,
                &[(1, 1, 9)],
                &light_then_default,
                held(&[(1, 1, 9)]),
            ),
        ];
        for (row, state, raw_held, writers, expected) in cases {
            let (universes, holds) = raw(raw_held);
            let mut out = held(&[(1, 9, 9)]); // whatever was there before
            frame(&state, &universes, &holds, &red, writers, &mut out);
            assert_eq!(out, expected, "{row}");
        }
    }

    fn output(number: u8) -> OutputNumber {
        OutputNumber::try_from(number).unwrap()
    }

    fn set_output(number: u8, rate: Rate) -> EngineCommand {
        EngineCommand::Attach {
            output: output(number),
            universes: RunNumbers { first: 1, count: 1 }
                .check(Universes::ONE)
                .unwrap(),
            rate,
        }
    }

    /// Per wake: the time in ms and the outputs served.
    type Wakes = Vec<(u64, Vec<u8>)>;

    /// Runs the schedule like the output thread does, on time: wake, hand every due
    /// output a frame. Returns, per wake, the time in ms and the outputs served.
    fn run(
        mut state: State,
        until_ms: u64,
        mut at: impl FnMut(u64) -> Vec<EngineCommand>,
    ) -> Wakes {
        let mut wakes = vec![];
        let mut universes = [Levels::ZERO];
        loop {
            let now = next_wake(&state);
            let ms = u64::try_from(now.as_millis()).unwrap();
            if ms > until_ms {
                return wakes;
            }
            state = update(
                state,
                &mut universes,
                &mut [],
                &mut [],
                EngineCommand::Tick { now },
            );
            for command in at(ms) {
                state = update(state, &mut universes, &mut [], &mut [], command);
            }
            let served: Vec<u8> = due(&state, now).map(|n| n.slot() as u8 + 1).collect();
            for &number in &served {
                let handed = EngineCommand::Handed {
                    output: output(number),
                    at: now,
                };
                state = update(state, &mut universes, &mut [], &mut [], handed);
            }
            wakes.push((ms, served));
        }
    }

    #[test]
    fn the_engine_wakes_when_the_next_output_is_due() {
        let calm = State::new(Priority::RealTime);
        let (thirty, forty_four, sixty) = (Rate::Thirty, Rate::FortyFour, Rate::Sixty);
        // (outputs set on the first wake, until ms, wakes: ms and outputs served)
        // 60 and 30 a second meet at 56.06 ms, 1 ns apart: one wake serves both.
        let cases: [(Vec<EngineCommand>, u64, Wakes); 4] = [
            (vec![], 70, vec![(22, vec![]), (45, vec![]), (68, vec![])]), // the beat
            (
                vec![set_output(1, sixty)],
                40,
                vec![(22, vec![1]), (39, vec![1])],
            ),
            (
                vec![set_output(1, sixty), set_output(2, thirty)],
                60,
                vec![(22, vec![1, 2]), (39, vec![1]), (56, vec![1, 2])],
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
                    (56, vec![1, 2]),
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
        let remove = EngineCommand::RemoveOutput { output: output(1) };
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
    fn a_new_output_joins_the_phase_of_its_rate() {
        let calm = State::new(Priority::RealTime);
        let (thirty, sixty) = (Rate::Thirty, Rate::Sixty);
        // (output added on the wake at 39 ms, wakes: ms and outputs served)
        let cases = [
            // a rate that runs: its next frame, with the others of its rate
            (
                set_output(3, thirty),
                vec![(22, vec![1, 2]), (39, vec![1]), (56, vec![1, 2, 3])],
            ),
            (
                set_output(3, sixty),
                vec![(22, vec![1, 2]), (39, vec![1, 3]), (56, vec![1, 2, 3])],
            ),
            // a rate of its own: at once
            (
                set_output(3, Rate::FortyFour),
                vec![(22, vec![1, 2]), (39, vec![1, 3]), (56, vec![1, 2])],
            ),
        ];
        for (added, expected) in cases {
            let wakes = run(calm, 60, |ms| match ms {
                22 => vec![set_output(1, sixty), set_output(2, thirty)],
                39 => vec![added],
                _ => vec![],
            });
            assert_eq!(wakes, expected, "{added:?}");
        }
    }

    #[test]
    fn after_the_last_output_is_removed_the_engine_wakes_within_one_beat() {
        let calm = State::new(Priority::RealTime);
        let one_beat = u64::try_from(BEAT.as_millis()).unwrap() + 1;
        let (thirty, forty_four, sixty) = (Rate::Thirty, Rate::FortyFour, Rate::Sixty);
        // (outputs set on the first wake, all removed on the first wake after ms)
        let cases: [(&[(u8, Rate)], u64); 5] = [
            (&[(1, sixty)], 50),
            (&[(1, sixty)], 17_000), // about 1000 wakes
            (&[(1, sixty), (2, forty_four)], 17_000),
            (&[(1, thirty)], 17_000),
            (&[(1, sixty), (2, thirty), (3, forty_four)], 17_000),
        ];
        for (outputs, after) in cases {
            let (mut added, mut removed) = (false, None);
            let wakes = run(calm, after + 100, |ms| {
                if !added {
                    added = true;
                    return outputs
                        .iter()
                        .map(|&(n, rate)| set_output(n, rate))
                        .collect();
                }
                if removed.is_some() || ms < after {
                    return vec![];
                }
                removed = Some(ms);
                let remove =
                    |&(n, _): &(u8, Rate)| EngineCommand::RemoveOutput { output: output(n) };
                outputs.iter().map(remove).collect()
            });
            let removed = removed.unwrap();
            let next = wakes.iter().map(|&(ms, _)| ms).find(|&ms| ms > removed);
            assert!(
                next.is_some_and(|ms| ms - removed <= one_beat),
                "{outputs:?}: removed at {removed} ms, next wake {next:?}"
            );
        }
    }

    #[test]
    fn after_a_long_stall_each_output_starts_over_from_now() {
        let mut universes = [Levels::ZERO];
        let mut state = State::new(Priority::RealTime);
        state = update(
            state,
            &mut universes,
            &mut [],
            &mut [],
            set_output(1, Rate::Sixty),
        );
        state = update(
            state,
            &mut universes,
            &mut [],
            &mut [],
            set_output(2, Rate::Thirty),
        );
        // the machine slept for a second
        let now = Duration::from_secs(1);
        state = update(
            state,
            &mut universes,
            &mut [],
            &mut [],
            EngineCommand::Tick { now },
        );
        let served: Vec<_> = due(&state, now).collect();
        assert_eq!(served, vec![output(1), output(2)]);
        for number in served {
            let handed = EngineCommand::Handed {
                output: number,
                at: now,
            };
            state = update(state, &mut universes, &mut [], &mut [], handed);
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
    fn a_wake_that_serves_an_output_spins_the_last_millisecond_a_beat_only_sleeps() {
        let mut universes = [Levels::ZERO];
        let calm = State::new(Priority::RealTime);
        let due = BEAT; // the beat, and the output's first frame
        let with_output = update(
            calm,
            &mut universes,
            &mut [],
            &mut [],
            set_output(1, Rate::Sixty),
        );
        let with_output = State {
            outputs: with_output.outputs.map(|o| o.map(|o| Output { due, ..o })),
            ..with_output
        };
        let us = Duration::from_micros;
        // (state, µs before due, µs to sleep, spin until due)
        let cases = [
            (with_output, 22_727, 21_727, true), // a whole frame ahead
            (with_output, 1_001, 1, true),
            (with_output, 1_000, 0, true), // spin all of it
            (with_output, 400, 0, true),
            (with_output, 0, 0, true),
            (calm, 22_727, 22_727, false), // no output: sleep to the beat
            (calm, 400, 400, false),
            (calm, 0, 0, false),
        ];
        for (state, ahead, sleep, spins) in cases {
            let now = due - us(ahead);
            let spin = spins.then_some(due);
            assert_eq!(
                wait(&state, now),
                (us(sleep), spin),
                "{ahead} µs ahead, spins {spins}"
            );
        }
        // past due: no sleep
        assert_eq!(wait(&with_output, due * 2), (Duration::ZERO, Some(due)));
        assert_eq!(wait(&calm, due * 2), (Duration::ZERO, None));
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
    fn percentile_is_the_upper_edge_of_its_bucket_and_never_past_the_worst() {
        let last = LATENESS_BUCKETS - 1;
        let late = 30_000; // past the last edge, 25 ms
        // (bucket and count, the worst time counted in µs, percent, result in µs)
        let cases = [
            (counts(&[]), 0, 99, None),
            (counts(&[(0, 1)]), 99, 99, Some(99)), // the worst is in the bucket
            (counts(&[(0, 99), (5, 1)]), 550, 99, Some(100)),
            (counts(&[(0, 98), (5, 2)]), 550, 99, Some(550)),
            (counts(&[(0, 98), (5, 2)]), 599, 99, Some(599)),
            (counts(&[(3, 1000), (100, 11)]), 10_050, 99, Some(10_050)),
            (counts(&[(3, 1000), (100, 11)]), late, 99, Some(10_100)),
            (counts(&[(0, 95), (5, 5)]), 550, 95, Some(100)),
            (counts(&[(0, 94), (5, 6)]), 550, 95, Some(550)),
            (counts(&[(0, 1), (9, 1)]), 950, 95, Some(950)),
            (counts(&[(0, 1), (last, 1)]), 2_000_000, 95, Some(2_000_000)), // a stall of 2 s
            (counts(&[(last, 3)]), late, 50, Some(late)),
            (counts(&[(0, 99), (last, 1)]), late, 99, Some(100)),
            (counts(&[(0, u64::MAX), (1, u64::MAX)]), 150, 99, Some(100)), // saturates
            // a total that is no multiple of 100: the rank is rounded up
            (counts(&[(0, 75), (5, 75)]), 550, 50, Some(100)), // rank 75
            (counts(&[(0, 74), (5, 76)]), 550, 50, Some(550)),
            (counts(&[(0, 76), (5, 75)]), 550, 50, Some(100)), // rank 76 of 151
            (counts(&[(0, 76), (5, 75)]), 550, 51, Some(550)), // rank 78
        ];
        for (histogram, worst, percent, expected) in cases {
            let worst = Duration::from_micros(worst);
            assert_eq!(
                percentile(&histogram, percent, worst),
                expected.map(Duration::from_micros),
                "p{percent} worst {worst:?}"
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
                Command::decl(&config),
                ChannelNumbers::decl(&config),
                RunNumbers::decl(&config),
                OutputNumber::decl(&config),
                OutputKind::decl(&config),
                Adapter::decl(&config),
                Color::decl(&config),
                Rate::decl(&config),
                Reply::decl(&config),
                Refusal::decl(&config),
                State::decl(&config),
                Priority::decl(&config),
                BestEffort::decl(&config),
                Levels::decl(&config),
                ChannelLevel::decl(&config),
                HeldLight::decl(&config),
                Rgb::decl(&config),
                Light::decl(&config),
                Position::decl(&config),
                Gobo::decl(&config),
                Step::decl(&config),
                Saved::decl(&config),
                PlayId::decl(&config),
                Play::decl(&config),
                Output::decl(&config),
                Run::decl(&config),
                Status::decl(&config),
                Counters::decl(&config),
                Library::decl(&config),
                Match::decl(&config),
                ModeSize::decl(&config),
                ModeChannels::decl(&config),
                NamedChannel::decl(&config),
                Failure::decl(&config),
                FixtureKind::decl(&config),
                ChannelFunction::decl(&config),
                Project::decl(&config),
                ProjectOutput::decl(&config),
                Target::decl(&config),
                Patched::decl(&config),
                Pixels::decl(&config),
                Layout::decl(&config),
                Member::decl(&config),
                Grouped::decl(&config),
                ShowFile::decl(&config),
                Unopened::decl(&config),
            ]
            .map(|declaration| format!("export {declaration}\n"))
            .join("\n"),
        ) + &format!("\nexport const API_VERSION = {API_VERSION};\n");
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
