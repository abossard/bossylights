//! The outputs. Every output, whatever its kind, is a [`Device`]: it takes a frame
//! without waiting and says what happened as an [`Outcome`]. The engine turns outcomes
//! into each output's state and counters (`engine_core::Status`, `engine_core::Counters`),
//! so they live in its state and nowhere else. Only this crate knows kinds and bytes.
//! A device that can vanish finds itself again off the output thread
//! ([`dmx_usb_pro`]): the engine sees only failed, then accepted frames.
//!
//! ```text
//! blocking pool: open(kind) -> Box<dyn Device>, then start its lane
//! tick thread:   gather route into a spare box, hand the box to the lane
//! output thread: device.take(stream) -> Outcome   never waits, never allocates
//! ```

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod ddp;
pub mod dmx_usb_pro;

pub use engine_core::{Adapter, Color, Levels, Outcome, OutputKind, Rate, Refusal, Run};
use engine_core::{ROUTE_MOST, RouteFault, SPANS_MOST, Span, route_length};

use std::io;
use std::num::NonZeroU16;

/// An output, opened and ready for the output thread. Made and dropped off it.
pub trait Device: Send {
    /// The route this opened device can send. A byte-oriented test sink takes any route.
    fn limits(&self) -> Limits {
        Limits::RAW
    }

    /// Whether the adapter already owns a worker that reopens a lost connection.
    fn reconnects_itself(&self) -> bool {
        false
    }

    /// Finishes accepted writes before a reroute. A lost port discards old-route retries.
    /// Called off the output thread, with exclusive access after the last `take`.
    fn finish_route(&mut self) {}

    /// Takes the route's bytes in order, without waiting or allocating. A busy device drops the frame.
    fn take(&mut self, stream: &[u8]) -> Outcome;
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

/// Whether a reroute must transfer the open device because it owns an exclusive port.
pub const fn exclusive(kind: OutputKind) -> bool {
    match kind {
        OutputKind::DmxUsbPro { .. } => true,
        OutputKind::Ddp { .. } => false,
    }
}

/// What a kind can send. An output whose run does not fit is refused when it is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most universes in one output's run.
    pub universes: u16,
    /// The most spans in one route.
    pub spans: usize,
    /// The most bytes in their joined stream.
    pub channels: u32,
    /// The run's channels go out in whole units of this many: the last channels of a run
    /// that make no whole unit are never sent. DDP: a pixel, 3 for RGB, 4 for RGBW.
    pub whole: NonZeroU16,
    /// The fastest rate it sends at (theory.md, idea 1: a USB DMX adapter only lower
    /// than its default).
    pub rate: Rate,
}

/// What each kind can send: a DMX USB Pro one universe of every channel, DDP every
/// universe an engine has, in whole pixels.
pub const fn limits(kind: OutputKind) -> Limits {
    match kind {
        OutputKind::Ddp { color, .. } => Limits::ddp(color),
        OutputKind::DmxUsbPro { .. } => Limits::USB,
    }
}

impl Limits {
    const USB: Self = Self {
        universes: 1,
        spans: 1,
        channels: engine_core::ADDRESSES as u32,
        whole: NonZeroU16::MIN,
        rate: Rate::FortyFour,
    };

    const fn ddp(color: Color) -> Self {
        Self {
            whole: ddp::pixel(color),
            ..Self::RAW
        }
    }

    /// A device brought through the engine's attach door takes every routed byte.
    pub const RAW: Self = Self {
        universes: engine_core::MAX_UNIVERSES,
        spans: SPANS_MOST,
        channels: ROUTE_MOST,
        whole: NonZeroU16::MIN,
        rate: Rate::Sixty,
    };

    /// The length in bytes of a route this kind can send.
    pub fn route(self, spans: &[Span]) -> Result<u32, RouteFault> {
        route_length(spans, self.spans, self.channels, self.whole)
    }

    /// The run, if all of it fits and the rate (`None`: the kind's default) is not
    /// faster than the kind sends; else why not.
    pub const fn check(self, run: Run, rate: Option<Rate>) -> Result<Run, Refusal> {
        if run.count() > self.universes {
            return Err(Refusal::TooManyUniverses {
                most: self.universes,
            });
        }
        match rate {
            Some(rate) if rate.per_second() > self.rate.per_second() => {
                Err(Refusal::TooFast { most: self.rate })
            }
            Some(_) | None => Ok(run),
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
    fn each_kind_checks_its_span_count_and_joined_length() {
        let ddp = OutputKind::Ddp {
            address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
            color: Color::Rgbw,
        };
        let usb = OutputKind::DmxUsbPro {
            adapter: Adapter::FIRST,
        };
        let span = |len| Span { start: 0, len };
        let cases = [
            (usb, vec![span(1); 2], None),
            (usb, vec![span(513)], None),
            (usb, vec![span(512)], Some(512)),
            (ddp, vec![span(319)], None),
            (ddp, vec![span(320)], Some(320)),
            (ddp, vec![span(64); SPANS_MOST], Some(ROUTE_MOST)),
            (ddp, vec![span(64); SPANS_MOST + 1], None),
            (ddp, vec![span(16_385); 4], None),
        ];
        for (kind, spans, expected) in cases {
            assert_eq!(limits(kind).route(&spans).ok(), expected, "{kind:?}");
        }
    }

    #[test]
    fn each_kind_takes_the_runs_it_can_send() {
        let ddp = OutputKind::Ddp {
            address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
            color: Color::Rgb,
        };
        let pro = OutputKind::DmxUsbPro {
            adapter: Adapter::FIRST,
        };
        let rgbw = OutputKind::Ddp {
            address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
            color: Color::Rgbw,
        };
        let too_many = |most| Err(Refusal::TooManyUniverses { most });
        let too_fast = Err(Refusal::TooFast {
            most: Rate::FortyFour,
        });
        // (kind, first, count, rate, refused with, channels of a whole unit).
        // Rates from theory.md, idea 1: USB DMX 44 or lower, DDP 30, 44, 60.
        let cases = [
            (pro, 1, 1, None, Ok(()), 1),
            (pro, 3, 1, None, Ok(()), 1),
            (pro, 1, 1, Some(Rate::Thirty), Ok(()), 1),
            (pro, 1, 1, Some(Rate::FortyFour), Ok(()), 1),
            (pro, 1, 1, Some(Rate::Sixty), too_fast, 1),
            (pro, 1, 2, None, too_many(1), 1),
            (pro, 1, 2, Some(Rate::Sixty), too_many(1), 1),
            (pro, 1, MAX_UNIVERSES, None, too_many(1), 1),
            (ddp, 1, 1, None, Ok(()), 3),
            (ddp, 1, 1, Some(Rate::Thirty), Ok(()), 3),
            (ddp, 1, 1, Some(Rate::Sixty), Ok(()), 3),
            (ddp, 1, MAX_UNIVERSES, None, Ok(()), 3),
            (rgbw, 1, MAX_UNIVERSES, Some(Rate::FortyFour), Ok(()), 4),
        ];
        for (kind, first, count, rate, expected, whole) in cases {
            let checked = Universes::new(MAX_UNIVERSES)
                .and_then(|most| RunNumbers { first, count }.check(most).ok())
                .map(|run| limits(kind).check(run, rate).map(|_| ()));
            assert_eq!(checked, Some(expected), "{kind:?} {first}, {count}");
            assert_eq!(limits(kind).whole.get(), whole, "{kind:?}");
        }
    }
}
