//! The Bossylights engine. Everything outside talks to it; it never calls out.
//!
//! One tick thread owns the state and serves every output. It wakes when the next output
//! is due (or on a steady beat when none is set), applies the waiting commands, computes
//! the frame and hands each due output its run of universes. The logic lives in
//! `engine-core` (no `std`, no `alloc`); this crate is the thin action around it.
//!
//! ```text
//! Sender::send ---- command (+ device, opened here) ----> queue --> tick thread --> outputs
//!      ^                                                                |
//!      +--------- retired devices, freed here <------ return queue -----+
//!                                                                       |
//! Engine::read, query <------------------- seqlock (atomics) <----------+
//! ```
//!
//! The tick thread does five things less (`docs/coding.md`, rule 7). Each has a check:
//!
//! | No | Check |
//! |---|---|
//! | locks | `clippy.toml` bans every std type that locks, here and in `outputs`. Commands and devices come and go through `rtrb`, wait-free; the state leaves through atomics |
//! | allocation | `engine-core` has no `alloc`; buffers are made in `start`; devices are opened and freed by the `Sender`; `tests/no_alloc.rs` counts every allocation while it ticks, sends DDP and takes commands |
//! | blocking calls, files, network | `engine-core` has no `std`; `clippy.toml` bans files, sockets, processes, stdin here and in `rt-thread`. The only send is an output's non-blocking `take`. The only wait is `thread::sleep`, then a spin, until the next output is due |
//! | logging | `clippy.toml` bans print macros and stdout/stderr; no logger dependency |
//! | calls outward | `clippy.toml` bans channels and condvars; the tick thread only writes atomics in `Shared`, which others read |

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub use engine_core::{
    ADDRESSES, BEAT, BestEffort, ChannelLevel, ChannelNumbers, Color, Command, Counters, Levels,
    MAX_OUTPUTS, MAX_UNIVERSES, Output, OutputKind, OutputNumber, Priority, Rate, Refusal, Reply,
    Request, RunNumbers, State, Status, Universes, changes,
};

use engine_core::{
    Histogram, LATENCY_BUCKETS, LATENESS_BUCKETS, OUTPUT_WORDS, Outcome, bucket, due, frame, from_words,
    next_wake, percentile, sleep_before, to_words, update, worst,
};
use outputs::Device;
use std::hint;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering, fence};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Commands the engine takes in one wake. More are refused with [`Refusal::QueueFull`].
pub const QUEUE: usize = 256;

/// The running engine: reads its state. Dropping it stops the tick thread.
pub struct Engine {
    shared: Arc<Shared>,
    tick: Option<JoinHandle<Ticker>>,
    priority: Priority,
    universes: Universes,
}

/// Sends commands to the engine, and opens and frees the outputs' devices, so the tick
/// thread never does. One sender: each queue has one producer and one consumer.
pub struct Sender {
    commands: rtrb::Producer<Sent>,
    retired: rtrb::Consumer<Device>,
    universes: Universes,
    start: Instant,
}

/// A command, when it was sent, and the device it brings.
struct Sent {
    command: Command,
    at: Duration,
    device: Option<Device>,
}

/// How late an output's frames were handed over, so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lateness {
    pub p99: Duration,
    pub worst: Duration,
}

/// Written by the tick thread, read by anyone. Atomics only.
struct Shared {
    frame: AtomicU64,
    stop: AtomicBool,
    /// Per output.
    lateness: [[AtomicU64; LATENESS_BUCKETS]; MAX_OUTPUTS],
    latency: [AtomicU64; LATENCY_BUCKETS],
    /// A seqlock around `blackout`, `outputs` and `levels`: odd while the tick thread writes.
    version: AtomicU64,
    blackout: AtomicBool,
    outputs: [[AtomicU64; OUTPUT_WORDS]; MAX_OUTPUTS],
    /// The held levels, universe after universe.
    levels: Box<[AtomicU8]>,
}

