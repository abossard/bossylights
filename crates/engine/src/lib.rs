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
    ADDRESSES, Adapter, BEAT, BestEffort, ChannelLevel, ChannelNumbers, Color, Command, Counters,
    Grouped, Levels, MAX_OUTPUTS, MAX_UNIVERSES, Member, MemberList, Members, OpenFailure, Outcome,
    Output, OutputKind, OutputNumber, Patched, Priority, Project, ProjectOutput, Rate, Refusal,
    RouteFault, Run, RunNumbers, Span, State, Status, Target, Text, Universes, changes,
};
/// A test's device refused the route it was given.
#[cfg(any(test, feature = "test-support"))]
const REFUSED_ROUTE: Refusal = Refusal::CannotOpen {
    why: OpenFailure::Device { with: None },
};

pub use engine_core::{
    AuthoredStep, Bpm, Brightness, GrabId, Light, ManualEnd, MixRules, OtherEnd, Palette, Pattern,
    PlayId, PlayState, Property, Rank, Repeat, Rgb, Saved, SavedLook, Sequence, SequenceItem,
    Source, Speed, Swatches, Use, Value, WaveTarget, When,
};
use engine_core::{
    EngineCommand, Histogram, LATENCY_BUCKETS, LATENESS_BUCKETS, Watch, bucket, due, percentile,
    update, wait,
};
pub use fixture_library::Fixture;
pub use looks::{Looks, PLAN_BYTES};
pub use rig::Patch;
mod front;
mod grabs;
mod publication;
mod snapshot;
mod tick;
#[cfg(feature = "test-support")]
mod timing;

pub use front::{
    Base, Begun, Handoff, Open, Opened, Opening, Prepared, ProjectView, Sender, Unsent,
};
use front::{free, recover};
use tick::{Ticker, TickerStart};

use engine_core::{
    Channels, End, LightRun, Lights, LookCommand, PLAYS, ProjectEffect, ProjectEvent, stops, unkept,
};

pub use outputs::Device;
mod cutover;
use cutover::{Effect as CutoverEffect, Event as CutoverEvent, State as CutoverState};
mod lane;
mod life;
mod recovery;
pub use lane::Route;
pub use life::TickFailed;
use life::{EngineLife, LifeEvent, TickGuard};
use publication::Reader;
pub use snapshot::{Lateness, Snapshot};
#[cfg(feature = "test-support")]
pub use timing::{Timing, Timings};
mod wake;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::{self, JoinHandle, Thread};
use std::time::Duration;
use wake::{Clock, Trace};
#[cfg(feature = "test-support")]
pub use wake::{Event, Wakes};

/// Commands the engine takes in one wake. More are refused with [`Refusal::QueueFull`].
pub const QUEUE: usize = 256;
/// The lights of every engine: the buffer of light values has this length.
const LIGHTS: Lights = Lights::MOST;
/// Boxes on their way back from the tick thread (devices, writers, plans), and the
/// plan of each play there, which goes back once when the play ends: its return is kept
/// for it when it starts. The sender sends nothing that adds one while this many are not
/// freed yet, so the queue back cannot fill up, and a release needs only a place in the
/// command queue.
const RETIRED: usize = QUEUE + MAX_OUTPUTS;
/// How often the free thread frees retired devices when nobody wakes it.
const FREE_EVERY: Duration = Duration::from_millis(100);
/// How long `SetOutput` waits for the devices on their way back before it opens.
const FREE_WITHIN: Duration = Duration::from_secs(1);

/// Whether `command` edits what the show file keeps (the patch, groups or looks).
/// A shared front prepares show edits and output routes without its publisher lock.
pub const fn edits(command: &Command) -> bool {
    match command {
        Command::Patch { .. }
        | Command::PatchStrip { .. }
        | Command::Move { .. }
        | Command::Unpatch { .. }
        | Command::Rename { .. }
        | Command::SetGroup { .. }
        | Command::RemoveGroup { .. }
        | Command::PutStep { .. }
        | Command::PutPattern { .. }
        | Command::PutSequence { .. }
        | Command::RemoveLook { .. }
        | Command::SetMixRules { .. } => true,
        Command::SetChannel { .. }
        | Command::ReleaseChannel { .. }
        | Command::ReleaseChannels
        | Command::ReleaseAll
        | Command::Blackout { .. }
        | Command::SetOutput { .. }
        | Command::RemoveOutput { .. }
        | Command::SetIntensity { .. }
        | Command::SetColour { .. }
        | Command::ReleaseIntensity { .. }
        | Command::ReleaseColour { .. }
        | Command::SetProperty { .. }
        | Command::UpdateGrab { .. }
        | Command::EndGrab { .. }
        | Command::ReleaseProperty { .. }
        | Command::StartLook { .. }
        | Command::StopLook { .. }
        | Command::EndFlash { .. }
        | Command::ReleaseHold { .. }
        | Command::SetInternalTempo { .. }
        | Command::TapTempo
        | Command::ReleaseTempo => false,
    }
}

