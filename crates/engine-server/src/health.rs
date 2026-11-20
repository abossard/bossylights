//! The reader turns cumulative engine snapshots into health rows and a bounded event log.
//! Only the view travels with a picture. The sampling history stays with this reader.

use engine::Snapshot;
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
            view: View::default(),
        }
    }

    pub(super) fn view(&self) -> &View {
        &self.view
    }

    pub(super) fn observe(&mut self, snapshot: &Snapshot) {
        self.observe_with(|output, sample| snapshot.health_sample(output, sample));
    }

    fn observe_with(&mut self, mut read: impl FnMut(OutputNumber, &mut HealthSample) -> bool) {
        let view = &mut self.view;
        let previous_end = view.end();
        let mut count = 0;
        let mut changed = false;
        for (output, tracker) in &mut self.outputs {
            let present = read(*output, &mut self.sample);
            if let Some(row) =
                tracker.observe(*output, present.then_some(&self.sample), &mut view.log)
            {
                let rows = view.rows.get_or_insert([row; MAX_OUTPUTS]);
                changed |= count >= view.count || rows[count] != row;
                rows[count] = row;
                count += 1;
            }
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
    fn record(
        &mut self,
        output: OutputNumber,
        generation: u64,
        at: Duration,
        event: health::HealthEvent,
    ) {
        let entry = OutputHealthEvent {
            output,
            generation,
            at_ms: u64::try_from(at.as_millis()).unwrap_or(u64::MAX),
            event: event.into(),
        };
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
        if self
            .previous
            .as_ref()
            .is_none_or(|p| p.generation != sample.generation)
        {
            self.reset(HealthInput::NewGeneration);
        } else {
            for event in status_events(self.previous.as_ref(), Some(sample))
                .into_iter()
                .flatten()
            {
                log.record(output, sample.generation, sample.at, event);
            }
        }

        let (coverage, step) = Coverage::observe(self.coverage, sample.generation, sample.at);
        self.coverage = Some(coverage);
        if let Some((at, event)) = self.timing(step, sample) {
            log.record(output, sample.generation, at, event);
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
        use Verdict::Insufficient;
        let (at, verdict, earlier) = match step {
            CoverageStep::SameGridPoint { end } => {
                let point = self
                    .grid
                    .back_mut()
                    .expect("coverage already stored this endpoint");
                debug_assert_eq!(point.end, end);
                point.sample = *sample;
                return None;
            }
            CoverageStep::BetweenGridPoints => {
                if matches!(sample.status, Status::Connecting | Status::Running) {
                    return None;
                }
                (sample.at, Insufficient, Insufficient)
            }
            CoverageStep::Measuring { end } | CoverageStep::ClockDiscontinuity { end } => {
                self.measurement = if matches!(step, CoverageStep::ClockDiscontinuity { .. }) {
                    self.grid.clear();
                    OutputMeasurement::ClockDiscontinuity
                } else {
                    OutputMeasurement::Measuring
                };
                if let Some(end) = end {
                    self.store(end, sample, Insufficient);
                }
                (sample.at, Insufficient, Insufficient)
            }
            CoverageStep::Window { end } => {
                let start = end - WINDOW_DURATION;
                let before = self
                    .grid
                    .iter()
                    .find(|p| p.end == start)
                    .expect("covered windows have both endpoints");
                let measured = window(&before.sample, self.at_endpoint(end, sample));
                let earlier = before.verdict;
                self.store(end, sample, measured.verdict);
                self.measurement = (end, measured).into();
                (end, measured.verdict, earlier)
            }
        };
        let (state, event) = next(
            self.state,
            verdict,
            earlier,
            HealthInput::Present(sample.status),
        );
        self.state = state;
        event.map(|event| (at, event))
    }

    /// After crossing an endpoint the preceding read was the newest sample at that time.
    fn at_endpoint<'a>(&'a self, end: Duration, sample: &'a HealthSample) -> &'a HealthSample {
        if sample.at == end {
            sample
        } else {
            self.previous
                .as_ref()
                .expect("crossing an endpoint needs a previous read")
        }
    }

    fn store(&mut self, end: Duration, sample: &HealthSample, verdict: Verdict) {
        let sample = *self.at_endpoint(end, sample);
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
        reader.observe_with(|number, to| {
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
    fn unavailable_status_resets_timing_between_endpoints() {
        for status in [Status::Reconnecting { failures: 1 }, Status::NotFound] {
            let mut reader = Reader::new();
            for i in 0..=16 {
                let mut clean = sample(i * 250, i * 2);
                clean.lateness.swap(0, 20);
                clean.lateness_worst = Duration::ZERO;
                observe(&mut reader, Some(&clean));
            }
            let healthy = row(&reader);
            assert_eq!(healthy.state, OutputHealthState::Healthy);
            let mut unavailable = sample(4_010, 32);
            unavailable.status = status;
            observe(&mut reader, Some(&unavailable));
            let current = row(&reader);
            assert_eq!(
                (current.status, current.state, current.measurement),
                (status, OutputHealthState::Measuring, healthy.measurement)
            );
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
