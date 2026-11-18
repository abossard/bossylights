//! The Bossylights engine. Everything outside talks to it; it never calls out.
//!
//! For now it only runs the real-time core: a tick thread that counts frames
//! 44 times a second. The tick thread never blocks, locks or allocates:
//! - no locks: `clippy.toml` in this crate bans every std type that can lock,
//!   and `cargo clippy -- -D warnings` runs in the test command.
//! - no allocations: `tests/no_alloc.rs` counts every allocation while it ticks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// A snapshot of what the engine is doing now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    /// Frames computed since the engine started.
    pub frame: u64,
}

/// The running engine. Dropping it stops the tick thread.
pub struct Engine {
    shared: Arc<Shared>,
    tick: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Shared {
    frame: AtomicU64,
    stop: AtomicBool,
}

impl Engine {
    pub const RATE_HZ: u32 = 44;

    pub fn start() -> Self {
        let shared = Arc::new(Shared::default());
        let tick = thread::Builder::new()
            .name("tick".into())
            .spawn({
                let shared = Arc::clone(&shared);
                move || tick(&shared)
            })
            .expect("the OS refused to start the tick thread");
        Self {
            shared,
            tick: Some(tick),
        }
    }

    pub fn query(&self) -> State {
        State {
            frame: self.shared.frame.load(Ordering::Relaxed),
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(tick) = self.tick.take() {
            let _ = tick.join();
        }
    }
}

/// The real-time loop. Only atomics, the clock and sleep: nothing here may lock or allocate.
fn tick(shared: &Shared) {
    let period = Duration::from_secs(1) / Engine::RATE_HZ;
    let mut next = Instant::now() + period;
    while !shared.stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        }
        shared.frame.fetch_add(1, Ordering::Relaxed);
        next = next_frame(next, Instant::now(), period);
    }
}

/// How many frames late the tick may fall before it gives up catching up.
const MAX_BEHIND: u32 = 10;

/// When the frame after `due` is due. A late frame is made up at once, so the rate
/// holds when the OS oversleeps. Far behind (the machine slept), it starts over from now.
fn next_frame(due: Instant, now: Instant, period: Duration) -> Instant {
    let next = due + period;
    if now > next + period * MAX_BEHIND {
        now + period
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_frame_catches_up_when_late_and_resyncs_after_a_stall() {
        let period = Duration::from_millis(10);
        let due = Instant::now();
        // (frames late, when the next frame is due)
        let cases = [
            (0, due + period),          // on time
            (3, due + period),          // oversleep: make it up at once
            (MAX_BEHIND, due + period), // still catching up
            (20, due + period * 21),    // machine slept: start over from now
        ];
        for (late, expected) in cases {
            let now = due + period * late;
            assert_eq!(next_frame(due, now, period), expected, "{late} frames late");
        }
    }
}
