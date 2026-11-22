//! The real-time core: everything that runs per tick, as calculations.
//!
//! No `std` and no `alloc`. The clock, files, the network and the heap do not exist
//! here, so code that would use them does not compile. Time comes in as a value: the
//! time since the engine started. `crates/engine` is the thin action around this crate:
//! it reads the clock, sleeps, calls [`update`] and publishes the state.

#![cfg_attr(not(any(test, feature = "ts")), no_std)] // tests and the TypeScript types may use std
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod api;
mod channel;
mod clock;
mod evaluate;
pub mod health;
pub use clock::{BEATS_PER_BAR, BeatTime, Bpm, Clock, Reading, Source, When, beats};
pub use evaluate::{Binding, Elapsed, Item, MAX_DEPTH, Node, Program};
mod kinds;
mod library;
mod mix;
pub use mix::{
    Brightness, Claims, End, GrabId, Held, ManualEnd, MixRules, OtherEnd, show, stops, unkept, wins,
};
mod output;
mod patch;
mod playback;
mod project;
mod property;
mod resolve;
mod show;

pub use api::{
    AuthoredStep, Definition, OpenFailure, OutOfRange, Palette, Pattern, Percent, Period, Refusal,
    Repeat, SavedLook, Sequence, SequenceItem, Speed, Swatches, Use, WaveTarget, Width,
};
pub use channel::{
    ADDRESSES, CHANNELS, Channel, ChannelLevel, ChannelNumbers, ChannelSet, Channels, Levels,
    MAX_UNIVERSES, SourceTag, Universes, changes,
};
use channel::{BLACKOUT, MODIFIERS};
pub use kinds::{Adapter, Color, OutputKind};
#[cfg(feature = "test-support")]
pub use kinds::{EXAMPLE as TEST_STREAM_KIND, OTHER as TEST_KIND};
pub use library::Text;
pub use output::{
    Counters, MAX_OUTPUTS, OFF_BEAT, Outcome, Output, OutputNumber, ROUTE_MOST, Rate, RouteFault,
    Run, RunNumbers, SPANS_MOST, Span, Status, Watch, bounds, gather, route_length,
};
pub use patch::{
    Grouped, Layout, Member, MemberList, Members, PatchCommand, Patched, Pixels, Target,
};
pub use playback::{
    Entry, LOOKS, Launches, LookCommand, PLAYS, Play, PlayId, PlayState, Plays, Rank, Saved, Step,
};
pub use project::{Project, ProjectEffect, ProjectEvent, ProjectOutput};
pub use property::{Gobo, Light, Position, Property, Rgb, Value, Wheel};
pub use resolve::{
    HeldLight, Less, LightNumber, LightRun, Lights, MAX_LIGHTS, Part, Place, Scale, Sense, Take,
    Travel, Writer, held_lights, light_changes, resolve,
};
pub use show::{ShowFile, Unopened};

use core::time::Duration;
use serde::{Deserialize, Serialize};

