//! Spike #23, throwaway: how steady are full DMX frames at 44 Hz from this machine?
//!
//! One output thread per run. Before the loop it raises its priority and allocates
//! everything it needs; inside the loop it takes no locks and allocates nothing
//! (`tests/no_alloc.rs`). The sink is a mock, an Open DMX (the host makes break and
//! mark-after-break), a DMX USB Pro (the device makes the timing) or DDP over UDP.

pub mod sink;
mod sys;

use sink::Sink;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const RATE_HZ: u32 = 44;
pub const CHANNELS: usize = 512;
/// An interval further than this from the period counts as a glitch.
pub const OFF_LIMIT: Duration = Duration::from_millis(2);
/// With `Wait::Spin`, the thread sleeps until this long before the frame is due, then spins.
pub const SPIN: Duration = Duration::from_millis(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    /// What a new thread gets.
    Default,
    /// macOS quality of service: user interactive.
    Qos,
    /// macOS Mach time-constraint policy: the scheduler's real-time class.
    Realtime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// Sleep until the frame is due.
    Sleep,
    /// Sleep until `SPIN` before the frame is due, then spin.
    Spin,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub priority: Priority,
    pub wait: Wait,
    pub duration: Duration,
}

/// Frame interval statistics, in microseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Intervals {
    pub count: usize,
    pub min: u64,
    pub max: u64,
    pub mean: f64,
    pub p50: u64,
    pub p99: u64,
    /// intervals more than `OFF_LIMIT` away from the period
    pub off: usize,
}

#[derive(Debug)]
pub struct Report {
    pub config: Config,
    /// `Err` when the priority could not be set: the run went on at default priority.
    pub priority: Result<(), String>,
    pub frames: usize,
    pub intervals: Intervals,
    /// how late the thread woke after the frame was due, p99 and max, µs
    pub late_p99: u64,
    pub late_max: u64,
    /// times the loop fell a whole frame behind and started over instead of sending a burst
    pub resyncs: u32,
    pub send_errors: u32,
    pub reconnects: u32,
    /// frames dropped because the link was busy (counted in `send_errors` too)
    pub dropped: u32,
}

/// A run in progress. `ready` turns true when the loop starts, `looped` when it ends.
pub struct Running {
    pub ready: Arc<AtomicBool>,
    pub looped: Arc<AtomicBool>,
    handle: JoinHandle<Report>,
}

impl Running {
    /// The OS id of the output thread, so a test can count its allocations alone.
    #[allow(
        clippy::unnecessary_cast,
        reason = "pthread_t is usize on macOS, u64 on Linux"
    )]
    pub fn os_thread(&self) -> usize {
        use std::os::unix::thread::JoinHandleExt;
        self.handle.as_pthread_t() as usize
    }

    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    pub fn finish(self) -> Report {
        self.handle
            .join()
            .unwrap_or_else(|_| panic!("the output thread panicked"))
    }
}

/// Starts the output thread. It runs for `config.duration` and then reports.
pub fn start(config: Config, sink: Sink) -> std::io::Result<Running> {
    let ready = Arc::new(AtomicBool::new(false));
    let looped = Arc::new(AtomicBool::new(false));
    let handle = thread::Builder::new().name("dmx".into()).spawn({
        let ready = Arc::clone(&ready);
        let looped = Arc::clone(&looped);
        move || output(config, sink, &ready, &looped)
    })?;
    Ok(Running {
        ready,
        looped,
        handle,
    })
}

