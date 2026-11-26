//! Cumulative output timing, four-second windows and health transitions.

use crate::{Histogram, LATENCY_BUCKETS, LATENESS_BUCKETS, Rate, Status, bound, bucket, rank};
use core::time::Duration;
use serde::{Serialize, Serializer};

/// One run's counters at an engine time and the outage history of its reused adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthSample {
    pub at: Duration,
    pub generation: u64,
    pub status: Status,
    pub lateness: Histogram<LATENESS_BUCKETS>,
    pub lateness_worst: Duration,
    pub press: Histogram<LATENCY_BUCKETS>,
    pub press_worst: Duration,
    pub press_under_target: u64,
    pub press_missing: u64,
    pub outages: u64,
    /// Consecutive reroutes that kept this adapter. Subtract from generation
    /// to identify it even when the reader missed several route changes.
    pub prior_runs: u64,
    /// Outages counted in those earlier runs of this adapter.
    pub prior_outages: u64,
    pub frames_handed: u64,
    pub dropped: u64,
    pub failed: u64,
    pub off_beat: u64,
}

impl HealthSample {
    /// Start an attached generation with empty histograms and counters.
    pub const fn new(at: Duration, generation: u64, status: Status) -> Self {
        Self {
            at,
            generation,
            status,
            lateness: [0; LATENESS_BUCKETS],
            press: [0; LATENCY_BUCKETS],
            lateness_worst: Duration::ZERO,
            press_worst: Duration::ZERO,
            press_under_target: 0,
            press_missing: 0,
            outages: 0,
            prior_runs: 0,
            prior_outages: 0,
            frames_handed: 0,
            dropped: 0,
            failed: 0,
            off_beat: 0,
        }
    }
}

/// Bounds on timing in this window: the median, the percentile (p99 for lateness, p95
/// for press) and the worst.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricWindow {
    pub median: Duration,
    pub percentile: Duration,
    pub worst: Duration,
    pub samples: u32,
}

/// Measurements and their timing verdict at one covered grid endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub lateness: Option<MetricWindow>,
    pub press_to_light: Option<MetricWindow>,
    pub press_missing: u32,
    pub frames_handed: u32,
    pub dropped: u32,
    pub failed: u32,
    pub off_beat: u32,
    pub verdict: Verdict,
}

/// What follows the handoff on the way to the light, in the order a frame passes: the
/// part of the delay the engine does not measure. Each part is computed from a spec or
/// named as not known, never a 0 that was not measured. `crates/outputs` fills it for
/// each kind and names its stages and sources: only it knows a kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "Array<HandoffPart>"))]
pub struct AfterHandoff {
    parts: [HandoffPart; AfterHandoff::MOST],
    count: usize,
}

impl AfterHandoff {
    /// The most parts one kind names.
    pub const MOST: usize = 5;

    pub const fn new<const N: usize>(parts: [HandoffPart; N]) -> Self {
        const { assert!(N <= Self::MOST, "more parts than an AfterHandoff holds") };
        let mut all = [HandoffPart {
            stage: "",
            span: HandoffSpan::NotKnown,
        }; Self::MOST];
        all.split_at_mut(N).0.copy_from_slice(&parts);
        Self {
            parts: all,
            count: N,
        }
    }

    pub fn parts(&self) -> &[HandoffPart] {
        self.parts.get(..self.count).unwrap_or(&self.parts)
    }
}

impl Serialize for AfterHandoff {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.parts())
    }
}

/// One part of the way from the handoff to the light, by the name its kind gives it,
/// and how long it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct HandoffPart {
    pub stage: &'static str,
    pub span: HandoffSpan,
}

/// How long a part takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum HandoffSpan {
    /// From a spec, in whole microseconds: the low end rounded down, the high end up.
    /// `source` names what it is computed from.
    Computed {
        low_us: u32,
        high_us: u32,
        source: &'static str,
    },
    /// Nothing the engine sees or reads gives it a time.
    NotKnown,
}

pub const LATENESS_TARGET: Duration = Duration::from_millis(1);
const LATENESS_PERCENT: u64 = 99;
const PRESS_PERCENT: u64 = 95;
const MEDIAN_PERCENT: u64 = 50;
const MIN_SAMPLES: u128 = 20;

pub const fn press_target(rate: Rate) -> Duration {
    rate.period().saturating_add(Duration::from_millis(5))
}

/// The tick counts these presses exactly because the target falls inside a bucket.
pub fn press_is_under_target(latency: Duration, rate: Rate) -> bool {
    latency < press_target(rate)
}

/// Counts added between two cumulative histograms, before display saturation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Difference<const N: usize> {
    counts: Histogram<N>,
    total: u128,
}

impl<const N: usize> Difference<N> {
    fn between(earlier: &Histogram<N>, later: &Histogram<N>) -> Self {
        let mut counts = [0; N];
        let mut total = 0;
        for ((count, after), before) in counts.iter_mut().zip(later).zip(earlier) {
            *count = after.wrapping_sub(*before);
            total += u128::from(*count);
        }
        Self { counts, total }
    }

    fn misses_rank(&self, percent: u64, under_target: u128) -> bool {
        under_target < rank(self.total, percent)
    }

    /// The same nearest rank and bucket bound as [`crate::percentile`], over the window.
    fn measurement(&self, percent: u64, cumulative_worst: Duration) -> Option<MetricWindow> {
        let bound = |bucket| bound::<N>(bucket, cumulative_worst);
        let highest = self.counts.iter().rposition(|&count| count != 0)?;
        let ranked = |percent| {
            let rank = rank(self.total, percent);
            let mut seen = 0;
            self.counts.iter().position(|&count| {
                seen += u128::from(count);
                seen >= rank
            })
        };
        Some(MetricWindow {
            median: bound(ranked(MEDIAN_PERCENT)?),
            percentile: bound(ranked(percent)?),
            worst: bound(highest),
            samples: u32::try_from(self.total).unwrap_or(u32::MAX),
        })
    }
}

