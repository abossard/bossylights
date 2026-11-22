//! The engine API as data: what a client sends and what it gets back, as JSON over
//! one WebSocket. `crates/engine-server` moves it; this file says what it is.
//!
//! | Client sends | Engine answers |
//! |---|---|
//! | `{"Send": {"command": ...}}` | `"Accepted"`, `{"Patched": {"fixture": ...}}` for `Patch`, `{"Started": {"play": ...}}` for `StartLook`, `{"Grabbed": {"grab": ...}}` for a set, or `{"Refused": {"reason": ...}}`. `"Stopping"` once the engine stops: nothing changed |
//! | `"Query"` | `{"Picture": ...}`: the state, the project, the patch, every level, every held light and output health |
//! | `"Subscribe"` | `{"Picture": ...}`, then `{"Changed": ...}` for new frames and `{"Health": ...}` for output timing and events |
//! | `"Stop"` | `"Stopping"`, then every connection closes and the engine process ends |
//! | `{"Search": {"text": ...}}` | `{"Found": ...}`: the best matches of the fixture library |
//! | `{"Fixture": {"maker": ..., "model": ...}}` | `{"Fixture": ...}`: its modes and channels, or `NoSuchFixture` |
//! | `"Failures"` | `{"Failures": ...}`: the definition files that could not be read |
//! | anything else | `{"Refused": {"reason": "NotACommand"}}` |

use crate::health;
use crate::library::{Failure, FixtureKind, Library, Match, ModeChannels, Text};
use crate::{
    ChannelLevel, Command, GrabId, Grouped, HeldLight, Levels, Light, OutputNumber, Patched,
    PlayId, Project, Property, Rgb, Saved, ShowFile, State, Status, Step, Target,
};
use core::time::Duration;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The version of this API. `GET /version` answers it as text. A client that speaks
/// another version does not talk to the engine. Raise it with every change a client of
/// the old version would get wrong.
pub const API_VERSION: u16 = 11;

/// Local time and phase of a root or a reference. Spread is a percentage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Use {
    #[cfg_attr(test, ts(optional = nullable))]
    pub speed: Speed,
    #[cfg_attr(test, ts(optional = nullable))]
    pub repeat: Repeat,
    #[cfg_attr(test, ts(optional = nullable))]
    pub spread: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Speed {
    Half,
    #[default]
    Normal,
    Double,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Repeat {
    #[default]
    Loop,
    Once,
}

/// Width is counted in sixteenths of a beat. Transition is the percentage of this
/// item's width spent moving from the preceding item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SequenceItem {
    pub look: u16,
    #[serde(default = "one_beat")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub width: u16,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub transition: u8,
    #[serde(default, rename = "use")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub usage: Use,
}

const fn one_beat() -> u16 {
    16
}

impl Default for SequenceItem {
    fn default() -> Self {
        Self {
            look: 0,
            width: one_beat(),
            transition: 0,
            usage: Use::default(),
        }
    }
}

/// A bounded wire list, carried in the same fixed-size command as groups. An
/// oversized command reaches admission as WorkLimit without allocating a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Sequence {
    #[cfg_attr(test, ts(type = "Array<SequenceItem>"))]
    items: Items,
}

impl Sequence {
    pub const MOST: usize = 64;

    pub fn new(items: &[SequenceItem]) -> Option<Self> {
        let mut storage = Items::default();
        storage.items.get_mut(..items.len())?.copy_from_slice(items);
        storage.len = u8::try_from(items.len()).ok()?;
        Some(Self { items: storage })
    }

    pub fn as_slice(&self) -> &[SequenceItem] {
        self.items
            .items
            .get(..usize::from(self.items.len))
            .unwrap_or_default()
    }

    pub fn too_many(&self) -> bool {
        usize::from(self.items.len) > Self::MOST
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Items {
    len: u8,
    items: [SequenceItem; Sequence::MOST],
}

impl Default for Items {
    fn default() -> Self {
        Self {
            len: 0,
            items: [SequenceItem::default(); Sequence::MOST],
        }
    }
}

impl Serialize for Items {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.items.iter().take(usize::from(self.len)))
    }
}

impl<'de> Deserialize<'de> for Items {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct List;
        impl<'de> serde::de::Visitor<'de> for List {
            type Value = Items;
            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("a list of sequence items")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut list: A,
            ) -> Result<Items, A::Error> {
                let mut storage = Items::default();
                while let Some(item) = list.next_element()? {
                    if let Some(slot) = storage.items.get_mut(usize::from(storage.len)) {
                        *slot = item;
                    }
                    storage.len = storage.len.saturating_add(1);
                }
                Ok(storage)
            }
        }
        d.deserialize_seq(List)
    }
}

