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

pub use engine_core::{Adapter, Color, Levels, Outcome, OutputKind, Rate, Refusal, Run};

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

/// What a kind can send. An output whose run does not fit is refused when it is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most universes in one output's run.
    pub universes: u16,
}

/// What each kind can send: a DMX USB Pro one universe, DDP every universe an engine has.
pub const fn limits(kind: OutputKind) -> Limits {
    match kind {
        OutputKind::Ddp { .. } => Limits {
            universes: engine_core::MAX_UNIVERSES,
        },
        OutputKind::DmxUsbPro { .. } => Limits { universes: 1 },
    }
}

impl Limits {
    /// The run, if all of it fits; else why not, naming the limit.
    pub const fn check(self, run: Run) -> Result<Run, Refusal> {
        if run.count() <= self.universes {
            Ok(run)
        } else {
            Err(Refusal::TooManyUniverses {
                most: self.universes,
            })
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

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::{MAX_UNIVERSES, RunNumbers, Universes};
    use std::net::{Ipv4Addr, SocketAddrV4};

    #[test]
    fn each_kind_takes_the_runs_it_can_send() {
        let ddp = OutputKind::Ddp {
            address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
            color: Color::Rgb,
        };
        let pro = OutputKind::DmxUsbPro {
            adapter: Adapter::FIRST,
        };
        let too_many = |most| Err(Refusal::TooManyUniverses { most });
        // (kind, first, count, refused with)
        let cases = [
            (pro, 1, 1, Ok(())),
            (pro, 3, 1, Ok(())),
            (pro, 1, 2, too_many(1)),
            (pro, 1, MAX_UNIVERSES, too_many(1)),
            (ddp, 1, 1, Ok(())),
            (ddp, 1, MAX_UNIVERSES, Ok(())),
        ];
        for (kind, first, count, expected) in cases {
            let checked = Universes::new(MAX_UNIVERSES)
                .and_then(|most| RunNumbers { first, count }.check(most).ok())
                .map(|run| limits(kind).check(run).map(|_| ()));
            assert_eq!(checked, Some(expected), "{kind:?} {first}, {count}");
        }
    }
}
