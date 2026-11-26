//! The reader turns cumulative engine snapshots into health rows and a bounded event log.
//! Only the view travels with a picture. The sampling history stays with this reader.

use engine::Readings;
use engine_core::health::{HealthSample, Timed, Tracker, WINDOW_DURATION};
use engine_core::{MAX_OUTPUTS, OutputKind, OutputNumber, Status};
use std::ops::RangeInclusive;
use std::time::Duration;
use wire::{Health, OutputHealth, OutputHealthEvent, OutputHealthState, OutputRow};

const EVENTS: usize = 64;

pub(super) struct Reader {
    outputs: Box<[(OutputNumber, Tracker)]>,
    sample: Box<HealthSample>,
    events_at_read: Vec<OutputHealthEvent>,
    view: View,
}

impl Reader {
    /// Its views count what each open device read about itself (`readings`).
    pub(super) fn new(readings: Readings) -> Self {
        let outputs = (1..)
            .take(MAX_OUTPUTS)
            .filter_map(|n| OutputNumber::try_from(n).ok())
            .map(|number| (number, Tracker::new()))
            .collect();
        Self {
            outputs,
            sample: Box::new(HealthSample::new(Duration::ZERO, 0, Status::NotFound)),
            events_at_read: Vec::with_capacity(3 * MAX_OUTPUTS),
            view: View {
                readings,
                ..View::default()
            },
        }
    }

    pub(super) fn publish(&self) -> View {
        self.view.clone()
    }

    #[cfg(test)]
    fn view(&self) -> &View {
        &self.view
    }

    /// Read the cumulative samples for all attached outputs from one snapshot.
    pub(super) fn observe(
        &mut self,
        mut read: impl FnMut(OutputNumber, &mut HealthSample) -> bool,
    ) {
        let view = &mut self.view;
        let previous_end = view.end();
        let mut count = 0;
        let mut changed = false;
        for (output, tracker) in &mut self.outputs {
            let present = read(*output, &mut self.sample);
            let observed = tracker.observe(present.then_some(&self.sample));
            let entry = |timed: Timed| OutputHealthEvent {
                output: *output,
                generation: timed.generation,
                at_ms: u64::try_from(timed.at.as_millis()).unwrap_or(u64::MAX),
                event: timed.event.into(),
            };
            if let Some(crossed) = observed.crossed {
                view.log.record(entry(crossed));
            }
            self.events_at_read
                .extend(observed.at_read.into_iter().flatten().map(entry));
            let row = observed.row.and_then(|row| {
                Some(OutputHealth {
                    output: *output,
                    generation: row.generation,
                    status: row.status,
                    state: OutputHealthState::of(row.state)?,
                    measurement: row.measurement.into(),
                })
            });
            if let Some(row) = row {
                let rows = view.rows.get_or_insert([row; MAX_OUTPUTS]);
                changed |= count >= view.count || rows[count] != row;
                rows[count] = row;
                count += 1;
            }
        }
        // Every output in a snapshot shares its read time. The crossed grid
        // endpoint comes first. Events at the read keep their original order.
        for event in self.events_at_read.drain(..) {
            view.log.record(event);
        }
        if changed || count != view.count || view.end() != previous_end {
            view.revision += 1;
        }
        view.count = count;
    }
}

/// Health as the API sends it.
pub(super) fn health(rows: &[OutputRow]) -> Health<'_> {
    Health {
        window_ms: WINDOW_DURATION.as_millis() as u16,
        outputs: rows,
    }
}

/// The compact rows and event log fit inline, so publishing them with a frame only copies.
#[derive(Clone, Default)]
pub(super) struct View {
    rows: Option<[OutputHealth; MAX_OUTPUTS]>,
    count: usize,
    revision: u64,
    log: Log,
    /// What each open device read about itself: read when the rows are sent.
    readings: Readings,
}

impl View {
    /// What the engine measured, a row for each attached output.
    pub(super) fn measured(&self) -> &[OutputHealth] {
        self.rows.as_ref().map_or(&[], |rows| &rows[..self.count])
    }