impl Engine {
    /// Starts the tick thread with the best priority the system gives. No outputs yet:
    /// send `SetOutput`. Fails only when the OS refuses a thread.
    pub fn start(universes: Universes) -> io::Result<(Self, Sender)> {
        let count = usize::from(universes.count());
        let shared = Arc::new(Shared {
            frame: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            lateness: [const { [const { AtomicU64::new(0) }; LATENESS_BUCKETS] }; MAX_OUTPUTS],
            latency: [const { AtomicU64::new(0) }; LATENCY_BUCKETS],
            version: AtomicU64::new(0),
            blackout: AtomicBool::new(false),
            outputs: [const { [const { AtomicU64::new(0) }; OUTPUT_WORDS] }; MAX_OUTPUTS],
            levels: (0..count * ADDRESSES).map(|_| AtomicU8::new(0)).collect(),
        });
        let (commands, incoming) = rtrb::RingBuffer::new(QUEUE);
        // Each applied command retires at most one device, and the sender empties this
        // queue before it sends: it cannot fill up.
        let (outgoing, retired) = rtrb::RingBuffer::new(QUEUE + MAX_OUTPUTS);
        let start = Instant::now();
        let ticker = Ticker {
            shared: Arc::clone(&shared),
            commands: incoming,
            retired: outgoing,
            devices: std::array::from_fn(|_| None),
            held: vec![Levels::ZERO; count],
            out: vec![Levels::ZERO; count],
            sent: [Duration::ZERO; QUEUE],
            start,
        };
        let (tick, priority) = rt_thread::spawn("tick", engine_core::BEAT, move |priority| {
            ticker.run(priority)
        })?;
        let engine = Self {
            shared,
            tick: Some(tick),
            priority,
            universes,
        };
        let sender = Sender {
            commands,
            retired,
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
        self.read(&mut [])
    }

    /// The state, and the held levels copied into `into`, as one consistent picture.
    /// Never waits for the tick thread: it tries again if it published meanwhile.
    pub fn read(&self, into: &mut [Levels]) -> State {
        let shared = &self.shared;
        loop {
            let before = shared.version.load(Ordering::Acquire);
            let mut state = State::new(self.priority);
            state.blackout = shared.blackout.load(Ordering::Relaxed);
            for (output, words) in state.outputs.iter_mut().zip(&shared.outputs) {
                *output = from_words(words.each_ref().map(|w| w.load(Ordering::Relaxed)));
            }
            for (universe, cells) in into.iter_mut().zip(shared.levels.chunks(ADDRESSES)) {
                for (level, cell) in universe.0.iter_mut().zip(cells) {
                    *level = cell.load(Ordering::Relaxed);
                }
            }
            fence(Ordering::Acquire);
            if before.is_multiple_of(2) && shared.version.load(Ordering::Relaxed) == before {
                state.frame = shared.frame.load(Ordering::Relaxed);
                return state;
            }
            hint::spin_loop();
        }
    }

    /// How late this output's frames were handed over, so far. `None` before its first.
    pub fn lateness(&self, output: OutputNumber) -> Option<Lateness> {
        let counts = load(self.shared.lateness.get(output.slot())?);
        Some(Lateness {
            p99: percentile(&counts, 99)?,
            worst: worst(&counts)?,
        })
    }

    /// The time from a send to the frame that carries it, for 95 of 100 commands, so
    /// far. `None` before the first command.
    pub fn send_to_frame_p95(&self) -> Option<Duration> {
        percentile(&load(&self.shared.latency), 95)
    }
}

impl Sender {
    /// Queues a command for the very next wake, or says why not. Nothing changes on a
    /// refusal. `SetOutput` opens its device here, off the tick thread.
    pub fn send(&mut self, command: Command<ChannelNumbers, RunNumbers>) -> Result<(), Refusal> {
        while let Ok(device) = self.retired.pop() {
            drop(device); // closed and freed here, never on the tick thread
        }
        let command = command.check(self.universes)?;
        let device = match command {
            Command::SetOutput { kind, .. } => {
                Some(Device::open(kind).map_err(|_| Refusal::CannotOpen)?)
            }
            Command::Tick { .. }
            | Command::Handed { .. }
            | Command::SetChannel { .. }
            | Command::ReleaseChannel { .. }
            | Command::ReleaseAll
            | Command::Blackout { .. }
            | Command::RemoveOutput { .. } => None,
        };
        let at = self.start.elapsed();
        self.commands
            .push(Sent {
                command,
                at,
                device,
            })
            .map_err(|_| Refusal::QueueFull)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(tick) = self.tick.take() {
            drop(tick.join()); // the devices are freed here, not on the tick thread
        }
    }
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
struct Ticker {
    shared: Arc<Shared>,
    commands: rtrb::Consumer<Sent>,
    retired: rtrb::Producer<Device>,
    /// By output number, beside `State::outputs`.
    devices: [Option<Device>; MAX_OUTPUTS],
    /// The held levels: the state's universes.
    held: Vec<Levels>,
    /// The frame that goes out.
    out: Vec<Levels>,
    /// When each command applied in this wake was sent.
    sent: [Duration; QUEUE],
    start: Instant,
}

impl Ticker {
    /// The real-time loop, an action: reads the clock, waits, hands frames over,
    /// publishes. The logic is in `engine-core`. Nothing here may lock, allocate, block,
    /// log or call out. Returns itself, so its devices are freed by whoever joins it.
    fn run(mut self, priority: Priority) -> Self {
        let mut state = State::new(priority);
        while !self.shared.stop.load(Ordering::Relaxed) {
            // Sleep, then spin the last millisecond: a sleep can wake late, a spin cannot.
            let wake = next_wake(&state);
            thread::sleep(sleep_before(wake, self.start.elapsed()));
            while self.start.elapsed() < wake {
                hint::spin_loop();
            }
            let now = self.start.elapsed();
            state = self.apply(state, Command::Tick { now });

            // Apply the waiting commands right before the frame: a press is in the next one.
            let mut applied = 0;
            while applied < QUEUE {
                let Ok(sent) = self.commands.pop() else {
                    break;
                };
                state = self.apply(state, sent.command);
                if let Some(slot) = self.sent.get_mut(applied) {
                    *slot = sent.at;
                }
                self.swap(sent.command, sent.device);
                applied += 1;
            }

            frame(&state, &self.held, now, &mut self.out);
            let mut serve = [None; MAX_OUTPUTS];
            for (slot, number) in serve.iter_mut().zip(due(&state, now)) {
                *slot = Some(number);
            }
            for number in serve.into_iter().flatten() {
                state = self.serve(state, number, now);
            }

            let handed = self.start.elapsed();
            for at in self.sent.iter().take(applied) {
                count(&self.shared.latency, handed.saturating_sub(*at));
            }
            self.publish(&state, applied > 0);
        }
        self
    }

