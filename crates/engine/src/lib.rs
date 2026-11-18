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
        Self { shared, tick: Some(tick) }
    }

    pub fn query(&self) -> State {
        State { frame: self.shared.frame.load(Ordering::Relaxed) }
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
        next += period;
        // Fell more than a frame behind (the machine slept): resync, never burst.
        let now = Instant::now();
        if now > next + period {
            next = now + period;
        }
    }
}
