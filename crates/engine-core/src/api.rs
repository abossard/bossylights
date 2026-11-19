//! The engine API as data: what a client sends and what it gets back, as JSON over
//! one WebSocket. `crates/engine-server` moves it; this file says what it is.
//!
//! | Client sends | Engine answers |
//! |---|---|
//! | `{"Send": {"command": ...}}` | `"Accepted"`, `{"Patched": {"fixture": ...}}` for `Patch`, or `{"Refused": {"reason": ...}}` |
//! | `"Query"` | `{"Picture": ...}`: the state, the project, the patch and every level |
//! | `"Subscribe"` | `{"Picture": ...}`, then `{"Changed": ...}` for each new frame, with the patch when it changed |
//! | `"Stop"` | `"Stopping"`, then every connection closes and the engine process ends |
//! | `{"Search": {"text": ...}}` | `{"Found": ...}`: the best matches of the fixture library |
//! | `{"Fixture": {"maker": ..., "model": ...}}` | `{"Fixture": ...}`: its modes and channels, or `NoSuchFixture` |
//! | `"Failures"` | `{"Failures": ...}`: the definition files that could not be read |
//! | anything else | `{"Refused": {"reason": "NotACommand"}}` |

use crate::library::{Failure, FixtureKind, Library, Match, ModeChannels, Text};
use crate::{ChannelLevel, Command, Grouped, Levels, Patched, Project, ShowFile, State};
use serde::{Deserialize, Serialize};

/// The version of this API. `GET /version` answers it as text. A client that speaks
/// another version does not talk to the engine. Raise it with every change a client of
/// the old version would get wrong.
pub const API_VERSION: u16 = 2;

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
    /// The engine stops: every connection closes, then the process ends.
    Stopping,
    /// The command was not taken. Nothing changed.
    Refused { reason: Refusal },
    /// The state, the project and the held levels of every universe.
    Picture {
        state: State,
        project: Project,
        library: Library,
        /// Every patched fixture, by number.
        patch: &'a [Patched<'a>],
        /// Every group, in the order they were made.
        groups: &'a [Grouped<'a>],
        /// Whether the patch and the groups are kept in a show file.
        show: ShowFile,
        universes: &'a [Levels],
    },
    /// A new frame: the state, and the channels whose held level changed since the last
    /// message.
    Changed {
        state: State,
        project: Project,
        library: Library,
        /// The whole patch when it changed since the last message, else `None`.
        patch: Option<&'a [Patched<'a>]>,
        /// Every group when the patch changed since the last message, else `None`.
        groups: Option<&'a [Grouped<'a>]>,
        /// Whether the patch and the groups are kept in a show file.
        show: ShowFile,
        channels: &'a [ChannelLevel],
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
    NoSuchUniverse { universes: u16 },
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
    TooManyUniverses { most: u16 },
    /// The fixture has no mode of that name.
    NoSuchMode,
    /// The mode's `channels` run past the engine's last universe from there.
    PastTheEnd { channels: u16 },
    /// The fixture would run over universes `first` to `last`, and no output streams
    /// them all as one run.
    NotStreamed { first: u16, last: u16 },
    /// Then no output would stream every universe fixture `fixture` runs over.
    StreamsFixture { fixture: u16 },
    /// The fixture's lights do not fit in the lights the engine has left.
    NoLightsLeft,
    /// No fixture is patched with that number.
    NotPatched,
    /// No patched fixture has that light.
    NoSuchLight,
    /// No group has that name.
    NoSuchGroup,
    /// A group has at most `most` members ([`crate::Members::MOST`] says why).
    TooManyMembers { most: u16 },
    /// Then the show file would be longer than its `most` bytes, and would not open.
    ShowFileFull { most: u32 },
}