/// Difference the samples stored at B minus four seconds and B once
/// [`CoverageStep::Window`] confirms continuous coverage in one generation.
/// Press under target is the tick's exact count: the target falls inside a bucket.
pub fn window(earlier: &HealthSample, later: &HealthSample) -> Window {
    let lateness = Difference::between(&earlier.lateness, &later.lateness);
    let press = Difference::between(&earlier.press, &later.press);
    let delta =
        |before: u64, after: u64| u32::try_from(after.wrapping_sub(before)).unwrap_or(u32::MAX);
    let press_missing = delta(earlier.press_missing, later.press_missing);
    let verdict = if lateness.total < MIN_SAMPLES {
        Verdict::Insufficient
    } else {
        let under_lateness = lateness
            .counts
            .iter()
            .take(bucket::<LATENESS_BUCKETS>(LATENESS_TARGET))
            .map(|&n| u128::from(n))
            .sum();
        let under_press = later
            .press_under_target
            .wrapping_sub(earlier.press_under_target);
        let late = lateness.misses_rank(LATENESS_PERCENT, under_lateness);
        let slow_press = press.total >= MIN_SAMPLES
            && press_missing == 0
            && press.misses_rank(PRESS_PERCENT, u128::from(under_press));
        match (late, slow_press) {
            (true, true) => Verdict::Bad(LateReason::Both),
            (true, false) => Verdict::Bad(LateReason::HandoffLateness),
            (false, true) => Verdict::Bad(LateReason::PressToLight),
            (false, false) => Verdict::Clean,
        }
    };
    Window {
        lateness: lateness.measurement(LATENESS_PERCENT, later.lateness_worst),
        press_to_light: press.measurement(PRESS_PERCENT, later.press_worst),
        press_missing,
        frames_handed: delta(earlier.frames_handed, later.frames_handed),
        dropped: delta(earlier.dropped, later.dropped),
        failed: delta(earlier.failed, later.failed),
        off_beat: delta(earlier.off_beat, later.off_beat),
        verdict,
    }
}

/// Voting metrics that missed their targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LateReason {
    HandoffLateness,
    PressToLight,
    Both,
}

impl LateReason {
    fn shared(self, earlier: Self) -> Option<Self> {
        use LateReason::{Both, HandoffLateness, PressToLight};
        match (self, earlier) {
            (Both, reason) | (reason, Both) => Some(reason),
            (HandoffLateness, HandoffLateness) => Some(HandoffLateness),
            (PressToLight, PressToLight) => Some(PressToLight),
            (HandoffLateness, PressToLight) | (PressToLight, HandoffLateness) => None,
        }
    }
}

/// Timing evidence from one four-second window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Bad(LateReason),
    Clean,
    Insufficient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthState {
    Absent,
    Measuring,
    Healthy,
    Late(LateReason),
    Recovering(LateReason),
}

/// How the current output relates to the generation being monitored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthInput {
    Removed,
    NewGeneration,
    Present(Status),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveredReason {
    WithinTargets,
    OutputAvailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthEvent {
    RunningLate(LateReason),
    Recovered(RecoveredReason),
    OutputLost,
}

/// Removal clears the state. A new generation starts measuring. For the same
/// generation, status takes precedence over timing. Only Connecting and Running
/// judge the two adjacent windows ending at B and B minus four seconds.
pub fn next(
    state: HealthState,
    now: Verdict,
    four_seconds_ago: Verdict,
    output: HealthInput,
) -> (HealthState, Option<HealthEvent>) {
    use HealthState::{Absent, Healthy, Late, Measuring, Recovering};
    use Verdict::{Bad, Clean, Insufficient};
    match output {
        HealthInput::Removed => return (Absent, None),
        HealthInput::NewGeneration => return (Measuring, None),
        HealthInput::Present(status) => {
            if !available(status) {
                return (if state == Absent { Absent } else { Measuring }, None);
            }
        }
    }
    match (state, now) {
        (Absent, Bad(_) | Clean | Insufficient) => (Absent, None),
        (Measuring | Healthy, Bad(reason)) => {
            let shared = match four_seconds_ago {
                Bad(earlier) => reason.shared(earlier),
                Clean | Insufficient => None,
            };
            match shared {
                Some(shared) => (Late(shared), Some(HealthEvent::RunningLate(shared))),
                None => (state, None),
            }
        }
        (Measuring | Healthy, Clean) => (Healthy, None),
        (Measuring | Healthy, Insufficient) => (Measuring, None),
        (Late(reason) | Recovering(reason), Bad(_) | Insufficient) => (Late(reason), None),
        (Late(reason), Clean) => (Recovering(reason), None),
        (Recovering(_), Clean) => (
            Healthy,
            Some(HealthEvent::Recovered(RecoveredReason::WithinTargets)),
        ),
    }
}

/// Device edges between two reader samples of the same adapter. Several outages
/// collapse into one loss and, if the output is available again, one return.
pub fn status_events(
    before: Option<&HealthSample>,
    after: Option<&HealthSample>,
) -> [Option<HealthEvent>; 2] {
    let (Some(before), Some(after)) = (before, after) else {
        return [None; 2];
    };
    let adapter = |sample: &HealthSample| sample.generation.wrapping_sub(sample.prior_runs);
    if adapter(before) != adapter(after) {
        return [None; 2];
    }
    let outages = |sample: &HealthSample| sample.prior_outages.wrapping_add(sample.outages);
    let lost = outages(before) != outages(after);
    let available = Some(HealthEvent::Recovered(RecoveredReason::OutputAvailable));
    match (lost, after.status) {
        (true, Status::Reconnecting { .. }) => [Some(HealthEvent::OutputLost), None],
        (true, Status::Connecting | Status::Running) => [Some(HealthEvent::OutputLost), available],
        (false, Status::Running) if matches!(before.status, Status::Reconnecting { .. }) => {
            [available, None]
        }
        (_, Status::NotFound)
        | (false, Status::Running | Status::Connecting | Status::Reconnecting { .. }) => [None; 2],
    }
}

/// Whether an output's device can send: only then are its timing windows judged.
pub const fn available(status: Status) -> bool {
    match status {
        Status::Connecting | Status::Running => true,
        Status::NotFound | Status::Reconnecting { .. } => false,
    }
}

/// Length of each timing window on the engine clock.
pub const WINDOW_DURATION: Duration = Duration::from_secs(4);
const GRID_INTERVAL: Duration = Duration::from_millis(500);

/// Continuous observations for one attached output. Start with `None` on attach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coverage {
    generation: u64,
    started_at: Duration,
    last_read: Duration,
}

