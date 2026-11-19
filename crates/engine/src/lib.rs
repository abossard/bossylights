//! The Bossylights engine. Everything outside talks to it; it never calls out.
//!
//! One tick thread owns the state and serves every output. It wakes when the next output
//! is due (or on a steady beat when none is set), applies the waiting commands, computes
//! the frame and hands each due output its run of universes. Each output sends on a
//! thread of its own (`lane.rs`), so the tick never waits behind a send. The logic lives
//! in `engine-core` (no `std`, no `alloc`); this crate is the thin action around it.
//!
//! ```text
//! Sender::send ---- command (+ device: opened, its thread started) --> queue --> tick thread --> each output's thread --> device
//!               (+ writers: the patch, compiled by the Rig)                    |   <-- what came of each frame --+
//!               (+ runs: the lights of a group)                                |
//! free thread: retired outputs, writers, runs freed here <-- return q. --------+
//!                                                                              |
//! Engine::latest, fresh --- an empty box ---> queue --> tick thread              |
//!                       <-- the box, filled -- queue <-- one wake's picture
//! ```
//!
//! The tick thread does five things less (`docs/coding.md`, rule 7). Each has a check:
//!
//! | No | Check |
//! |---|---|
//! | locks | `clippy.toml` bans every std type that locks, here and in `outputs`. Commands, devices and snapshots come and go through `rtrb`, wait-free |
//! | allocation | `engine-core` has no `alloc`; buffers and snapshot boxes are made in `start`; devices are opened, their threads and frame boxes made by the `Sender` and freed by the free thread; `tests/no_alloc.rs` counts every allocation, on every thread, while it ticks, serves outputs, takes commands and a reader asks for a snapshot on every frame |
//! | blocking calls, files, network | `engine-core` has no `std`; `clippy.toml` bans files, sockets, processes, stdin here and in `rt-thread`. The tick sends nothing: it copies the frame into a spare box of the output's thread. The only wait is `thread::sleep`, then a spin, until the next output is due |
//! | logging | `clippy.toml` bans print macros and stdout/stderr; no logger dependency |
//! | calls outward | `clippy.toml` bans channels and condvars; the tick thread fills the boxes readers sent and wakes no reader. It wakes an output's own thread when it hands it a frame (`unpark`, wait-free) |

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub use engine_core::{
    ADDRESSES, API_VERSION, Adapter, BEAT, BestEffort, ChannelLevel, ChannelNumbers, Color,
    Command, Counters, Grouped, Levels, MAX_OUTPUTS, MAX_UNIVERSES, Member, MemberList, Members,
    Outcome, Output, OutputKind, OutputNumber, Patched, Priority, Project, ProjectOutput, Rate,
    Refusal, Reply, Request, Run, RunNumbers, State, Status, Target, Text, Universes, changes,
};
use engine_core::{
    EngineCommand, Histogram, LATENCY_BUCKETS, LATENESS_BUCKETS, MAX_LIGHTS, bucket, due, frame,
    percentile, update, wait,
};
pub use fixture_library::Fixture;
pub use rig::Patch;
mod snapshot;

use engine_core::{LightRun, Lights, ProjectEffect, ProjectEvent, Writer};
use rig::{PatchEffect, Whole};

pub use outputs::Device;
mod lane;
use lane::Lane;
use snapshot::{Hand, Reader};
pub use snapshot::{Lateness, Snapshot};
mod wake;
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};
use wake::{Clock, Trace};
#[cfg(feature = "test-support")]
pub use wake::{Event, Wakes};

/// Commands the engine takes in one wake. More are refused with [`Refusal::QueueFull`].
pub const QUEUE: usize = 256;
/// The lights of every engine: the buffer of light values has this length.
const LIGHTS: Lights = Lights::MOST;
/// Devices and writers on their way back from the tick thread. Each applied command
/// retires at most one; the sender sends no more while this many are not freed yet: it
/// cannot fill up.
const RETIRED: usize = QUEUE + MAX_OUTPUTS;
/// How often the free thread frees retired devices when nobody wakes it.
const FREE_EVERY: Duration = Duration::from_millis(100);
/// How long `SetOutput` waits for the devices on their way back before it opens.
const FREE_WITHIN: Duration = Duration::from_secs(1);

