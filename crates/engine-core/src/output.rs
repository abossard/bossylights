//! What the engine knows about every output, whatever its kind: a run of universes, a
//! rate, a state and counters. What kind it is, its device and its bytes belong to
//! `crates/outputs`; this file is the data and the calculations around it.
//!
//! ```text
//! [*] -> NotFound --accepted--> Connecting --accepted--> Running --failed--> Reconnecting --accepted--> Running
//!                               Connecting --failed----------------------->  Reconnecting
//! ```
//!
//! "Accepted" is the device's word: a device that must be found accepts only once it is.
//! A kind that gets no answer from its device accepts what it sent: for it, Running means
//! the frames were sent.

use crate::{
    ADDRESSES, CHANNELS, Levels, Outcome, Rate, Refusal, Status, Universes, Watch,
};
use core::num::NonZeroU16;
use core::time::Duration;
use serde::{Deserialize, Serialize};

/// One contiguous part of a route in the channel space, counted from zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub len: u32,
}

/// The most channels and spans an output can join in one frame.
pub const ROUTE_MOST: u32 = 65_536;
pub const SPANS_MOST: usize = 1_024;

/// Why a route cannot be handed to a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteFault {
    NoSpans,
    EmptySpan,
    PastChannels,
    TooManySpans { most: usize },
    TooLong { most: u32 },
    NotWhole { whole: NonZeroU16 },
}

/// Checks each source span and the size of the assembled stream. A whole pixel can
/// straddle a span boundary.
pub fn route_length(
    spans: &[Span],
    most_spans: usize,
    most_channels: u32,
    whole: NonZeroU16,
) -> Result<u32, RouteFault> {
    let most_spans = most_spans.min(SPANS_MOST);
    let most_channels = most_channels.min(ROUTE_MOST);
    if spans.is_empty() {
        return Err(RouteFault::NoSpans);
    }
    if spans.len() > most_spans {
        return Err(RouteFault::TooManySpans { most: most_spans });
    }
    let mut length = 0_u32;
    for span in spans {
        if span.len == 0 {
            return Err(RouteFault::EmptySpan);
        }
        if span
            .start
            .checked_add(span.len)
            .is_none_or(|end| end > CHANNELS as u32)
        {
            return Err(RouteFault::PastChannels);
        }
        length = length
            .checked_add(span.len)
            .filter(|&n| n <= most_channels)
            .ok_or(RouteFault::TooLong {
                most: most_channels,
            })?;
    }
    if !length.is_multiple_of(u32::from(whole.get())) {
        return Err(RouteFault::NotWhole { whole });
    }
    Ok(length)
}

/// Joins the spans into a lane's frame, copying a universe slice at a time.
pub fn gather(frame: &[Levels], spans: &[Span], mut out: &mut [u8]) {
    for span in spans {
        let mut at = span.start as usize;
        let mut left = span.len as usize;
        while left > 0 && !out.is_empty() {
            let Some(from) = frame
                .get(at / ADDRESSES)
                .and_then(|row| row.0.get(at % ADDRESSES..))
            else {
                return;
            };
            let n = left.min(from.len()).min(out.len());
            let (to, rest) = out.split_at_mut(n);
            if let Some(from) = from.get(..n) {
                to.copy_from_slice(from);
            }
            at += n;
            left -= n;
            out = rest;
        }
    }
}

/// The first through last universes touched by these spans. Empty or out-of-range
/// spans have no bounds.
pub fn bounds(spans: &[Span]) -> Option<Run> {
    let mut first = spans.first()?.start;
    let mut last = first;
    for span in spans {
        first = first.min(span.start);
        last = last.max(span.start.checked_add(span.len.checked_sub(1)?)?);
    }
    if last >= CHANNELS as u32 {
        return None;
    }
    let first = (first / ADDRESSES as u32) as u16;
    let last = (last / ADDRESSES as u32) as u16;
    Some(Run {
        first: first + 1,
        count: last - first + 1,
    })
}

/// An interval further than this from the output's period is off its beat.
pub const OFF_BEAT: Duration = Duration::from_millis(2);

/// A run of universes as a client sends it: `count` universes from `first`, counted from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RunNumbers {
    pub first: u16,
    pub count: u16,
}

/// A run of this engine's universes, sent as one stream of channels. Until the Rig
/// exists, this is how an output knows what to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Run {
    /// Counted from 1, as the client sent it.
    first: u16,
    count: u16,
}

