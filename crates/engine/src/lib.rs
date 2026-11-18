//! The Bossylights engine. Everything outside talks to it; it never calls out.
//!
//! One tick thread owns the state and serves every output. It wakes when the next output
//! is due (or on a steady beat when none is set), applies the waiting commands, computes
//! the frame and hands each due output its run of universes. The logic lives in
//! `engine-core` (no `std`, no `alloc`); this crate is the thin action around it.
//!
//! ```text
//! Sender::send ---- command (+ device: opened, then attach) --> queue --> tick thread --> outputs
//!                                                                       |
//! free thread: retired devices freed here <------------ return queue ---+
//!                                                                       |
//! Engine::read, query <------------------- seqlock (atomics) <----------+
//! ```
//!
//! The tick thread does five things less (`docs/coding.md`, rule 7). Each has a check:
//!
//! | No | Check |
//! |---|---|
//! | locks | `clippy.toml` bans every std type that locks, here and in `outputs`. Commands and devices come and go through `rtrb`, wait-free; the state leaves through atomics |
//! | allocation | `engine-core` has no `alloc`; buffers are made in `start`; devices are opened by the `Sender` and freed by the free thread; `tests/no_alloc.rs` counts every allocation while it ticks, serves outputs and takes commands |
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
    ADDRESSES, API_VERSION, BEAT, BestEffort, ChannelLevel, ChannelNumbers, Color, Command,
    Counters, Levels, MAX_OUTPUTS, MAX_UNIVERSES, Outcome, Output, OutputKind, OutputNumber,
    Priority, Rate, Refusal, Reply, Request, Run, RunNumbers, State, Status, Universes, changes,
};
pub use engine_core::{
    Checked, Histogram, LATENCY_BUCKETS, LATENESS_BUCKETS, OUTPUT_WORDS, bucket, due, frame,
    from_words, next_wake, percentile, sleep_before, to_words, update, worst,
};
pub use outputs::Device;
use std::hint;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering, fence};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

/// Commands the engine takes in one wake. More are refused with [`Refusal::QueueFull`].
pub const QUEUE: usize = 256;
/// Devices on their way back from the tick thread. Each applied command retires at most
/// one; the sender sends no more while this many are not freed yet: it cannot fill up.
const RETIRED: usize = QUEUE + MAX_OUTPUTS;
/// How often the free thread frees retired devices when nobody wakes it.
const FREE_EVERY: Duration = Duration::from_millis(100);
/// How long `SetOutput` waits for the devices on their way back before it opens.
const FREE_WITHIN: Duration = Duration::from_secs(1);

/// The running engine: reads its state. Dropping it stops the tick thread.
pub struct Engine {
    shared: Arc<Shared>,
    tick: Option<JoinHandle<Ticker>>,
    free: Option<JoinHandle<()>>,
    priority: Priority,
    universes: Universes,
}

/// Sends commands to the engine and opens the outputs' devices, so the tick thread
/// never does. A `SetOutput` for the kind an output already has keeps its device.
/// One sender: each queue has one producer and one consumer.
pub struct Sender {
    open: Open,
    commands: rtrb::Producer<Sent>,
    /// What each output holds once the tick thread applied every command sent so far.
    held: [Held; MAX_OUTPUTS],
    /// Devices this sender sent the tick thread away from, so far. The free thread
    /// counts the ones it freed in `Shared::freed`.
    retired: u64,
    shared: Arc<Shared>,
    free: Thread,
    universes: Universes,
    start: Instant,
}

/// What an output holds, as the sender knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    Nothing,
    /// A device from `Sender::attach`: its kind is not known.
    Attached,
    /// A device `SetOutput` opened for this kind.
    Opened(OutputKind),
}

/// Opens the device for an output kind, off the tick thread. The engine's own is
/// `outputs::open`; a test brings one that opens fake devices.
pub type Open = Box<dyn FnMut(OutputKind) -> io::Result<Box<dyn Device>> + Send>;

