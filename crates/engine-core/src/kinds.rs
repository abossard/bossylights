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
        color: Color,
    },
    /// An ENTTEC DMX USB Pro, found by its USB identity: `adapter` 1 is the first such
    /// adapter in the order of their USB serial numbers. One universe, 512 channels.
    DmxUsbPro { adapter: u8 },
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
pub const OTHER: OutputKind = OutputKind::DmxUsbPro { adapter: 1 };