impl RunNumbers {
    /// The run, if all of it is in this engine.
    pub fn check(self, universes: Universes) -> Result<Run, Refusal> {
        let last = self.first.saturating_add(self.count.saturating_sub(1));
        if self.first < 1 || self.count < 1 || last > universes.count() {
            return Err(Refusal::NoSuchUniverse {
                universes: universes.count(),
            });
        }
        Ok(Run {
            first: self.first,
            count: self.count,
        })
    }
}

impl Run {
    /// The first universe, counted from 1.
    pub const fn first(self) -> u16 {
        self.first
    }

    /// How many universes.
    pub const fn count(self) -> u16 {
        self.count
    }

    /// The wire run as a channel span, ending at the last whole unit.
    pub const fn span(self, whole: NonZeroU16) -> Span {
        let len = self.count as u32 * ADDRESSES as u32;
        Span {
            start: (self.first as u32 - 1) * ADDRESSES as u32,
            len: len - len % whole.get() as u32,
        }
    }

    /// Whether universes `first` to `last` (counted from 1) are all in this run.
    pub const fn covers(self, first: u16, last: u16) -> bool {
        self.first <= first && last < self.first.saturating_add(self.count)
    }

}

impl Output {
    /// Every status transition. A dropped frame changes nothing.
    const fn after_status(status: Status, outcome: Outcome, watch: Watch) -> Status {
        match (status, outcome) {
            (Status::NotFound, Outcome::Accepted) => Status::Connecting,
            (
                Status::Connecting | Status::Running | Status::Reconnecting { .. },
                Outcome::Accepted,
            ) => Status::Running,
            (Status::Connecting | Status::Running, Outcome::Failed) => {
                Status::Reconnecting { failures: 1 }
            }
            (Status::Reconnecting { failures }, Outcome::Failed) => Status::Reconnecting {
                failures: failures.saturating_add(1),
            },
            (Status::NotFound, Outcome::Failed) => match watch {
                Watch::Unwatched => Status::NotFound,
                Watch::Watched => Status::Reconnecting { failures: 1 },
            },
            (status, Outcome::Dropped) => status,
        }
    }
}

/// Frames counted since the output was set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Counters {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub accepted: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub dropped: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub failed: u64,
    /// Intervals more than [`OFF_BEAT`] away from the period.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub off_beat: u64,
}

/// One output as the engine knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Output {
    pub universes: Run,
    rate: Rate,
    status: Status,
    pub counters: Counters,
    /// When its next frame is due, since the engine started.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub due: Duration,
    /// When its last frame reached its device, or was dropped.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub last: Option<Duration>,
}

impl Output {
    /// A new output, due at once. Not found until its device takes a frame.
    pub const fn new(universes: Run, rate: Rate, now: Duration) -> Self {
        Self {
            universes,
            rate,
            status: Status::NotFound,
            counters: Counters {
                accepted: 0,
                dropped: 0,
                failed: 0,
                off_beat: 0,
            },
            due: now,
            last: None,
        }
    }

    /// A reroute keeps the adapter's state but starts fresh counts and a new schedule.
    pub const fn rerouted(self, universes: Run, rate: Rate, now: Duration) -> Self {
        Self {
            status: self.status,
            ..Self::new(universes, rate, now)
        }
    }

    /// A new lane resumes the old lane's device state when the adapter is reused.
    pub const fn resumed_from(mut self, old: Self) -> Self {
        self.status = old.status;
        self
    }

    /// A new lane sees a device as not found until that lane first hears from it.
    pub const fn fresh_lane(mut self) -> Self {
        self.status = Status::NotFound;
        self
    }

    /// The interval between this output's frames.
    pub const fn period(self) -> Duration {
        self.rate.period()
    }

    /// Frames per second for this output's schedule.
    pub const fn frames_per_second(self) -> u32 {
        self.rate.per_second()
    }

    /// Whether this schedule runs at `rate`.
    pub fn runs_at(self, rate: Rate) -> bool {
        self.rate == rate
    }

    /// Whether this output is in `status`.
    pub fn has_status(self, status: Status) -> bool {
        self.status == status
    }

    pub const fn is_running(self) -> bool {
        matches!(self.status, Status::Running)
    }

    pub const fn is_reconnecting(self) -> bool {
        matches!(self.status, Status::Reconnecting { .. })
    }

    /// The output's current connection state.
    pub const fn connection(self) -> Status {
        self.status
    }