/// Ordered RGB16 stops. The decoder rejects a fifth swatch before storing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(type = "Array<Rgb>"))]
pub struct Swatches {
    colours: [Rgb; Self::MOST],
    len: u8,
}

impl Default for Swatches {
    fn default() -> Self {
        Self {
            colours: [Rgb::BLACK; Self::MOST],
            len: 0,
        }
    }
}

impl Swatches {
    pub const MOST: usize = 4;

    pub fn new(colours: &[Rgb]) -> Option<Self> {
        let mut swatches = Self::default();
        swatches
            .colours
            .get_mut(..colours.len())?
            .copy_from_slice(colours);
        swatches.len = u8::try_from(colours.len()).ok()?;
        Some(swatches)
    }

    pub fn as_slice(&self) -> &[Rgb] {
        self.colours
            .get(..usize::from(self.len))
            .unwrap_or_default()
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Serialize for Swatches {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.as_slice().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Swatches {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct List;
        impl<'de> serde::de::Visitor<'de> for List {
            type Value = Swatches;

            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("up to four RGB swatches")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut list: A,
            ) -> Result<Swatches, A::Error> {
                let mut swatches = Swatches::default();
                while let Some(colour) = list.next_element()? {
                    let slot = swatches
                        .colours
                        .get_mut(usize::from(swatches.len))
                        .ok_or_else(|| {
                            serde::de::Error::invalid_length(Swatches::MOST + 1, &self)
                        })?;
                    *slot = colour;
                    swatches.len += 1;
                }
                Ok(swatches)
            }
        }
        d.deserialize_seq(List)
    }
}

/// A step may also lend its swatches to a Wave. Only its target and values play directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AuthoredStep {
    pub target: Target,
    pub values: Light,
    #[serde(default, skip_serializing_if = "Swatches::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub swatches: Swatches,
}

impl AuthoredStep {
    pub const fn step(&self) -> Step {
        Step {
            target: self.target,
            values: self.values,
        }
    }
}

impl From<Step> for AuthoredStep {
    fn from(step: Step) -> Self {
        Self {
            target: step.target,
            values: step.values,
            swatches: Swatches::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Pattern {
    Wave {
        target: WaveTarget,
        period: u8,
        palette: Palette,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum WaveTarget {
    Group {
        #[cfg_attr(test, ts(type = "string"))]
        group: Text,
    },
    Matrix {
        fixture: u16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Palette {
    Inline(Swatches),
    Step { look: u16 },
}

/// A definition has exactly one kind for its lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "the wire definitions are bounded and travel inline with the command"
)]
pub enum Definition {
    Step(AuthoredStep),
    Sequence(Sequence),
    Pattern(Pattern),
}

/// Each saved row contains exactly one definition, under `step`, `sequence` or `pattern`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    test,
    derive(ts_rs::TS),
    ts(
        type = "{ look: number, name: string, step: AuthoredStep } | { look: number, name: string, sequence: Sequence } | { look: number, name: string, pattern: Pattern }"
    )
)]
pub struct SavedLook {
    pub look: u16,
    pub name: Text,
    pub definition: Definition,
}

impl From<Saved> for SavedLook {
    fn from(saved: Saved) -> Self {
        Self {
            look: saved.look,
            name: saved.name,
            definition: Definition::Step(saved.step.into()),
        }
    }
}

impl Serialize for SavedLook {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut row = s.serialize_struct("SavedLook", 3)?;
        row.serialize_field("look", &self.look)?;
        row.serialize_field("name", &self.name)?;
        match &self.definition {
            Definition::Step(step) => row.serialize_field("step", step)?,
            Definition::Sequence(sequence) => row.serialize_field("sequence", sequence)?,
            Definition::Pattern(pattern) => row.serialize_field("pattern", pattern)?,
        }
        row.end()
    }
}

// A present key must contain a definition, even when another key is present too.
fn present<'de, T: Deserialize<'de>, D: Deserializer<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "S: Deserialize<'de>"))]
struct Row<S> {
    look: u16,
    name: Text,
    #[serde(default, deserialize_with = "present")]
    step: Option<S>,
    #[serde(default, deserialize_with = "present")]
    sequence: Option<Sequence>,
    #[serde(default, deserialize_with = "present")]
    pattern: Option<Pattern>,
}

