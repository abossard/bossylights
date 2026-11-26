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
#[path = "model/failure.rs"]
mod model_failure;
#[path = "model/kinds.rs"]
mod model_kinds;
#[path = "model/light.rs"]
mod model_light;
#[path = "model/lights.rs"]
mod model_lights;
#[path = "model/mix.rs"]
mod model_mix;
#[path = "model/output.rs"]
mod model_output;
#[path = "model/text.rs"]
mod model_text;
pub use clock::{BEATS_PER_BAR, BeatTime, Bpm, Clock, Reading, Source, When, beats};
pub use evaluate::{Binding, Elapsed, Item, MAX_DEPTH, Node, Program, Sampled};
mod mix;
pub use mix::{
    End, GRABS, Grab, GrabChange, GrabEntry, Grabs, Holding, Offers, SOURCES, stops, unkept, wins,
};
mod output;
mod patch;
mod playback;
mod project;
mod show;

pub use api::{
    AuthoredStep, Definition, HeldLight, OutOfRange, Palette, Pattern, Percent, Period, Repeat,
    Saved, SavedLook, Sequence, SequenceItem, Speed, Step, Swatches, Use, WaveTarget, Width,
};
pub use channel::{
    ADDRESSES, CHANNELS, Channel, ChannelLevel, ChannelNumbers, ChannelSet, Channels, Configured,
    Field, Flags, Follows, Levels, MAX_UNIVERSES, Pair, SourceTag, Universes, changes,
};
use channel::{BLACKOUT, MASTER, MODIFIERS};
pub use model_failure::{OpenFailure, Refusal};
pub use model_kinds::{Adapter, Color, OutputKind};
#[cfg(test)]
pub(crate) use model_kinds::{EXAMPLE, OTHER};
#[cfg(feature = "test-support")]
pub use model_kinds::{EXAMPLE as TEST_STREAM_KIND, OTHER as TEST_KIND};
pub use model_light::{Gobo, Light, Position, Property, Rgb, Value, Wheel};
pub use model_lights::{LightNumber, LightRun, Lights, MAX_LIGHTS};
pub use model_mix::{Brightness, GrabId, ManualEnd, MixRules, OtherEnd};
pub use model_output::{MAX_OUTPUTS, Outcome, OutputNumber, Rate, Status, Watch};
pub use model_text::Text;
pub use output::{
    Counters, OFF_BEAT, Output, ROUTE_MOST, RouteFault, Run, RunNumbers, SPANS_MOST, Span, bounds,
    gather, route_length,
};
pub use patch::{
    Grouped, Layout, Member, MemberList, Members, PatchCommand, Patched, Pixels, Target,
};
pub use playback::{LOOKS, Launches, LookCommand, PLAYS, Play, PlayId, PlayState, Plays, Rank};
pub use project::{Project, ProjectEffect, ProjectEvent, ProjectOutput};
pub use show::{ShowFile, Unopened};

use core::num::NonZeroU32;
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
    /// While on, every channel that follows Blackout goes to its dark. The held levels
    /// stay. On while the latch is on or any connection holds it (`blackout_hold`).
    pub blackout: bool,
    /// Who keeps Blackout on: the latch and how many connections hold it.
    pub blackout_hold: BlackoutHold,
    /// How much shows of every channel that follows Master: 255 full.
    pub master: u8,
    /// The outputs, by number. `None` where no output is set.
    #[cfg_attr(feature = "ts", ts(type = "Array<Output | null>"))]
    outputs: [Option<Output>; MAX_OUTPUTS],
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
    plays: Plays,
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
    clock: Clock,
}