    /// The two outputs are at the same kind of connection state.
    pub fn same_status_as(self, other: Self) -> bool {
        core::mem::discriminant(&self.status) == core::mem::discriminant(&other.status)
    }

    /// The output's next frame deadline.
    pub const fn deadline(self) -> Duration {
        self.due
    }

    /// This schedule, with its next frame due at `due`.
    pub const fn due_at(self, due: Duration) -> Self {
        Self { due, ..self }
    }

    /// Whether `latency` met this output's press target.
    pub fn press_met(self, latency: Duration) -> bool {
        crate::health::press_is_under_target(latency, self.rate)
    }

    /// Copies the device state and counters into a health sample.
    pub fn record_health(self, sample: &mut crate::health::HealthSample) {
        sample.status = self.status;
        sample.dropped = self.counters.dropped;
        sample.failed = self.counters.failed;
        sample.off_beat = self.counters.off_beat;
    }

    /// A frame went to the output's own thread at `now`: when the next is due.
    pub fn handed(self, now: Duration) -> Self {
        Self {
            due: crate::next_frame(self.due, now, self.rate.period()),
            ..self
        }
    }

    /// What came of a frame on the output's own thread, its device handed it at `at` (or
    /// it was dropped then): state, counters and the interval since the last.
    pub fn sent(self, outcome: Outcome, watch: Watch, at: Duration) -> Self {
        let period = self.rate.period();
        let off = self
            .last
            .is_some_and(|last| at.saturating_sub(last).abs_diff(period) > OFF_BEAT);
        let counters = self.counters;
        Self {
            status: Self::after_status(self.status, outcome, watch),
            counters: Counters {
                accepted: counters.accepted + u64::from(outcome == Outcome::Accepted),
                dropped: counters.dropped + u64::from(outcome == Outcome::Dropped),
                failed: counters.failed + u64::from(outcome == Outcome::Failed),
                off_beat: counters.off_beat + u64::from(off),
            },
            last: Some(at),
            ..self
        }
    }

}

impl core::fmt::Display for Output {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "{:?}, accepted {}, dropped {}, failed {}, off by > 2 ms {}",
            self.status,
            self.counters.accepted,
            self.counters.dropped,
            self.counters.failed,
            self.counters.off_beat
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OutputNumber;

    #[test]
    fn the_joined_route_fits_the_device_and_the_channel_space() {
        use RouteFault::*;
        use core::num::NonZeroU16;
        let span = |start, len| Span { start, len };
        let one = NonZeroU16::MIN;
        let rgbw = NonZeroU16::new(4).unwrap();
        let channels = crate::CHANNELS as u32;
        let caps = (SPANS_MOST, ROUTE_MOST, one);
        let larger = (usize::MAX, u32::MAX, one);
        let slots = (1, 512, one);
        let pixels = (SPANS_MOST, ROUTE_MOST, rgbw);
        let repeated = |n| vec![span(0, 1); n];
        let cases = [
            (vec![], caps, Err(NoSpans)),
            (vec![span(0, 1), span(1, 0)], caps, Err(EmptySpan)),
            (vec![span(channels - 1, 1)], caps, Ok(1)),
            (vec![span(channels - 1, 2)], caps, Err(PastChannels)),
            (vec![span(1, u32::MAX)], caps, Err(PastChannels)),
            (repeated(1_024), caps, Ok(1_024)),
            (repeated(1_025), caps, Err(TooManySpans { most: 1_024 })),
            (vec![span(0, 32_768); 2], caps, Ok(65_536)),
            (
                vec![span(0, 32_768), span(0, 32_768), span(0, 1)],
                caps,
                Err(TooLong { most: 65_536 }),
            ),
            (repeated(1_025), larger, Err(TooManySpans { most: 1_024 })),
            (
                vec![span(0, 32_768), span(0, 32_768), span(0, 1)],
                larger,
                Err(TooLong { most: 65_536 }),
            ),
            (vec![span(0, 319)], pixels, Err(NotWhole { whole: rgbw })),
            (vec![span(0, 320)], pixels, Ok(320)),
            (vec![span(0, 256); 2], slots, Err(TooManySpans { most: 1 })),
            (vec![span(0, 513)], slots, Err(TooLong { most: 512 })),
            (vec![span(0, 512)], slots, Ok(512)),
        ];
        for (spans, (most_spans, most_channels, whole), expected) in cases {
            assert_eq!(
                route_length(&spans, most_spans, most_channels, whole),
                expected
            );
        }
    }