fn output(config: Config, mut sink: Sink, ready: &AtomicBool, looped: &AtomicBool) -> Report {
    let period = Duration::from_secs(1) / RATE_HZ;
    let priority = sys::raise_priority(config.priority, period);
    let planned = (config.duration.as_secs_f64() * f64::from(RATE_HZ) * 1.1) as usize + 64;
    let mut sent: Vec<Instant> = Vec::with_capacity(planned);
    let mut late: Vec<u64> = Vec::with_capacity(planned);
    let mut channels = [0_u8; CHANNELS];
    let (mut resyncs, mut send_errors) = (0, 0);

    // The first sleep and clock read can set things up lazily: do them before the loop.
    thread::sleep(Duration::from_micros(100));
    let _ = Instant::now();
    ready.store(true, Ordering::SeqCst);
    let start = Instant::now();
    let mut due = start + period;
    let mut frame: u32 = 0;
    while due - start < config.duration {
        wait_until(due, config.wait);
        let now = Instant::now();
        if late.len() < late.capacity() {
            late.push(micros(now - due));
        }
        chase(&mut channels, frame);
        match sink.send(&channels) {
            Ok(()) if sent.len() < sent.capacity() => sent.push(now),
            Ok(()) => {}
            Err(_) => send_errors += 1,
        }
        frame = frame.wrapping_add(1);
        due += period;
        let after = Instant::now();
        if after > due + period {
            resyncs += 1;
            due = after + period;
        }
    }
    looped.store(true, Ordering::SeqCst);

    let gaps: Vec<u64> = sent.windows(2).map(|w| micros(w[1] - w[0])).collect();
    let mut late_sorted = late;
    late_sorted.sort_unstable();
    Report {
        config,
        priority,
        frames: sent.len(),
        intervals: intervals(&gaps, micros(period), micros(OFF_LIMIT)),
        late_p99: percentile(&late_sorted, 99),
        late_max: late_sorted.last().copied().unwrap_or(0),
        resyncs,
        send_errors,
        reconnects: sink.reconnects(),
        dropped: sink.dropped(),
    }
}

fn wait_until(due: Instant, wait: Wait) {
    let now = Instant::now();
    match wait {
        Wait::Sleep => {
            if due > now {
                thread::sleep(due - now);
            }
        }
        Wait::Spin => {
            if due > now + SPIN {
                thread::sleep(due - now - SPIN);
            }
            while Instant::now() < due {
                std::hint::spin_loop();
            }
        }
    }
}

/// Every channel changes every frame, so a real fixture would show a dropped frame.
fn chase(channels: &mut [u8; CHANNELS], frame: u32) {
    for (i, value) in channels.iter_mut().enumerate() {
        *value = (frame as usize).wrapping_add(i) as u8;
    }
}

fn micros(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

/// The value at percentile `p` of sorted values (nearest rank).
pub fn percentile(sorted: &[u64], p: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (p * sorted.len()).div_ceil(100).max(1);
    sorted[rank.min(sorted.len()) - 1]
}

/// Statistics of frame intervals (µs) against the period (µs).
pub fn intervals(gaps: &[u64], period: u64, limit: u64) -> Intervals {
    if gaps.is_empty() {
        return Intervals::default();
    }
    let mut sorted = gaps.to_vec();
    sorted.sort_unstable();
    Intervals {
        count: gaps.len(),
        min: sorted[0],
        max: sorted[sorted.len() - 1],
        mean: gaps.iter().sum::<u64>() as f64 / gaps.len() as f64,
        p50: percentile(&sorted, 50),
        p99: percentile(&sorted, 99),
        off: gaps.iter().filter(|g| g.abs_diff(period) > limit).count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_is_the_nearest_rank() {
        // (values, p, expected)
        let cases: [(&[u64], usize, u64); 5] = [
            (&[], 99, 0),
            (&[7], 99, 7),
            (&[1, 2, 3, 4], 50, 2),
            (&[1, 2, 3, 4], 99, 4),
            (&[10, 20, 30, 40, 50, 60, 70, 80, 90, 100], 90, 90),
        ];
        for (values, p, expected) in cases {
            assert_eq!(percentile(values, p), expected, "{values:?} p{p}");
        }
    }

    #[test]
    fn intervals_count_the_ones_off_by_more_than_the_limit() {
        // (gaps, expected min, max, p50, off) with period 22727 and limit 2000
        let cases: [(&[u64], u64, u64, u64, usize); 4] = [
            (&[], 0, 0, 0, 0),
            (&[22_727, 22_700, 22_750], 22_700, 22_750, 22_727, 0),
            (&[22_727, 24_728, 20_726], 20_726, 24_728, 22_727, 2),
            (&[22_727, 24_727, 20_727], 20_727, 24_727, 22_727, 0), // exactly 2 ms off is not off
        ];
        for (gaps, min, max, p50, off) in cases {
            let i = intervals(gaps, 22_727, 2_000);
            assert_eq!(
                (i.min, i.max, i.p50, i.off, i.count),
                (min, max, p50, off, gaps.len()),
                "{gaps:?}"
            );
        }
    }
}