    /// For `SetOutput` and `RemoveOutput`: puts `device` at the output, sends the old one
    /// back to the sender and starts the output's lateness afresh. Others change nothing.
    fn swap(&mut self, command: Command, device: Option<Device>) {
        let (Command::SetOutput { output, .. } | Command::RemoveOutput { output }) = command
        else {
            return;
        };
        let Some(slot) = self.devices.get_mut(output.slot()) else {
            return;
        };
        if let Some(old) = std::mem::replace(slot, device) {
            // The queue has room for every device a wake can retire (see `start`).
            let _ = self.retired.push(old);
        }
        if let Some(lateness) = self.shared.lateness.get(output.slot()) {
            for count in lateness {
                count.store(0, Ordering::Relaxed);
            }
        }
    }

    /// Hands a due output its run of universes and counts what happened.
    fn serve(&mut self, state: State, number: OutputNumber, now: Duration) -> State {
        let slot = number.slot();
        let Some(Some(output)) = state.outputs.get(slot) else {
            return state;
        };
        if let Some(lateness) = self.shared.lateness.get(slot) {
            count(lateness, now.saturating_sub(output.due));
        }
        let outcome = match self.devices.get_mut(slot) {
            Some(Some(device)) => device.take(output.universes.of(&self.out)),
            _ => Outcome::Failed,
        };
        self.apply(
            state,
            Command::Handed {
                output: number,
                outcome,
            },
        )
    }

    fn apply(&mut self, state: State, command: Command) -> State {
        let (state, effect) = update(state, &mut self.held, command);
        if let Some(effect) = effect {
            match effect {}
        }
        state
    }

    /// The writer side of the seqlock: readers retry while `version` is odd or moved.
    /// The levels are written only when commands changed them.
    fn publish(&self, state: &State, levels: bool) {
        let shared = &self.shared;
        let version = shared.version.load(Ordering::Relaxed);
        shared.version.store(version.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        shared.blackout.store(state.blackout, Ordering::Relaxed);
        for (words, output) in shared.outputs.iter().zip(&state.outputs) {
            for (word, value) in words.iter().zip(to_words(output.as_ref())) {
                word.store(value, Ordering::Relaxed);
            }
        }
        if levels {
            let held = self.held.iter().flat_map(|universe| universe.0.iter());
            for (cell, level) in shared.levels.iter().zip(held) {
                cell.store(*level, Ordering::Relaxed);
            }
        }
        shared.version.store(version.wrapping_add(2), Ordering::Release);
        shared.frame.store(state.frame, Ordering::Relaxed);
    }
}