/// What the engine is doing now, without the levels. Sent to the UI as JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct State {
    /// Frames computed since the engine started.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub frame: u64,
    /// What the system gave the output thread. Asked once, when the thread starts.
    pub priority: Priority,
    /// While on, every channel goes out at zero. The held levels stay.
    pub blackout: bool,
    /// The outputs, by number. `None` where no output is set.
    #[cfg_attr(feature = "ts", ts(type = "Array<Output | null>"))]
    pub outputs: [Option<Output>; MAX_OUTPUTS],
    /// When the engine last woke, since it started.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub now: Duration,
    /// When it wakes next if no output is set: the state stays alive on this beat.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub beat: Duration,
    /// The looks playing or kept, by slot.
    #[cfg_attr(feature = "ts", ts(type = "Array<Play>"))]
    pub plays: Plays,
    /// The Mixer's settings as this frame applies them.
    pub rules: MixRules,
    /// The start order the next hold or play takes: a later start wins.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub order: u64,
    /// The start orders kept for the launches at the clock's instant.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub launches: Launches,
    /// The beat clock, settled to the frame's time.
    #[cfg_attr(feature = "ts", ts(type = "Reading"))]
    pub clock: Clock,
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
            rules: MixRules {
                brightness: Brightness::Latest,
                manual_end: ManualEnd::UntilRelease,
                other_end: OtherEnd::Reveal,
            },
            order: Launches::from(1).1,
            launches: Launches::from(1).0,
            clock: Clock::START,
        }
    }

    /// A new run for the same adapter starts a fresh set of counters. Its
    /// current status, including consecutive failures, belongs to the adapter.
    pub fn reroute(self, output: OutputNumber, run: Run, rate: Rate) -> Self {
        let Some(Some(previous)) = self.outputs.get(output.slot()) else {
            return self;
        };
        self.with_output(output, run, rate, previous.status)
    }

    fn with_output(mut self, output: OutputNumber, run: Run, rate: Rate, status: Status) -> Self {
        let due = first_due(&self.outputs, rate, self.now);
        if let Some(slot) = self.outputs.get_mut(output.slot()) {
            *slot = Some(Output {
                status,
                ..Output::new(run, rate, due)
            });
        }
        self
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Priority {
    /// The system's real-time class: the thread wakes on time.
    RealTime,
    /// The default class: the thread may wake late.
    BestEffort { reason: BestEffort },
}

/// Why the output thread runs best effort.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[expect(
    clippy::large_enum_variant,
    reason = "a client's command has no heap: `Patch` carries its four names in place, `SetGroup` its members. Moved once per request, off the tick thread"
)]
pub enum Command {
    /// Hold a channel at a level. It wins over the light that writes it.
    SetChannel { channel: ChannelNumbers, value: u8 },
    /// Let a channel go: the light that writes it shows again, else zero.
    ReleaseChannel { channel: ChannelNumbers },
    /// Let every raw channel go.
    ReleaseChannels,
    /// Let every Manual play, every direct hold and every raw channel go. Show,
    /// Autopilot, blackout and tempo stay.
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
    /// its number. A fixture on any of the same channels is refused (`Overlaps`).
    Patch {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        name: Text,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        maker: Text,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        model: Text,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        mode: Text,
        at: ChannelNumbers,
    },
    /// Add a generic strip of `pixels` pixels, each red, green, blue (and white for
    /// `Rgbw`), from `at` on: one light per pixel, no definition file. As `Patch` otherwise.
    PatchStrip {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
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
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        name: Text,
    },
    /// `SetProperty` of an intensity: 0 to 65 535 is 0 to 1.
    SetIntensity { target: Target, intensity: u16 },
    /// `SetProperty` of a colour, 16 bits each.
    SetColour {
        target: Target,
        red: u16,
        green: u16,
        blue: u16,
    },
    /// `ReleaseProperty` of the intensity.
    ReleaseIntensity { target: Target },
    /// `ReleaseProperty` of the colour.
    ReleaseColour { target: Target },
    /// Grab a property of a light, or of every light of a fixture or a group, at Manual:
    /// answered `Grabbed` with its grab, the latest start. It holds until its end
    /// (`EndGrab`) or a release.
    SetProperty { target: Target, value: Value },
    /// A new value for a grab where it still holds: it keeps its start, so what started
    /// after it still wins. An ended or replaced grab is a harmless no-op.
    UpdateGrab { grab: GrabId, value: Value },
    /// End a grab under the Manual end rule ([`MixRules`]): kept until released, or let
    /// go. An ended or replaced grab is a harmless no-op.
    EndGrab { grab: GrabId },
    /// Let every direct hold of `property` go on the lights `target` names: each shows
    /// what plays underneath, else the definition's default. Manual plays stay.
    ReleaseProperty { target: Target, property: Property },
    /// Make a group, or replace the one of this name. Its members are lights or fixtures,
    /// in the order given, each patched. Unpatching a fixture takes it out of its groups.
    SetGroup {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        name: Text,
        #[cfg_attr(feature = "ts", ts(type = "Array<Member>"))]
        members: MemberList,
    },
    /// Remove a group. Nothing happens if there is none.
    RemoveGroup {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        name: Text,
    },
    /// Save `step` as look `look` named `name`, in place of the one saved there. A
    /// playing look shows the new step in the next frame and keeps its start.
    PutStep {
        look: u16,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        name: Text,
        step: AuthoredStep,
    },
    PutPattern {
        look: u16,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        name: Text,
        pattern: Pattern,
    },
    PutSequence {
        look: u16,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        name: Text,
        sequence: Sequence,
    },
    /// Remove a saved look that no play uses. Nothing happens if there is none.
    RemoveLook { look: u16 },
    /// Play a saved look at `rank`, Manual when not given: answered `Started` with its
    /// play. The look playing or kept at that rank already starts afresh, as the latest
    /// start; another rank plays on its own.
    StartLook {
        look: u16,
        #[serde(default, rename = "use")]
        #[cfg_attr(feature = "ts", ts(optional = nullable))]
        usage: Use,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional = nullable))]
        rank: Rank,
        /// At once when not given, or on the next beat or bar of the grid: then the
        /// play waits, and plays from the frame of that crossing.
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional = nullable))]
        when: When,
    },
    /// Stop a play under the Mixer's end rule ([`MixRules`]): kept, or ended. A kept or
    /// ended play is a harmless no-op.
    StopLook { play: PlayId },
    /// End a Manual play at once, whatever the end rule: a flash let go. A play of
    /// another rank, or an ended one, is a harmless no-op.
    EndFlash { play: PlayId },
    /// End a play, playing or kept: what it held shows what is underneath in the next
    /// frame. An ended play is a harmless no-op.
    ReleaseHold { play: PlayId },
    /// Save the Mixer's three settings at once. Switching to `UntilStop` or `Reveal`
    /// ends the kept plays of those ranks.
    SetMixRules { rules: MixRules },
    /// The internal feed's tempo, 30 to 300 beats a minute: the tempo, unless taps set it.
    SetInternalTempo { bpm: Bpm },
    /// A tap at the time the engine takes it: it marks a beat, and a second one 200 ms
    /// to 2 s later sets the tempo for every play, until `ReleaseTempo`.
    TapTempo,
    /// The feed's tempo again, from the grid's phase now.
    ReleaseTempo,
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
        watch: Watch,
        at: Duration,
    },
    SetChannel {
        channel: Channel,
        value: u8,
    },
    ReleaseChannel {
        channel: Channel,
    },
    ReleaseChannels,
    /// Every raw channel, every direct hold and every Manual play let go; the plans of
    /// the plays go back to be freed.
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
    /// Each light of `lights` takes `change`.
    Lights {
        lights: LightRun,
        change: LightChange,
    },
    /// The lights of an unpatched fixture: their held values go.
    Darken {
        lights: LightRun,
    },
    /// Runs of a group arrive with this command, in place: each takes `change`. A
    /// group's commands are queued at once, so one frame shows all of them or none. Alone
    /// it changes nothing.
    Runs {
        change: LightChange,
    },
    /// `play` of `look` starts at `rank` in its slot, in place of the play there. Its
    /// plan arrives with this command and takes the place of the slot's plan.
    Start {
        play: PlayId,
        look: u16,
        rank: Rank,
        when: When,
    },
    /// `play` is kept: its values stay at its rank and start. A waiting play is
    /// cancelled.
    Keep {
        play: PlayId,
    },
    /// The Mixer's settings from this frame on. The kept plays they no longer keep end;
    /// their plans go back to be freed.
    Rules {
        rules: MixRules,
    },
    /// The plan of `play` arrives with this command: the play keeps its start order.
    Plan {
        play: PlayId,
    },
    /// `play` ends; its plan goes back to be freed.
    Release {
        play: PlayId,
    },
    /// Time reaches `at`, if it was not there yet: the commands sent by then are
    /// applied, the waiting plays whose beat the grid reaches launch, one tap lapses.
    /// The tick settles before each command at the time it was sent, and at the frame.
    Settle {
        at: Duration,
    },
    /// A tap, at the time the clock was settled to.
    Tap,
    ReleaseTempo,
    Feed {
        bpm: Bpm,
    },
}