impl State {
    /// A new engine's state: nothing held, no outputs.
    pub const fn new(priority: Priority) -> Self {
        Self {
            frame: 0,
            priority,
            blackout: false,
            blackout_hold: BlackoutHold::NONE,
            master: u8::MAX,
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
        let Some(schedule) = self.rerouted_output(output, run, rate) else {
            return self;
        };
        self.with_output_schedule(output, schedule)
    }

    fn with_output(mut self, output: OutputNumber, run: Run, rate: Rate) -> Self {
        let due = self.first_due(rate);
        if let Some(slot) = self.outputs.get_mut(output.slot()) {
            *slot = Some(Output::new(run, rate, due));
        }
        self
    }

    /// The attached output at `output`, as one immutable snapshot.
    pub fn attached_output(&self, output: OutputNumber) -> Option<Output> {
        self.outputs.get(output.slot()).copied().flatten()
    }

    /// A reroute schedule for `output`, with a fresh run and preserved adapter state.
    pub fn rerouted_output(&self, output: OutputNumber, run: Run, rate: Rate) -> Option<Output> {
        Some(
            self.attached_output(output)?
                .rerouted(run, rate, self.first_due(rate)),
        )
    }

    /// Installs an output schedule without exposing the schedule array.
    pub fn with_output_schedule(mut self, output: OutputNumber, schedule: Output) -> Self {
        if let Some(slot) = self.outputs.get_mut(output.slot()) {
            *slot = Some(schedule);
        }
        self
    }

    /// Clears every output from a schedule while retaining the performance state.
    pub fn without_output_schedules(mut self) -> Self {
        self.outputs.fill(None);
        self
    }

    /// Commits a switched schedule, carrying the old adapter status on reuse.
    pub fn commit_output_schedule(
        self,
        output: OutputNumber,
        mut schedule: Output,
        reuse: bool,
    ) -> Self {
        if reuse && let Some(old) = self.attached_output(output) {
            schedule = schedule.resumed_from(old);
        }
        self.with_output_schedule(output, schedule)
    }

    /// Whether `output` has an attached schedule.
    pub fn has_output(&self, output: OutputNumber) -> bool {
        self.attached_output(output).is_some()
    }

    /// The next due output deadline, when any output is attached.
    pub fn next_output_due(&self) -> Option<Duration> {
        self.outputs
            .iter()
            .flatten()
            .map(|output| output.deadline())
            .min()
    }

    /// Whether the attached output is reconnecting.
    pub fn output_reconnecting(&self, output: OutputNumber) -> bool {
        self.attached_output(output)
            .is_some_and(Output::is_reconnecting)
    }

    /// Copies the attached output's status and counters into a health sample.
    pub fn record_output_health(
        &self,
        output: OutputNumber,
        sample: &mut health::HealthSample,
    ) -> bool {
        let Some(attached) = self.attached_output(output) else {
            return false;
        };
        attached.record_health(sample);
        true
    }

    /// Every play the performance still owns, in slot order.
    pub fn active_plays(&self) -> impl Iterator<Item = &Play> {
        self.plays.iter()
    }

    /// Every playing or kept play the Mixer hears, in slot order.
    pub fn sounding_plays(&self) -> impl Iterator<Item = &Play> {
        self.plays.sounding()
    }

    /// The play currently occupying `slot`.
    pub fn play_at(&self, slot: usize) -> Option<Play> {
        self.plays.at(slot)
    }

    /// Whether a play currently occupies `slot`.
    pub fn has_play(&self, slot: usize) -> bool {
        self.plays.has(slot)
    }

    /// Whether a plan for `play` can still install.
    pub fn takes_plan(&self, play: PlayId) -> bool {
        self.plays.takes_plan(play)
    }

    /// The settled beat count now.
    pub fn beat_now(&self) -> BeatTime {
        self.clock.beat_at(self.clock.at())
    }

    /// Elapsed beat time from `start` to the settled instant now.
    pub fn elapsed_since(&self, start: BeatTime) -> Elapsed {
        Elapsed::between(self.beat_now(), start)
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

/// Who keeps Blackout on: the latch, and how many connections hold it. The front counts
/// them (`docs/design/channel-model.md`, slot 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BlackoutHold {
    pub latched: bool,
    pub holders: u32,
}

impl BlackoutHold {
    /// Nobody keeps it on.
    pub const NONE: Self = Self {
        latched: false,
        holders: 0,
    };

    /// Dark while the latch is on or any connection holds it.
    pub const fn dark(self) -> bool {
        self.latched || self.holders > 0
    }
}

/// What a connection asks of Blackout. The front decides with who asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlackoutAsk {
    /// Its own hold: `Command::Blackout`.
    Hold { on: bool },
    /// The latch, whoever asks: `Command::SetBlackoutLatch`.
    Latch { on: bool },
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
    /// Autopilot, blackout, Master and tempo stay.
    ReleaseAll,
    /// This connection holds Blackout, or lets it go. While the latch is on or any
    /// connection holds, every channel that follows Blackout goes to its dark: dimmers,
    /// emitters no dimmer carries, shutters with a Closed range. Fog, motion and the rest
    /// stay. The held levels stay and come back when off. A connection that closes lets
    /// its hold go in the next frame.
    Blackout { on: bool },
    /// The Blackout latch: on until a `SetBlackoutLatch` off, whoever sent it. No
    /// connection's hold changes it.
    SetBlackoutLatch { on: bool },
    /// How much shows of every channel that follows Master, 255 full: a dimmer at 70%
    /// under Master at 50% sends 35%. Full when the engine starts.
    SetMaster { level: u8 },
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
    /// How long from a press to the light is acceptable on this output, in µs, as the
    /// user sets it on its card. `None` clears it. 0 is not a number this takes.
    /// Refused (`NoSuchOutput`) while the output shows nothing.
    SetAcceptableDelay {
        output: OutputNumber,
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
        delay_us: Option<NonZeroU32>,
    },
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
    /// Who keeps Blackout on now, as the front counts them: dark while latched or held.
    Blackout {
        hold: BlackoutHold,
    },
    Master {
        level: u8,
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
    /// The patch's field table arrives with this command and takes the place of the one
    /// there, which goes back to be freed: each patched channel as its file says, and
    /// each light's fields. No offer changes.
    Fields,
    /// The grab at `entry` takes `change`. Offers that start or replace a grab's arrive
    /// with this command.
    Grab {
        entry: GrabEntry,
        change: GrabChange,
    },
    /// The grab changes of one press arrive with this command, each a [`EngineCommand::Grab`]
    /// the tick applies in order, all in one frame.
    Grabs,
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

/// The only place state changes: state and command in, new state out. `universes` are
/// the levels of the raw channels, `holds` which of them are held (a channel not held is
/// at 0), `grabs` the direct grabs by entry: buffers the caller owns; a command changes
/// them in place, so nothing allocates. No command needs anything done outside yet: the
/// first that does returns it as data beside the state, never in a `Vec`.
pub fn update(
    state: State,
    universes: &mut [Levels],
    holds: &mut [ChannelSet],
    grabs: &mut Grabs,
    command: EngineCommand,
) -> State {
    state.apply(universes, holds, grabs, command)
}

impl State {
    fn apply(
        self,
        universes: &mut [Levels],
        holds: &mut [ChannelSet],
        grabs: &mut Grabs,
        command: EngineCommand,
    ) -> State {
        let state = self;
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
            } => state.with_output(output, run, rate),
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
                grabs.release_all();
                let mut plays = state.plays;
                plays.release_where(|play| play.rank == Rank::Manual);
                State { plays, ..state }
            }
            EngineCommand::Blackout { hold } => State {
                blackout: hold.dark(),
                blackout_hold: hold,
                ..state
            },
            EngineCommand::Master { level } => State {
                master: level,
                ..state
            },
            // `Plan`: the plays stay; whether its plan installs is `Plays::takes_plan`.
            EngineCommand::Fields | EngineCommand::Grabs | EngineCommand::Plan { .. } => state,
            EngineCommand::Grab { entry, change } => {
                if grabs.change(entry, change, state.order) {
                    held
                } else {
                    state
                }
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
                grabs.unkeep(rules);
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
                State {
                    clock,
                    launches,
                    order,
                    ..state
                }
                .launch()
            }
            EngineCommand::Tap => State {
                clock: state.clock.tap(),
                ..state
            }
            .launch()
            .retempo(),
            EngineCommand::ReleaseTempo => State {
                clock: state.clock.release(),
                ..state
            }
            .retempo(),
            EngineCommand::Feed { bpm } => State {
                clock: state.clock.set_feed(bpm),
                ..state
            }
            .retempo(),
        }
    }

