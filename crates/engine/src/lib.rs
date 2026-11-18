//! The Bossylights engine. Everything outside talks to it; it never calls out.
//!
//! For now it only runs the tick thread, which counts frames 44 times a second.
//! The logic lives in `engine-core` (no `std`, no `alloc`); this crate is the thin
//! action around it: it reads the clock, sleeps, calls `update` and publishes the state.
//!
//! The tick thread does five things less (`docs/coding.md`, rule 7). Each has a check:
//!
//! | No | Check |
//! |---|---|
//! | locks | `clippy.toml` bans every std type that locks |
//! | allocation | `engine-core` has no `alloc`; `tests/no_alloc.rs` counts every allocation while it ticks |
//! | blocking calls, files, network | `engine-core` has no `std`; `clippy.toml` bans files, sockets, processes, stdin here. The only wait is `thread::sleep` until the next frame |
//! | logging | `clippy.toml` bans print macros and stdout/stderr; no logger dependency |
//! | calls outward | `clippy.toml` bans channels and condvars; the tick thread only writes atomics in `Shared`, which others read |

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub use engine_core::State;

use engine_core::{Command, LATENESS_BUCKETS, lateness_bucket, next_frame, p99, update};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The running engine. Dropping it stops the tick thread.
pub struct Engine {
    shared: Arc<Shared>,
    tick: Option<JoinHandle<()>>,
}

/// Written by the tick thread, read by anyone. Atomics only.
struct Shared {
    frame: AtomicU64,
    stop: AtomicBool,
    lateness: [AtomicU64; LATENESS_BUCKETS],
}

impl Engine {
    pub const RATE_HZ: u32 = 44;

    /// Starts the tick thread. Fails only when the OS refuses a thread.
    pub fn start() -> io::Result<Self> {
        let shared = Arc::new(Shared {
            frame: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            lateness: [const { AtomicU64::new(0) }; LATENESS_BUCKETS],
        });
        let tick = thread::Builder::new().name("tick".into()).spawn({
            let shared = Arc::clone(&shared);
            move || tick(&shared)
        })?;
        Ok(Self {
            shared,
            tick: Some(tick),
        })
    }

    pub fn query(&self) -> State {
        State {
            frame: self.shared.frame.load(Ordering::Relaxed),
        }
    }

    /// How late 99 of 100 ticks woke up, so far. `None` before the first tick.
    pub fn lateness_p99(&self) -> Option<Duration> {
        p99(&self
            .shared
            .lateness
            .each_ref()
            .map(|n| n.load(Ordering::Relaxed)))
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

/// The real-time loop, an action: reads the clock, sleeps, publishes. The logic is in
/// `engine-core`. Nothing here may lock, allocate, block, log or call out.
fn tick(shared: &Shared) {
    let period = Duration::from_secs(1) / Engine::RATE_HZ;
    let start = Instant::now();
    let mut state = State { frame: 0 };
    let mut due = period;
    while !shared.stop.load(Ordering::Relaxed) {
        let now = start.elapsed();
        if due > now {
            thread::sleep(due - now);
        }
        let late = start.elapsed().saturating_sub(due);
        if let Some(count) = shared.lateness.get(lateness_bucket(late)) {
            count.fetch_add(1, Ordering::Relaxed);
        }
        let effect;
        (state, effect) = update(state, Command::Tick);
        if let Some(effect) = effect {
            match effect {}
        }
        shared.frame.store(state.frame, Ordering::Relaxed);
        due = next_frame(due, start.elapsed(), period);
    }
}