/// What a light command does to the direct holds of each light it names, without
/// naming them. All Manual; who holds what: [`Held`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightChange {
    /// `grab` holds the value, as the latest start.
    Grab { value: Value, grab: GrabId },
    /// The value, where `grab` still holds its property. The start stays.
    Update { value: Value, grab: GrabId },
    /// Where `grab` still holds `property`: kept, or let go.
    End {
        property: Property,
        grab: GrabId,
        end: End,
    },
    /// Every direct hold of `property` let go.
    Release { property: Property },
}

impl LightChange {
    /// The command that makes this change to the lights of `run`.
    pub const fn on(self, run: LightRun) -> EngineCommand {
        EngineCommand::Lights {
            lights: run,
            change: self,
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
            watch,
            at,
        } => {
            let mut outputs = state.outputs;
            if let Some(Some(sent)) = outputs.get_mut(output.slot()) {
                *sent = sent.sent(outcome, watch, at);
            }
            State { outputs, ..state }
        }
        EngineCommand::Attach {
            output,
            universes: run,
            rate,
        } => state.with_output(output, run, rate, Status::NotFound),
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
        EngineCommand::ReleaseChannels => {
            universes.fill(Levels::ZERO);
            holds.fill(ChannelSet::NONE);
            state
        }
        EngineCommand::ReleaseAll => {
            universes.fill(Levels::ZERO);
            holds.fill(ChannelSet::NONE);
            lights.fill(Held::NONE);
            let mut plays = state.plays;
            plays.release_where(|play| play.rank == Rank::Manual);
            State { plays, ..state }
        }
        EngineCommand::Blackout { on } => State {
            blackout: on,
            ..state
        },
        // `Plan`: the plays stay; whether its plan installs is `Plays::takes_plan`.
        EngineCommand::Writers | EngineCommand::Runs { .. } | EngineCommand::Plan { .. } => state,
        EngineCommand::Lights {
            lights: run,
            change,
        } => {
            for light in run.of(lights) {
                light.change(change, state.order);
            }
            match change {
                LightChange::Grab { .. } => held,
                LightChange::Update { .. }
                | LightChange::End { .. }
                | LightChange::Release { .. } => state,
            }
        }
        EngineCommand::Darken { lights: run } => {
            run.of(lights).fill(Held::NONE);
            state
        }
        EngineCommand::Start {
            play,
            look,
            rank,
            when,
        } => {
            let (mut plays, mut launches) = (state.plays, state.launches);
            plays.start(
                (play, look, rank),
                when,
                &state.clock,
                state.now,
                (state.order, &mut launches),
            );
            State {
                plays,
                launches,
                ..held
            }
        }
        EngineCommand::Keep { play } => {
            let mut plays = state.plays;
            plays.keep(play);
            State { plays, ..state }
        }
        EngineCommand::Rules { rules } => {
            let mut plays = state.plays;
            plays.release_where(|play| unkept(play.rank, play.state, rules));
            for light in lights.iter_mut() {
                light.unkeep(rules);
            }
            State {
                plays,
                rules,
                ..state
            }
        }
        EngineCommand::Release { play } => {
            let mut plays = state.plays;
            plays.release(play);
            State { plays, ..state }
        }
        EngineCommand::Settle { at } => {
            let clock = state.clock.settle(at);
            // a new instant keeps the orders of its launches before its commands
            let (launches, order) = if clock.at() > state.clock.at() {
                Launches::from(state.order)
            } else {
                (state.launches, state.order)
            };
            launch(State {
                clock,
                launches,
                order,
                ..state
            })
        }
        EngineCommand::Tap => retempo(launch(State {
            clock: state.clock.tap(),
            ..state
        })),
        EngineCommand::ReleaseTempo => retempo(State {
            clock: state.clock.release(),
            ..state
        }),
        EngineCommand::Feed { bpm } => retempo(State {
            clock: state.clock.set_feed(bpm),
            ..state
        }),
    }
}

