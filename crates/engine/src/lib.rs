//! The Bossylights engine. Everything outside talks to it; it never calls out.
//!
//! One tick thread owns the state. 44 times a second it applies the waiting commands,
//! computes a frame and hands it to the output. The logic lives in `engine-core` (no
//! `std`, no `alloc`); this crate is the thin action around it: it reads the clock,
//! waits, calls `update` and `frame`, and publishes the state.
//!
//! ```text
//! Sender::send --> queue (rtrb) --> tick thread: update, frame --> output
//!                                       |
//!                                       +--> seqlock (atomics) --> Engine::read, query
//! ```
//!
//! The tick thread does five things less (`docs/coding.md`, rule 7). Each has a check:
//!
//! | No | Check |
//! |---|---|
//! | locks | `clippy.toml` bans every std type that locks. Commands come through `rtrb`, wait-free; the state leaves through atomics |
//! | allocation | `engine-core` has no `alloc`; buffers are made in `start`; `tests/no_alloc.rs` counts every allocation while it ticks and takes commands |
//! | blocking calls, files, network | `engine-core` has no `std`; `clippy.toml` bans files, sockets, processes, stdin here and in `rt-thread`. The only wait is `thread::sleep`, then a spin, until the next frame |
//! | logging | `clippy.toml` bans print macros and stdout/stderr; no logger dependency |
//! | calls outward | `clippy.toml` bans channels and condvars; the tick thread only writes atomics in `Shared`, which others read, and calls the output it was given |

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub use engine_core::{
    ADDRESSES, BestEffort, ChannelLevel, ChannelNumbers, Command, Levels, MAX_UNIVERSES, Priority,
    Refusal, Reply, Request, State, Universes, changes,
};

use engine_core::{
    Histogram, LATENCY_BUCKETS, LATENESS_BUCKETS, bucket, frame, next_frame, percentile,
    sleep_before, update, worst,
};
use std::hint;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering, fence};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Commands the engine takes in one frame. More are refused with [`Refusal::QueueFull`].
pub const QUEUE: usize = 256;

/// The running engine: reads its state. Dropping it stops the tick thread.
pub struct Engine {
    shared: Arc<Shared>,
    tick: Option<JoinHandle<()>>,
    priority: Priority,
    universes: Universes,
}

/// Sends commands to the engine. One sender: the queue has one producer.
pub struct Sender {
    commands: rtrb::Producer<Sent>,
    universes: Universes,
    start: Instant,
}

/// A command and when it was sent, since the engine started.
struct Sent {
    command: Command,
    at: Duration,
}

/// Written by the tick thread, read by anyone. Atomics only.
struct Shared {
    frame: AtomicU64,
    stop: AtomicBool,
    lateness: [AtomicU64; LATENESS_BUCKETS],
    latency: [AtomicU64; LATENCY_BUCKETS],
    /// A seqlock around `blackout` and `levels`: odd while the tick thread writes.
    version: AtomicU64,
    blackout: AtomicBool,
    /// The held levels, universe after universe.
    levels: Box<[AtomicU8]>,
}

impl Engine {
    pub const RATE_HZ: u32 = 44;

    /// Starts the tick thread with the best priority the system gives. Every frame goes
    /// to `output`, on the tick thread: it must not lock, allocate, block or log.
    /// Fails only when the OS refuses a thread.
    pub fn start<O>(universes: Universes, output: O) -> io::Result<(Self, Sender)>
    where
        O: FnMut(&[Levels]) + Send + 'static,
    {
        let count = usize::from(universes.count());
        let shared = Arc::new(Shared {
            frame: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            lateness: [const { AtomicU64::new(0) }; LATENESS_BUCKETS],
            latency: [const { AtomicU64::new(0) }; LATENCY_BUCKETS],
            version: AtomicU64::new(0),
            blackout: AtomicBool::new(false),
            levels: (0..count * ADDRESSES).map(|_| AtomicU8::new(0)).collect(),
        });
        let (producer, consumer) = rtrb::RingBuffer::new(QUEUE);
        let start = Instant::now();
        let ticker = Ticker {
            shared: Arc::clone(&shared),
            commands: consumer,
            output,
            held: vec![Levels::ZERO; count],
            out: vec![Levels::ZERO; count],
            sent: [Duration::ZERO; QUEUE],
            start,
        };
        let (tick, priority) = rt_thread::spawn("tick", period(), move |priority| {
            ticker.run(priority);
        })?;
        let engine = Self {
            shared,
            tick: Some(tick),
            priority,
            universes,
        };
        let sender = Sender {
            commands: producer,
            universes,
            start,
        };
        Ok((engine, sender))
    }

    pub fn universes(&self) -> Universes {
        self.universes
    }

    /// The state, without the levels.
    pub fn query(&self) -> State {
        State {
            frame: self.shared.frame.load(Ordering::Relaxed),
            priority: self.priority,
            blackout: self.shared.blackout.load(Ordering::Relaxed),
        }
    }

