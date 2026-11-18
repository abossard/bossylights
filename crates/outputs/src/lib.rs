//! The outputs. Every output, whatever its kind, is a [`Device`]: it takes a frame
//! without waiting and says what happened as an [`Outcome`]. The engine turns outcomes
//! into each output's state and counters (`engine_core::Status`, `engine_core::Counters`),
//! so they live in its state and nowhere else. Only this crate knows kinds and bytes.
//! A device that can vanish finds itself again off the output thread
//! ([`dmx_usb_pro`]): the engine sees only failed, then accepted frames.
//!
//! ```text
//! API thread:    open(kind) -> Box<dyn Device>   socket made, connected, non-blocking
//!                    |  engine: Sender::attach (the same door for a test's own device)
//!                    v
//! output thread: device.take(frame) -> Outcome   never waits, never allocates
//! ```

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub mod ddp;
pub mod dmx_usb_pro;

pub use engine_core::{Color, Levels, Outcome, OutputKind, Rate};

use std::io;

/// An output, opened and ready for the output thread. Made and dropped off it.
pub trait Device: Send {
    /// Hands over a frame: the output's run of universes. Never waits: a device that is
    /// busy drops the frame.
    fn take(&mut self, frame: &[Levels]) -> Outcome;
}

/// Opens the device for `kind`. Blocks for as long as the system takes: call it off the
/// output thread.
pub fn open(kind: OutputKind) -> io::Result<Box<dyn Device>> {
    match kind {
        OutputKind::Ddp { address, color } => {
            Ok(Box::new(ddp::Ddp::open(address, color)?) as Box<dyn Device>)
        }
        OutputKind::DmxUsbPro { adapter } => {
            let find = dmx_usb_pro::find(adapter);
            Ok(Box::new(dmx_usb_pro::DmxUsbPro::open(find)?) as Box<dyn Device>)
        }
    }
}

/// The rate when the command names none: 60 for DDP, 44 for DMX kinds.
pub const fn default_rate(kind: OutputKind) -> Rate {
    match kind {
        OutputKind::Ddp { .. } => Rate::Sixty,
        OutputKind::DmxUsbPro { .. } => Rate::FortyFour,
    }
}