/// The waiting plays whose beat the grid reached by now play.
fn launch(state: State) -> State {
    let (mut plays, mut launches) = (state.plays, state.launches);
    plays.launch(&state.clock, &mut launches);
    State {
        plays,
        launches,
        ..state
    }
}

/// After a change of tempo: the plays started at once begin at the frame as it is now.
fn retempo(state: State) -> State {
    let mut plays = state.plays;
    plays.begin_at(&state.clock, state.now);
    State { plays, ..state }
}

/// The bytes that go out now, written into `channels`, which the caller owns. Every
/// channel starts at its rest; the resolver writes what the `writers` make of the
/// `lights`; then one pass decides each channel: the raw channels held (`universes` at
/// `holds`) replace their bytes, and every follower moves by its modifiers, Blackout
/// among them. No level depends on time yet: the first that does takes the time as an
/// argument.
pub fn frame(
    state: &State,
    universes: &[Levels],
    holds: &[ChannelSet],
    lights: &[Light],
    writers: &[Writer],
    channels: &mut Channels,
) {
    let mut lit = [ChannelSet::NONE; MAX_UNIVERSES as usize];
    mix::rest(channels);
    resolve::write(lights, writers, &mut channels.value, &mut lit);
    mix::channels(channels, &lit, (universes, holds), modifiers(state));
}

/// The level of each modifier slot this frame: Master full until `SetMaster` comes,
/// Blackout 0 while on.
fn modifiers(state: &State) -> [u8; MODIFIERS] {
    let mut levels = [u8::MAX; MODIFIERS];
    if let Some(blackout) = levels.get_mut(BLACKOUT) {
        *blackout = if state.blackout { 0 } else { u8::MAX };
    }
    levels
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
    let total = histogram.iter().map(|n| u128::from(*n)).sum::<u128>();
    let rank = rank(total, percent);
    let mut seen = 0_u128;
    for (bucket, count) in histogram.iter().enumerate() {
        seen += u128::from(*count);
        if total > 0 && seen >= rank {
            return Some(bound::<N>(bucket, worst));
        }
    }
    None
}

/// Nearest rank: how many of `total` counts stay at or under the `percent` percentile,
/// ceil(percent / 100 x total). A percent above 100 is 100. A `u128` total never overflows.
fn rank(total: u128, percent: u64) -> u128 {
    (u128::from(percent.min(100)) * total).div_ceil(100)
}