/// Progress of the timing window at one reader poll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverageStep {
    /// Keep the previous timing measurement. Status edges still apply at every read.
    BetweenGridPoints,
    /// Store the sample for `end` if present. The window is still warming up.
    Measuring { end: Option<Duration> },
    /// Discard sample and verdict history. An on-grid read gives the new baseline.
    ClockDiscontinuity { end: Option<Duration> },
    /// Store the sample for `end` and difference it with the one four seconds earlier.
    Window { end: Duration },
    /// Another read at the grid endpoint just stored. Store this newer sample for
    /// `end` in its place. The verdict at `end` stands, so the machine has no new input.
    SameGridPoint { end: Duration },
}

impl CoverageStep {
    /// Judge a covered window once. A gap or warm-up supplies an insufficient verdict.
    /// Between grid points the machine has no new input.
    pub fn verdict(self, judge: impl FnOnce(Duration) -> Verdict) -> Option<Verdict> {
        match self {
            Self::BetweenGridPoints | Self::SameGridPoint { .. } => None,
            Self::Measuring { .. } | Self::ClockDiscontinuity { .. } => Some(Verdict::Insufficient),
            Self::Window { end } => Some(judge(end)),
        }
    }
}

impl Coverage {
    /// Observe *every* reader sample, in engine-time order. A new generation starts
    /// fresh typical if the reader stalled. Discard the old samples and verdicts on a
    /// generation change or gap. Save the newest cumulative sample at each returned
    /// grid endpoint, including endpoints during warm-up. Use the current read
    /// when it falls exactly on the grid, and a later read at that same time in its
    /// place. When a read crosses the endpoint, the preceding read is the newest
    /// sample available at that engine time.
    pub fn observe(previous: Option<Self>, generation: u64, at: Duration) -> (Self, CoverageStep) {
        // The half-second grid divides every whole second exactly.
        let remainder = at.subsec_nanos() % GRID_INTERVAL.subsec_nanos();
        let end = at - Duration::from_nanos(u64::from(remainder));
        let baseline = (end == at).then_some(end);
        let fresh = Self {
            generation,
            started_at: at,
            last_read: at,
        };
        let Some(previous) = previous.filter(|p| p.generation == generation) else {
            return (fresh, CoverageStep::Measuring { end: baseline });
        };
        if at.saturating_sub(previous.last_read) >= GRID_INTERVAL {
            return (fresh, CoverageStep::ClockDiscontinuity { end: baseline });
        }
        let step = if end == at && at == previous.last_read {
            CoverageStep::SameGridPoint { end }
        } else if end <= previous.last_read {
            CoverageStep::BetweenGridPoints
        } else if end - previous.started_at >= WINDOW_DURATION {
            // At grid endpoints this first passes exactly four seconds after the
            // first grid point at or after the generation change or gap.
            CoverageStep::Window { end }
        } else {
            CoverageStep::Measuring { end: Some(end) }
        };
        (
            Self {
                last_read: at,
                ..previous
            },
            step,
        )
    }
}

/// The grid ends one output keeps: the two ends of a window and every one between.
const GRID_POINTS: usize = 9;

#[derive(Clone, Copy, Debug)]
struct GridPoint {
    end: Duration,
    sample: HealthSample,
    verdict: Verdict,
}

/// The newest grid ends of one output, oldest first: a fixed ring, no heap.
#[derive(Clone, Copy, Debug)]
struct Grid {
    /// Taken from the front; the newest is the last `Some`.
    points: [Option<GridPoint>; GRID_POINTS],
}

impl Grid {
    const EMPTY: Self = Self {
        points: [None; GRID_POINTS],
    };

    fn newest(&mut self) -> Option<&mut GridPoint> {
        self.points.iter_mut().rev().find_map(Option::as_mut)
    }

    fn at(&self, end: Duration) -> Option<&GridPoint> {
        self.points.iter().flatten().find(|point| point.end == end)
    }

    /// Keeps `point` as the newest; the oldest goes when all are taken.
    fn push(&mut self, point: GridPoint) {
        if self.points.iter().all(Option::is_some) {
            self.points.rotate_left(1);
            if let Some(oldest) = self.points.last_mut() {
                *oldest = None;
            }
        }
        if let Some(free) = self.points.iter_mut().find(|point| point.is_none()) {
            *free = Some(point);
        }
    }
}

/// What a row shows of an output's last window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measurement {
    Measuring,
    ClockDiscontinuity,
    Window { end: Duration, window: Window },
}

/// One read of an output as a row shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub generation: u64,
    pub status: Status,
    pub state: HealthState,
    pub measurement: Measurement,
}

/// An event of one output's generation at an engine time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timed {
    pub at: Duration,
    pub generation: u64,
    pub event: HealthEvent,
}

/// What one read of an output gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observed {
    /// `None` while the output is absent.
    pub row: Option<Row>,
    /// A timing event at a grid end this read crossed: it comes before every event at
    /// the read.
    pub crossed: Option<Timed>,
    /// The events at the read's own time, in order: device edges, then a timing event
    /// at a grid end the read falls on.
    pub at_read: [Option<Timed>; 3],
}