/// The running engine: reads its state. Dropping it stops the tick thread.
pub struct Engine {
    shared: Arc<Shared>,
    reader: Reader,
    tick: Option<JoinHandle<Ticker>>,
    free: Option<JoinHandle<()>>,
    retry: Option<JoinHandle<()>>,
    universes: Universes,
    /// The machine's one engine slot in tests (#319). Free again after `drop` joined the threads.
    #[cfg(any(test, feature = "test-support"))]
    _slot: engine_slot::Slot,
}

/// Between the threads of the engine. Atomics only.
struct Shared {
    /// The engine's life ([`life::Life`]): lanes send only while it runs.
    life: Arc<EngineLife>,
    /// Odd from just before the FIFO cutoff until this wake has handed all due frames.
    wake: AtomicU64,
    /// Boxes the free thread freed, so far.
    freed: AtomicU64,
    plan_bytes_freed: AtomicU64,
    /// Of them, the devices. A plan the tick thread sends back on its own is counted
    /// in `freed` before the front counts it, so an opening counts devices only.
    devices_freed: AtomicU64,
    /// Changes the sender ended, so far ([`Sender::revise`]). A wake reads it at its cutoff.
    revision: AtomicU64,
    /// The last generation ended in each slot, including waits cancelled and
    /// once-uses that finished before the sender observed them.
    ended: [AtomicU32; PLAYS],
    /// FIFO command number of the version frozen in each slot.
    kept: [AtomicU64; PLAYS],
    /// Tail of the most recently completed frame. Prefix work can then be reused.
    applied: AtomicU64,
    /// A test holds the free thread: what goes back is not freed while set.
    #[cfg(any(test, feature = "test-support"))]
    stalled: AtomicBool,
    /// Samples in the first press bucket when the most recent lane took its frame.
    #[cfg(all(test, feature = "test-support"))]
    presses_at_handoff: AtomicU64,
}

impl Shared {
    /// Order FIFO publication against the tick's next cutoff. Both operations
    /// update this same atomic so a later cutoff sees the published command.
    fn published(&self) -> Option<u64> {
        let wake = self.wake.fetch_add(0, Ordering::AcqRel);
        (life::phase(wake) == life::Phase::Handing).then_some(wake)
    }

