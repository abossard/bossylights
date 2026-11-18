//! The Bossylights engine. Everything outside talks to it; it never calls out.
//!
//! For now it only runs the real-time core: a tick thread that counts frames
//! 44 times a second. State changes only in [`update`], a calculation; the tick
//! thread reads the clock, calls it, performs the effects and publishes the state.
//!
//! The tick thread does five things less (`docs/coding.md`, rule 7). Each has a check:
//!
//! | No | Check |
//! |---|---|
//! | locks | `clippy.toml` bans every std type that locks |
//! | allocation | `tests/no_alloc.rs` counts every allocation while it ticks |
//! | blocking calls, files, network | `clippy.toml` bans files, sockets, processes, stdin. The only wait is `thread::sleep` until the next frame |
//! | logging | `clippy.toml` bans print macros and stdout/stderr; the crate has no dependencies, so no logger |
//! | calls outward | `clippy.toml` bans channels and condvars; the tick thread only writes atomics in `Shared`, which others read |
//!
//! `cargo clippy -- -D warnings` runs in `npm test`.

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

/// What changes the state. Only the tick, for now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Tick,
}

/// What must happen outside after an update, as data. None exist yet.
#[derive(Debug)]
pub(crate) enum Effect {}

/// The only place state changes: state and command in, new state and effects out.
pub(crate) fn update(state: State, command: Command) -> (State, Vec<Effect>) {
    match command {
        Command::Tick => (
            State {
                frame: state.frame.wrapping_add(1),
            },
            Vec::new(),
        ),
    }
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

/// The real-time loop, an action: reads the clock, sleeps, publishes. The logic is in
/// [`update`] and [`next_frame`]. Nothing here may lock, allocate, block, log or call out.
fn tick(shared: &Shared) {
    let period = Duration::from_secs(1) / Engine::RATE_HZ;
    let mut state = State { frame: 0 };
    let mut next = Instant::now() + period;
    while !shared.stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        }
        let effects;
        (state, effects) = update(state, Command::Tick);
        #[expect(clippy::never_loop, reason = "no effect exists yet")]
        for effect in effects {
            match effect {}
        }
        shared.frame.store(state.frame, Ordering::Relaxed);
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
    fn update_turns_a_command_into_a_new_state() {
        // (before, command, after); no command has effects yet
        let cases = [
            (0, Command::Tick, 1),
            (41, Command::Tick, 42),
            (u64::MAX, Command::Tick, 0), // wraps, never panics
        ];
        for (before, command, after) in cases {
            let (state, effects) = update(State { frame: before }, command);
            assert_eq!(state, State { frame: after }, "{before} + {command:?}");
            assert!(effects.is_empty());
        }
    }

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