/// A command, when it was sent, and the device it brings.
struct Sent {
    command: Command,
    at: Duration,
    device: Option<Box<dyn Device>>,
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
    /// Devices the free thread freed, so far.
    freed: AtomicU64,
}

impl Engine {
    /// Starts the tick thread with the best priority the system gives. No outputs yet:
    /// send `SetOutput`. Fails only when the OS refuses a thread.
    pub fn start(universes: Universes) -> io::Result<(Self, Sender)> {
        Self::start_with(universes, Box::new(outputs::open))
    }

    /// [`Engine::start`], with `open` making the devices `SetOutput` asks for.
    pub fn start_with(universes: Universes, open: Open) -> io::Result<(Self, Sender)> {
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
            freed: AtomicU64::new(0),
        });
        let (commands, incoming) = rtrb::RingBuffer::new(QUEUE);
        let (outgoing, retired) = rtrb::RingBuffer::new(RETIRED);
        let free = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("free devices".into())
                .spawn(move || free(retired, &shared))?
        };
        let start = Instant::now();
        let ticker = Ticker {
            shared: Arc::clone(&shared),
            commands: incoming,
            retired: outgoing,
            devices: std::array::from_fn(|_| None),
            held: vec![Levels::ZERO; count],
            out: vec![Levels::ZERO; count],
            carried: Carried::new(),
            start,
        };
        let (tick, priority) = rt_thread::spawn("tick", engine_core::BEAT, move |priority| {
            ticker.run(priority)
        })?;
        let sender = Sender {
            open,
            commands,
            held: [Held::Nothing; MAX_OUTPUTS],
            retired: 0,
            shared: Arc::clone(&shared),
            free: free.thread().clone(),
            universes,
            start,
        };
        let engine = Self {
            shared,
            tick: Some(tick),
            free: Some(free),
            priority,
            universes,
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
    /// far: counted once per output, when that output's next frame is handed over.
    /// `None` before the first frame that carried a command.
    pub fn send_to_frame_p95(&self) -> Option<Duration> {
        percentile(&load(&self.shared.latency), 95)
    }
}

impl Sender {
    /// Queues a command for the very next wake, or says why not. Nothing changes on a
    /// refusal. `SetOutput` opens its device here, off the tick thread, then attaches it.
    pub fn send(
        &mut self,
        command: Command<ChannelNumbers, RunNumbers, OutputKind>,
    ) -> Result<(), Refusal> {
        match command.check(self.universes)? {
            // Of these, only `RemoveOutput` changes what an output holds: nothing.
            Checked::Command(command) => self.push(command, None, Held::Nothing),
            Checked::SetOutput {
                output,
                kind,
                universes,
                rate,
            } => {
                let rate = rate.unwrap_or(outputs::default_rate(kind));
                let attach = Command::Attach {
                    output,
                    universes,
                    rate,
                };
                if self.held(output) == Held::Opened(kind) {
                    // The same device: only its run and rate change.
                    return self.push(attach, None, Held::Opened(kind));
                }
                // A device on its way back may hold what this one opens, a port say.
                self.wait_until_freed();
                let device = (self.open)(kind).map_err(|_| Refusal::CannotOpen)?;
                self.push(attach, Some(device), Held::Opened(kind))
            }
        }
    }

    /// Puts an opened device on the tick thread as `output`, in place of the one there.
    /// Every output gets there this way: `SetOutput` after opening its kind, a test with
    /// its own device. Its counters start at zero.
    pub fn attach(
        &mut self,
        output: OutputNumber,
        device: Box<dyn Device>,
        universes: Run,
        rate: Rate,
    ) -> Result<(), Refusal> {
        let command = Command::Attach {
            output,
            universes,
            rate,
        };
        self.push(command, Some(device), Held::Attached)
    }

    /// What `output` holds once every command sent so far is applied.
    fn held(&self, output: OutputNumber) -> Held {
        self.held
            .get(output.slot())
            .copied()
            .unwrap_or(Held::Nothing)
    }

    /// Retired devices not freed yet.
    fn on_their_way(&self) -> u64 {
        let freed = self.shared.freed.load(Ordering::Acquire);
        self.retired.saturating_sub(freed)
    }