impl<S: Into<AuthoredStep>> Row<S> {
    fn checked<E: serde::de::Error>(self) -> Result<SavedLook, E> {
        let definition = match (self.step, self.sequence, self.pattern) {
            (Some(step), None, None) => Definition::Step(step.into()),
            (None, Some(sequence), None) => Definition::Sequence(sequence),
            (None, None, Some(pattern)) => Definition::Pattern(pattern),
            _ => return Err(E::custom("a look has exactly one definition")),
        };
        Ok(SavedLook {
            look: self.look,
            name: self.name,
            definition,
        })
    }
}

impl SavedLook {
    pub fn has_swatches(&self) -> bool {
        matches!(&self.definition, Definition::Step(step) if !step.swatches.is_empty())
    }
}

impl<'de> Deserialize<'de> for SavedLook {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Row::<AuthoredStep>::deserialize(d)?.checked()
    }
}

/// What a client sends. A key it does not know, here or in any type it carries, refuses
/// the request: a misspelled field would otherwise be ignored without a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[expect(
    clippy::large_enum_variant,
    reason = "a client's command has no heap: `Patch` carries its four names in place, `SetGroup` its members. Moved once per request, off the tick thread"
)]
pub enum Request {
    /// Change the state. Answered at once, before the frame that carries it.
    Send { command: Command },
    /// The whole picture, once.
    Query,
    /// The whole picture, then what changed with every frame.
    Subscribe,
    /// Stop the engine in an orderly way: answer, close every connection, end.
    Stop,
    /// The fixtures of the library whose maker and model match `text`, best first.
    Search {
        #[cfg_attr(test, ts(type = "string"))]
        text: Text,
    },
    /// One fixture of the library: its modes, each channel by name and function.
    Fixture {
        #[cfg_attr(test, ts(type = "string"))]
        maker: Text,
        #[cfg_attr(test, ts(type = "string"))]
        model: Text,
    },
    /// The definition files that could not be read, and why.
    Failures,
}

/// Timing and device status for each attached output, in output number order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Health<'a> {
    pub window_ms: u16,
    pub outputs: &'a [OutputHealth],
}

