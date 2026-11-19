//! The engine API as data: what a client sends and what it gets back, as JSON over
//! one WebSocket. `crates/engine-server` moves it; this file says what it is.
//!
//! | Client sends | Engine answers |
//! |---|---|
//! | `{"Send": {"command": ...}}` | `"Accepted"`, `{"Patched": {"fixture": ...}}` for `Patch`, `{"Started": {"play": ...}}` for `StartLook`, `{"Grabbed": {"grab": ...}}` for a set, or `{"Refused": {"reason": ...}}`. `"Stopping"` once the engine stops: nothing changed |
//! | `"Query"` | `{"Picture": ...}`: the state, the project, the patch, every level and every held light |
//! | `"Subscribe"` | `{"Picture": ...}`, then `{"Changed": ...}` for each new frame, with the patch when it changed |
//! | `"Stop"` | `"Stopping"`, then every connection closes and the engine process ends |
//! | `{"Search": {"text": ...}}` | `{"Found": ...}`: the best matches of the fixture library |
//! | `{"Fixture": {"maker": ..., "model": ...}}` | `{"Fixture": ...}`: its modes and channels, or `NoSuchFixture` |
//! | `"Failures"` | `{"Failures": ...}`: the definition files that could not be read |
//! | anything else | `{"Refused": {"reason": "NotACommand"}}` |

use crate::library::{Failure, FixtureKind, Library, Match, ModeChannels, Text};
use crate::{
    ChannelLevel, Command, GrabId, Grouped, HeldLight, Levels, Patched, PlayId, Project, Property,
    Saved, ShowFile, State, Step,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The version of this API. `GET /version` answers it as text. A client that speaks
/// another version does not talk to the engine. Raise it with every change a client of
/// the old version would get wrong.
pub const API_VERSION: u16 = 9;

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

/// A definition has exactly one kind for its lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "both wire definitions are bounded and travel inline with the command"
)]
pub enum Definition {
    Step(Step),
    Sequence(Sequence),
}

/// Saved rows keep the step format that existing clients and files already read.
/// A sequence row carries its ordered references in the sequence field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    test,
    derive(ts_rs::TS),
    ts(type = "Saved | { look: number, name: string, sequence: Sequence }")
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
            definition: Definition::Step(saved.step),
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
        }
        row.end()
    }
}

impl<'de> Deserialize<'de> for SavedLook {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        fn present<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
            d: D,
        ) -> Result<Option<T>, D::Error> {
            T::deserialize(d).map(Some)
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Row {
            look: u16,
            name: Text,
            #[serde(default, deserialize_with = "present")]
            step: Option<Step>,
            #[serde(default, deserialize_with = "present")]
            sequence: Option<Sequence>,
        }
        let row = Row::deserialize(d)?;
        let definition = match (row.step, row.sequence) {
            (Some(step), None) => Definition::Step(step),
            (None, Some(sequence)) => Definition::Sequence(sequence),
            _ => {
                return Err(serde::de::Error::custom(
                    "a look has exactly one definition",
                ));
            }
        };
        Ok(Self {
            look: row.look,
            name: row.name,
            definition,
        })
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
