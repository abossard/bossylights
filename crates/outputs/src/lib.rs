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

pub mod artnet;
mod ddp;
pub mod dmx_usb_pro;
pub mod idn;
pub mod sacn;
mod udp;

use engine_core::health::{AfterHandoff, HandoffPart, HandoffSpan};
pub use engine_core::{Adapter, Color, Levels, Outcome, OutputKind, Rate, Refusal, Run};
use engine_core::{ROUTE_MOST, RouteFault, SPANS_MOST, Span, route_length};

use std::io;
use std::num::NonZeroU16;
use std::ops::RangeInclusive;
use std::sync::Weak;
use std::time::Duration;

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

    /// What it reads about itself, for [`after_handoff`]. Most devices read nothing.
    /// Called off the output thread.
    fn reading(&self) -> Reading {
        Reading::default()
    }

    /// Finishes accepted writes before a reroute. A lost port discards old-route retries.
    /// Called off the output thread, with exclusive access after the last `take`.
    fn finish_route(&mut self) {}

    /// Ends the output's stream for good: a protocol that tells its receivers so sends it
    /// now (sACN: three rounds of "stream terminated", and nothing after). Others do nothing.
    /// Called off the output thread, with exclusive access after the last `take`, when the
    /// output is removed or the engine stops; never on a reroute.
    fn terminate(&mut self) {}

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

/// What an open device read about itself, readable from any thread: the settings a DMX
/// USB Pro's widget answered. Reads nothing once its device is gone.
#[derive(Clone, Default)]
pub struct Reading(Weak<dmx_usb_pro::Shared>);

impl Reading {
    /// Whether its device is still open.
    pub fn is_open(&self) -> bool {
        self.0.strong_count() > 0
    }

    /// The settings its device read, if it is open and read them.
    pub fn settings(&self) -> Option<dmx_usb_pro::Settings> {
        self.0.upgrade()?.settings()
    }
}

/// What follows the handoff on the way to the light, for an output of `kind` whose
/// lowest and highest patched channels, counted from 1 in its stream, are `patched`.
/// `None`: a device brought through the engine's attach door, its kind not known.
/// `read`: the settings its device read ([`Reading`]), if any.
pub fn after_handoff(
    kind: Option<OutputKind>,
    patched: Option<RangeInclusive<u32>>,
    read: Option<dmx_usb_pro::Settings>,
) -> AfterHandoff {
    let not_known = |stage| {
        AfterHandoff::new([HandoffPart {
            stage,
            span: HandoffSpan::NotKnown,
        }])
    };
    match kind {
        Some(OutputKind::DmxUsbPro { .. }) => dmx_usb_pro::after_handoff(patched, read),
        // DDP has no timing; the frame's other packets, the network, the controller and its
        // strips are the user's.
        Some(OutputKind::Ddp { .. }) => not_known("NetworkAndController"),
        None => not_known("Device"),
    }
}

