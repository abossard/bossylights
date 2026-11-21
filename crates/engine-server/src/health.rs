//! The reader turns cumulative engine snapshots into health rows and a bounded event log.
//! Only the view travels with a picture. The sampling history stays with this reader.

use engine_core::health::{
    self, Coverage, CoverageStep, HealthInput, HealthSample, HealthState, Verdict, WINDOW_DURATION,
    next, status_events, window,
};
use engine_core::{
    Health, MAX_OUTPUTS, OutputHealth, OutputHealthEvent, OutputHealthState, OutputMeasurement,
    OutputNumber, Status,
};
use std::collections::VecDeque;
use std::time::Duration;

const GRID_POINTS: usize = 9;
const EVENTS: usize = 64;

pub(super) struct Reader {
    outputs: Box<[(OutputNumber, Tracker)]>,
    sample: Box<HealthSample>,
    events_at_read: Vec<OutputHealthEvent>,
    view: View,
}

impl Reader {
    pub(super) fn new() -> Self {
        let outputs = (1..)
            .take(MAX_OUTPUTS)
            .filter_map(|n| OutputNumber::try_from(n).ok())
            .map(|number| (number, Tracker::new()))
            .collect();
        Self {
            outputs,
            sample: Box::new(HealthSample::new(Duration::ZERO, 0, Status::NotFound)),
            events_at_read: Vec::with_capacity(3 * MAX_OUTPUTS),
            view: View::default(),
        }
    }