// Commands read a one-based output number. Replies write it with the same numbering.
impl Serialize for OutputNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (self.slot() + 1).serialize(serializer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OutputHealth {
    pub output: OutputNumber,
    #[cfg_attr(test, ts(type = "number"))]
    pub generation: u64,
    pub status: Status,
    pub state: OutputHealthState,
    pub measurement: OutputMeasurement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum OutputHealthState {
    Measuring,
    Healthy,
    Late { reason: LateReason },
    Recovering { reason: LateReason },
}

impl From<health::HealthState> for Option<OutputHealthState> {
    fn from(state: health::HealthState) -> Self {
        Some(match state {
            health::HealthState::Absent => return None,
            health::HealthState::Measuring => OutputHealthState::Measuring,
            health::HealthState::Healthy => OutputHealthState::Healthy,
            health::HealthState::Late(reason) => OutputHealthState::Late {
                reason: reason.into(),
            },
            health::HealthState::Recovering(reason) => OutputHealthState::Recovering {
                reason: reason.into(),
            },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum OutputMeasurement {
    Measuring,
    ClockDiscontinuity,
    Window {
        #[cfg_attr(test, ts(type = "number"))]
        end_ms: u64,
        lateness: Option<LatenessWindow>,
        press_to_light: Option<PressWindow>,
        press_missing: u32,
        dropped: u32,
        failed: u32,
        off_beat: u32,
        frames_handed: u32,
    },
}

impl From<(Duration, health::Window)> for OutputMeasurement {
    fn from((end, window): (Duration, health::Window)) -> Self {
        Self::Window {
            end_ms: u64::try_from(end.as_millis()).unwrap_or(u64::MAX),
            lateness: window.lateness.map(Into::into),
            press_to_light: window.press_to_light.map(Into::into),
            press_missing: window.press_missing,
            dropped: window.dropped,
            failed: window.failed,
            off_beat: window.off_beat,
            frames_handed: window.frames_handed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LatenessWindow {
    pub p99_us: u32,
    pub worst_us: u32,
    pub samples: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PressWindow {
    pub p95_us: u32,
    pub worst_us: u32,
    pub samples: u32,
}

fn micros(time: Duration) -> u32 {
    u32::try_from(time.as_micros()).unwrap_or(u32::MAX)
}

impl From<health::MetricWindow> for LatenessWindow {
    fn from(metric: health::MetricWindow) -> Self {
        Self {
            p99_us: micros(metric.percentile),
            worst_us: micros(metric.worst),
            samples: metric.samples,
        }
    }
}

impl From<health::MetricWindow> for PressWindow {
    fn from(metric: health::MetricWindow) -> Self {
        Self {
            p95_us: micros(metric.percentile),
            worst_us: micros(metric.worst),
            samples: metric.samples,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum LateReason {
    HandoffLateness,
    PressToLight,
    Both,
}

impl From<health::LateReason> for LateReason {
    fn from(reason: health::LateReason) -> Self {
        match reason {
            health::LateReason::HandoffLateness => Self::HandoffLateness,
            health::LateReason::PressToLight => Self::PressToLight,
            health::LateReason::Both => Self::Both,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum RecoveredReason {
    WithinTargets,
    OutputAvailable,
}

impl From<health::RecoveredReason> for RecoveredReason {
    fn from(reason: health::RecoveredReason) -> Self {
        match reason {
            health::RecoveredReason::WithinTargets => Self::WithinTargets,
            health::RecoveredReason::OutputAvailable => Self::OutputAvailable,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OutputHealthEvent {
    pub output: OutputNumber,
    #[cfg_attr(test, ts(type = "number"))]
    pub generation: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
    pub event: HealthEvent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum HealthEvent {
    RunningLate { reason: LateReason },
    Recovered { reason: RecoveredReason },
    OutputLost,
}

impl From<health::HealthEvent> for HealthEvent {
    fn from(event: health::HealthEvent) -> Self {
        match event {
            health::HealthEvent::RunningLate(reason) => Self::RunningLate {
                reason: reason.into(),
            },
            health::HealthEvent::Recovered(reason) => Self::Recovered {
                reason: reason.into(),
            },
            health::HealthEvent::OutputLost => Self::OutputLost,
        }
    }
}

/// What the engine sends back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Reply<'a> {
    /// The command is queued for the very next frame.
    Accepted,
    /// The fixture is patched as `fixture`; its writers are queued for the very next frame.
    Patched { fixture: u16 },
    /// The look plays as `play` from the very next frame.
    Started { play: PlayId },
    /// The grab holds from the very next frame.
    Grabbed { grab: GrabId },
    /// The engine stops: every connection closes, then the process ends.
    Stopping,
    /// The command was not taken. Nothing changed.
    Refused { reason: Refusal },
    /// The state, the project, the held levels of every universe and the held parts of
    /// every light.
    Picture {
        state: State,
        project: Project,
        library: Library,
        /// Every patched fixture, by number.
        patch: &'a [Patched<'a>],
        /// Every group, in the order they were made.
        groups: &'a [Grouped<'a>],
        /// Every saved look, by number.
        looks: &'a [SavedLook],
        /// Whether the patch, the groups and the looks are kept in a show file.
        show: ShowFile,
        universes: &'a [Levels],
        /// Every light that holds a part, by number.
        lights: &'a [HeldLight],
        health: Health<'a>,
    },
    /// Current rows and the events since this subscriber's last health delivery.
    Health {
        health: Health<'a>,
        events: &'a [OutputHealthEvent],
        missed: u32,
    },
    /// A new frame: the state, and the channels and the lights whose held values changed
    /// since the last message.
    Changed {
        state: State,
        project: Project,
        library: Library,
        /// The whole patch when it changed since the last message, else `None`.
        patch: Option<&'a [Patched<'a>]>,
        /// Every group when the patch changed since the last message, else `None`.
        groups: Option<&'a [Grouped<'a>]>,
        /// Every saved look when they changed since the last message, else `None`.
        looks: Option<&'a [SavedLook]>,
        /// Whether the patch, the groups and the looks are kept in a show file.
        show: ShowFile,
        channels: &'a [ChannelLevel],
        /// Every light whose held parts changed, as it holds them now: a part let go is
        /// `None`.
        lights: &'a [HeldLight],
    },
    /// The answer to `Search`: at most [`MATCHES`], best first. Empty while the library
    /// is not read.
    Found { fixtures: &'a [Match<'a>] },
    /// The answer to `Fixture`.
    Fixture {
        maker: &'a str,
        model: &'a str,
        kind: FixtureKind<'a>,
        modes: &'a [ModeChannels<'a>],
    },
    /// The answer to `Failures`, in the order of the files' names.
    Failures { failures: &'a [Failure<'a>] },
}

/// The most fixtures one `Found` carries.
pub const MATCHES: usize = 50;

/// Why a command was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Refusal {
    /// Not one of the listed requests or commands, or not in their shape.
    NotACommand,
    /// This engine's universes are 1 to `universes`.
    NoSuchUniverse {
        universes: u16,
    },
    /// Addresses are 1 to 512.
    NoSuchAddress,
    /// More commands are waiting than the engine takes in one frame. Send again.
    QueueFull,
    /// The output's device or socket could not be opened.
    CannotOpen,
    /// The library has no fixture of that maker and model.
    NoSuchFixture,
    /// The output's device is being opened. Send again once it is open.
    Opening,
    /// This kind of output takes at most `most` universes.
    TooManyUniverses {
        most: u16,
    },
    /// The fixture has no mode of that name.
    NoSuchMode,
    /// The mode's `channels` run past the engine's last universe from there.
    PastTheEnd {
        channels: u16,
    },
    /// The fixture would run over universes `first` to `last`, and no output streams
    /// them all as one run.
    NotStreamed {
        first: u16,
        last: u16,
    },
    /// Output `output` streams the fixture's channel at `universe`, `address` and never
    /// sends it: its run ends in channels that make no whole unit (a pixel, for some kinds).
    NotSent {
        output: u8,
        universe: u16,
        address: u16,
    },
    /// Then fixture `fixture` would lose a channel: no output would stream every universe
    /// it runs over, or an output would stream one of its channels and never send it.
    StreamsFixture {
        fixture: u16,
    },
    /// The fixture would be on a channel of fixture `fixture`: no two fixtures share one.
    Overlaps {
        fixture: u16,
    },
    /// The fixture's lights do not fit in the lights the engine has left.
    NoLightsLeft,
    /// No fixture is patched with that number.
    NotPatched,
    /// No patched fixture has that light.
    NoSuchLight,
    /// No group has that name.
    NoSuchGroup,
    /// A group has at most `most` members ([`crate::Members::MOST`] says why).
    TooManyMembers {
        most: u16,
    },
    /// Then the show file would be longer than its `most` bytes, and would not open.
    ShowFileFull {
        most: u32,
    },
    /// No look is saved with that number.
    NoSuchLook,
    WrongLookKind,
    LookCycle,
    InvalidTiming,
    /// A Wave needs at least one swatch in its inline or referenced palette.
    EmptyPalette,
    /// The fixture has no matrix layout matching its heads.
    NoMatrix,
    WorkLimit,
    /// Look `look` uses it: a saved look's target, or a play of the look.
    LookInUse {
        look: u16,
    },
    /// At most `most` looks are saved.
    TooManyLooks {
        most: u16,
    },
    /// Every play slot is taken. Release a play first.
    PlayFull,
    /// Light `light` (counted from 1) has a wheel for `property` but no slot that shows
    /// the value: black on a wheel without black. Or a fixture mixes colour beside a
    /// colour wheel that has no neutral slot to hold.
    Unsupported {
        light: u16,
        property: Property,
    },
    /// Another change came first: the command was checked against the project, the
    /// patch or the looks as they were before it. Nothing changed. Send it again.
    Stale,
    /// The group has no members: a saved look's target names at least one light.
    EmptyGroup,
    /// The grab holds `property`: an update gives a value of it.
    OtherProperty {
        property: Property,
    },
    /// The live grabs keep `most` runs of lights. End or release a grab first.
    GrabFull {
        most: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::{LatenessWindow, PressWindow};
    use crate::health::MetricWindow;
    use core::time::Duration;

    #[test]
    fn metric_times_are_whole_microseconds_saturated_to_u32() {
        for (percentile, worst, p_us, worst_us) in [
            (Duration::from_nanos(999), Duration::from_nanos(2_999), 0, 2),
            (
                Duration::from_micros(u64::from(u32::MAX) + 1),
                Duration::MAX,
                u32::MAX,
                u32::MAX,
            ),
        ] {
            let metric = MetricWindow {
                percentile,
                worst,
                samples: 20,
            };
            assert_eq!(
                LatenessWindow::from(metric),
                LatenessWindow {
                    p99_us: p_us,
                    worst_us,
                    samples: 20
                }
            );
            assert_eq!(
                PressWindow::from(metric),
                PressWindow {
                    p95_us: p_us,
                    worst_us,
                    samples: 20
                }
            );
        }
    }
}
