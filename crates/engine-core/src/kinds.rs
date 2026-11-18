//! The list of output kinds a client can ask for, as data and nothing else. Nothing in
//! the engine acts on a kind: `crates/outputs` opens it, knows its bytes and its default
//! rate. A new kind is a new row here and its code there.

use core::net::SocketAddrV4;
use serde::Deserialize;

/// What kind of output, and where it sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum OutputKind {
    /// DDP over UDP, as WLED takes it. `address` is `"192.168.1.50:4048"`.
    Ddp {
        #[cfg_attr(test, ts(type = "string"))]
        address: SocketAddrV4,
        color: Color,
    },
}

/// Channels per pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Color {
    Rgb,
    Rgbw,
}
