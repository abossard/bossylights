//! The list of output kinds a client can ask for, as data and nothing else. Nothing in
//! the engine acts on a kind: `crates/outputs` opens it, knows its bytes and its default
//! rate. A new kind is a new row here and its code there.

use core::net::SocketAddrV4;
use serde::{Deserialize, Serialize};

/// What kind of output, and where it sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum OutputKind {
    /// DDP over UDP, as WLED takes it. `address` is `"192.168.1.50:4048"`.
    Ddp {
        #[cfg_attr(test, ts(type = "string"))]
        address: SocketAddrV4,
        /// Whole pixels only: RGB sends 510 of a universe's 512 channels, RGBW all 512.
        color: Color,
    },
    /// An ENTTEC DMX USB Pro, found by its USB identity. One universe, 512 channels.
    DmxUsbPro { adapter: Adapter },
}

/// Which DMX USB Pro: 1 is the first such adapter in the order of their USB serial
/// numbers. 0 does not parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8")]
#[cfg_attr(test, derive(ts_rs::TS), ts(type = "number"))]
pub struct Adapter(u8);

impl Adapter {
    pub const FIRST: Self = Self(1);

    /// Counted from 1.
    pub const fn number(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for Adapter {
    type Error = &'static str;
    fn try_from(number: u8) -> Result<Self, Self::Error> {
        if number >= 1 {
            Ok(Self(number))
        } else {
            Err("adapters are counted from 1")
        }
    }
}

/// Channels per pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Color {
    Rgb,
    Rgbw,
}

/// A kind for tests elsewhere in `engine-core`, which name none.
#[cfg(test)]
pub const EXAMPLE: OutputKind = OutputKind::Ddp {
    address: core::net::SocketAddrV4::new(core::net::Ipv4Addr::LOCALHOST, 4048),
    color: Color::Rgb,
};

/// Another kind, for tests that need two.
#[cfg(test)]
pub const OTHER: OutputKind = OutputKind::DmxUsbPro {
    adapter: Adapter::FIRST,
};