/// One output's health between reads: the grid ends of its last window, the read
/// before and the state. Each read goes through [`Tracker::observe`].
#[derive(Clone, Copy, Debug)]
pub struct Tracker {
    grid: Grid,
    previous: Option<HealthSample>,
    coverage: Option<Coverage>,
    state: HealthState,
    measurement: Measurement,
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracker {
    pub const fn new() -> Self {
        Self {
            grid: Grid::EMPTY,
            previous: None,
            coverage: None,
            state: HealthState::Absent,
            measurement: Measurement::Measuring,
        }
    }

    fn reset(&mut self, input: HealthInput) {
        self.coverage = None;
        self.previous = None;
        self.grid = Grid::EMPTY;
        self.measurement = Measurement::Measuring;
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

    /// One read: the output's cumulative `sample`, `None` when it is not attached.
    /// Reads come in engine-time order.
    pub fn observe(&mut self, sample: Option<&HealthSample>) -> Observed {
        let mut observed = Observed {
            row: None,
            crossed: None,
            at_read: [None; 3],
        };
        let Some(sample) = sample else {
            if self.previous.is_some() {
                self.reset(HealthInput::Removed);
            }
            return observed;
        };
        let timed = |at: Duration, event: HealthEvent| Timed {
            at,
            generation: sample.generation,
            event,
        };
        // A reused adapter can return between two reads of different routes.
        // Compare its device edges before discarding the old timing windows.
        let [lost, back] = status_events(self.previous.as_ref(), Some(sample));
        let mut at_read = [lost, back]
            .into_iter()
            .flatten()
            .map(|event| timed(sample.at, event));
        if self
            .previous
            .as_ref()
            .is_none_or(|previous| previous.generation != sample.generation)
        {
            self.reset(HealthInput::NewGeneration);
        }
        let (coverage, step) = Coverage::observe(self.coverage, sample.generation, sample.at);
        self.coverage = Some(coverage);
        let timing = self.timing(step, sample);
        let on_read = timing.filter(|(at, _)| *at == sample.at);
        observed.crossed = timing
            .filter(|(at, _)| *at != sample.at)
            .map(|(at, event)| timed(at, event));
        let mut on_read = on_read.map(|(at, event)| timed(at, event));
        for slot in &mut observed.at_read {
            *slot = at_read.next().or_else(|| on_read.take());
        }
        // The row follows the latest read, even between verdicts and on a repeat of
        // a grid end.
        if !available(sample.status) {
            self.without_window(HealthInput::Present(sample.status));
        }
        self.previous = Some(*sample);
        observed.row = Some(Row {
            generation: sample.generation,
            status: sample.status,
            state: self.state,
            measurement: self.measurement,
        });
        observed
    }

    fn timing(
        &mut self,
        step: CoverageStep,
        sample: &HealthSample,
    ) -> Option<(Duration, HealthEvent)> {
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
            .newest()
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
    ) -> Option<(Duration, HealthEvent)> {
        use Verdict::Insufficient;
        let end = match step {
            CoverageStep::BetweenGridPoints => return None,
            CoverageStep::SameGridPoint { end } => {
                if let Some(point) = self.grid.newest().filter(|point| point.end == end) {
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
                self.measurement = Measurement::Measuring;
                end?
            }
            CoverageStep::Window { end } => end,
        };

        // A crossed endpoint uses the preceding read for both counters and status.
        let endpoint = if end == sample.at {
            *sample
        } else if let Some(previous) = self.previous {
            previous
        } else {
            self.restart_coverage(sample);
            return None;
        };
        let (verdict, earlier) = if matches!(step, CoverageStep::Window { .. }) {
            let start = end.saturating_sub(WINDOW_DURATION);
            let Some(before) = self.grid.at(start) else {
                self.restart_coverage(sample);
                return None;
            };
            let measured = window(&before.sample, &endpoint);
            let earlier = before.verdict;
            self.measurement = Measurement::Window {
                end,
                window: measured,
            };
            (measured.verdict, earlier)
        } else {
            (Insufficient, Insufficient)
        };
        let status = endpoint.status;
        let verdict = if available(status) {
            verdict
        } else {
            Insufficient
        };
        self.grid.push(GridPoint {
            end,
            sample: endpoint,
            verdict,
        });
        let (state, event) = next(self.state, verdict, earlier, HealthInput::Present(status));
        self.state = state;
        event.map(|event| (end, event))
    }

    /// A missing endpoint breaks coverage just like a gap between reads.
    fn restart_coverage(&mut self, sample: &HealthSample) {
        let (coverage, first) = Coverage::observe(None, sample.generation, sample.at);
        self.coverage = Some(coverage);
        self.grid = Grid::EMPTY;
        if let CoverageStep::Measuring { end: Some(end) } = first {
            self.grid.push(GridPoint {
                end,
                sample: *sample,
                verdict: Verdict::Insufficient,
            });
        }
        self.measurement = Measurement::ClockDiscontinuity;
        self.without_window(HealthInput::Present(sample.status));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start() -> HealthSample {
        HealthSample::new(Duration::ZERO, 1, Status::Running)
    }

    fn record<const N: usize>(histogram: &mut Histogram<N>, time: Duration, count: u64) {
        if let Some(slot) = histogram.get_mut(bucket::<N>(time)) {
            *slot = slot.wrapping_add(count);
        }
    }

    #[test]
    fn targets_at_nanosecond_edges() {
        assert_eq!(LATENESS_TARGET, Duration::from_nanos(1_000_000));
        for (rate, target_ns) in [
            (Rate::Thirty, 38_333_333),
            (Rate::FortyFour, 27_727_272),
            (Rate::Sixty, 21_666_666),
        ] {
            assert_eq!(press_target(rate), Duration::from_nanos(target_ns));
            for (ns, under) in [
                (target_ns - 1, true),
                (target_ns, false),
                (target_ns + 1, false),
            ] {
                assert_eq!(
                    press_is_under_target(Duration::from_nanos(ns), rate),
                    under,
                    "{rate:?}, {ns}"
                );
            }
        }

        for (ns, verdict) in [
            (999_999, Verdict::Clean),
            (1_000_000, Verdict::Bad(LateReason::HandoffLateness)),
            (1_000_001, Verdict::Bad(LateReason::HandoffLateness)),
        ] {
            let earlier = start();
            let mut later = HealthSample {
                at: Duration::from_secs(4),
                ..earlier
            };
            later.lateness_worst = Duration::from_nanos(ns);
            record(&mut later.lateness, later.lateness_worst, 20);
            assert_eq!(window(&earlier, &later).verdict, verdict, "{ns}");
        }
    }

    #[test]
    fn window_votes_from_exact_ranks() {
        use LateReason::{Both, HandoffLateness, PressToLight};
        use Verdict::{Bad, Clean, Insufficient};

        // (lateness under/over, press under/over, missing in window, verdict)
        let many = u64::MAX / 100;
        let table = [
            ((0, 0), (0, 0), 0, Insufficient),
            ((0, 19), (0, 0), 0, Insufficient),
            ((19, 0), (0, 20), 0, Insufficient),
            ((0, 20), (0, 0), 0, Bad(HandoffLateness)),
            ((20, 0), (0, 19), 0, Clean),
            ((20, 0), (0, 20), 0, Bad(PressToLight)),
            ((0, 20), (0, 20), 0, Bad(Both)),
            ((20, 0), (0, 20), 1, Clean),
            ((20, 0), (0, 20), 2, Clean),
            ((0, 20), (0, 20), 1, Bad(HandoffLateness)),
            ((99, 1), (19, 1), 0, Clean),
            ((98, 2), (19, 1), 0, Bad(HandoffLateness)),
            ((99, 1), (18, 2), 0, Bad(PressToLight)),
            ((99, 2), (19, 2), 0, Bad(Both)), // round 99.99 and 19.95 up
            ((100, 1), (20, 1), 0, Clean),
            (
                (99 * many - 1, many + 1),
                (95 * many - 1, 5 * many + 1),
                0,
                Bad(Both),
            ),
        ];
        let press_target = press_target(Rate::FortyFour);
        for ((late_ok, late_miss), (press_ok, press_miss), missing, verdict) in table {
            let mut earlier = start();
            // Both sides of this strict press target are in the same 100 us bucket.
            earlier.press_under_target = u64::MAX - 1;
            earlier.press_missing = 7;
            earlier.press_worst = press_target;
            record(&mut earlier.press, press_target, u64::MAX);
            let mut later = HealthSample {
                at: Duration::from_secs(4),
                lateness_worst: Duration::from_millis(1),
                press_missing: earlier.press_missing + missing,
                press_under_target: earlier.press_under_target.wrapping_add(press_ok),
                ..earlier
            };
            record(&mut later.lateness, Duration::from_nanos(999_999), late_ok);
            record(&mut later.lateness, Duration::from_millis(1), late_miss);
            record(
                &mut later.press,
                press_target - Duration::from_nanos(1),
                press_ok,
            );
            record(&mut later.press, press_target, press_miss);
            assert_eq!(
                window(&earlier, &later).verdict,
                verdict,
                "late {late_ok}/{late_miss}, press {press_ok}/{press_miss}, missing {missing}"
            );
        }
    }

    #[test]
    fn window_reports_percentile_and_worst_bounds() {
        // Each metric gains its row's counts: low, middle, high. All times below are microseconds.
        // History may supply a larger worst. Only buckets gained in this window count.
        let typical = ([95, 4, 1], [95, 4, 1]);
        // p99 and p95 rank into the middle bucket here, p98 and p94 would not.
        let edge_ranks = ([98, 1, 1], [94, 1, 5]);
        // The median ranks into the low bucket of the first, the middle one of the second:
        // p51 and p49 would not.
        let median_ranks = ([50, 1, 49], [49, 2, 49]);
        // 101 samples: the median's nearest rank is 51 (50.5 rounded up), the middle bucket.
        // A rank rounded down would stay in the low one.
        let odd_total = ([50, 1, 50], [50, 1, 50]);
        // Bounds are (median, percentile, worst).
        for (history, late, press, counts, late_bounds, press_bounds) in [
            (
                90_000,
                [10, 201, 714],
                [499, 819, 929],
                typical,
                (100, 300, 800),
                (500, 500, 1_000),
            ),
            (
                0,
                [10, 201, 214],
                [430, 460, 470],
                typical,
                (100, 214, 214),
                (470, 470, 470),
            ),
            (
                0,
                [25_005, 29_000, 64_001],
                [50_100, 55_000, 81_002],
                typical,
                (64_001, 64_001, 64_001),
                (81_002, 81_002, 81_002),
            ),
            (
                0,
                [10, 201, 714],
                [10, 201, 714],
                edge_ranks,
                (100, 300, 714),
                (100, 300, 714),
            ),
            (
                0,
                [10, 201, 714],
                [10, 201, 714],
                median_ranks,
                (100, 714, 714),
                (300, 714, 714),
            ),
            (
                0,
                [10, 201, 714],
                [10, 201, 714],
                odd_total,
                (300, 714, 714),
                (300, 714, 714),
            ),
        ] {
            let mut earlier = start();
            earlier.lateness_worst = Duration::from_micros(history);
            earlier.press_worst = Duration::from_micros(history);
            record(&mut earlier.lateness, earlier.lateness_worst, 500);
            record(&mut earlier.press, earlier.press_worst, 500);
            let mut later = HealthSample {
                at: Duration::from_secs(4),
                ..earlier
            };
            for (((late_us, press_us), late_count), press_count) in
                late.into_iter().zip(press).zip(counts.0).zip(counts.1)
            {
                let latency = Duration::from_micros(press_us);
                let lateness = Duration::from_micros(late_us);
                record(&mut later.lateness, lateness, late_count);
                record(&mut later.press, latency, press_count);
                later.lateness_worst = later.lateness_worst.max(lateness);
                later.press_worst = later.press_worst.max(latency);
            }
            let measured = window(&earlier, &later);
            for (metric, gained, (median, percentile, worst)) in [
                (measured.lateness, counts.0, late_bounds),
                (measured.press_to_light, counts.1, press_bounds),
            ] {
                assert_eq!(
                    metric,
                    Some(MetricWindow {
                        median: Duration::from_micros(median),
                        percentile: Duration::from_micros(percentile),
                        worst: Duration::from_micros(worst),
                        samples: u32::try_from(gained.iter().sum::<u64>()).unwrap_or(u32::MAX),
                    })
                );
            }
        }
    }

    #[test]
    fn window_ranks_a_total_past_u64() {
        let lateness_worst = Duration::from_millis(1);
        let press_worst = Duration::from_millis(30);
        let mut later = HealthSample {
            at: Duration::from_secs(4),
            lateness_worst,
            press_worst,
            press_under_target: u64::MAX,
            ..start()
        };
        // Half the samples are in the upper bucket.
        for (lateness, press) in [
            (Duration::ZERO, Duration::ZERO),
            (lateness_worst, press_worst),
        ] {
            record(&mut later.lateness, lateness, u64::MAX);
            record(&mut later.press, press, u64::MAX);
        }
        let measured = window(&start(), &later);
        assert_eq!(measured.verdict, Verdict::Bad(LateReason::Both));
        assert_eq!(
            [measured.lateness, measured.press_to_light],
            // Half the samples rank the median into the first bucket: its upper edge.
            [lateness_worst, press_worst].map(|time| Some(MetricWindow {
                median: Duration::from_micros(100),
                percentile: time,
                worst: time,
                samples: u32::MAX,
            }))
        );
    }

    #[test]
    fn window_counts_differences_across_wrap_and_saturates_for_display() {
        let many = u64::from(u32::MAX) + 1;
        // (old counters, increments, reported counts), in the order exercised below.
        for (base, deltas, expected) in [
            (17, [0; 7], [0; 7]),
            (u64::MAX - 3, [4, 5, 6, 7, 8, 9, 10], [4, 5, 6, 7, 8, 9, 10]),
            (u64::MAX - 3, [many; 7], [u32::MAX; 7]),
        ] {
            let mut earlier = HealthSample {
                press_missing: base,
                frames_handed: base,
                dropped: base,
                failed: base,
                off_beat: base,
                ..start()
            };
            record(&mut earlier.lateness, Duration::ZERO, base);
            record(&mut earlier.press, Duration::ZERO, base);
            let [lateness, press, missing, frames, drops, fails, off_beat] = deltas;
            let mut later = HealthSample {
                at: Duration::from_secs(4),
                press_missing: base.wrapping_add(missing),
                frames_handed: base.wrapping_add(frames),
                dropped: base.wrapping_add(drops),
                failed: base.wrapping_add(fails),
                off_beat: base.wrapping_add(off_beat),
                ..earlier
            };
            record(&mut later.lateness, Duration::ZERO, lateness);
            record(&mut later.press, Duration::ZERO, press);
            let measured = window(&earlier, &later);
            assert_eq!(measured.lateness.is_some(), lateness != 0);
            assert_eq!(measured.press_to_light.is_some(), press != 0);
            assert_eq!(
                [
                    measured.lateness.map_or(0, |m| m.samples),
                    measured.press_to_light.map_or(0, |m| m.samples),
                    measured.press_missing,
                    measured.frames_handed,
                    measured.dropped,
                    measured.failed,
                    measured.off_beat
                ],
                expected
            );
        }
    }

    #[test]
    fn an_open_press_warning_recovers_on_quiet_windows() {
        use HealthState::{Healthy, Late, Recovering};
        use LateReason::PressToLight;

        let end = |millis, frames| {
            let mut sample = HealthSample::new(Duration::from_millis(millis), 1, Status::Running);
            record(&mut sample.lateness, Duration::ZERO, frames);
            sample
        };
        let mut state = Late(PressToLight);
        // Consecutive verdicts at two adjacent 500 ms grid points.
        for (start, finish, expected) in [
            ((0, 0), (4_000, 240), (Recovering(PressToLight), None)),
            (
                (500, 30),
                (4_500, 270),
                (
                    Healthy,
                    Some(HealthEvent::Recovered(RecoveredReason::WithinTargets)),
                ),
            ),
        ] {
            let earlier = end(start.0, start.1);
            let later = end(finish.0, finish.1);
            let quiet = window(&earlier, &later);
            assert_eq!(quiet.press_to_light, None);
            let transition = next(
                state,
                quiet.verdict,
                Verdict::Bad(PressToLight),
                HealthInput::Present(Status::Running),
            );
            assert_eq!(transition, expected);
            state = transition.0;
        }
    }

    #[test]
    fn reader_samples_cover_grid_windows_across_gaps_and_generations() {
        use CoverageStep::{
            BetweenGridPoints, ClockDiscontinuity, Measuring, SameGridPoint, Window,
        };
        let ms = Duration::from_millis;
        let ns = Duration::from_nanos;
        // (generation, polls, time per poll, expected result of last poll)
        let cases = [
            (1, 1, ms(0), Measuring { end: Some(ms(0)) }),
            (1, 1, ms(499), BetweenGridPoints),
            (1, 1, ms(1), Measuring { end: Some(ms(500)) }),
            (1, 13, ms(250), BetweenGridPoints), // 3.750 s
            (1, 1, ms(249), BetweenGridPoints),
            (1, 1, ms(1), Window { end: ms(4_000) }),
            (1, 1, ms(3), BetweenGridPoints),
            (1, 1, ms(499), Window { end: ms(4_500) }), // crossed at 4.502 s
            (1, 1, ms(500), ClockDiscontinuity { end: None }),
            (
                1,
                1,
                ms(499),
                Measuring {
                    end: Some(ms(5_500)),
                },
            ),
            (
                1,
                14,
                ms(250),
                Measuring {
                    end: Some(ms(9_000)),
                },
            ),
            (1, 1, ms(499), Window { end: ms(9_500) }),
            (1, 1, ms(0), SameGridPoint { end: ms(9_500) }),
            (
                1,
                1,
                ms(500),
                ClockDiscontinuity {
                    end: Some(ms(10_000)),
                },
            ),
            (1, 1, ms(501), ClockDiscontinuity { end: None }),
            (
                2,
                1,
                ms(499),
                Measuring {
                    end: Some(ms(11_000)),
                },
            ),
            (2, 16, ms(250), Window { end: ms(15_000) }),
            (3, 1, ns(1), Measuring { end: None }), // first eligible baseline is 15.5 s
            (
                3,
                16,
                ms(250),
                Measuring {
                    end: Some(ms(19_000)),
                },
            ),
            (3, 1, ms(499), BetweenGridPoints),
            (3, 1, ms(1), Window { end: ms(19_500) }),
            (4, 1, ms(10_000), Measuring { end: None }), // reattach during a stall
        ];
        let mut at = Duration::ZERO;
        let mut previous = None;
        for (generation, polls, elapsed, expected) in cases {
            let mut step = BetweenGridPoints;
            for _ in 0..polls {
                at += elapsed;
                let (coverage, observed) = Coverage::observe(previous, generation, at);
                previous = Some(coverage);
                step = observed;
            }
            assert_eq!(step, expected, "generation {generation} at {at:?}");

            let mut judged = None;
            let reason = LateReason::PressToLight;
            let bad = Verdict::Bad(reason);
            let verdict = step.verdict(|end| {
                judged = Some(end);
                bad
            });
            let next_state = verdict.map(|now| {
                next(
                    HealthState::Healthy,
                    now,
                    bad,
                    HealthInput::Present(Status::Running),
                )
            });
            let timing = match expected {
                BetweenGridPoints | SameGridPoint { .. } => (None, None, None),
                Measuring { .. } | ClockDiscontinuity { .. } => (
                    None,
                    Some(Verdict::Insufficient),
                    Some((HealthState::Measuring, None)),
                ),
                Window { end } => (
                    Some(end),
                    Some(bad),
                    Some((
                        HealthState::Late(reason),
                        Some(HealthEvent::RunningLate(reason)),
                    )),
                ),
            };
            assert_eq!((judged, verdict, next_state), timing, "at {at:?}");
        }
    }

    #[test]
    fn repeated_grid_read_keeps_the_newest_sample() {
        use CoverageStep::{Measuring, SameGridPoint, Window};
        let ms = Duration::from_millis;
        let baseline = ms(500);
        let end = ms(4_500);
        let sample = |at, count| {
            let mut sample = HealthSample::new(at, 1, Status::Running);
            record(&mut sample.lateness, Duration::ZERO, count);
            sample
        };
        // Two reads before the baseline is final, with 0 and then 1 handoff.
        for times in [[ms(499), baseline], [baseline, baseline]] {
            let mut coverage = None;
            let mut earlier = None;
            for (at, count) in times.into_iter().zip([0, 1]) {
                let (seen, step) = Coverage::observe(coverage, 1, at);
                coverage = Some(seen);
                if let Measuring { end: Some(end) } | SameGridPoint { end } = step {
                    assert_eq!(end, baseline);
                    earlier = Some(sample(at, count));
                }
            }
            for millis in (750..4_500).step_by(250) {
                coverage = Some(Coverage::observe(coverage, 1, ms(millis)).0);
            }
            let (_, step) = Coverage::observe(coverage, 1, end);
            assert_eq!(step, Window { end });
            let measured = window(&earlier.expect("grid baseline"), &sample(end, 20));
            assert_eq!(
                (measured.lateness.map(|m| m.samples), measured.verdict),
                (Some(19), Verdict::Insufficient),
                "{times:?}"
            );
        }
    }

    #[test]
    fn every_health_state_and_input() {
        use HealthEvent::{Recovered, RunningLate};
        use HealthInput::{NewGeneration, Present, Removed};
        use HealthState::{Absent, Healthy, Late, Measuring, Recovering};
        use LateReason::{Both, HandoffLateness, PressToLight};
        use Verdict::{Bad, Clean, Insufficient};

        let running = Present(Status::Running);
        let connecting = Present(Status::Connecting);
        let quiet = |state| (state, None);
        for open in [HandoffLateness, PressToLight, Both] {
            // Each column is the result for the corresponding starting state.
            let states = [Absent, Measuring, Healthy, Late(open), Recovering(open)];
            let confirmed = |shared| {
                let warning = (Late(shared), Some(RunningLate(shared)));
                [
                    quiet(Absent),
                    warning,
                    warning,
                    quiet(Late(open)),
                    quiet(Late(open)),
                ]
            };
            let unconfirmed = [
                quiet(Absent),
                quiet(Measuring),
                quiet(Healthy),
                quiet(Late(open)),
                quiet(Late(open)),
            ];
            let clean = [
                quiet(Absent),
                quiet(Healthy),
                quiet(Healthy),
                quiet(Recovering(open)),
                (Healthy, Some(Recovered(RecoveredReason::WithinTargets))),
            ];
            let insufficient = [
                quiet(Absent),
                quiet(Measuring),
                quiet(Measuring),
                quiet(Late(open)),
                quiet(Late(open)),
            ];
            let unavailable = [
                quiet(Absent),
                quiet(Measuring),
                quiet(Measuring),
                quiet(Measuring),
                quiet(Measuring),
            ];
            let table = [
                // Input, window ending now, window ending four seconds ago, results.
                (
                    running,
                    Bad(HandoffLateness),
                    Bad(HandoffLateness),
                    confirmed(HandoffLateness),
                ),
                (
                    running,
                    Bad(HandoffLateness),
                    Bad(PressToLight),
                    unconfirmed,
                ),
                (
                    connecting,
                    Bad(HandoffLateness),
                    Bad(Both),
                    confirmed(HandoffLateness),
                ),
                (
                    running,
                    Bad(PressToLight),
                    Bad(HandoffLateness),
                    unconfirmed,
                ),
                (
                    connecting,
                    Bad(PressToLight),
                    Bad(PressToLight),
                    confirmed(PressToLight),
                ),
                (
                    running,
                    Bad(PressToLight),
                    Bad(Both),
                    confirmed(PressToLight),
                ),
                (
                    running,
                    Bad(Both),
                    Bad(HandoffLateness),
                    confirmed(HandoffLateness),
                ),
                (
                    running,
                    Bad(Both),
                    Bad(PressToLight),
                    confirmed(PressToLight),
                ),
                (running, Bad(Both), Bad(Both), confirmed(Both)),
                (running, Bad(Both), Clean, unconfirmed),
                (running, Bad(Both), Insufficient, unconfirmed),
                (connecting, Clean, Bad(Both), clean),
                (running, Clean, Insufficient, clean),
                (running, Insufficient, Bad(Both), insufficient),
                (Present(Status::NotFound), Clean, Clean, unavailable),
                (
                    Present(Status::Reconnecting { failures: 1 }),
                    Bad(Both),
                    Bad(Both),
                    unavailable,
                ),
                (NewGeneration, Bad(Both), Bad(Both), [quiet(Measuring); 5]),
                (Removed, Bad(Both), Bad(Both), [quiet(Absent); 5]),
            ];
            for (input, now, four_seconds_ago, results) in table {
                for (state, expected) in states.into_iter().zip(results) {
                    assert_eq!(
                        next(state, now, four_seconds_ago, input),
                        expected,
                        "{state:?}, {input:?}, {now:?}, previous {four_seconds_ago:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn status_edges_in_reader_order() {
        use HealthEvent::{OutputLost, Recovered};
        use Status::{Connecting, NotFound, Running};

        let reconnecting = Status::Reconnecting { failures: 1 };
        let available = Some(Recovered(RecoveredReason::OutputAvailable));
        let silence = [None, None];
        let lost = [Some(OutputLost), None];
        let returned = [available, None];
        let lost_then_back = [Some(OutputLost), available];
        let table = [
            // Before and after (status, outages), later generation, events.
            (Some((reconnecting, 3)), Some((Running, 3)), 1, returned),
            (Some((Running, 3)), Some((reconnecting, 4)), 1, lost),
            (Some((Connecting, 3)), Some((reconnecting, 4)), 1, lost),
            (Some((Running, 3)), Some((Connecting, 4)), 1, lost_then_back),
            (Some((Running, 3)), Some((Running, 6)), 1, lost_then_back),
            (
                Some((reconnecting, 3)),
                Some((Running, 6)),
                1,
                lost_then_back,
            ),
            (
                Some((Running, u64::MAX)),
                Some((Running, 0)),
                1,
                lost_then_back,
            ),
            (Some((NotFound, 0)), Some((NotFound, 0)), 1, silence),
            (Some((NotFound, 0)), Some((Connecting, 0)), 1, silence),
            (Some((Connecting, 3)), Some((Running, 3)), 1, silence),
            (Some((Running, 3)), Some((Running, 3)), 1, silence),
            (Some((reconnecting, 3)), Some((reconnecting, 4)), 1, lost),
            (Some((reconnecting, 3)), Some((reconnecting, 3)), 1, silence),
            (Some((reconnecting, 3)), Some((Connecting, 3)), 1, silence),
            (Some((reconnecting, 3)), None, 1, silence),
            (None, Some((Running, 1)), 1, silence),
            (Some((reconnecting, 3)), Some((Running, 0)), 2, silence),
        ];
        let sample = |(status, outages), generation| HealthSample {
            outages,
            ..HealthSample::new(core::time::Duration::ZERO, generation, status)
        };
        for (before, after, generation, expected) in table {
            let earlier = before.map(|s| sample(s, 1));
            let later = after.map(|s| sample(s, generation));
            assert_eq!(
                status_events(earlier.as_ref(), later.as_ref()),
                expected,
                "{before:?} to {after:?}, generation {generation}"
            );
        }
    }

    /// Expected rows: docs/engine.md (an output with no device attached has no row, a
    /// new generation measures, status before timing) and output-cutover.md R4 (a loss
    /// and return between reads is one `OutputLost` and one `OutputAvailable`).
    #[test]
    fn a_tracker_shows_presence_and_device_edges() {
        use HealthEvent::{OutputLost, Recovered};
        let reconnecting = Status::Reconnecting { failures: 1 };
        let read = |ms: u64, status: Status, outages: u64| HealthSample {
            outages,
            ..HealthSample::new(Duration::from_millis(ms), 1, status)
        };
        // name, reads, the last row's state, its events at the read
        type Case<'a> = (
            &'a str,
            &'a [Option<HealthSample>],
            Option<HealthState>,
            &'a [HealthEvent],
        );
        let cases: [Case; 6] = [
            ("never attached", &[None], None, &[]),
            (
                "first read",
                &[Some(read(0, Status::Running, 0))],
                Some(HealthState::Measuring),
                &[],
            ),
            (
                "removed",
                &[Some(read(0, Status::Running, 0)), None],
                None,
                &[],
            ),
            (
                "lost and back between reads",
                &[
                    Some(read(0, Status::Running, 0)),
                    Some(read(100, Status::Running, 1)),
                ],
                Some(HealthState::Measuring),
                &[OutputLost, Recovered(RecoveredReason::OutputAvailable)],
            ),
            (
                "lost, not back",
                &[
                    Some(read(0, Status::Running, 0)),
                    Some(read(100, reconnecting, 1)),
                ],
                Some(HealthState::Measuring),
                &[OutputLost],
            ),
            (
                "back after reconnecting",
                &[
                    Some(read(0, reconnecting, 1)),
                    Some(read(100, Status::Running, 1)),
                ],
                Some(HealthState::Measuring),
                &[Recovered(RecoveredReason::OutputAvailable)],
            ),
        ];
        for (name, reads, state, events) in cases {
            let mut tracker = Tracker::new();
            let mut last = None;
            for sample in reads {
                last = Some(tracker.observe(sample.as_ref()));
            }
            let last = last.unwrap();
            let at_read: Vec<HealthEvent> =
                last.at_read.iter().flatten().map(|t| t.event).collect();
            assert_eq!(last.row.map(|row| row.state), state, "{name}");
            assert_eq!(at_read, events, "{name}");
            assert_eq!(last.crossed, None, "{name}");
        }
    }
}