/// The running engine: reads its state. Dropping it stops the tick thread.
pub struct Engine {
    shared: Arc<Shared>,
    reader: Reader,
    tick: Option<JoinHandle<Ticker>>,
    free: Option<JoinHandle<()>>,
    universes: Universes,
}

/// The engine front: owns the [`Project`], checks every client command against it,
/// sends the tick thread what follows and opens the outputs' devices, so the tick thread
/// never does. The only sender: each queue has one producer and one consumer, and a
/// check is never stale.
pub struct Sender {
    /// `None` while an [`Opening`] has it: one device opens at a time.
    open: Option<Open>,
    commands: rtrb::Producer<Sent>,
    project: Project,
    /// The fixtures, their lights and writers. Shared with readers as it is: a change
    /// makes a new one.
    patch: Arc<Patch>,
    /// Devices this sender sent the tick thread away from, so far. The free thread
    /// counts the ones it freed in `Shared::freed`.
    retired: u64,
    /// Commands queued for the tick thread, so far: [`Snapshot::applied`] counts the same.
    sent: u64,
    shared: Arc<Shared>,
    free: Thread,
    clock: Clock,
    /// The engine's universes: an output's frame boxes hold every one.
    universes: Universes,
}

/// Opens the device for an output kind, off the tick thread. The engine's own is
/// `outputs::open`; a test brings one that opens fake devices.
pub type Open = Box<dyn FnMut(OutputKind) -> io::Result<Box<dyn Device>> + Send>;

/// A device to open for `SetOutput`: [`Opening::open`] blocks, so it runs where it may,
/// then [`Sender::finish`] puts the device on the tick thread.
pub struct Opening {
    output: OutputNumber,
    kind: OutputKind,
    universes: Run,
    rate: Rate,
    open: Open,
    shared: Arc<Shared>,
    free: Thread,
    /// The devices sent away before this one opens: a port they hold must be free.
    retired: u64,
}

/// What came of an [`Opening`]. It brings the `open` back.
pub struct Opened {
    open: Open,
    output: OutputNumber,
    kind: OutputKind,
    universes: Run,
    rate: Rate,
    device: io::Result<Box<dyn Device>>,
}

/// A command, when it was sent, and what it brings.
struct Sent {
    command: EngineCommand,
    at: Duration,
    brings: Brings,
}

/// What a command brings the tick thread, made before it is sent.
enum Brings {
    Nothing,
    /// An opened device on its own thread, for `Attach`.
    Device(Box<Lane>),
    /// The writers of the patch, for `Writers`.
    Writers(Vec<Writer>),
    /// The runs of a group, for `Runs`.
    Runs(Vec<LightRun>),
}

/// What goes back from the tick thread, to be freed on the free thread.
#[expect(
    dead_code,
    reason = "held only to be dropped on the free thread, never read"
)]
enum Retired {
    Device(Box<Lane>),
    Writers(Vec<Writer>),
    Runs(Vec<LightRun>),
}

/// What came of [`Sender::begin`].
pub enum Begun {
    /// Queued for the very next wake.
    Queued,
    /// Patched as `fixture`, its writers queued for the very next wake.
    Patched { fixture: u16 },
    /// A device must be opened first: [`Opening::open`], then [`Sender::finish`].
    Opening(Opening),
}

