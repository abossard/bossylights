//! The outputs. Each one does two things: [`Device::take`] a frame without waiting, and
//! report what happened as an [`Outcome`]. The engine turns outcomes into each output's
//! state and counters (`engine_core::Status`, `engine_core::Counters`), so they live in
//! its state and nowhere else.
//!
//! ```text
//! API thread:    Device::open(kind)  -- socket made, connected, non-blocking --+
//!                                                                              v
//! output thread: Device::take(frame) -> Outcome      (never waits, never allocates)
//! ```
//!
//! Only this crate knows bytes (`docs/design/modules.md`).

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub mod ddp;

pub use engine_core::{Color, Levels, Outcome, OutputKind};

use std::io;

/// An output, opened and ready for the output thread.
pub enum Device {
    /// Boxed: its packet buffer is large, and a device travels through the engine's
    /// command queue. Made and freed off the output thread.
    Ddp(Box<ddp::Ddp>),
    /// Takes every frame and sends it nowhere.
    Mock,
}

impl Device {
    /// Opens the device for `kind`. Blocks for as long as the system takes: call it off
    /// the output thread.
    pub fn open(kind: OutputKind) -> io::Result<Self> {
        match kind {
            OutputKind::Ddp { address, color } => {
                ddp::Ddp::open(address, color).map(|ddp| Self::Ddp(Box::new(ddp)))
            }
            OutputKind::Mock => Ok(Self::Mock),
        }
    }

    /// Hands over a frame: the output's run of universes. Never waits: a device that is
    /// busy drops the frame.
    pub fn take(&mut self, frame: &[Levels]) -> Outcome {
        match self {
            Self::Ddp(ddp) => ddp.take(frame),
            Self::Mock => Outcome::Accepted,
        }
    }
}