    /// The state, and the held levels copied into `into`, as one consistent picture.
    /// Never waits for the tick thread: it tries again if a frame was published meanwhile.
    pub fn read(&self, into: &mut [Levels]) -> State {
        let shared = &self.shared;
        loop {
            let before = shared.version.load(Ordering::Acquire);
            let blackout = shared.blackout.load(Ordering::Relaxed);
            for (universe, cells) in into.iter_mut().zip(shared.levels.chunks(ADDRESSES)) {
                for (level, cell) in universe.0.iter_mut().zip(cells) {
                    *level = cell.load(Ordering::Relaxed);
                }
            }
            fence(Ordering::Acquire);
            if before.is_multiple_of(2) && shared.version.load(Ordering::Relaxed) == before {
                return State {
                    blackout,
                    ..self.query()
                };
            }
            hint::spin_loop();
        }
    }

    /// How late 99 of 100 ticks woke up, so far. `None` before the first tick.
    pub fn lateness_p99(&self) -> Option<Duration> {
        percentile(&load(&self.shared.lateness), 99)
    }

    /// How late the latest-waking tick woke up, so far. `None` before the first tick.
    pub fn lateness_worst(&self) -> Option<Duration> {
        worst(&load(&self.shared.lateness))
    }

    /// The time from a send to the frame that carries it, for 95 of 100 commands, so
    /// far. `None` before the first command.
    pub fn send_to_frame_p95(&self) -> Option<Duration> {
        percentile(&load(&self.shared.latency), 95)
    }
}

impl Sender {
    /// Queues a command for the very next frame, or says why not. Nothing changes on
    /// a refusal.
    pub fn send(&mut self, command: Command<ChannelNumbers>) -> Result<(), Refusal> {
        let command = command.check(self.universes)?;
        let at = self.start.elapsed();
        self.commands
            .push(Sent { command, at })
            .map_err(|_| Refusal::QueueFull)
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

fn period() -> Duration {
    Duration::from_secs(1) / Engine::RATE_HZ
}

fn load<const N: usize>(counts: &[AtomicU64; N]) -> Histogram<N> {
    counts.each_ref().map(|n| n.load(Ordering::Relaxed))
}

fn count<const N: usize>(counts: &[AtomicU64; N], time: Duration) {
    if let Some(count) = counts.get(bucket::<N>(time)) {
        count.fetch_add(1, Ordering::Relaxed);
    }
}

/// What the tick thread owns. Every buffer is made before it starts.
struct Ticker<O> {
    shared: Arc<Shared>,
    commands: rtrb::Consumer<Sent>,
    output: O,
    /// The held levels: the state's universes.
    held: Vec<Levels>,
    /// The frame that goes out.
    out: Vec<Levels>,
    /// When each command applied in this frame was sent.
    sent: [Duration; QUEUE],
    start: Instant,
}

impl<O: FnMut(&[Levels])> Ticker<O> {
    /// The real-time loop, an action: reads the clock, waits, publishes. The logic is
    /// in `engine-core`. Nothing here may lock, allocate, block, log or call out.
    fn run(mut self, priority: Priority) {
        let period = period();
        let mut state = State {
            frame: 0,
            priority,
            blackout: false,
        };
        let mut due = period;
        while !self.shared.stop.load(Ordering::Relaxed) {
            // Sleep, then spin the last millisecond: a sleep can wake late, a spin cannot.
            thread::sleep(sleep_before(due, self.start.elapsed()));
            while self.start.elapsed() < due {
                hint::spin_loop();
            }
            count(
                &self.shared.lateness,
                self.start.elapsed().saturating_sub(due),
            );

            // Apply the waiting commands right before the frame: a press is in the next one.
            let mut applied = 0;
            while applied < QUEUE {
                let Ok(Sent { command, at }) = self.commands.pop() else {
                    break;
                };
                state = self.apply(state, command);
                if let Some(sent) = self.sent.get_mut(applied) {
                    *sent = at;
                }
                applied += 1;
            }
            state = self.apply(state, Command::Tick);
            frame(&state, &self.held, &mut self.out);
            (self.output)(&self.out);

            let now = self.start.elapsed();
            for at in self.sent.iter().take(applied) {
                count(&self.shared.latency, now.saturating_sub(*at));
            }
            if applied > 0 {
                self.publish(&state);
            }
            self.shared.frame.store(state.frame, Ordering::Relaxed);
            due = next_frame(due, now, period);
        }
    }

    fn apply(&mut self, state: State, command: Command) -> State {
        let (state, effect) = update(state, &mut self.held, command);
        if let Some(effect) = effect {
            match effect {}
        }
        state
    }

    /// The writer side of the seqlock: readers retry while `version` is odd or moved.
    fn publish(&self, state: &State) {
        let shared = &self.shared;
        let version = shared.version.load(Ordering::Relaxed);
        shared
            .version
            .store(version.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        shared.blackout.store(state.blackout, Ordering::Relaxed);
        let levels = self.held.iter().flat_map(|universe| universe.0.iter());
        for (cell, level) in shared.levels.iter().zip(levels) {
            cell.store(*level, Ordering::Relaxed);
        }
        shared
            .version
            .store(version.wrapping_add(2), Ordering::Release);
    }
}