/// Between the threads of the engine. Atomics only.
struct Shared {
    stop: AtomicBool,
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
        Self::start_on(universes, open, Clock::wall(), Trace::none())
    }

    /// [`Engine::start_with`] on a clock the test steps ([`Wakes`]), which also reads
    /// each due hand-over and what came of it. The clock starts running.
    #[cfg(feature = "test-support")]
    pub fn start_stepped(universes: Universes, open: Open) -> io::Result<(Self, Sender, Wakes)> {
        let (clock, trace, wakes) = wake::stepped();
        let (engine, sender) = Self::start_on(universes, open, clock, trace)?;
        Ok((engine, sender, wakes))
    }

    fn start_on(
        universes: Universes,
        open: Open,
        clock: Clock,
        trace: Trace,
    ) -> io::Result<(Self, Sender)> {
        let count = usize::from(universes.count());
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            freed: AtomicU64::new(0),
        });
        let (hand, queues) = snapshot::hand_over();
        let (commands, incoming) = rtrb::RingBuffer::new(QUEUE);
        let (outgoing, retired) = rtrb::RingBuffer::new(RETIRED);
        let free = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("free devices".into())
                .spawn(move || free(retired, &shared))?
        };
        let ticker = Ticker {
            shared: Arc::clone(&shared),
            commands: incoming,
            retired: outgoing,
            devices: std::array::from_fn(|_| None),
            writers: Vec::new(),
            // `run` puts in the priority, once the thread has it.
            now: Box::new(Snapshot::new(universes, Priority::RealTime)),
            hand,
            out: vec![Levels::ZERO; count],
            carried: Carried::new(),
            clock: clock.clone(),
            trace,
        };
        let (tick, priority) = rt_thread::spawn("tick", move |priority| ticker.run(priority))?;
        let sender = Sender {
            open: Some(open),
            commands,
            project: Project::new(universes),
            patch: Arc::new(Patch::new(
                universes,
                LIGHTS,
                Whole(|kind| outputs::limits(kind).whole),
            )),
            retired: 0,
            sent: 0,
            shared: Arc::clone(&shared),
            free: free.thread().clone(),
            clock,
            universes,
        };
        let engine = Self {
            shared,
            reader: Reader::new(queues, Snapshot::new(universes, priority)),
            tick: Some(tick),
            free: Some(free),
            universes,
        };
        Ok((engine, sender))
    }

    pub fn universes(&self) -> Universes {
        self.universes
    }

    /// The newest picture the tick thread sent back, and asks for the next one. Never
    /// waits: before the first comes back, the state the engine started with.
    pub fn latest(&mut self) -> &Snapshot {
        self.reader.latest()
    }

    /// A picture that carries every command sent before this call. Waits for it, about
    /// two wakes.
    pub fn fresh(&mut self) -> &Snapshot {
        self.reader.fresh()
    }
}

impl Sender {
    /// Queues a command for the very next wake, or says why not. Nothing changes on a
    /// refusal. `SetOutput` opens its device here and waits for it: a caller that must
    /// not wait uses [`Sender::begin`], [`Opening::open`] and [`Sender::finish`]. With no
    /// library: `Patch` is refused (`NoSuchFixture`), use [`Sender::begin`] for it.
    pub fn send(&mut self, command: Command) -> Result<(), Refusal> {
        match self.begin(command, &[])? {
            Begun::Queued | Begun::Patched { .. } => Ok(()),
            Begun::Opening(opening) => self.finish(opening.open()),
        }
    }