    #[test]
    fn gather_joins_each_span_in_order_and_bounds_cover_them_all() {
        let frame: Vec<Levels> = (0..3)
            .map(|u| {
                Levels(core::array::from_fn(|n| {
                    ((u * crate::ADDRESSES + n) % 251) as u8
                }))
            })
            .collect();
        let span = |start, len| Span { start, len };
        let cases = [
            (vec![span(18, 3)], vec![18, 19, 20], 1, 1),
            (vec![span(511, 3)], vec![9, 10, 11], 1, 2),
            (vec![span(1024, 1), span(0, 2)], vec![20, 0, 1], 1, 3),
            (
                vec![span(512, 3), span(515, 2)],
                vec![10, 11, 12, 13, 14],
                2,
                1,
            ),
            (vec![span(10, 2), span(100, 1)], vec![10, 11, 100], 1, 1),
            (vec![span(512, 512)], frame[1].0.to_vec(), 2, 1),
        ];
        for (spans, expected, first, count) in cases {
            let mut out = vec![255; expected.len()];
            gather(&frame, &spans, &mut out);
            assert_eq!(out, expected, "{spans:?}");
            assert_eq!(bounds(&spans), Some(Run { first, count }), "{spans:?}");
        }
    }

    #[test]
    fn a_wire_run_becomes_one_span_cut_to_whole_units() {
        let four = Universes::new(4).unwrap();
        let cases = [
            (1, 1, 3, 0, 510),
            (1, 1, 4, 0, 512),
            (2, 3, 3, 512, 1536),
            (2, 1, 1, 512, 512),
        ];
        for (first, count, whole, start, len) in cases {
            let run = RunNumbers { first, count }.check(four).unwrap();
            let span = run.span(core::num::NonZeroU16::new(whole).unwrap());
            assert_eq!(span, Span { start, len });
            assert_eq!(bounds(&[span]), Some(run));
        }
    }

    const R: fn(u16) -> Status = |failures| Status::Reconnecting { failures };

    #[test]
    fn every_status_and_outcome() {
        use Outcome::{Accepted, Dropped, Failed};
        use Status::{Connecting, NotFound, Running};
        // (state, outcome, next state unwatched, next state watched): docs/engine.md
        let table = [
            (NotFound, Accepted, Connecting, Connecting), // the device appeared
            (NotFound, Dropped, NotFound, NotFound),
            (NotFound, Failed, NotFound, R(1)), // watched: a device known lost
            (Connecting, Accepted, Running, Running), // first frame accepted
            (Connecting, Dropped, Connecting, Connecting),
            (Connecting, Failed, R(1), R(1)),
            (Running, Accepted, Running, Running),
            (Running, Dropped, Running, Running),
            (Running, Failed, R(1), R(1)), // lost
            (R(1), Accepted, Running, Running),
            (R(1), Dropped, R(1), R(1)),
            (R(1), Failed, R(2), R(2)),
            (R(u16::MAX), Failed, R(u16::MAX), R(u16::MAX)), // keeps looking, never wraps
        ];
        for (state, outcome, unwatched, watched) in table {
            let next = (
                Output::after_status(state, outcome, Watch::Unwatched),
                Output::after_status(state, outcome, Watch::Watched),
            );
            assert_eq!(next, (unwatched, watched), "{state:?} + {outcome:?}");
        }
    }

    fn fresh(rate: Rate) -> Output {
        let run = RunNumbers { first: 1, count: 1 }
            .check(Universes::ONE)
            .unwrap();
        Output::new(run, rate, Duration::ZERO)
    }

