//! Output health as the API shows it: views of `engine_core::health`.

use core::time::Duration;
use engine_core::health::{self, AfterHandoff};
use engine_core::{OutputNumber, Status};
use serde::Serialize;

/// Timing and device status for each attached output, in output number order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Health<'a> {
    pub window_ms: u16,
    pub outputs: &'a [OutputRow],
}

/// An output's row: its health, and what follows its handoff on the way to the light,
/// for its kind and its patched channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OutputRow {
    #[serde(flatten)]
    pub health: OutputHealth,
    pub after_handoff: AfterHandoff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OutputHealth {
    pub output: OutputNumber,
    #[cfg_attr(test, ts(type = "number"))]
    pub generation: u64,
    pub status: Status,
    pub state: OutputHealthState,
    pub measurement: OutputMeasurement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum OutputHealthState {
    Measuring,
    Healthy,
    Late { reason: LateReason },
    Recovering { reason: LateReason },
}

impl OutputHealthState {
    /// The state as a row shows it; an absent output has no row.
    pub fn of(state: health::HealthState) -> Option<Self> {
        Some(match state {
            health::HealthState::Absent => return None,
            health::HealthState::Measuring => Self::Measuring,
            health::HealthState::Healthy => Self::Healthy,
            health::HealthState::Late(reason) => Self::Late {
                reason: reason.into(),
            },
            health::HealthState::Recovering(reason) => Self::Recovering {
                reason: reason.into(),
            },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum OutputMeasurement {
    Measuring,
    ClockDiscontinuity,
    Window {
        #[cfg_attr(test, ts(type = "number"))]
        end_ms: u64,
        lateness: Option<LatenessWindow>,
        press_to_light: Option<PressWindow>,
        press_missing: u32,
        dropped: u32,
        failed: u32,
        off_beat: u32,
        frames_handed: u32,
    },
}

impl From<health::Measurement> for OutputMeasurement {
    fn from(measurement: health::Measurement) -> Self {
        let (end, window) = match measurement {
            health::Measurement::Measuring => return Self::Measuring,
            health::Measurement::ClockDiscontinuity => return Self::ClockDiscontinuity,
            health::Measurement::Window { end, window } => (end, window),
        };
        Self::Window {
            end_ms: u64::try_from(end.as_millis()).unwrap_or(u64::MAX),
            lateness: window.lateness.map(Into::into),
            press_to_light: window.press_to_light.map(Into::into),
            press_missing: window.press_missing,
            dropped: window.dropped,
            failed: window.failed,
            off_beat: window.off_beat,
            frames_handed: window.frames_handed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LatenessWindow {
    pub p99_us: u32,
    pub worst_us: u32,
    pub samples: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PressWindow {
    pub p50_us: u32,
    pub p95_us: u32,
    pub worst_us: u32,
    pub samples: u32,
}

fn micros(time: Duration) -> u32 {
    u32::try_from(time.as_micros()).unwrap_or(u32::MAX)
}

impl From<health::MetricWindow> for LatenessWindow {
    fn from(metric: health::MetricWindow) -> Self {
        Self {
            p99_us: micros(metric.percentile),
            worst_us: micros(metric.worst),
            samples: metric.samples,
        }
    }
}

impl From<health::MetricWindow> for PressWindow {
    fn from(metric: health::MetricWindow) -> Self {
        Self {
            p50_us: micros(metric.median),
            p95_us: micros(metric.percentile),
            worst_us: micros(metric.worst),
            samples: metric.samples,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum LateReason {
    HandoffLateness,
    PressToLight,
    Both,
}

impl From<health::LateReason> for LateReason {
    fn from(reason: health::LateReason) -> Self {
        match reason {
            health::LateReason::HandoffLateness => Self::HandoffLateness,
            health::LateReason::PressToLight => Self::PressToLight,
            health::LateReason::Both => Self::Both,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum RecoveredReason {
    WithinTargets,
    OutputAvailable,
}

impl From<health::RecoveredReason> for RecoveredReason {
    fn from(reason: health::RecoveredReason) -> Self {
        match reason {
            health::RecoveredReason::WithinTargets => Self::WithinTargets,
            health::RecoveredReason::OutputAvailable => Self::OutputAvailable,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OutputHealthEvent {
    pub output: OutputNumber,
    #[cfg_attr(test, ts(type = "number"))]
    pub generation: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
    pub event: HealthEvent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum HealthEvent {
    RunningLate { reason: LateReason },
    Recovered { reason: RecoveredReason },
    OutputLost,
}

impl From<health::HealthEvent> for HealthEvent {
    fn from(event: health::HealthEvent) -> Self {
        match event {
            health::HealthEvent::RunningLate(reason) => Self::RunningLate {
                reason: reason.into(),
            },
            health::HealthEvent::Recovered(reason) => Self::Recovered {
                reason: reason.into(),
            },
            health::HealthEvent::OutputLost => Self::OutputLost,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LatenessWindow, PressWindow};
    use core::time::Duration;
    use engine_core::health::MetricWindow;

    #[test]
    fn metric_times_are_whole_microseconds_saturated_to_u32() {
        // (median, percentile, worst, and each in whole microseconds)
        for (median, percentile, worst, p50_us, p_us, worst_us) in [
            (
                Duration::from_nanos(1_999),
                Duration::from_nanos(999),
                Duration::from_nanos(2_999),
                1,
                0,
                2,
            ),
            // Past u32 by 2 and 3 µs: a cast that wraps would give 1 and 2.
            (
                Duration::from_micros(u64::from(u32::MAX) + 2),
                Duration::from_micros(u64::from(u32::MAX) + 1),
                Duration::from_micros(u64::from(u32::MAX) + 3),
                u32::MAX,
                u32::MAX,
                u32::MAX,
            ),
        ] {
            let metric = MetricWindow {
                median,
                percentile,
                worst,
                samples: 20,
            };
            assert_eq!(
                LatenessWindow::from(metric),
                LatenessWindow {
                    p99_us: p_us,
                    worst_us,
                    samples: 20
                }
            );
            assert_eq!(
                PressWindow::from(metric),
                PressWindow {
                    p50_us,
                    p95_us: p_us,
                    worst_us,
                    samples: 20
                }
            );
        }
    }
}