    /// A wake already past its cutoff may still owe the old lane a frame.
    /// Only the thread completing the handoff waits, never the tick or publisher.
    fn finish_wake(&self, unfinished: Option<u64>) {
        let Some(wake) = unfinished else { return };
        while self.wake.load(Ordering::Acquire) == wake && self.life.get().sends() {
            thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Engine {
    /// Starts the tick thread with the best priority the system gives. No outputs yet:
    /// send `SetOutput`. Fails only when the OS refuses a thread.
    ///
    /// In tests every start first waits for the machine's one engine slot (`engine-slot`),
    /// before the engine's clock runs: at most one engine runs on the machine at a time.
    pub fn start(universes: Universes) -> io::Result<(Self, Sender)> {
        Self::start_with(universes, Box::new(outputs::open))
    }

    /// [`Engine::start`], with `open` making the devices `SetOutput` asks for.
    pub fn start_with(universes: Universes, open: Open) -> io::Result<(Self, Sender)> {
        Self::start_on(
            #[cfg(any(test, feature = "test-support"))]
            engine_slot::take()?,
            universes,
            open,
            Clock::wall(),
            Trace::none(),
            #[cfg(feature = "test-support")]
            None,
        )
    }

    /// [`Engine::start_with`] on a clock the test steps ([`Wakes`]), which also reads
    /// each due hand-over and what came of it. The clock starts running.
    #[cfg(feature = "test-support")]
    pub fn start_stepped(universes: Universes, open: Open) -> io::Result<(Self, Sender, Wakes)> {
        let slot = engine_slot::take()?;
        let (clock, trace, wakes) = wake::stepped();
        let (engine, sender) = Self::start_on(slot, universes, open, clock, trace, None)?;
        Ok((engine, sender, wakes))
    }

    /// Pauses off-tick retirement for a capacity test. Live output continues.
    #[cfg(feature = "test-support")]
    pub fn stall_reclamation(&self, stalled: bool) {
        self.shared.stalled.store(stalled, Ordering::Release);
    }

    /// Runs the wall-clock engine and real outputs, with a wait-free timing stream.
    #[cfg(feature = "test-support")]
    pub fn start_measured(universes: Universes) -> io::Result<(Self, Sender, Timings)> {
        let slot = engine_slot::take()?;
        let clock = Clock::wall();
        let (writer, reader) = timing::TimingWriter::new(clock.clone());
        let (engine, sender) = Self::start_on(
            slot,
            universes,
            Box::new(outputs::open),
            clock,
            Trace::none(),
            Some(writer),
        )?;
        Ok((engine, sender, reader))
    }

    fn start_on(
        #[cfg(any(test, feature = "test-support"))] slot: engine_slot::Slot,
        universes: Universes,
        open: Open,
        clock: Clock,
        trace: Trace,
        #[cfg(feature = "test-support")] timings: Option<timing::TimingWriter>,
    ) -> io::Result<(Self, Sender)> {
        let shared = Arc::new(Shared {
            life: Arc::new(EngineLife::new()),
            wake: AtomicU64::new(0),
            freed: AtomicU64::new(0),
            plan_bytes_freed: AtomicU64::new(0),
            devices_freed: AtomicU64::new(0),
            revision: AtomicU64::new(0),
            ended: [const { AtomicU32::new(0) }; PLAYS],
            kept: [const { AtomicU64::new(0) }; PLAYS],
            applied: AtomicU64::new(0),
            #[cfg(any(test, feature = "test-support"))]
            stalled: AtomicBool::new(false),
            #[cfg(all(test, feature = "test-support"))]
            presses_at_handoff: AtomicU64::new(0),
        });
        let (hand, queues) = publication::hand_over();
        let (commands, incoming) = rtrb::RingBuffer::new(QUEUE);
        let (outgoing, retired) = rtrb::RingBuffer::new(RETIRED);
        let (retry, requests) = std::sync::mpsc::channel();
        let retry_worker = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("reopen outputs".into())
                .spawn(move || recover(requests, &shared))?
        };
        let free = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("free devices".into())
                .spawn(move || free(retired, &shared))?
        };
        let ticker = Ticker::new(TickerStart {
            universes,
            shared: Arc::clone(&shared),
            commands: incoming,
            retired: outgoing,
            hand,
            clock: clock.clone(),
            trace,
            #[cfg(feature = "test-support")]
            timings,
        })?;
        let life = Arc::clone(&shared.life);
        let (tick, priority) = rt_thread::spawn("tick", move |priority| {
            let _unwinds = TickGuard(&life);
            ticker.run(priority)
        })?;
        let sender = Sender::new(
            universes,
            open,
            retry,
            commands,
            Arc::clone(&shared),
            free.thread().clone(),
            clock,
        );
        let engine = Self {
            shared,
            reader: Reader::new(queues, Snapshot::new(universes, priority)),
            tick: Some(tick),
            free: Some(free),
            retry: Some(retry_worker),
            universes,
            #[cfg(any(test, feature = "test-support"))]
            _slot: slot,
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
    /// two wakes. [`TickFailed`] once the tick thread is gone: no picture comes again.
    pub fn fresh(&mut self) -> Result<&Snapshot, TickFailed> {
        self.reader.fresh(&self.shared.life)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.life.apply(LifeEvent::Drop);
        if let Some(tick) = self.tick.take() {
            drop(tick.join()); // the devices are freed here, not on the tick thread
        }
        if let Some(retry) = self.retry.take() {
            drop(retry.join());
        }
        if let Some(free) = self.free.take() {
            free.thread().unpark();
            drop(free.join());
        }
    }
}
