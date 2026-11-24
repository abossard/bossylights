//! Plain values that describe an output across the API and the tick.

use core::time::Duration;
use serde::{Deserialize, Serialize};

/// The most outputs an engine has at once.
pub const MAX_OUTPUTS: usize = 16;

/// Which output, 1 to [`crate::MAX_OUTPUTS`] as a client counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u8")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "number"))]
pub struct OutputNumber(u8);

impl OutputNumber {
    /// The number for a zero-based output slot, if the slot exists.
    pub const fn from_slot(slot: usize) -> Option<Self> {
        if slot < MAX_OUTPUTS {
            Some(Self(slot as u8))
        } else {
            None
        }
    }

    /// Counted from 0, below [`crate::MAX_OUTPUTS`].
    pub const fn slot(self) -> usize {
        self.0 as usize
    }
}

impl TryFrom<u8> for OutputNumber {
    type Error = &'static str;
    fn try_from(number: u8) -> Result<Self, Self::Error> {
        if number >= 1 && usize::from(number) <= MAX_OUTPUTS {
            Ok(Self(number - 1))
        } else {
            Err("outputs are 1 to 16")
        }
    }
}

/// Frames a second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Rate {
    #[serde(rename = "30")]
    Thirty,
    #[serde(rename = "44")]
    FortyFour,
    #[serde(rename = "60")]
    Sixty,
}

impl Rate {
    pub const fn per_second(self) -> u32 {
        match self {
            Self::Thirty => 30,
            Self::FortyFour => 44,
            Self::Sixty => 60,
        }
    }

    pub const fn period(self) -> Duration {
        Duration::from_nanos(1_000_000_000 / self.per_second() as u64)
    }
}

/// Where an attached output is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Status {
    /// No frame accepted since the output was set. Served like any other status: it
    /// stays so the user tells a device never seen from one lost (`Reconnecting`).
    NotFound,
    Connecting,
    Running,
    /// Sends failed; `failures` in a row so far, counted up to `u16::MAX`.
    Reconnecting {
        failures: u16,
    },
}

/// What happened when a frame was handed to an output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The device took all of it. For a kind with a thread of its own: that thread took it.
    Accepted,
    /// The device was busy: the frame was dropped, nothing waited.
    Dropped,
    /// The device refused or is gone.
    Failed,
}

/// Whether the engine's recovery worker holds a request for an output's lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watch {
    Unwatched,
    Watched,
}