    /// The rows as the API sends them: each with what follows its output's handoff, for
    /// the output's kind over its patched channels (`output`), with what its device read.
    pub(super) fn rows(
        &self,
        output: impl Fn(OutputNumber) -> (Option<OutputKind>, Option<RangeInclusive<u32>>),
    ) -> Vec<OutputRow> {
        self.measured()
            .iter()
            .map(|&health| {
                let (kind, patched) = output(health.output);
                OutputRow {
                    health,
                    after_handoff: self.readings.after_handoff(kind, patched),
                }
            })
            .collect()
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn end(&self) -> u64 {
        self.log.next
    }

    /// Return the events after the socket's last delivery and how many have expired.
    pub(super) fn events_since(&self, cursor: u64) -> (Vec<OutputHealthEvent>, u32) {
        let first = self.log.next.saturating_sub(EVENTS as u64);
        let missed = u32::try_from(first.saturating_sub(cursor)).unwrap_or(u32::MAX);
        let events = self.log.ring.as_ref().map_or_else(Vec::new, |ring| {
            (first.max(cursor)..self.log.next)
                .map(|sequence| ring[sequence as usize % EVENTS])
                .collect()
        });
        (events, missed)
    }
}

#[derive(Clone, Default)]
struct Log {
    /// Sequence n lives at n % EVENTS until n + EVENTS overwrites it.
    ring: Option<[OutputHealthEvent; EVENTS]>,
    next: u64,
}

impl Log {
    fn record(&mut self, entry: OutputHealthEvent) {
        let ring = self.ring.get_or_insert([entry; EVENTS]);
        ring[self.next as usize % EVENTS] = entry;
        self.next += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::Reader;
    use engine::Readings;
    use engine_core::health::HealthSample;
    use engine_core::{OutputNumber, Status};
    use std::time::Duration;
    use wire::{
        HealthEvent, LateReason, OutputHealth, OutputHealthEvent, OutputHealthState,
        OutputMeasurement, RecoveredReason,
    };

    fn output() -> OutputNumber {
        OutputNumber::try_from(1).unwrap()
    }

    fn sample(ms: u64, handed: u64) -> HealthSample {
        let mut read = HealthSample::new(Duration::from_millis(ms), 1, Status::Running);
        read.frames_handed = handed;
        read.lateness[20] = handed;
        read.lateness_worst = Duration::from_millis(2);
        read
    }

    fn observe(reader: &mut Reader, sample: Option<&HealthSample>) {
        reader.observe(|number, to| {
            if let Some(read) = sample.filter(|_| number == output()) {
                *to = *read;
                true
            } else {
                false
            }
        });
    }

    fn row(reader: &Reader) -> OutputHealth {
        let [row] = reader.view().measured() else {
            panic!("expected one attached output");
        };
        *row
    }

    fn event(ms: u64, event: HealthEvent) -> OutputHealthEvent {
        OutputHealthEvent {
            output: output(),
            generation: 1,
            at_ms: ms,
            event,
        }
    }

    fn window(reader: &Reader) -> (u64, u32) {
        match row(reader).measurement {
            OutputMeasurement::Window {
                end_ms,
                frames_handed,
                ..
            } => (end_ms, frames_handed),
            other @ (OutputMeasurement::Measuring | OutputMeasurement::ClockDiscontinuity) => {
                panic!("expected a covered window, got {other:?}")
            }
        }
    }

    #[test]
    fn a_crossed_endpoint_uses_the_last_read_before_it() {
        for (offset_ms, expected_end, expected_frames) in [(0, 4_500, 320), (10, 4_500, 288)] {
            let mut reader = Reader::new(Readings::default());
            // Quadratic counts distinguish a read on either side of each endpoint.
            for i in 0..=18 {
                observe(&mut reader, Some(&sample(i * 250 + offset_ms, i * i)));
            }
            assert_eq!(
                window(&reader),
                (expected_end, expected_frames),
                "offset {offset_ms}"
            );
        }
    }

    #[test]
    fn a_repeat_replaces_the_sample_and_keeps_the_verdict_for_four_seconds_later() {
        let mut reader = Reader::new(Readings::default());
        for i in 0..=16 {
            observe(&mut reader, Some(&sample(i * 250, i * 2)));
        }
        assert_eq!(window(&reader), (4_000, 32));
        let before = reader.view().revision();
        observe(&mut reader, Some(&sample(4_000, 100)));
        assert_eq!(reader.view().revision(), before);
        assert_eq!(window(&reader), (4_000, 32));

        for i in 1..=16 {
            observe(&mut reader, Some(&sample(4_000 + i * 250, 100 + i * 3)));
            if i < 16 {
                assert_eq!(row(&reader).state, OutputHealthState::Measuring);
            }
        }
        let reason = LateReason::HandoffLateness;
        assert_eq!(window(&reader), (8_000, 48));
        assert_eq!(row(&reader).state, OutputHealthState::Late { reason });
        assert_eq!(
            reader.view().events_since(0),
            (vec![event(8_000, HealthEvent::RunningLate { reason })], 0)
        );

        // An exact 500 ms gap keeps the warning, and starts four seconds of warm-up.
        for (ms, measurement) in [
            (8_500, OutputMeasurement::ClockDiscontinuity),
            (8_750, OutputMeasurement::ClockDiscontinuity),
            (9_000, OutputMeasurement::Measuring),
        ] {
            observe(&mut reader, Some(&sample(ms, 150)));
            assert_eq!(
                (row(&reader).state, row(&reader).measurement),
                (OutputHealthState::Late { reason }, measurement),
                "read at {ms}"
            );
        }
    }

    #[test]
    fn unavailable_status_resets_timing_even_on_a_second_read_of_the_endpoint() {
        let lost = Status::Reconnecting { failures: 1 };
        for (ms, status) in [(4_000, lost), (4_010, lost), (4_000, Status::NotFound)] {
            let mut reader = Reader::new(Readings::default());
            for i in 0..=16 {
                let mut clean = sample(i * 250, i * 2);
                clean.lateness.swap(0, 20);
                clean.lateness_worst = Duration::ZERO;
                observe(&mut reader, Some(&clean));
            }
            let healthy = row(&reader);
            assert_eq!(healthy.state, OutputHealthState::Healthy);
            let mut unavailable = sample(ms, 32);
            unavailable.status = status;
            unavailable.outages = 1;
            observe(&mut reader, Some(&unavailable));
            let current = row(&reader);
            assert_eq!(
                (current.status, current.state, current.measurement),
                (status, OutputHealthState::Measuring, healthy.measurement)
            );
        }
    }

    #[test]
    fn crossing_an_endpoint_keeps_timing_and_status_events_in_engine_time_order() {
        let reason = LateReason::HandoffLateness;
        for (status, state, status_events) in [
            (
                Status::Running,
                OutputHealthState::Late { reason },
                vec![
                    HealthEvent::OutputLost,
                    HealthEvent::Recovered {
                        reason: RecoveredReason::OutputAvailable,
                    },
                ],
            ),
            (
                Status::Reconnecting { failures: 1 },
                OutputHealthState::Measuring,
                vec![HealthEvent::OutputLost],
            ),
        ] {
            let mut reader = Reader::new(Readings::default());
            for i in 0..=31 {
                observe(&mut reader, Some(&sample(i * 250, i * 2)));
            }
            let mut after = sample(8_010, 64);
            after.status = status;
            after.outages = 1;
            observe(&mut reader, Some(&after));

            let mut expected = vec![event(8_000, HealthEvent::RunningLate { reason })];
            expected.extend(status_events.into_iter().map(|edge| event(8_010, edge)));
            assert_eq!(reader.view().events_since(0), (expected, 0), "{status:?}");
            assert_eq!(row(&reader).state, state);
        }
    }

    #[test]
    fn a_return_at_or_after_the_endpoint_uses_the_status_at_the_endpoint() {
        let late = HealthEvent::RunningLate {
            reason: LateReason::HandoffLateness,
        };
        let back = HealthEvent::Recovered {
            reason: RecoveredReason::OutputAvailable,
        };
        for (ms, state, events) in [
            (
                8_010,
                OutputHealthState::Measuring,
                vec![event(8_010, back)],
            ),
            (
                8_000,
                OutputHealthState::Late {
                    reason: LateReason::HandoffLateness,
                },
                vec![event(8_000, back), event(8_000, late)],
            ),
        ] {
            let mut reader = Reader::new(Readings::default());
            for i in 0..=31 {
                observe(&mut reader, Some(&sample(i * 250, i * 2)));
            }
            let mut down = sample(7_990, 63);
            down.status = Status::Reconnecting { failures: 1 };
            down.outages = 1;
            observe(&mut reader, Some(&down));
            assert_eq!(row(&reader).state, OutputHealthState::Measuring);
            let cursor = reader.view().end();

            let mut up = sample(ms, 64);
            up.outages = 1;
            observe(&mut reader, Some(&up));
            assert_eq!(reader.view().events_since(cursor), (events, 0), "at {ms}");
            assert_eq!(row(&reader).state, state);
        }
    }

    #[test]
    fn only_available_endpoints_can_confirm_a_later_warning() {
        let reason = LateReason::HandoffLateness;
        for (status, expected) in [
            (Status::Running, OutputHealthState::Late { reason }),
            (Status::Connecting, OutputHealthState::Late { reason }),
            (Status::NotFound, OutputHealthState::Measuring),
            (
                Status::Reconnecting { failures: 1 },
                OutputHealthState::Measuring,
            ),
        ] {
            let mut reader = Reader::new(Readings::default());
            for i in 0..=32 {
                let mut read = sample(i * 250, i * 2);
                if i == 16 {
                    read.status = status;
                }
                observe(&mut reader, Some(&read));
            }
            assert_eq!(row(&reader).state, expected, "{status:?} at 4 s");
        }
    }

    #[test]
    fn a_rise_in_outages_seen_on_reaching_an_endpoint_cannot_confirm_a_later_warning() {
        let late = OutputHealthState::Late {
            reason: LateReason::HandoffLateness,
        };
        let measuring = OutputHealthState::Measuring;
        // Reads every 250 ms up to 8 s, with `at_4s` in place of the read at 4 s.
        // From read number `lost_from` on, the output was lost and is back:
        // Running, outages 1.
        for (case, at_4s, lost_from, expected) in [
            ("no loss", &[4_000][..], None, late),
            (
                "first read on the endpoint",
                &[4_000][..],
                Some(16),
                measuring,
            ),
            (
                "repeat read on the endpoint",
                &[4_000, 4_000][..],
                Some(17),
                measuring,
            ),
            (
                "read crossing the endpoint",
                &[4_100][..],
                Some(16),
                measuring,
            ),
            (
                "on the 6 s grid point, mid-window",
                &[4_000][..],
                Some(24),
                late,
            ),
        ] {
            let times = (0..16)
                .map(|i| i * 250)
                .chain(at_4s.iter().copied())
                .chain((17..=32).map(|i| i * 250));
            let mut reader = Reader::new(Readings::default());
            for (index, ms) in times.enumerate() {
                let mut read = sample(ms, ms / 125);
                read.outages = u64::from(lost_from.is_some_and(|from| index >= from));
                observe(&mut reader, Some(&read));
            }
            assert_eq!(window(&reader).0, 8_000, "{case}");
            assert_eq!(row(&reader).state, expected, "{case}");
        }
    }

    #[test]
    fn a_loss_on_a_repeated_endpoint_cannot_confirm_a_later_warning() {
        let reconnecting = Status::Reconnecting { failures: 1 };
        for (repeat, back_at, status) in [
            (false, 4_250, reconnecting),
            (true, 4_250, reconnecting),
            (true, 4_000, reconnecting),
            (true, 4_000, Status::Running),
        ] {
            let mut reader = Reader::new(Readings::default());
            for i in 0..16 {
                observe(&mut reader, Some(&sample(i * 250, i * 2)));
            }
            if repeat {
                observe(&mut reader, Some(&sample(4_000, 32)));
            }
            let mut lost = sample(4_000, 32);
            lost.status = status;
            lost.outages = 1;
            observe(&mut reader, Some(&lost));
            assert_eq!(row(&reader).state, OutputHealthState::Measuring);

            for ms in (back_at..=8_000).step_by(250) {
                let mut running = sample(ms, ms / 125);
                running.outages = 1;
                observe(&mut reader, Some(&running));
            }
            assert_eq!(window(&reader), (8_000, 32));
            assert_eq!(
                reader.view().events_since(0),
                (
                    vec![
                        event(4_000, HealthEvent::OutputLost),
                        event(
                            back_at,
                            HealthEvent::Recovered {
                                reason: RecoveredReason::OutputAvailable,
                            },
                        ),
                    ],
                    0,
                ),
                "repeat {repeat}, status {status:?}, back at {back_at}"
            );
            assert_eq!(
                row(&reader).state,
                OutputHealthState::Measuring,
                "repeat {repeat}, status {status:?}, back at {back_at}"
            );
        }
    }

    #[test]
    fn a_reader_gap_needs_four_seconds_of_new_grid_samples_for_a_window() {
        for (gap_at, first_window) in [(4_500, 8_500), (4_510, 9_000)] {
            let mut reader = Reader::new(Readings::default());
            for ms in (0..=4_000).step_by(250) {
                observe(&mut reader, Some(&sample(ms, ms / 25)));
            }
            assert_eq!(window(&reader), (4_000, 160));

            // The gap may land on a grid point or just past it. A repeat of
            // that read cannot restore coverage from before the gap.
            for ms in [gap_at, gap_at, 4_750] {
                observe(&mut reader, Some(&sample(ms, ms / 25)));
                assert_eq!(
                    row(&reader).measurement,
                    OutputMeasurement::ClockDiscontinuity
                );
            }
            for ms in (5_000..first_window).step_by(250) {
                observe(&mut reader, Some(&sample(ms, ms / 25)));
                assert_eq!(row(&reader).measurement, OutputMeasurement::Measuring);
            }
            observe(&mut reader, Some(&sample(first_window, first_window / 25)));
            assert_eq!(window(&reader), (first_window, 160));
            assert_eq!(row(&reader).state, OutputHealthState::Measuring);
            assert_eq!(reader.view().events_since(0), (vec![], 0));
        }
    }

    #[test]
    fn a_slow_reader_reports_the_edges_of_only_the_adapter_that_was_reused() {
        let lost = HealthEvent::OutputLost;
        let back = HealthEvent::Recovered {
            reason: RecoveredReason::OutputAvailable,
        };
        let down = Status::Reconnecting { failures: 3 };
        // The reader skips a complete intermediate generation. A replacement
        // can have the very same status and outage count as the reused port.
        for (was, runs, outages_before, expected) in [
            (Status::Running, 2, 1, vec![lost, back]),
            (down, 2, 1, vec![back]),
            (Status::Running, 0, 0, vec![]),
            (down, 1, 1, vec![]),
        ] {
            let mut reader = Reader::new(Readings::default());
            let mut before = sample(0, 2);
            before.status = was;
            before.outages = u64::from(was == down);
            observe(&mut reader, Some(&before));
            let mut after = HealthSample::new(Duration::from_secs(6), 3, Status::Running);
            after.prior_runs = runs;
            after.prior_outages = outages_before;
            observe(&mut reader, Some(&after));
            observe(&mut reader, Some(&after));
            let entries: Vec<_> = expected
                .into_iter()
                .map(|edge| OutputHealthEvent {
                    generation: 3,
                    ..event(6_000, edge)
                })
                .collect();
            assert_eq!(
                reader.view().events_since(0),
                (entries, 0),
                "{was:?}, {runs} reroutes"
            );
            assert_eq!(row(&reader).measurement, OutputMeasurement::Measuring);
        }
    }

    #[test]
    fn replacement_and_removal_end_the_old_generation_without_an_event() {
        let mut reader = Reader::new(Readings::default());
        for i in 0..=32 {
            observe(&mut reader, Some(&sample(i * 250, i * 2)));
        }
        assert!(matches!(row(&reader).state, OutputHealthState::Late { .. }));
        let cursor = reader.view().end();
        for (generation, status, outages) in [
            (2, Status::Reconnecting { failures: 1 }, 1),
            (3, Status::Running, 0),
        ] {
            let mut next = sample(8_010, 0);
            next.generation = generation;
            next.status = status;
            next.outages = outages;
            let before = reader.view().revision();
            observe(&mut reader, Some(&next));
            assert!(reader.view().revision() > before);
            assert_eq!(
                row(&reader),
                OutputHealth {
                    output: output(),
                    generation,
                    status,
                    state: OutputHealthState::Measuring,
                    measurement: OutputMeasurement::Measuring
                }
            );
            assert_eq!(reader.view().events_since(cursor), (vec![], 0));
        }
        let before = reader.view().revision();
        observe(&mut reader, None);
        assert!(reader.view().revision() > before);
        assert!(reader.view().measured().is_empty());
        assert_eq!(reader.view().events_since(cursor), (vec![], 0));

        // Even the same generation number after removal starts without a baseline.
        let mut again = sample(8_020, 0);
        again.generation = 3;
        again.outages = 1;
        observe(&mut reader, Some(&again));
        assert_eq!(row(&reader).measurement, OutputMeasurement::Measuring);
        assert_eq!(reader.view().events_since(cursor), (vec![], 0));
    }

    #[test]
    fn every_event_of_a_partly_filled_log_is_delivered_in_order() {
        let mut reader = Reader::new(Readings::default());
        observe(&mut reader, Some(&sample(0, 0)));
        let mut expected = Vec::new();
        for ms in 1_u64..=63 {
            let mut read = sample(ms, 0);
            read.outages = ms.div_ceil(2);
            let edge = if ms % 2 == 1 {
                read.status = Status::Reconnecting { failures: 1 };
                HealthEvent::OutputLost
            } else {
                HealthEvent::Recovered {
                    reason: RecoveredReason::OutputAvailable,
                }
            };
            observe(&mut reader, Some(&read));
            expected.push(event(ms, edge));
            let (events, missed) = reader.view().events_since(0);
            assert_eq!(missed, 0);
            assert_eq!(events, expected, "log after {ms} events");
        }
    }

    /// A reply goes to a subscriber at most every 500 ms: the cap catches a row that grows
    /// by mistake, it is no cost. 28 KiB since #318 P1c: what follows each handoff, at its
    /// most (every part computed, the longest names a kind gives, the largest numbers),
    /// adds 11.6 KB to 16 outputs, 28.0 KB in all.
    #[test]
    fn the_largest_health_reply_fits_in_28_kib() {
        use engine_core::health::{AfterHandoff, HandoffPart, HandoffSpan};
        use engine_core::{Adapter, Color, MAX_OUTPUTS, OutputKind};
        use outputs::dmx_usb_pro::Settings;
        use std::net::{Ipv4Addr, SocketAddrV4};
        use wire::{Health, LatenessWindow, OutputRow, PressWindow, Reply};

        let reason = LateReason::HandoffLateness;
        let usb = OutputKind::DmxUsbPro {
            adapter: Adapter::FIRST,
        };
        let ddp = OutputKind::Ddp {
            address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
            color: Color::Rgb,
        };
        // Each kind, nothing patched and every slot, with nothing read and with each kind of
        // answer a widget gives.
        let read = [None, Settings::new(9, 1, 40), Settings::new(9, 1, 0)];
        let named: Vec<HandoffPart> = [Some(usb), Some(ddp), None]
            .into_iter()
            .flat_map(|kind| [(kind, None), (kind, Some(1..=512))])
            .flat_map(|(kind, patched)| read.map(|read| (kind, patched.clone(), read)))
            .flat_map(|(kind, patched, read)| {
                outputs::after_handoff(kind, patched, read).parts().to_vec()
            })
            .collect();
        let stage = named.iter().map(|part| part.stage);
        let source = named.iter().filter_map(|part| match part.span {
            HandoffSpan::Computed { source, .. } => Some(source),
            HandoffSpan::NotKnown => None,
        });
        let part = HandoffPart {
            stage: stage.max_by_key(|name| name.len()).unwrap(),
            span: HandoffSpan::Computed {
                low_us: u32::MAX,
                high_us: u32::MAX,
                source: source.max_by_key(|name| name.len()).unwrap(),
            },
        };
        let after_handoff = AfterHandoff::new([part; AfterHandoff::MOST]);
        let rows: [_; MAX_OUTPUTS] = std::array::from_fn(|slot| OutputHealth {
            output: OutputNumber::try_from(u8::try_from(slot + 1).unwrap()).unwrap(),
            generation: u64::MAX,
            status: Status::Reconnecting { failures: u16::MAX },
            state: OutputHealthState::Recovering { reason },
            measurement: OutputMeasurement::Window {
                end_ms: u64::MAX,
                lateness: Some(LatenessWindow {
                    p99_us: u32::MAX,
                    worst_us: u32::MAX,
                    samples: u32::MAX,
                }),
                press_to_light: Some(PressWindow {
                    p50_us: u32::MAX,
                    p95_us: u32::MAX,
                    worst_us: u32::MAX,
                    samples: u32::MAX,
                }),
                press_missing: u32::MAX,
                dropped: u32::MAX,
                failed: u32::MAX,
                off_beat: u32::MAX,
                frames_handed: u32::MAX,
            },
        });
        let rows = rows.map(|health| OutputRow {
            health,
            after_handoff,
        });
        let events = [OutputHealthEvent {
            output: rows[MAX_OUTPUTS - 1].health.output,
            generation: u64::MAX,
            at_ms: u64::MAX,
            event: HealthEvent::RunningLate { reason },
        }; super::EVENTS];
        let wire = crate::json(&Reply::Health {
            health: Health {
                window_ms: 4_000,
                outputs: &rows,
            },
            events: &events,
            missed: u32::MAX,
        })
        .unwrap();
        eprintln!("Largest Health reply: {} bytes", wire.len());
        assert!(wire.len() < 28 * 1024, "Health is {} bytes", wire.len());
    }

    #[test]
    fn cursors_count_overwritten_events_and_keep_the_newest_64_in_order() {
        let mut reader = Reader::new(Readings::default());
        observe(&mut reader, Some(&sample(0, 0)));
        let mut expected = Vec::new();
        for ms in 1..=40 {
            let mut recovered = sample(ms, 0);
            recovered.outages = ms;
            let before = reader.view().revision();
            observe(&mut reader, Some(&recovered));
            assert!(
                reader.view().revision() > before,
                "events alone change the revision"
            );
            expected.push(event(ms, HealthEvent::OutputLost));
            expected.push(event(
                ms,
                HealthEvent::Recovered {
                    reason: RecoveredReason::OutputAvailable,
                },
            ));
        }
        for (cursor, first, missed) in [
            (0, 16, 16),
            (15, 16, 1),
            (16, 16, 0),
            (17, 17, 0),
            (79, 79, 0),
            (80, 80, 0),
        ] {
            let (events, actual_missed) = reader.view().events_since(cursor);
            assert_eq!(events, expected[first..], "cursor {cursor}");
            assert_eq!(actual_missed, missed, "cursor {cursor}");
        }
    }
}