    /// Checks `command` against the project and sends what follows. `library` is where
    /// `Patch` looks up its fixture. [`Begun::Opening`] when a device must be opened
    /// first: until [`Sender::finish`] the output is `Opening`.
    pub fn begin(&mut self, command: Command, library: &[Fixture]) -> Result<Begun, Refusal> {
        let (project, effect) = self.project.update(ProjectEvent::Client(command))?;
        if let Some(
            ProjectEffect::Keep {
                kind, universes, ..
            }
            | ProjectEffect::Open {
                kind, universes, ..
            },
        ) = effect
        {
            outputs::limits(kind).check(universes)?;
        }
        // No patch where a fixture loses a channel: a change of the outputs that would
        // leave one is refused.
        if let Some(
            ProjectEffect::Keep { .. }
            | ProjectEffect::Open { .. }
            | ProjectEffect::Send(EngineCommand::RemoveOutput { .. }),
        ) = effect
            && let Some((fixture, _)) = self.patch.unstreamed(&project)
        {
            return Err(Refusal::StreamsFixture { fixture });
        }
        let begun = match effect {
            None => Begun::Queued,
            Some(ProjectEffect::Send(command)) => {
                self.push(command, Brings::Nothing)?;
                Begun::Queued
            }
            Some(ProjectEffect::Patch(command)) => {
                let (patch, effects) = self.patch.update(command, library)?;
                // and a change of the patch that would make one is refused
                if let Some((_, reason)) = patch.as_ref().and_then(|p| p.unstreamed(&project)) {
                    return Err(reason);
                }
                let begun = self.follow(effects)?;
                if let Some(patch) = patch {
                    self.patch = Arc::new(patch);
                }
                begun
            }
            Some(ProjectEffect::Keep {
                output,
                kind,
                universes,
                rate,
            }) => {
                let rate = rate.unwrap_or(outputs::default_rate(kind));
                let attach = EngineCommand::Attach {
                    output,
                    universes,
                    rate,
                };
                self.push(attach, Brings::Nothing)?;
                Begun::Queued
            }
            Some(ProjectEffect::Open {
                output,
                kind,
                universes,
                rate,
            }) => Begun::Opening(Opening {
                output,
                kind,
                universes,
                rate: rate.unwrap_or(outputs::default_rate(kind)),
                open: self.open.take().ok_or(Refusal::Opening)?,
                shared: Arc::clone(&self.shared),
                free: self.free.clone(),
                retired: self.retired,
            }),
        };
        self.project = project;
        Ok(begun)
    }

    /// Does what follows a change of the patch: all of it, or none of it.
    fn follow(&mut self, effects: Vec<PatchEffect>) -> Result<Begun, Refusal> {
        // Every effect sends at most one command, which retires at most one box.
        if self.commands.slots() < effects.len()
            || on_their_way(&self.shared, self.retired) + effects.len() as u64 > RETIRED as u64
        {
            self.free.unpark();
            return Err(Refusal::QueueFull);
        }
        let mut begun = Begun::Queued;
        for effect in effects {
            match effect {
                PatchEffect::Patched { fixture } => begun = Begun::Patched { fixture },
                PatchEffect::Writers(writers) => {
                    self.push(EngineCommand::Writers, Brings::Writers(writers))?;
                    self.free.unpark(); // the old writers are on their way back
                }
                PatchEffect::Send(command) => self.push(command, Brings::Nothing)?,
                PatchEffect::Runs { change, runs } => {
                    self.push(EngineCommand::Runs { change }, Brings::Runs(runs))?;
                }
            }
        }
        Ok(begun)
    }

    /// Puts the device of `opened` on the tick thread, or says why not.
    pub fn finish(&mut self, opened: Opened) -> Result<(), Refusal> {
        let Opened {
            open,
            output,
            kind,
            universes,
            rate,
            device,
        } = opened;
        self.open = Some(open);
        let attach = EngineCommand::Attach {
            output,
            universes,
            rate,
        };
        let sent = device
            .and_then(|device| {
                Lane::start(device, self.universes, self.clock.clone()).map(Box::new)
            })
            .map_err(|_| Refusal::CannotOpen)
            .and_then(|lane| self.push(attach, Brings::Device(lane)));
        let event = match sent {
            Ok(()) => ProjectEvent::Opened { output, kind },
            Err(_) => ProjectEvent::OpenFailed { output },
        };
        if let Ok((project, _)) = self.project.update(event) {
            self.project = project;
        }
        sent
    }