    #[test]
    fn sent_counts_judges_the_interval_and_moves_the_status() {
        let ms = Duration::from_millis;
        let us = Duration::from_micros;
        let ns = Duration::from_nanos;
        let at_16_667 = Output {
            last: Some(ms(100)),
            ..fresh(Rate::Sixty)
        };
        let period = Rate::Sixty.period();
        // (output, outcome, sent at, counters: accepted, dropped, failed, off beat, status after)
        let cases = [
            (
                fresh(Rate::Sixty),
                Outcome::Accepted,
                ms(0),
                [1, 0, 0, 0],
                Status::Connecting,
            ), // no interval yet
            (
                at_16_667,
                Outcome::Accepted,
                us(116_667),
                [1, 0, 0, 0],
                Status::Connecting,
            ),
            (
                at_16_667,
                Outcome::Dropped,
                us(118_600),
                [0, 1, 0, 0],
                Status::NotFound,
            ), // 1.9 ms late: on beat
            (
                at_16_667,
                Outcome::Failed,
                us(118_700),
                [0, 0, 1, 1],
                Status::NotFound,
            ), // 2.03 ms late
            (
                at_16_667,
                Outcome::Accepted,
                us(114_600),
                [1, 0, 0, 1],
                Status::Connecting,
            ), // 2.07 ms early
            (
                at_16_667,
                Outcome::Accepted,
                ms(100) + period + OFF_BEAT,
                [1, 0, 0, 0],
                Status::Connecting,
            ), // exactly 2 ms late: on beat
            (
                at_16_667,
                Outcome::Accepted,
                ms(100) + period + OFF_BEAT + ns(1),
                [1, 0, 0, 1],
                Status::Connecting,
            ), // 1 ns more: off beat
        ];
        for (output, outcome, at, [accepted, dropped, failed, off_beat], status) in cases {
            let sent = output.sent(outcome, Watch::Unwatched, at);
            let expected = Counters {
                accepted,
                dropped,
                failed,
                off_beat,
            };
            assert_eq!(sent.counters, expected, "{outcome:?} at {at:?}");
            assert_eq!(sent.status, status, "{outcome:?} at {at:?}");
            assert_eq!(sent.last, Some(at));
            assert_eq!(
                sent.due, output.due,
                "the hand-over moves the due, not the send"
            );
        }
    }

    #[test]
    fn handed_moves_only_the_due() {
        let ms = Duration::from_millis;
        let period = Rate::Sixty.period();
        let at_100 = Output {
            due: ms(100),
            last: Some(ms(90)),
            ..fresh(Rate::Sixty)
        };
        // (handed at, next due)
        let cases = [
            (ms(100), ms(100) + period),
            (ms(101), ms(100) + period), // a little late: the beat holds
            (ms(100) + period * 20, ms(100) + period * 21), // far behind: starts over
        ];
        for (at, due) in cases {
            let handed = at_100.handed(at);
            assert_eq!(handed.due, due, "{at:?}");
            assert_eq!(
                Output {
                    due: at_100.due,
                    ..handed
                },
                at_100,
                "only the due moves"
            );
        }
    }

    #[test]
    fn a_run_is_all_inside_this_engine() {
        let four = Universes::new(4).unwrap();
        // (first, count, ok)
        let cases = [
            (1, 1, true),
            (1, 4, true),
            (4, 1, true),
            (3, 2, true),
            (0, 1, false),
            (1, 0, false),
            (4, 2, false),
            (5, 1, false),
            (u16::MAX, u16::MAX, false),
        ];
        for (first, count, ok) in cases {
            let checked = RunNumbers { first, count }.check(four);
            let expected = if ok {
                Ok(Run { first, count })
            } else {
                Err(Refusal::NoSuchUniverse { universes: 4 })
            };
            assert_eq!(checked, expected, "{first}, {count}");
        }
    }

    #[test]
    fn a_run_takes_its_universes_of_the_frame() {
        // universe n of the frame is filled with n
        let frame: Vec<Levels> = (1..=4).map(|n| Levels([n; crate::ADDRESSES])).collect();
        let four = Universes::new(4).unwrap();
        let one = NonZeroU16::MIN;
        // (first, count, frame universes there, the first byte of each universe gathered)
        let cases = [
            (1, 1, 4, vec![1]),
            (2, 3, 4, vec![2, 3, 4]),
            (4, 1, 4, vec![4]),
            (4, 1, 2, vec![0]), // a shorter frame gives nothing
        ];
        for (first, count, there, expected) in cases {
            let run = RunNumbers { first, count }.check(four).unwrap();
            let mut out = vec![0; usize::from(count) * crate::ADDRESSES];
            gather(&frame[..there], &[run.span(one)], &mut out);
            let taken: Vec<u8> = out.chunks(crate::ADDRESSES).map(|u| u[0]).collect();
            assert_eq!(run.count(), count, "{first}, {count}");
            assert_eq!(taken, expected, "{first}, {count}, a frame of {there}");
        }
    }

    #[test]
    fn output_numbers_are_1_to_16() {
        // (number, slot)
        let cases = [
            (0, None),
            (1, Some(0)),
            (16, Some(15)),
            (17, None),
            (u8::MAX, None),
        ];
        for (number, slot) in cases {
            assert_eq!(
                OutputNumber::try_from(number).ok().map(OutputNumber::slot),
                slot
            );
        }
    }
}