    /// The waiting plays whose beat the grid reached by now play.
    fn launch(self) -> State {
        let (mut plays, mut launches) = (self.plays, self.launches);
        plays.launch(&self.clock, &mut launches);
        State {
            plays,
            launches,
            ..self
        }
    }

    /// After a change of tempo: the plays started at once begin at the frame as it is now.
    fn retempo(self) -> State {
        let mut plays = self.plays;
        plays.begin_at(&self.clock, self.now);
        State { plays, ..self }
    }
}

/// The patched channels as their files say, and the follower index: the channels that
/// follow a modifier, a pair as one word.
pub type PatchedChannels<'a> = (&'a [Configured], &'a [Field]);

/// The bytes that go out now, written into `channels`, which the caller owns. Every
/// `patched` channel starts at its rest; `offer` makes each sounding play and grab offer
/// its words ([`Offers`]), and who wins takes each channel; then the raw channels held
/// (`raw` at `holds`) replace their bytes, and each of `followers` (a pair as one word)
/// moves by its modifiers: Master and the virtual dimmers, then Blackout. Allocates
/// nothing.
pub fn frame(
    state: &State,
    grabs: &Grabs,
    (raw, holds): (&[Levels], &[ChannelSet]),
    (patched, followers): PatchedChannels<'_>,
    channels: &mut Channels,
    offer: impl FnOnce(&mut Offers<'_>),
) {
    channels.reset(patched);
    offer(&mut Offers {
        channels,
        plays: &state.plays,
        grabs,
    });
    channels.apply_winners(followers, raw, holds, modifiers(state));
}

/// The level of each modifier slot this frame: Master as `SetMaster` left it, Blackout 0
/// while on.
fn modifiers(state: &State) -> [u8; MODIFIERS] {
    let mut levels = [u8::MAX; MODIFIERS];
    if let Some(master) = levels.get_mut(MASTER) {
        *master = state.master;
    }
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
impl State {
    fn next_wake(&self) -> Duration {
        self.outputs
            .iter()
            .flatten()
            .map(|output| output.deadline())
            .min()
            .unwrap_or(self.beat)
    }

    /// When a new output of `rate` is first due: at the next frame of an output that
    /// runs at that rate already, so they go out in one wake. At `now` if none does.
    fn first_due(&self, rate: Rate) -> Duration {
        self.outputs
            .iter()
            .flatten()
            .filter(|output| output.runs_at(rate))
            .map(|output| output.deadline())
            .min()
            .unwrap_or(self.now)
    }

    fn due(&self, now: Duration) -> impl Iterator<Item = OutputNumber> + '_ {
        let soon = now.saturating_add(SPIN);
        (1..)
            .zip(&self.outputs)
            .filter(move |(_, output)| output.is_some_and(|output| output.deadline() <= soon))
            .filter_map(|(number, _)| OutputNumber::try_from(number).ok())
    }

    fn wait(&self, now: Duration) -> (Duration, Option<Duration>) {
        let wake = self.next_wake();
        if self.outputs.iter().all(Option::is_none) {
            return (wake.saturating_sub(now), None);
        }
        (sleep_before(wake, now), Some(wake))
    }
}

/// The outputs to serve at `now`, by number: those due, and those due within [`SPIN`]
/// after. A wake costs a spin, so outputs due that close together go out in one.
pub fn due(state: &State, now: Duration) -> impl Iterator<Item = OutputNumber> + '_ {
    state.due(now)
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
    state.wait(now)
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
#[path = "../tests/unit/engine.rs"]
mod tests;