    /// Puts an opened device on the tick thread as `output`, in place of the one there:
    /// a test's own device, whose kind the project does not know. Its counters start at
    /// zero. `CannotOpen` when the system gives the device no thread.
    pub fn attach(
        &mut self,
        output: OutputNumber,
        device: Box<dyn Device>,
        universes: Run,
        rate: Rate,
    ) -> Result<(), Refusal> {
        let (project, _) = self
            .project
            .update(ProjectEvent::Attached { output, universes })?;
        if let Some((fixture, _)) = self.patch.unstreamed(&project) {
            return Err(Refusal::StreamsFixture { fixture });
        }
        let command = EngineCommand::Attach {
            output,
            universes,
            rate,
        };
        let lane = Lane::start(device, self.universes, self.clock.clone())
            .map(Box::new)
            .map_err(|_| Refusal::CannotOpen)?;
        self.push(command, Brings::Device(lane))?;
        self.project = project;
        Ok(())
    }

    /// Takes `patch` in place of the patch, as a show file brings it: made from
    /// [`Sender::patch`], so it has this engine's universes and lights. Its writers go to
    /// the tick thread. Refused as a change of the patch is: nothing changes.
    pub fn load(&mut self, patch: Patch) -> Result<(), Refusal> {
        if let Some((_, reason)) = patch.unstreamed(&self.project) {
            return Err(reason);
        }
        self.follow(vec![PatchEffect::Writers(patch.writers().to_vec())])?;
        self.patch = Arc::new(patch);
        Ok(())
    }

    /// What the engine was asked to be: its outputs and what each sends to.
    pub fn project(&self) -> Project {
        self.project
    }

    /// The patch as it is now.
    pub fn patch(&self) -> Arc<Patch> {
        Arc::clone(&self.patch)
    }

    /// How many commands this sender queued for the tick thread, so far. A snapshot whose
    /// [`Snapshot::applied`] is this number was taken under the project and the patch as
    /// they are now, or as they were when this number was last read.
    pub fn sent(&self) -> u64 {
        self.sent
    }

    /// Queues `command` with what it `brings`. A device `Attach` brings and
    /// `RemoveOutput` send the old one back; `Attach` with no device keeps it. `Writers`
    /// send the old writers back, `Runs` the runs it brought.
    fn push(&mut self, command: EngineCommand, brings: Brings) -> Result<(), Refusal> {
        // The output this command changes, and whether the device there goes back.
        let (changes, sends_back) = match command {
            EngineCommand::Attach { output, .. } => {
                (Some(output), matches!(brings, Brings::Device(_)))
            }
            EngineCommand::RemoveOutput { output } => (Some(output), true),
            EngineCommand::Tick { .. }
            | EngineCommand::Handed { .. }
            | EngineCommand::Sent { .. }
            | EngineCommand::SetChannel { .. }
            | EngineCommand::ReleaseChannel { .. }
            | EngineCommand::ReleaseAll
            | EngineCommand::Blackout { .. }
            | EngineCommand::Writers
            | EngineCommand::Runs { .. }
            | EngineCommand::SetIntensity { .. }
            | EngineCommand::SetColour { .. }
            | EngineCommand::ReleaseIntensity { .. }
            | EngineCommand::ReleaseColour { .. }
            | EngineCommand::Darken { .. } => (None, false),
        };
        let retires = matches!(command, EngineCommand::Writers | EngineCommand::Runs { .. })
            || sends_back && changes.is_some_and(|output| self.project.output(output).is_some());
        if retires && on_their_way(&self.shared, self.retired) >= RETIRED as u64 {
            self.free.unpark();
            free_elsewhere(brings);
            return Err(Refusal::QueueFull);
        }
        let at = self.clock.now();
        if let Err(rtrb::PushError::Full(refused)) = self.commands.push(Sent {
            command,
            at,
            brings,
        }) {
            free_elsewhere(refused.brings);
            return Err(Refusal::QueueFull);
        }
        if retires {
            self.retired += 1;
        }
        self.sent += 1;
        Ok(())
    }
}

/// Frees a device the tick thread did not take, and stops its thread, on a thread of its
/// own: closing a port can take long, and the sender may be shared behind a lock. Without
/// a thread from the system, it is freed here after all. Writers are freed here.
fn free_elsewhere(brings: Brings) {
    if let Brings::Device(lane) = brings {
        drop(
            thread::Builder::new()
                .name("free a refused device".into())
                .spawn(move || drop(lane)),
        );
    }
}