    pub(super) fn view(&self) -> &View {
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
            if let Some(row) = tracker.observe(
                *output,
                present.then_some(&self.sample),
                &mut view.log,
                &mut self.events_at_read,
            ) {
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

/// The compact rows and event log fit inline, so publishing them with a frame only copies.
#[derive(Clone, Default)]
pub(super) struct View {
    rows: Option<[OutputHealth; MAX_OUTPUTS]>,
    count: usize,
    revision: u64,
    log: Log,
}

impl View {
    pub(super) fn health(&self) -> Health<'_> {
        Health {
            window_ms: WINDOW_DURATION.as_millis() as u16,
            outputs: self.rows.as_ref().map_or(&[], |rows| &rows[..self.count]),
        }
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

struct GridPoint {
    end: Duration,
    sample: HealthSample,
    verdict: Verdict,
}

struct Tracker {
    grid: VecDeque<GridPoint>,
    previous: Option<HealthSample>,
    coverage: Option<Coverage>,
    state: HealthState,
    measurement: OutputMeasurement,
}

fn available(status: Status) -> bool {
    matches!(status, Status::Running | Status::Connecting)
}

impl Tracker {
    fn new() -> Self {
        Self {
            grid: VecDeque::with_capacity(GRID_POINTS),
            previous: None,
            coverage: None,
            state: HealthState::Absent,
            measurement: OutputMeasurement::Measuring,
        }
    }

    fn reset(&mut self, input: HealthInput) {
        self.coverage = None;
        self.previous = None;
        self.grid.clear();
        self.measurement = OutputMeasurement::Measuring;
        self.without_window(input);
    }

    fn without_window(&mut self, input: HealthInput) {
        self.state = next(
            self.state,
            Verdict::Insufficient,
            Verdict::Insufficient,
            input,
        )
        .0;
    }

    fn observe(
        &mut self,
        output: OutputNumber,
        sample: Option<&HealthSample>,
        log: &mut Log,
        events_at_read: &mut Vec<OutputHealthEvent>,
    ) -> Option<OutputHealth> {
        let Some(sample) = sample else {
            if self.previous.is_some() {
                self.reset(HealthInput::Removed);
            }
            return None;
        };
        if let Some(previous) = &self.previous {
            debug_assert!(sample.at >= previous.at);
        }
        let entry = |at: Duration, event: health::HealthEvent| OutputHealthEvent {
            output,
            generation: sample.generation,
            at_ms: u64::try_from(at.as_millis()).unwrap_or(u64::MAX),
            event: event.into(),
        };
        // A reused adapter can return between two reads of different routes.
        // Compare its device edges before discarding the old timing windows.
        for event in status_events(self.previous.as_ref(), Some(sample))
            .into_iter()
            .flatten()
        {
            events_at_read.push(entry(sample.at, event));
        }
        if self
            .previous
            .as_ref()
            .is_none_or(|p| p.generation != sample.generation)
        {
            self.reset(HealthInput::NewGeneration);
        }

        let (coverage, step) = Coverage::observe(self.coverage, sample.generation, sample.at);
        self.coverage = Some(coverage);
        if let Some((at, event)) = self.timing(step, sample) {
            if at == sample.at {
                events_at_read.push(entry(at, event));
            } else {
                log.record(entry(at, event));
            }
        }
        // The published row follows the latest read, even between verdicts
        // and on a repeat of a grid endpoint.
        if !available(sample.status) {
            self.without_window(HealthInput::Present(sample.status));
        }
        self.previous = Some(*sample);
        Some(OutputHealth {
            output,
            generation: sample.generation,
            status: sample.status,
            state: Option::<OutputHealthState>::from(self.state)?,
            measurement: self.measurement,
        })
    }

    fn timing(
        &mut self,
        step: CoverageStep,
        sample: &HealthSample,
    ) -> Option<(Duration, health::HealthEvent)> {
        let reached = match step {
            CoverageStep::SameGridPoint { end } | CoverageStep::Window { end } => Some(end),
            CoverageStep::Measuring { end } => end,
            CoverageStep::BetweenGridPoints | CoverageStep::ClockDiscontinuity { .. } => None,
        };
        let judged = self.judge(step, sample);
        // A rise in the outage count seen by the read that reaches an endpoint (on it,
        // across it, or again at it) may lie at or before that endpoint: a loss and
        // return between reads. Its verdict cannot confirm a later warning.
        let lost = self
            .previous
            .is_some_and(|previous| previous.outages != sample.outages);
        if let Some(point) = self
            .grid
            .back_mut()
            .filter(|point| lost && Some(point.end) == reached)
        {
            point.verdict = Verdict::Insufficient;
        }
        judged
    }

    fn judge(
        &mut self,
        step: CoverageStep,
        sample: &HealthSample,
    ) -> Option<(Duration, health::HealthEvent)> {
        use Verdict::Insufficient;
        let end = match step {
            CoverageStep::BetweenGridPoints => return None,
            CoverageStep::SameGridPoint { end } => {
                if let Some(point) = self.grid.back_mut().filter(|point| point.end == end) {
                    if !available(sample.status) {
                        point.verdict = Insufficient;
                    }
                    point.sample = *sample;
                } else {
                    self.restart_coverage(sample);
                }
                return None;
            }
            CoverageStep::ClockDiscontinuity { .. } => {
                self.restart_coverage(sample);
                return None;
            }
            CoverageStep::Measuring { end } => {
                self.measurement = OutputMeasurement::Measuring;
                end?
            }
            CoverageStep::Window { end } => end,
        };

        // A crossed endpoint uses the preceding read for both counters and status.
        let endpoint = if end == sample.at {
            sample
        } else if let Some(previous) = self.previous.as_ref() {
            previous
        } else {
            self.restart_coverage(sample);
            return None;
        };
        let (verdict, earlier) = if matches!(step, CoverageStep::Window { .. }) {
            let start = end - WINDOW_DURATION;
            let Some(before) = self.grid.iter().find(|point| point.end == start) else {
                self.restart_coverage(sample);
                return None;
            };
            let measured = window(&before.sample, endpoint);
            self.measurement = (end, measured).into();
            (measured.verdict, before.verdict)
        } else {
            (Insufficient, Insufficient)
        };
        let status = endpoint.status;
        let verdict = if available(status) {
            verdict
        } else {
            Insufficient
        };
        self.store(end, *endpoint, verdict);
        let (state, event) = next(self.state, verdict, earlier, HealthInput::Present(status));
        self.state = state;
        event.map(|event| (end, event))
    }

    /// A missing endpoint breaks coverage just like a gap between reads.
    fn restart_coverage(&mut self, sample: &HealthSample) {
        let (coverage, first) = Coverage::observe(None, sample.generation, sample.at);
        self.coverage = Some(coverage);
        self.grid.clear();
        if let CoverageStep::Measuring { end: Some(end) } = first {
            self.store(end, *sample, Verdict::Insufficient);
        }
        self.measurement = OutputMeasurement::ClockDiscontinuity;
        self.without_window(HealthInput::Present(sample.status));
    }

    fn store(&mut self, end: Duration, sample: HealthSample, verdict: Verdict) {
        if self.grid.len() == GRID_POINTS {
            self.grid.pop_front();
        }
        self.grid.push_back(GridPoint {
            end,
            sample,
            verdict,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::Reader;
    use engine_core::health::HealthSample;
    use engine_core::{
        HealthEvent, LateReason, OutputHealth, OutputHealthEvent, OutputHealthState,
        OutputMeasurement, OutputNumber, RecoveredReason, Status,
    };
    use std::time::Duration;

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
        let [row] = reader.view().health().outputs else {
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
            let mut reader = Reader::new();
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
        let mut reader = Reader::new();
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
            let mut reader = Reader::new();
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
            let mut reader = Reader::new();
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
            let mut reader = Reader::new();
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
            let mut reader = Reader::new();
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
            let mut reader = Reader::new();
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
            let mut reader = Reader::new();
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
            let mut reader = Reader::new();
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
            let mut reader = Reader::new();
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
        let mut reader = Reader::new();
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
        assert!(reader.view().health().outputs.is_empty());
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
        let mut reader = Reader::new();
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

    #[test]
    fn the_largest_health_reply_fits_in_16_kib() {
        use engine_core::{Health, LatenessWindow, MAX_OUTPUTS, PressWindow, Reply};

        let reason = LateReason::HandoffLateness;
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
        let events = [OutputHealthEvent {
            output: rows[MAX_OUTPUTS - 1].output,
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
        assert!(wire.len() < 16 * 1024, "Health is {} bytes", wire.len());
    }

    #[test]
    fn cursors_count_overwritten_events_and_keep_the_newest_64_in_order() {
        let mut reader = Reader::new();
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