/// A span from a spec in whole microseconds, rounded outward.
fn computed(low: Duration, high: Duration, source: &'static str) -> HandoffSpan {
    let micros = |time: u128| u32::try_from(time).unwrap_or(u32::MAX);
    HandoffSpan::Computed {
        low_us: micros(low.as_micros()),
        high_us: micros(high.as_nanos().div_ceil(1_000)),
        source,
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
    use dmx_usb_pro::Settings;
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

    /// Worked by hand (#318 P1c, P1c2). A message is 518 bytes x 11 bits at 500,000 baud:
    /// 11,396 µs. Slot n ends at break + mark + (n + 1) x 44 µs, a break or mark step
    /// 10.67 µs (DMX512, API p.3). The defaults, break 9 and mark 1 (106.7 µs): slot 1 at
    /// 194.7, slot 48 at 2,262.7, slot 50 at 2,350.7, slot 512 at 22,678.7; 40 frames a
    /// second, a frame every 25,000 µs. Break 20 and mark 4 (256.08 µs): slot 1 at 344.08,
    /// slot 50 at 2,500.08, slot 512 at 22,828.08; rate 30, every 33,333.3 µs. Break and
    /// mark 127 (2,710.18 µs): slot 1 at 2,798.18, slot 50 at 4,954.18; the frame on the
    /// wire, 2,710.18 + 513 x 44 = 25,282.18 µs, is longer than 1 s / 40. Rate 0: the frame
    /// on the wire only, 22,678.7 µs at the defaults. Rate 1: 1 s.
    #[test]
    fn each_kind_names_what_follows_its_handoff() {
        let ddp = |color| {
            Some(OutputKind::Ddp {
                address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
                color,
            })
        };
        let pro = Some(OutputKind::DmxUsbPro {
            adapter: Adapter::FIRST,
        });
        let part = |stage, span| HandoffPart { stage, span };
        let computed = |low_us, high_us, source| HandoffSpan::Computed {
            low_us,
            high_us,
            source,
        };
        let pro_parts = |(frame_high, frame), wire_low, wire_high, wire| {
            vec![
                part("HostToAdapter", HandoffSpan::NotKnown),
                part("SerialLink", computed(11_396, 11_396, "Baud")),
                part("AdapterFrame", computed(0, frame_high, frame)),
                part("DmxWire", computed(wire_low, wire_high, wire)),
                part("SlotGap", HandoffSpan::NotKnown),
            ]
        };
        let default = (25_000, "AdapterDefault");
        let read = |high| (high, "AdapterRead");
        let patched = "Dmx512AdapterDefault";
        let nothing = "Dmx512AdapterDefaultNothingPatched";
        let read_patched = "Dmx512AdapterRead";
        let settings = |break_steps, mark_steps, rate| Settings::new(break_steps, mark_steps, rate);
        let network = vec![part("NetworkAndController", HandoffSpan::NotKnown)];
        let cases = [
            (
                "usb, slots 1 to 50",
                pro,
                Some(1..=50),
                None,
                pro_parts(default, 194, 2_351, patched),
            ),
            (
                "usb, slots 48 to 50",
                pro,
                Some(48..=50),
                None,
                pro_parts(default, 2_262, 2_351, patched),
            ),
            (
                "usb, every slot",
                pro,
                Some(1..=512),
                None,
                pro_parts(default, 194, 22_679, patched),
            ),
            (
                "usb, the last slot",
                pro,
                Some(512..=512),
                None,
                pro_parts(default, 22_678, 22_679, patched),
            ),
            (
                "usb, nothing patched",
                pro,
                None,
                None,
                pro_parts(default, 194, 22_679, nothing),
            ),
            (
                "usb, read break 20, mark 4, rate 30, slots 1 to 50",
                pro,
                Some(1..=50),
                settings(20, 4, 30),
                pro_parts(read(33_334), 344, 2_501, read_patched),
            ),
            (
                "usb, read the defaults, slots 1 to 50",
                pro,
                Some(1..=50),
                settings(9, 1, 40),
                pro_parts(read(25_000), 194, 2_351, read_patched),
            ),
            (
                "usb, read break and mark 127, rate 40: the frame is longer than the period",
                pro,
                Some(1..=50),
                settings(127, 127, 40),
                pro_parts(read(25_283), 2_798, 4_955, read_patched),
            ),
            (
                "usb, read rate 0: as fast as it can",
                pro,
                Some(1..=50),
                settings(9, 1, 0),
                pro_parts(
                    (22_679, "AdapterReadAsFastAsItCan"),
                    194,
                    2_351,
                    read_patched,
                ),
            ),
            (
                "usb, read rate 1",
                pro,
                Some(1..=50),
                settings(9, 1, 1),
                pro_parts(read(1_000_000), 194, 2_351, read_patched),
            ),
            (
                "usb, read, nothing patched",
                pro,
                None,
                settings(20, 4, 30),
                pro_parts(read(33_334), 344, 22_829, "Dmx512AdapterReadNothingPatched"),
            ),
            (
                "ddp rgb",
                ddp(Color::Rgb),
                Some(1..=50),
                None,
                network.clone(),
            ),
            (
                "ddp rgbw, nothing patched",
                ddp(Color::Rgbw),
                None,
                None,
                network,
            ),
            (
                "a test's own device",
                None,
                Some(1..=50),
                None,
                vec![part("Device", HandoffSpan::NotKnown)],
            ),
        ];
        for (case, kind, patched, read, expected) in cases {
            assert_eq!(
                after_handoff(kind, patched, read).parts(),
                expected,
                "{case}"
            );
        }
    }
}
