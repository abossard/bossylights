//! The engine API as data: what a client sends and what it gets back, as JSON over
//! one WebSocket. `crates/engine-server` moves it; this file says what it is.
//!
//! | Client sends | Engine answers |
//! |---|---|
//! | `{"Send": {"command": ...}}` | `"Accepted"`, or `{"Refused": {"reason": ...}}` |
//! | `"Query"` | `{"Picture": ...}`: the state and every level |
//! | `"Subscribe"` | `{"Picture": ...}`, then `{"Changed": ...}` for each new frame |
//! | `"Stop"` | `"Stopping"`, then every connection closes and the engine process ends |
//! | anything else | `{"Refused": {"reason": "NotACommand"}}` |

use crate::{ChannelLevel, ChannelNumbers, Command, Levels, OutputKind, RunNumbers, State};
use serde::{Deserialize, Serialize};

/// The version of this API. `GET /version` answers it as text. A client that speaks
/// another version does not talk to the engine. Raise it with every change a client of
/// the old version would get wrong.
pub const API_VERSION: u16 = 1;

/// What a client sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Request {
    /// Change the state. Answered at once, before the frame that carries it.
    Send {
        command: Command<ChannelNumbers, RunNumbers, OutputKind>,
    },
    /// The whole picture, once.
    Query,
    /// The whole picture, then what changed with every frame.
    Subscribe,
    /// Stop the engine in an orderly way: answer, close every connection, end.
    Stop,
}

/// What the engine sends back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Reply<'a> {
    /// The command is queued for the very next frame.
    Accepted,
    /// The engine stops: every connection closes, then the process ends.
    Stopping,
    /// The command was not taken. Nothing changed.
    Refused { reason: Refusal },
    /// The state and the held levels of every universe.
    Picture {
        state: State,
        universes: &'a [Levels],
    },
    /// A new frame: the state, and the channels whose held level changed since the last
    /// message.
    Changed {
        state: State,
        channels: &'a [ChannelLevel],
    },
}

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
}