/// The upper edge of `bucket`, never past `worst`. The last of `N` buckets holds
/// everything later: `worst`.
fn bound<const N: usize>(bucket: usize, worst: Duration) -> Duration {
    if bucket + 1 == N {
        worst
    } else {
        BUCKET
            .saturating_mul(u32::try_from(bucket + 1).unwrap_or(u32::MAX))
            .min(worst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// One wake at `now`: the commands sent by then, each settled at the time it was
    /// sent, then the frame's time, as the tick applies them.
    fn wake(state: State, now: u64, sent: &[(u64, EngineCommand)]) -> State {
        let apply = |state, command| update(state, &mut [], &mut [], &mut [], command);
        let mut state = apply(state, tick(now));
        for (at, command) in sent {
            let at = Duration::from_millis(*at);
            state = apply(state, EngineCommand::Settle { at });
            state = apply(state, *command);
        }
        apply(
            state,
            EngineCommand::Settle {
                at: Duration::from_millis(now),
            },
        )
    }

    /// An engine with the feed at 120 beats a minute: beat n at n * 500 ms.
    fn at_120() -> State {
        wake(
            CALM,
            0,
            &[(
                0,
                EngineCommand::Feed {
                    bpm: Bpm::new(120).unwrap(),
                },
            )],
        )
    }

    /// A count of the clock is `beats`, to a millionth of a beat.
    fn close(count: impl Into<BeatTime>, want: f64) -> bool {
        (count.into().beats() - want).abs() < 1e-6
    }

    fn play(slot: usize) -> PlayId {
        PlayId::new(slot, 1).unwrap()
    }

    #[test]
    fn a_waiting_play_launches_on_its_beat_in_time_order() {
        let start = |slot, when| EngineCommand::Start {
            play: play(slot),
            look: 7,
            rank: Rank::Manual,
            when,
        };
        let keep = EngineCommand::Keep { play: play(0) };
        let release = EngineCommand::Release { play: play(0) };
        let tap = EngineCommand::Tap;
        let (beat, bar, now) = (When::Beat, When::Bar, When::Now);
        let waits = |beat| PlayState::Waiting { beat };
        let (playing, kept) = (PlayState::Playing, PlayState::Kept);
        type Wakes<'a> = &'a [(u64, &'a [(u64, EngineCommand)])];
        // (name, wakes, plays after the last, the latest start that sounds)
        type Case<'a> = (&'a str, Wakes<'a>, &'a [(usize, PlayState)], Option<usize>);
        let cases: [Case; 23] = [
            (
                "a tap supplies a crossing at the instant of a later request: it wins",
                &[
                    (1100, &[(1100, start(0, beat))]),
                    (1300, &[(1300, start(1, now)), (1300, tap)]),
                ],
                &[(0, playing), (1, playing)],
                Some(1),
            ),
            (
                "a tap supplies a crossing asked at its instant, after a start at once",
                &[(
                    1300,
                    &[(1300, start(1, now)), (1300, start(0, beat)), (1300, tap)],
                )],
                &[(0, playing), (1, playing)],
                Some(0),
            ),
            (
                "a launched play, then a later start: it wins",
                &[
                    (1100, &[(1100, start(0, beat))]),
                    (1500, &[]),
                    (1500, &[(1500, start(1, now))]),
                ],
                &[(0, playing), (1, playing)],
                Some(1),
            ),
            (
                "two launched at one instant by a tap, then a later start",
                &[
                    (1100, &[(1100, start(0, beat)), (1100, start(2, beat))]),
                    (1300, &[(1300, tap), (1300, start(1, now))]),
                ],
                &[(0, playing), (1, playing), (2, playing)],
                Some(1),
            ),
            (
                "a beat, before it",
                &[(1100, &[(1100, start(0, beat))]), (1499, &[])],
                &[(0, waits(3))],
                None,
            ),
            (
                "a beat, on it",
                &[(1100, &[(1100, start(0, beat))]), (1500, &[])],
                &[(0, playing)],
                Some(0),
            ),
            (
                "a beat, asked on the grid",
                &[(1500, &[(1500, start(0, beat))])],
                &[(0, playing)],
                Some(0),
            ),
            (
                "a beat, asked on the grid, late",
                &[(1600, &[(1500, start(0, beat))])],
                &[(0, playing)],
                Some(0),
            ),
            (
                "a bar, through the beats",
                &[
                    (2600, &[(2600, start(0, bar))]),
                    (3000, &[]),
                    (3500, &[]),
                    (3999, &[]),
                ],
                &[(0, waits(8))],
                None,
            ),
            (
                "a bar, on it",
                &[(2600, &[(2600, start(0, bar))]), (3000, &[]), (4000, &[])],
                &[(0, playing)],
                Some(0),
            ),
            (
                "kept while waiting: cancelled",
                &[
                    (1100, &[(1100, start(0, beat))]),
                    (1200, &[(1200, keep)]),
                    (2000, &[]),
                ],
                &[],
                None,
            ),
            (
                "released while waiting",
                &[
                    (1100, &[(1100, start(0, beat))]),
                    (1200, &[(1200, release)]),
                    (2000, &[]),
                ],
                &[],
                None,
            ),
            (
                "kept after its crossing, in a late wake",
                &[(1100, &[(1100, start(0, beat))]), (1600, &[(1550, keep)])],
                &[(0, kept)],
                Some(0),
            ),
            (
                "kept before its crossing, in a late wake",
                &[(1100, &[(1100, start(0, beat))]), (1600, &[(1450, keep)])],
                &[],
                None,
            ),
            (
                "kept on its crossing: it launched first",
                &[(1100, &[(1100, start(0, beat))]), (1600, &[(1500, keep)])],
                &[(0, kept)],
                Some(0),
            ),
            (
                "two boundaries in one late wake: the later beat starts later",
                &[
                    (1100, &[(1100, start(0, bar)), (1100, start(1, beat))]),
                    (2100, &[]),
                ],
                &[(0, playing), (1, playing)],
                Some(0),
            ),
            (
                "two on one beat: the later request starts later",
                &[
                    (1100, &[(1100, start(0, beat)), (1100, start(1, beat))]),
                    (1500, &[]),
                ],
                &[(0, playing), (1, playing)],
                Some(1),
            ),
            (
                "sent before the crossing, at once: the launch is later",
                &[
                    (1100, &[(1100, start(0, beat))]),
                    (1600, &[(1200, start(1, now))]),
                ],
                &[(0, playing), (1, playing)],
                Some(0),
            ),
            (
                "sent after the crossing, at once: it is later",
                &[
                    (1100, &[(1100, start(0, beat))]),
                    (1600, &[(1550, start(1, now))]),
                ],
                &[(0, playing), (1, playing)],
                Some(1),
            ),
            (
                "a tap supplies the crossing",
                &[(1100, &[(1100, start(0, beat))]), (1300, &[(1300, tap)])],
                &[(0, playing)],
                Some(0),
            ),
            (
                "an off-grid tap moves a bar's crossing: before it",
                &[
                    (2600, &[(2600, start(0, bar))]),
                    (3150, &[(3150, tap)]),
                    (3649, &[]),
                ],
                &[(0, waits(8))],
                None,
            ),
            (
                "an off-grid tap moves a bar's crossing: on it",
                &[
                    (2600, &[(2600, start(0, bar))]),
                    (3150, &[(3150, tap)]),
                    (3650, &[]),
                ],
                &[(0, playing)],
                Some(0),
            ),
            (
                "started again while waiting: waits afresh",
                &[
                    (1100, &[(1100, start(0, beat))]),
                    (1200, &[(1200, start(0, bar))]),
                    (1500, &[]),
                ],
                &[(0, waits(4))],
                None,
            ),
        ];
        for (name, wakes, plays, latest) in cases {
            let mut state = at_120();
            for (now, sent) in wakes {
                state = wake(state, *now, sent);
            }
            let now: Vec<_> = state
                .plays
                .iter()
                .map(|play| (play.play.slot(), play.state))
                .collect();
            assert_eq!(now, plays, "{name}");
            let mut orders: Vec<_> = state.plays.sounding().map(|play| play.order).collect();
            orders.sort_unstable();
            orders.dedup();
            assert_eq!(
                orders.len(),
                state.plays.sounding().count(),
                "{name}: a tie"
            );
            let last = state.plays.sounding().max_by_key(|play| play.order);
            assert_eq!(last.map(|play| play.play.slot()), latest, "{name}");
        }
    }

    #[test]
    fn a_cancelled_play_ends_and_frees_its_slot() {
        let start = EngineCommand::Start {
            play: play(0),
            look: 7,
            rank: Rank::Show,
            when: When::Bar,
        };
        let state = wake(at_120(), 1100, &[(1100, start)]);
        let state = wake(
            state,
            1200,
            &[(1200, EngineCommand::Keep { play: play(0) })],
        );
        assert!(!state.plays.has(0));
        // and it never sounds: past its bar, nothing launched
        let later = wake(state, 2500, &[]);
        assert!(later.plays.iter().next().is_none());
    }

    /// Every cell of "A play on the tick" (`docs/engine.md`): beat n at n * 500 ms, the
    /// play in slot 0 set up at 1100 ms, each event in a later wake.
    #[test]
    fn a_play_on_the_tick_follows_its_table() {
        let (first, second) = (play(0), PlayId::new(0, 2).unwrap());
        let start = |play, rank, when| EngineCommand::Start {
            play,
            look: 7,
            rank,
            when,
        };
        let removing = MixRules {
            manual_end: ManualEnd::UntilStop,
            ..MixRules::default()
        };
        let staying = MixRules {
            other_end: OtherEnd::Stay,
            ..MixRules::default()
        };
        let set_up = |commands: &[EngineCommand]| {
            let sent: Vec<_> = commands.iter().map(|command| (1100, *command)).collect();
            wake(at_120(), 1100, &sent)
        };
        let keep = EngineCommand::Keep { play: first };
        let manual = |when| start(first, Rank::Manual, when);
        let (off, waiting) = (set_up(&[]), set_up(&[manual(When::Beat)]));
        let (playing, kept) = (set_up(&[manual(When::Now)]), set_up(&[manual(When::Now), keep]));
        // (event, wake at, its command): the table's columns, in order
        let events = [
            ("Start, Now", 1200, Some(start(second, Rank::Manual, When::Now))),
            ("Start, Beat, not reached", 1200, Some(start(second, Rank::Manual, When::Beat))),
            ("Start, boundary reached", 1500, Some(start(second, Rank::Manual, When::Beat))),
            ("grid reaches the beat", 1600, None),
            ("Keep", 1200, Some(keep)),
            ("Release", 1200, Some(EngineCommand::Release { play: first })),
            ("ReleaseAll", 1200, Some(EngineCommand::ReleaseAll)),
            ("Rules, Manual ends on a stop", 1200, Some(EngineCommand::Rules { rules: removing })),
            ("Rules, Manual kept", 1200, Some(EngineCommand::Rules { rules: staying })),
        ];
        let first_is = |state| Some((1, state));
        let (fresh, fresh_waiting) = (
            Some((2, PlayState::Playing)),
            Some((2, PlayState::Waiting { beat: 3 })),
        );
        let (on_3, plays, held) = (
            first_is(PlayState::Waiting { beat: 3 }),
            first_is(PlayState::Playing),
            first_is(PlayState::Kept),
        );
        // (state, slot 0 after each event as (generation, state), takes a plan for it)
        let rows = [
            ("off", off, [fresh, fresh_waiting, fresh, None, None, None, None, None, None], false),
            ("Waiting", waiting, [fresh, fresh_waiting, fresh, plays, None, None, None, on_3, on_3], true),
            ("Playing", playing, [fresh, fresh_waiting, fresh, plays, held, None, None, plays, plays], true),
            ("Kept", kept, [fresh, fresh_waiting, fresh, held, held, None, None, None, held], false),
        ];
        for (name, state, after, takes_plan) in rows {
            assert_eq!(state.plays.takes_plan(first), takes_plan, "{name}: Plan");
            assert!(!state.plays.takes_plan(second), "{name}: Plan of another play");
            for ((event, at, command), expected) in events.iter().zip(after) {
                let sent: Vec<_> = command.iter().map(|command| (*at, *command)).collect();
                let slot = wake(state, *at, &sent).plays.at(0);
                let got = slot.map(|play| (play.play.generation, play.state));
                assert_eq!(got, expected, "{name} + {event}");
            }
        }
        // ReleaseAll lets only Manual go; the rules end a kept play of their rank only
        let show_kept = set_up(&[start(first, Rank::Show, When::Now), keep]);
        let show_cases = [
            (EngineCommand::ReleaseAll, Some(PlayState::Kept)),
            (EngineCommand::Rules { rules: staying }, Some(PlayState::Kept)),
            (EngineCommand::Rules { rules: MixRules::default() }, None),
        ];
        for (command, expected) in show_cases {
            let state = wake(show_kept, 1200, &[(1200, command)]);
            let got = state.plays.at(0).map(|play| play.state);
            assert_eq!(got, expected, "Show, Kept + {command:?}");
        }
    }

    #[test]
    fn a_play_starts_at_its_crossing_and_a_tap_moves_no_running_play() {
        let start = |when| EngineCommand::Start {
            play: play(0),
            look: 7,
            rank: Rank::Manual,
            when,
        };
        // at once: beat zero in its applying frame, the wake at 1600
        let state = wake(at_120(), 1600, &[(1100, start(When::Now))]);
        assert!(close(state.plays.iter().next().unwrap().start, 3.2));
        // a change of tempo after it, in the same wake: still beat zero in that frame
        let tempo = |sent: &[(u64, EngineCommand)]| {
            let mut sent = sent.to_vec();
            sent.insert(0, (100, start(When::Now)));
            let state = wake(at_120(), 500, &sent);
            let play = *state.plays.iter().next().unwrap();
            (play.start, state.clock.beat_at(state.clock.at()))
        };
        let slow = EngineCommand::Feed {
            bpm: Bpm::new(60).unwrap(),
        };
        let keep = EngineCommand::Keep { play: play(0) };
        let changes: [(&str, &[(u64, EngineCommand)]); 5] = [
            ("the feed at 60", &[(200, slow)]),
            ("a tap", &[(200, EngineCommand::Tap)]),
            ("a tap at its frame", &[(500, EngineCommand::Tap)]),
            ("released", &[(200, EngineCommand::ReleaseTempo)]),
            ("kept, then the feed at 60", &[(150, keep), (200, slow)]),
        ];
        for (name, sent) in changes {
            let (start, elapsed) = tempo(sent);
            assert_eq!(start, elapsed, "{name}");
        }
        // on a beat in a late wake: from its crossing at 1500, 0.2 beats on by the frame
        let state = wake(at_120(), 1100, &[(1100, start(When::Beat))]);
        let state = wake(state, 1600, &[]);
        assert!(close(state.plays.iter().next().unwrap().start, 3.0));
        // a tap at 1700 moves the grid from 3.4 to 4; the elapsed count runs on
        let untapped = wake(state, 1700, &[]);
        let tapped = wake(state, 1700, &[(1700, EngineCommand::Tap)]);
        assert_eq!(tapped.clock.elapsed_now(), untapped.clock.elapsed_now());
        assert!(close(untapped.clock.grid(), 3.4) && close(tapped.clock.grid(), 4.0));
        assert_eq!(tapped.plays, untapped.plays);
    }

    #[test]
    fn a_press_sent_before_its_boundary_but_seen_after_plays_from_the_frame() {
        let start = |when| EngineCommand::Start {
            play: play(0),
            look: 7,
            rank: Rank::Manual,
            when,
        };
        let playing = PlayState::Playing;
        // 120 BPM: beat n at n * 500 ms, a bar every 2000 ms. The wake at `missed` counted
        // its commands before the press came in; the wake at `seen` takes it.
        // (name, when, sent, missed, seen, state, start in beats when playing)
        type Case = (&'static str, When, u64, u64, u64, PlayState, Option<f64>);
        let cases: [Case; 4] = [
            ("a beat 1 ms before beat 3", When::Beat, 1499, 1501, 1520, playing, Some(3.04)),
            ("a beat on beat 3", When::Beat, 1500, 1501, 1520, playing, Some(3.04)),
            ("a bar 1 ms before bar 1", When::Bar, 1999, 2001, 2020, playing, Some(4.04)),
            (
                "a beat 1 ms after beat 3",
                When::Beat,
                1501,
                1502,
                1520,
                PlayState::Waiting { beat: 4 },
                None,
            ),
        ];
        for (name, when, sent, missed, seen, expected, beats) in cases {
            let state = wake(wake(at_120(), missed, &[]), seen, &[(sent, start(when))]);
            let play = *state.plays.iter().next().unwrap();
            assert_eq!(play.state, expected, "{name}");
            if let Some(beats) = beats {
                assert!(close(play.start, beats), "{name}: {:?}", play.start);
            }
        }
    }

    #[test]
    fn taps_drained_in_one_wake_count_at_their_own_times() {
        let cases = [
            (
                "750 ms apart",
                1100,
                1850,
                Source::Tapped {
                    at: Duration::from_millis(1850),
                },
                750,
            ),
            (
                "sent at once: a bounce",
                1100,
                1100,
                Source::OneTap {
                    at: Duration::from_millis(1100),
                },
                500,
            ),
        ];
        for (name, first, second, source, period) in cases {
            let tap = EngineCommand::Tap;
            let state = wake(at_120(), 2000, &[(first, tap), (second, tap)]);
            assert_eq!(state.clock.source(), source, "{name}");
            assert_eq!(
                state.clock.period(),
                Duration::from_millis(period),
                "{name}"
            );
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
        let grab = GrabId { generation: 1 };
        let set = |value| move |lights| LightChange::Grab { value, grab }.on(lights);
        let set_red = set(Value::Colour(Rgb {
            red: 0xffff,
            green: 0,
            blue: 0,
        }));
        let half = set(Value::Intensity(0x8000));
        let full_red = Light {
            intensity: Some(0x8000),
            ..red
        };
        let release = |property| move |lights| LightChange::Release { property }.on(lights);
        let let_go = release(Property::Intensity);
        let black = release(Property::Colour);
        // (lights before, command, lights after)
        let cases = [
            ([dark; 4], set_red(run(1, 2)), [dark, red, red, dark]),
            ([dark; 4], half(run(3, 1)), [dark, dark, dark, lit]),
            // black set is held black, not nothing held
            (
                [dark; 4],
                set(Value::Colour(Rgb::BLACK))(run(0, 1)),
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
                let each = [
                    values.intensity.map(Value::Intensity),
                    values.colour.map(Value::Colour),
                ];
                for value in each.into_iter().flatten() {
                    held.change(LightChange::Grab { value, grab }, 1);
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
            assert_eq!(lights.map(|held| *held.values()), after, "{command:?}");
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
            let mut channels = Box::new(Channels::NEW);
            channels.value[0].0[8] = 9; // whatever was there before
            frame(&state, &universes, &holds, &red, writers, &mut channels);
            assert_eq!(channels.values()[..2], expected, "{row}");
            assert!(
                channels.values()[2..].iter().all(|u| *u == Levels::ZERO),
                "{row}"
            );
        }
    }

    #[test]
    fn frame_says_who_set_each_channel_and_a_release_gives_it_back() {
        let all = Lights::new(1).unwrap();
        let light = LightNumber::new(0, all).unwrap();
        // a dimmer at 1, a shutter open at 200 at 2
        let writers = [
            Writer::Takes {
                light,
                take: Take::INTENSITY,
                at: Place::Byte(channel(1, 1)),
                unheld: 0,
            },
            Writer::Fixed {
                at: channel(1, 2),
                value: 200,
            },
        ];
        let lit = [Light {
            intensity: Some(0xffff),
            ..Light::DARK
        }];
        let set = |universe, address, value| EngineCommand::SetChannel {
            channel: channel(universe, address),
            value,
        };
        let release = |universe, address| EngineCommand::ReleaseChannel {
            channel: channel(universe, address),
        };
        let blackout = |on| EngineCommand::Blackout { on };
        use SourceTag as S;
        // (row, commands, channels 1/1, 1/2 and 2/512: byte and source)
        type Seen = [(u8, SourceTag); 3];
        let cases: [(&str, Vec<EngineCommand>, Seen); 7] = [
            (
                "nothing held",
                vec![],
                [(255, S::LIGHTS), (200, S::REST), (0, S::REST)],
            ),
            (
                "raw 91 over the light",
                vec![set(1, 1, 91)],
                [(91, S::RAW), (200, S::REST), (0, S::REST)],
            ),
            (
                "raw 91 released",
                vec![set(1, 1, 91), release(1, 1)],
                [(255, S::LIGHTS), (200, S::REST), (0, S::REST)],
            ),
            (
                "raw on the last channel",
                vec![set(2, 512, 91)],
                [(255, S::LIGHTS), (200, S::REST), (91, S::RAW)],
            ),
            (
                "blackout over raw and light",
                vec![set(2, 512, 91), blackout(true)],
                [(0, S::LIGHTS), (0, S::REST), (0, S::RAW)],
            ),
            (
                "blackout off, the hold comes back",
                vec![set(2, 512, 91), blackout(true), blackout(false)],
                [(255, S::LIGHTS), (200, S::REST), (91, S::RAW)],
            ),
            (
                "every raw channel released",
                vec![
                    set(1, 1, 91),
                    set(2, 512, 91),
                    EngineCommand::ReleaseChannels,
                ],
                [(255, S::LIGHTS), (200, S::REST), (0, S::REST)],
            ),
        ];
        for (row, commands, expected) in cases {
            let (mut universes, mut holds) = raw(&[]);
            let mut state = CALM;
            for command in commands {
                state = update(state, &mut universes, &mut holds, &mut [], command);
            }
            let mut channels = Box::new(Channels::NEW);
            frame(&state, &universes, &holds, &lit, &writers, &mut channels);
            let seen = [(1, 1), (1, 2), (2, 512)].map(|(universe, address)| {
                let at = channel(universe, address);
                let byte = channels.values()[usize::from(universe - 1)].0[usize::from(address - 1)];
                (byte, channels.source(at))
            });
            assert_eq!(seen, expected, "{row}");
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
    fn rank_is_the_nearest_rank() {
        // (total, percent, rank): ceil(percent / 100 x total), the textbook nearest rank
        let full = 501 * u128::from(u64::MAX); // every bucket of a window at its most
        let cases = [
            (0, 99, 0),
            (1, 0, 0),
            (1, 99, 1),
            (99, 99, 99),   // 98.01
            (100, 99, 99),  // 99
            (101, 99, 100), // 99.99
            (100, 95, 95),  // 95
            (101, 95, 96),  // 95.95
            (100, 100, 100),
            (100, 0, 0),
            (100, 150, 100), // above 100 is 100
            (full, 99, 9_149_400_593_119_200_505_524),
        ];
        for (total, percent, expected) in cases {
            assert_eq!(rank(total, percent), expected, "{total}, {percent}");
        }
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
            // half of 2 x u64::MAX counts in each: p99 is in the second, capped at the worst
            (counts(&[(0, u64::MAX), (1, u64::MAX)]), 150, 99, Some(150)),
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
}