    /// Waits, bounded, until the free thread freed every device sent away so far.
    fn wait_until_freed(&self) {
        let started = Instant::now();
        while self.on_their_way() > 0 && started.elapsed() < FREE_WITHIN {
            self.free.unpark();
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// Queues `command` with the `device` it brings. `then` is what its output holds
    /// once it is applied. A device `Attach` brings and `RemoveOutput` send the old one
    /// back; `Attach` with no device keeps it.
    fn push(
        &mut self,
        command: Command,
        device: Option<Box<dyn Device>>,
        then: Held,
    ) -> Result<(), Refusal> {
        // The output this command changes, and whether the device there goes back.
        let (changes, sends_back) = match command {
            Command::Attach { output, .. } => (Some(output), device.is_some()),
            Command::RemoveOutput { output } => (Some(output), true),
            Command::Tick { .. }
            | Command::Handed { .. }
            | Command::SetChannel { .. }
            | Command::ReleaseChannel { .. }
            | Command::ReleaseAll
            | Command::Blackout { .. }
            | Command::SetOutput { .. } => (None, false),
        };
        let retires =
            sends_back && changes.is_some_and(|output| self.held(output) != Held::Nothing);
        if retires && self.on_their_way() >= RETIRED as u64 {
            self.free.unpark();
            return Err(Refusal::QueueFull);
        }
        let at = self.start.elapsed();
        self.commands
            .push(Sent {
                command,
                at,
                device,
            })
            .map_err(|_| Refusal::QueueFull)?;
        if retires {
            self.retired += 1;
        }
        if let Some(held) = changes.and_then(|output| self.held.get_mut(output.slot())) {
            *held = then;
        }
        Ok(())
    }
}

/// The free thread: frees the devices the tick thread retired, closing their ports and
/// sockets, on a steady beat or when the sender wakes it. Until the engine stops.
fn free(mut retired: rtrb::Consumer<Box<dyn Device>>, shared: &Shared) {
    loop {
        while let Ok(device) = retired.pop() {
            drop(device);
            shared.freed.fetch_add(1, Ordering::Release);
        }
        if shared.stop.load(Ordering::Relaxed) {
            return; // what comes back later is freed with the tick thread's queue end
        }
        thread::park_timeout(FREE_EVERY);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(tick) = self.tick.take() {
            drop(tick.join()); // the devices are freed here, not on the tick thread
        }
        if let Some(free) = self.free.take() {
            free.thread().unpark();
            drop(free.join());
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
    retired: rtrb::Producer<Box<dyn Device>>,
    /// By output number, beside `State::outputs`.
    devices: [Option<Box<dyn Device>>; MAX_OUTPUTS],
    /// The held levels: the state's universes.
    held: Vec<Levels>,
    /// The frame that goes out.
    out: Vec<Levels>,
    /// When the commands were sent, until each output carried them.
    carried: Carried,
    start: Instant,
}

/// Commands an output's frame may still have to carry. Older ones are not counted.
const CARRY: usize = 4 * QUEUE;

/// When the last [`CARRY`] applied commands were sent, and how many each output's
/// frames carried so far: so each output counts the time from a send to its own next
/// frame. A calculation over fixed buffers: it never allocates.
struct Carried {
    sent: [Duration; CARRY],
    /// Commands applied so far.
    applied: u64,
    /// Per output: the commands applied before its last frame, or before it was attached.
    carried: [u64; MAX_OUTPUTS],
}

impl Carried {
    const fn new() -> Self {
        Self {
            sent: [Duration::ZERO; CARRY],
            applied: 0,
            carried: [0; MAX_OUTPUTS],
        }
    }

    /// A command sent at `at` was applied.
    fn applied(&mut self, at: Duration) {
        if let Some(slot) = self.sent.get_mut(ring(self.applied)) {
            *slot = at;
        }
        self.applied = self.applied.wrapping_add(1);
    }

    /// A new device at `output`: it carries what is applied from now on.
    fn attached(&mut self, output: usize) {
        if let Some(carried) = self.carried.get_mut(output) {
            *carried = self.applied;
        }
    }

    /// `output` handed over a frame at `handed`: each command applied since its last
    /// frame took `handed - sent` to get there.
    fn handed(&mut self, output: usize, handed: Duration, mut count: impl FnMut(Duration)) {
        let Some(carried) = self.carried.get_mut(output) else {
            return;
        };
        let oldest = self.applied.saturating_sub(CARRY as u64);
        for command in (*carried).max(oldest)..self.applied {
            if let Some(at) = self.sent.get(ring(command)) {
                count(handed.saturating_sub(*at));
            }
        }
        *carried = self.applied;
    }
}

/// Where command number `n` lives in the ring of [`Carried`].
fn ring(n: u64) -> usize {
    usize::try_from(n % CARRY as u64).unwrap_or(0)
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
                self.swap(sent.command, sent.device);
                self.carried.applied(sent.at);
                applied += 1;
            }

            frame(&state, &self.held, now, &mut self.out);
            let mut serve = [None; MAX_OUTPUTS];
            for (slot, number) in serve.iter_mut().zip(due(&state, now)) {
                *slot = Some(number);
            }
            for number in serve.into_iter().flatten() {
                state = self.serve(state, number, now);
                let (handed, latency) = (self.start.elapsed(), &self.shared.latency);
                self.carried
                    .handed(number.slot(), handed, |time| count(latency, time));
            }
            self.publish(&state, applied > 0);
        }
        self
    }

    /// For `Attach` and `RemoveOutput`: puts `device` at the output, sends the old one
    /// back to be freed and starts the output's lateness afresh. `Attach` without a
    /// device keeps the one there. Others change nothing.
    fn swap(&mut self, command: Command, device: Option<Box<dyn Device>>) {
        let (Command::Attach { output, .. } | Command::RemoveOutput { output }) = command else {
            return;
        };
        let Some(slot) = self.devices.get_mut(output.slot()) else {
            return;
        };
        let keeps = matches!(command, Command::Attach { .. }) && device.is_none();
        if !keeps {
            self.carried.attached(output.slot());
        }
        let old = if keeps {
            None
        } else {
            std::mem::replace(slot, device)
        };
        if let Some(old) = old {
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
        shared
            .version
            .store(version.wrapping_add(1), Ordering::Relaxed);
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
        shared
            .version
            .store(version.wrapping_add(2), Ordering::Release);
        shared.frame.store(state.frame, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_output_counts_a_command_when_its_own_frame_carries_it() {
        let ms = Duration::from_millis;
        #[derive(Clone, Copy, Debug)]
        enum Step {
            Attach(usize),
            Sent(u64),
            Handed(usize, u64),
        }
        use Step::{Attach, Handed, Sent};
        // (steps, times counted in ms)
        let cases = [
            (vec![Attach(0), Sent(10), Handed(0, 12)], vec![2]),
            // applied on a wake that served nothing: counted at the next frame
            (vec![Attach(0), Sent(10), Handed(0, 30)], vec![20]),
            (
                vec![Attach(0), Attach(1), Sent(10), Handed(0, 12), Handed(1, 33)],
                vec![2, 23],
            ),
            (vec![Attach(0), Sent(1), Sent(2), Handed(0, 5)], vec![4, 3]),
            (
                vec![Attach(0), Sent(1), Handed(0, 5), Handed(0, 9)],
                vec![4],
            ), // once
            (vec![Sent(1), Attach(0), Handed(0, 5)], vec![]), // sent before its device
        ];
        for (steps, expected) in cases {
            let mut carried = Carried::new();
            let mut counted = vec![];
            for step in &steps {
                match *step {
                    Attach(output) => carried.attached(output),
                    Sent(at) => carried.applied(ms(at)),
                    Handed(output, at) => carried.handed(output, ms(at), |t| counted.push(t)),
                }
            }
            assert_eq!(
                counted,
                expected.into_iter().map(ms).collect::<Vec<_>>(),
                "{steps:?}"
            );
        }
    }
}
