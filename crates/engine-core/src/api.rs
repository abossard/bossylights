//! The engine API as data: what a client sends and what it gets back, as JSON over
//! one WebSocket. `crates/engine-server` moves it; this file says what it is.
//!
//! | Client sends | Engine answers |
//! |---|---|
//! | `{"Send": {"command": ...}}` | `"Accepted"`, or `{"Refused": {"reason": ...}}` |
//! | `"Query"` | `{"Picture": ...}`: the state and every level |
//! | `"Subscribe"` | `{"Picture": ...}`, then `{"Changed": ...}` for each new frame |
//! | anything else | `{"Refused": {"reason": "NotACommand"}}` |

use crate::{ChannelLevel, ChannelNumbers, Command, Levels, State};
use serde::{Deserialize, Serialize};

/// What a client sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Request {
    /// Change the state. Answered at once, before the frame that carries it.
    Send { command: Command<ChannelNumbers> },
    /// The whole picture, once.
    Query,
    /// The whole picture, then what changed with every frame.
    Subscribe,
}

/// What the engine sends back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Reply<'a> {
    /// The command is queued for the very next frame.
    Accepted,
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
}