impl Opening {
    /// Opens the device. Blocks: first, bounded, until the devices sent away before it
    /// are freed (one may hold the port this opens), then for as long as opening takes.
    pub fn open(mut self) -> Opened {
        let started = Instant::now();
        while on_their_way(&self.shared, self.retired) > 0 && started.elapsed() < FREE_WITHIN {
            self.free.unpark();
            thread::sleep(Duration::from_millis(1));
        }
        // A panic in `open` is a failed open: the `open` comes back either way.
        let kind = self.kind;
        let open = &mut self.open;
        let device = panic::catch_unwind(AssertUnwindSafe(|| open(kind)))
            .unwrap_or_else(|_| Err(io::Error::other("opening the device panicked")));
        Opened {
            open: self.open,
            output: self.output,
            kind: self.kind,
            universes: self.universes,
            rate: self.rate,
            device,
        }
    }
}

/// Of the `retired` devices sent away so far, those not freed yet.
fn on_their_way(shared: &Shared, retired: u64) -> u64 {
    retired.saturating_sub(shared.freed.load(Ordering::Acquire))
}

/// The free thread: frees the devices and writers the tick thread retired, closing ports
/// and sockets, on a steady beat or when the sender wakes it. Until the engine stops.
fn free(mut retired: rtrb::Consumer<Retired>, shared: &Shared) {
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

/// What the tick thread owns. Every buffer is made before it starts.
struct Ticker {
    shared: Arc<Shared>,
    commands: rtrb::Consumer<Sent>,
    retired: rtrb::Producer<Retired>,
    /// Each output's own thread, by output number, beside `State::outputs`.
    devices: [Option<Box<Lane>>; MAX_OUTPUTS],
    /// The patch's writers: lights to bytes, every frame.
    writers: Vec<Writer>,
    /// The state, the held levels and what was counted: what readers get a copy of.
    now: Box<Snapshot>,
    hand: Hand,
    /// The frame that goes out.
    out: Vec<Levels>,
    /// When the commands were sent, until each output carried them.
    carried: Carried,
    clock: Clock,
    trace: Trace,
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
    /// The real-time loop, an action: reads the clock, waits, hands frames over and
    /// the snapshot. The logic is in `engine-core`. Nothing here may lock, allocate,
    /// block, log or call out. Returns itself, so its devices are freed by whoever joins it.
    fn run(mut self, priority: Priority) -> Self {
        self.now.state = State::new(priority);
        while !self.shared.stop.load(Ordering::Relaxed) {
            // Sleep, then spin the last millisecond before an output: a sleep can wake
            // late, a spin cannot. A beat with no output only sleeps.
            let (sleep, spin) = wait(&self.now.state, self.clock.now());
            self.clock.wait(sleep, spin, &self.shared.stop);
            // The cutoff (`wake.rs`): the picture asked for and the commands published
            // so far belong to this wake, then the frame time is read. A command
            // published after it waits for the next wake.
            let asked = self.hand.asked();
            let published = self.commands.slots().min(QUEUE);
            let now = self.clock.cutoff(&self.shared.stop);
            self.apply(EngineCommand::Tick { now });

            // Apply them right before the frame: a press is in the next one.
            for _ in 0..published {
                let Ok(sent) = self.commands.pop() else {
                    break;
                };
                self.apply(sent.command);
                self.swap(sent.command, sent.brings);
                self.carried.applied(sent.at);
                self.now.applied_one();
            }

            frame(
                &self.now.state,
                self.now.universes(),
                self.now.holds(),
                self.now.lights(),
                &self.writers,
                &mut self.out,
            );
            self.back();
            let mut serve = [None; MAX_OUTPUTS];
            for (slot, number) in serve.iter_mut().zip(due(&self.now.state, now)) {
                *slot = Some(number);
            }
            for number in serve.into_iter().flatten() {
                self.serve(number);
                let (handed, now) = (self.clock.now(), &mut self.now);
                self.carried
                    .handed(number.slot(), handed, |time| now.carried(time));
            }
            if let Some(asked) = asked {
                self.hand.fill(asked, &self.now);
            }
        }
        self
    }

    /// For `Attach` and `RemoveOutput`: puts the device brought at the output, sends the
    /// old one back to be freed and starts the output's lateness afresh. `Attach` without
    /// a device keeps the one there. For `Writers`: the writers brought take the place of
    /// the old ones, which go back. For `Runs`: each run brought takes the change, in this
    /// wake, and the runs go back. Others change nothing.
    fn swap(&mut self, command: EngineCommand, brings: Brings) {
        let (device, writers) = match brings {
            Brings::Nothing => (None, None),
            Brings::Device(device) => (Some(device), None),
            Brings::Writers(writers) => (None, Some(writers)),
            Brings::Runs(runs) => {
                if let EngineCommand::Runs { change } = command {
                    for run in &runs {
                        self.apply(change.on(*run));
                    }
                }
                // The queue has room for every box a wake can retire (see `start`).
                let _ = self.retired.push(Retired::Runs(runs));
                return;
            }
        };
        if let (EngineCommand::Writers, Some(writers)) = (command, writers) {
            let old = std::mem::replace(&mut self.writers, writers);
            // The queue has room for every box a wake can retire (see `start`).
            let _ = self.retired.push(Retired::Writers(old));
            return;
        }
        let (EngineCommand::Attach { output, .. } | EngineCommand::RemoveOutput { output }) =
            command
        else {
            return;
        };
        let Some(slot) = self.devices.get_mut(output.slot()) else {
            return;
        };
        let keeps = matches!(command, EngineCommand::Attach { .. }) && device.is_none();
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
            let _ = self.retired.push(Retired::Device(old));
        }
        self.now.attached(output.slot());
    }

    /// Hands a due output its run of universes, and when its next is due, on to its own
    /// thread. What comes of it is counted when its thread hands the box back ([`Ticker::back`]). With no spare
    /// box, the device is still on older frames: this one is dropped and counted now.
    fn serve(&mut self, number: OutputNumber) {
        let slot = number.slot();
        let Some(Some(output)) = self.now.state.outputs.get(slot).copied() else {
            return;
        };
        let at = self.clock.now();
        self.apply(EngineCommand::Handed { output: number, at });
        self.trace.handed(number, output.due, at);
        let next = match self.now.state.outputs.get(slot) {
            Some(Some(handed)) => handed.due,
            _ => at,
        };
        let frame = output.universes.of(&self.out);
        let dropped = match self.devices.get_mut(slot) {
            Some(Some(lane)) => match lane.hand(frame, output.due, next) {
                lane::Hand::Handed => {
                    self.now.handed_one(slot);
                    None
                }
                lane::Hand::Full => Some(Outcome::Dropped),
            },
            _ => Some(Outcome::Failed),
        };
        if let Some(outcome) = dropped {
            self.trace.came(number, outcome, output.due);
            self.now.late(slot, at.saturating_sub(output.due));
            self.apply(EngineCommand::Sent {
                output: number,
                outcome,
                at,
            });
        }
    }

    /// Counts what came of the frames each output's thread handed back since the last
    /// wake: the outcome, and how late the device was handed each.
    fn back(&mut self) {
        let Self {
            devices,
            now,
            trace,
            ..
        } = self;
        for (lane, number) in devices.iter_mut().zip(1..) {
            let (Some(lane), Ok(output)) = (lane, OutputNumber::try_from(number)) else {
                continue;
            };
            lane.back(|sent| {
                trace.came(output, sent.outcome, sent.due);
                now.late(output.slot(), sent.at.saturating_sub(sent.due));
                now.update(EngineCommand::Sent {
                    output,
                    outcome: sent.outcome,
                    at: sent.at,
                });
            });
        }
    }

    fn apply(&mut self, command: EngineCommand) {
        self.now.update(command);
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
