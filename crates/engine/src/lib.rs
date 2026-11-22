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
    Refusal, Reply, Request, RouteFault, Run, RunNumbers, Span, State, Status, Target, Text,
    Universes, changes,
};
pub use engine_core::{
    AuthoredStep, Bpm, Brightness, GrabId, Light, ManualEnd, MixRules, OtherEnd, Palette, Pattern,
    PlayId, PlayState, Property, Rank, Repeat, Rgb, Saved, SavedLook, Sequence, SequenceItem,
    Source, Speed, Swatches, Use, Value, WaveTarget, When,
};
use engine_core::{
    EngineCommand, Histogram, LATENCY_BUCKETS, LATENESS_BUCKETS, MAX_LIGHTS, bucket, due, frame,
    percentile, update, wait,
};
pub use fixture_library::Fixture;
pub use looks::{Looks, PLAN_BYTES};
pub use rig::Patch;
mod grabs;
use grabs::Grabs;
mod snapshot;

use engine_core::{
    Channels, Claims, End, Entry, LightChange, LightRun, Lights, LookCommand, PLAYS, ProjectEffect,
    ProjectEvent, Writer, stops, unkept,
};
use looks::Plan;
use rig::{PatchEffect, Whole};

pub use outputs::Device;
mod cutover;
use cutover::{Effect as CutoverEffect, Event as CutoverEvent, State as CutoverState};
mod lane;
use lane::Lane;
mod life;
mod recovery;
pub use life::TickFailed;
use life::{EngineLife, LifeEvent, TickGuard};
pub use lane::Route;
#[cfg(feature = "test-support")]
pub use snapshot::timing::{Timing, Timings};
use snapshot::{Hand, Reader, Times};
pub use snapshot::{Lateness, Snapshot};
mod wake;
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};
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
/// A release of every other light makes the most disjoint runs possible.
/// Even that selection must fit in an empty FIFO, all in one wake.
const RUNS_IN_PLACE: usize = (MAX_LIGHTS as usize).div_ceil(2 * QUEUE);
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
        | Command::PutAuthoredStep { .. }
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
        | Command::StartUse { .. }
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
}

/// The engine front: owns the [`Project`], checks every client command against it,
/// sends the tick thread what follows and opens the outputs' devices, so the tick thread
/// never does. The only sender: each queue has one producer and one consumer. A command
/// checked against an older [`Base`] is refused `Stale` when admitted.
pub struct Sender {
    /// `None` while an [`Opening`] has it: one device opens at a time.
    open: Option<Opener>,
    /// An opening dropped unfinished sends its opener back here ([`Lease`]).
    abandon: Abandon,
    abandoned: Abandoned,
    lanes: [Option<lane::Control>; MAX_OUTPUTS],
    generations: [u64; MAX_OUTPUTS],
    previous_outputs: [Option<PriorOutput>; MAX_OUTPUTS],
    retry: Retry,
    #[cfg(feature = "test-support")]
    lane_pause: Option<(OutputNumber, lane::StartPause)>,
    commands: CommandQueue,
    project: Project,
    /// Explicit routes brought by attach_route. The v11 project carries only their bounds.
    routed: Arc<AttachedRoutes>,
    /// The fixtures, their lights and writers. Shared with readers as it is: a change
    /// makes a new one.
    patch: Arc<Patch>,
    /// The saved looks. Shared with readers as they are: a change makes new ones.
    looks: Arc<Looks>,
    /// The plays queued for the tick thread, by slot: the tick thread's plays once it
    /// applied every command sent.
    plays: [Option<Live>; PLAYS],
    /// Visits booked for expanded light commands and retiring plans until their FIFO prefix completes.
    prefix_work: std::collections::VecDeque<(u64, PrefixWork)>,
    /// Bytes of immutable plans sent, including versions still waiting for reclamation.
    plan_bytes: u64,
    /// The generation of the last play started.
    generation: u32,
    /// The live grabs: those that still hold a light.
    grabs: Grabs,
    /// The generation of the last grab.
    grabbed: u32,
    /// Boxes this sender sent back from the tick thread, so far. The free thread counts
    /// the ones it freed in `Shared::freed`.
    retired: u64,
    /// Of them, the devices: an opening waits for each (`Shared::devices_freed`).
    devices: u64,
    /// Commands queued for the tick thread, so far: [`Snapshot::applied`] counts the same.
    sent: u64,
    shared: Arc<Shared>,
    free: Thread,
    clock: Clock,
    /// Bounds for devices brought through the attachment door (tests only).
    #[cfg(any(test, feature = "test-support"))]
    universes: Universes,
}

enum PrefixWork {
    Plan(usize),
    Lights(usize),
}

/// Opens the device for an output kind, off the tick thread. The engine's own is
/// `outputs::open`; a test brings one that opens fake devices.
pub type Open = Box<dyn FnMut(OutputKind) -> io::Result<Box<dyn Device>> + Send>;

/// Only opening workers call the configured factory. A retry can share it with a
/// later SetOutput without holding the publisher, the tick, or an adapter's thread.
#[derive(Clone)]
#[expect(
    clippy::disallowed_types,
    reason = "only blocking device-open workers take this lock"
)]
struct Opener(Arc<std::sync::Mutex<Open>>);

impl Opener {
    fn device(&self, kind: OutputKind) -> io::Result<Box<dyn Device>> {
        let mut open = self.0.lock().unwrap_or_else(|e| e.into_inner());
        panic::catch_unwind(AssertUnwindSafe(|| open(kind)))
            .unwrap_or_else(|_| Err(io::Error::other("opening the output panicked")))
    }
}

/// A device to open for `SetOutput`: [`Opening::open`] blocks, so it runs where it may,
/// then [`Sender::finish`] puts the device on the tick thread.
#[must_use = "an opening holds the one opener: open it and finish it"]
pub struct Opening {
    retry: Retry,
    lease: Lease,
    output: OutputNumber,
    kind: OutputKind,
    universes: Run,
    rate: Rate,
    shared: Arc<Shared>,
    free: Thread,
    /// The devices sent away before this one opens: a port they hold must be free.
    devices: u64,
    route: Route,
    clock: Clock,
    #[cfg(feature = "test-support")]
    lane_pause: Option<lane::StartPause>,
    /// Can the installed lane be reused, or does this opening need a fresh device?
    source: DeviceSource,
}

/// An exclusive port moves at admission. A socket can also be opened afresh if
/// the previous attachment has not passed through the tick and reclaimer in time.
enum DeviceSource<Probe = Arc<AtomicBool>> {
    Fresh,
    Exclusive,
    Reusable(Probe),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenAction {
    Wait,
    Open,
    Reuse,
}

/// A socket can fall back to a fresh open at the limit. An exclusive device
/// is already reserved for this output and transfers at T8.
fn open_action(port: DeviceSource<bool>, reclaiming: bool, elapsed: Duration) -> OpenAction {
    match port {
        DeviceSource::Exclusive | DeviceSource::Reusable(true) => OpenAction::Reuse,
        DeviceSource::Fresh if !reclaiming => OpenAction::Open,
        DeviceSource::Fresh | DeviceSource::Reusable(false) if elapsed >= FREE_WITHIN => {
            OpenAction::Open
        }
        DeviceSource::Fresh | DeviceSource::Reusable(false) => OpenAction::Wait,
    }
}

/// What came of an [`Opening`]. It brings the `open` back.
#[must_use = "an opened output holds the one opener: finish it"]
pub struct Opened {
    /// The opener and, when the open succeeded, the new lane.
    lease: Lease,
    output: OutputNumber,
    kind: OutputKind,
    universes: Run,
    rate: Rate,
    opened: io::Result<()>,
    reuse: bool,
}

/// The one opener, lent to an output edit with what the edit holds. Finished by
/// [`Sender::publish_output`]. Dropped unfinished (the caller let go of an [`Opening`]
/// or an [`Opened`]), it goes back to the front, which undoes the edit like a failed
/// open (output-cutover T4): the opener back, the old lane serving again, the project
/// as before. A new lane it held is freed off the caller's thread.
struct Lease {
    open: Opener,
    output: OutputNumber,
    previous: Option<lane::Control>,
    lane: Option<Box<Lane>>,
    back: Abandon,
    finished: bool,
}

/// What an unfinished [`Lease`] brings back to the front.
struct Unfinished {
    open: Opener,
    output: OutputNumber,
    previous: Option<lane::Control>,
    lane: Option<Box<Lane>>,
}

#[expect(
    clippy::disallowed_types,
    reason = "only a dropped opening sends, off the tick, and only the front receives"
)]
type Abandon = std::sync::mpsc::Sender<Unfinished>;
#[expect(
    clippy::disallowed_types,
    reason = "only a dropped opening sends, off the tick, and only the front receives"
)]
type Abandoned = std::sync::mpsc::Receiver<Unfinished>;

impl Lease {
    /// The opener and the new lane, for publication.
    fn finish(mut self) -> (Opener, Option<Box<Lane>>) {
        self.finished = true;
        (self.open.clone(), self.lane.take())
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.back.send(Unfinished {
                open: self.open.clone(),
                output: self.output,
                previous: self.previous.take(),
                lane: self.lane.take(),
            });
        }
    }
}

/// Completes the transfer of an accepted output, outside the publisher lock.
/// Dropping the receipt also completes the handoff if the caller is cancelled.
pub struct Handoff {
    /// The engine's outputs stopped before the switch was published (T13): nothing to
    /// complete, the writer gets `Stopping`.
    stopped: bool,
    previous: Option<lane::Control>,
    next: Option<Recovery>,
    retry: Retry,
    reuse: bool,
    shared: Arc<Shared>,
    unfinished_wake: Option<u64>,
}

impl Handoff {
    /// T5 waits only for a cutoff already in progress. T8 completes on this thread.
    /// Returns false if stopping cancelled the switch.
    pub fn wait(mut self) -> bool {
        self.complete()
    }

    fn complete(&mut self) -> bool {
        let (Some(previous), Some(next)) = (self.previous.take(), self.next.take()) else {
            return !self.stopped;
        };
        self.shared.finish_wake(self.unfinished_wake);
        previous.publish();
        match previous.finish_handoff(&next.lane, self.reuse) {
            Some(watch) => {
                if watch && let Err(request) = self.retry.send(next) {
                    request.0.lane.worker_gone();
                }
                true
            }
            None => false,
        }
    }
}

impl Drop for Handoff {
    fn drop(&mut self) {
        self.complete();
    }
}

/// A command, when it was sent, and what it brings.
struct Sent {
    command: EngineCommand,
    at: Duration,
    brings: Brings,
}

/// A replacement reserves one command and one retirement so its prepared lane can
/// always be committed. Other commands, including multi-command edits, share the rest.
struct CommandQueue {
    fifo: rtrb::Producer<Sent>,
    reserved: bool,
}

impl CommandQueue {
    fn slots(&self) -> usize {
        self.fifo.slots().saturating_sub(usize::from(self.reserved))
    }

    fn write_chunk_uninit(
        &mut self,
        n: usize,
    ) -> Result<rtrb::chunks::WriteChunkUninit<'_, Sent>, Refusal> {
        if n > self.slots() {
            return Err(Refusal::QueueFull);
        }
        self.fifo
            .write_chunk_uninit(n)
            .map_err(|_| Refusal::QueueFull)
    }
}

/// What a command brings the tick thread, made before it is sent.
enum Brings {
    Nothing,
    /// An opened device on its own thread, for `Attach`.
    Device(Box<Lane>),
    /// An accepted route change to the same adapter, in its new lane.
    Reroute(Box<Lane>, bool),
    /// Tell the opener when prior attachments passed the tick and reclaimer.
    Probe(Arc<AtomicBool>),
    /// The writers of the patch, for `Writers`.
    Writers(Vec<Writer>),
    /// Some runs of a group in place, for `Runs`: nothing goes back.
    Runs([Option<LightRun>; RUNS_IN_PLACE]),
    /// The plan of a play, for `Start` and `Plan`.
    Plan(Version),
}

/// What goes back from the tick thread, to be freed on the free thread.
#[expect(
    dead_code,
    reason = "held only to be dropped on the free thread, never read"
)]
enum Retired {
    Probe(Arc<AtomicBool>),
    Device(Box<Lane>),
    Writers(Vec<Writer>),
    Plan(Arc<Plan>),
}

/// Why a command was not queued: refused, or the engine's outputs stopped (T13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsent {
    Refused(Refusal),
    /// The outputs stopped ([`Sender::stop_outputs`]): an output switch is not published.
    Stopping,
}

impl From<Refusal> for Unsent {
    fn from(reason: Refusal) -> Self {
        Self::Refused(reason)
    }
}

/// What came of [`Sender::begin`].
pub enum Begun {
    /// Queued for the very next wake.
    Queued,
    /// Patched as `fixture`, its writers queued for the very next wake.
    Patched { fixture: u16 },
    /// The look plays as `play` from the very next wake.
    Started { play: PlayId },
    /// The grab holds from the very next wake.
    Grabbed { grab: GrabId },
    /// A device must be opened first: [`Opening::open`], then [`Sender::finish`].
    Opening(Opening),
}

/// The project, the patch and the looks as the front had them: what a command is
/// prepared against, without the front ([`Sender::base`]).
///
/// ```text
/// under the lock: base  -->  off it: prepare (check, compile, measure the file)  -->  under the lock: admit
///                                     another client's press is admitted meanwhile      the base still the front's, or Stale
/// ```
#[derive(Clone)]
pub struct Base {
    reservations: [Option<Live>; PLAYS],
    project: Project,
    pending: [Option<PriorOutput>; MAX_OUTPUTS],
    routed: Arc<AttachedRoutes>,
    patch: Arc<Patch>,
    looks: Arc<Looks>,
}

/// Only explicit attachments need a span list here. A v11 run supplies its own span.
#[derive(Clone, Default)]
struct AttachedRoutes([Option<Arc<[Span]>>; MAX_OUTPUTS]);

impl AttachedRoutes {
    /// Refuse a route edit that moves or discards a byte belonging to a patched fixture.
    fn check_edit(
        &self,
        patch: &Patch,
        project: &Project,
        output: OutputNumber,
        after: &[Span],
    ) -> Result<(), Refusal> {
        let run;
        let before = match project.output(output) {
            Some(ProjectOutput::Kind { kind, universes }) => {
                run = universes.span(outputs::limits(kind).whole);
                std::slice::from_ref(&run)
            }
            Some(ProjectOutput::Attached { universes }) => {
                run = universes.span(outputs::Limits::RAW.whole);
                self.0
                    .get(output.slot())
                    .and_then(Option::as_deref)
                    .unwrap_or(std::slice::from_ref(&run))
            }
            None => return Ok(()),
        };
        if before == after {
            return Ok(());
        }
        let mut patched: Vec<_> = patch
            .fixtures()
            .into_iter()
            .map(|fixture| {
                let start = u32::from(fixture.universe - 1) * ADDRESSES as u32
                    + u32::from(fixture.address - 1);
                (start..start + u32::from(fixture.channels), fixture.fixture)
            })
            .collect();
        // Patched fixtures do not overlap, so their ends are also in address order.
        patched.sort_unstable_by_key(|(range, _)| range.start);
        let mut next = after.iter().copied().peekable();
        for mut old in before.iter().copied() {
            while old.len > 0 {
                // Walk the two streams in step, stopping at each span boundary.
                let len = next.peek().map_or(old.len, |new| old.len.min(new.len));
                if next.peek().is_none_or(|new| old.start != new.start) {
                    let first = patched.partition_point(|(range, _)| range.end <= old.start);
                    if let Some((range, fixture)) = patched.get(first)
                        && range.start < old.start + len
                    {
                        return Err(Refusal::StreamsFixture { fixture: *fixture });
                    }
                }
                old.start += len;
                old.len -= len;
                if let Some(new) = next.peek_mut() {
                    new.start += len;
                    new.len -= len;
                    if new.len == 0 {
                        next.next();
                    }
                }
            }
        }
        Ok(())
    }

    /// A patch added during an open must fit both the current and requested routes.
    fn check_patch(&self, patch: &Patch, project: &Project) -> Result<(), Refusal> {
        if let Some((_, reason)) = patch.unstreamed(project) {
            return Err(reason);
        }
        for (number, kind, run) in project.sends() {
            if let (Ok(output), Some(kind)) = (OutputNumber::try_from(number), kind) {
                self.check_edit(
                    patch,
                    project,
                    output,
                    &[run.span(outputs::limits(kind).whole)],
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct PriorOutput {
    number: OutputNumber,
    generation: u64,
    descriptor: ProjectOutput,
    explicit: bool,
}

impl PriorOutput {
    /// Restore both the visible descriptor and whether its bounds guarantee a stream.
    fn restore(self, mut project: Project) -> Project {
        let output = self.number;
        let universes = match self.descriptor {
            ProjectOutput::Attached { universes } | ProjectOutput::Kind { universes, .. } => {
                universes
            }
        };
        let event = if self.explicit {
            ProjectEvent::Routed {
                output,
                bounds: universes,
            }
        } else {
            ProjectEvent::Attached { output, universes }
        };
        if let Ok((cleared, _)) = project.update(ProjectEvent::OpenFailed { output })
            && let Ok((restored, _)) = cleared.update(event)
        {
            project = restored;
            if let Some(slot) = project.outputs.get_mut(output.slot()) {
                *slot = Some(self.descriptor);
            }
        }
        project
    }
}

/// An immutable project revision. A published cutover becomes visible at the
/// same output generation in both the project and the sampled engine state.
#[derive(Clone, Copy)]
pub struct ProjectView {
    project: Project,
    previous: [Option<PriorOutput>; MAX_OUTPUTS],
}

impl ProjectView {
    /// Whether publishing these two revisions changes an output descriptor.
    pub fn same_outputs(&self, other: &Self) -> bool {
        self.project.outputs == other.project.outputs
    }

    pub fn at(&self, snapshot: &Snapshot) -> Project {
        let mut project = self.project;
        for (slot, prior) in self.previous.iter().enumerate() {
            if let Some(old) = prior
                && snapshot.generation(slot) == old.generation
            {
                project = old.restore(project);
            }
        }
        project
    }
}

/// A command checked and compiled against its [`Base`], for [`Sender::admit`].
pub struct Prepared {
    base: Base,
    /// The project once admitted.
    project: Project,
    change: Change,
}

/// What a prepared command changes, made before it is admitted.
enum Change {
    Nothing,
    Send(EngineCommand),
    Open {
        output: OutputNumber,
        kind: OutputKind,
        universes: Run,
        rate: Rate,
    },
    /// The new patch, `None` when it stays, and the saved looks whose lights it changes.
    Patch {
        patch: Option<Patch>,
        effects: Vec<PatchEffect>,
        rebound: Option<Looks>,
        invalid: Vec<Arc<Plan>>,
    },
    PutLook {
        looks: Looks,
        /// The edited step if it would lose its last swatch.
        emptied_palette: Option<u16>,
    },
    RemoveLook {
        look: u16,
        looks: Looks,
    },
    StartLook {
        look: u16,
        rank: Rank,
        when: When,
        plan: Arc<Plan>,
        usage: Use,
    },
    StopLook {
        play: PlayId,
    },
    EndFlash {
        play: PlayId,
    },
    ReleaseHold {
        play: PlayId,
    },
    ReleaseAll,
    Rules {
        looks: Looks,
    },
    ReleaseProperty {
        property: Property,
        runs: Vec<LightRun>,
    },
    UpdateGrab {
        grab: GrabId,
        value: Value,
    },
    EndGrab {
        grab: GrabId,
    },
}

impl Prepared {
    /// Plan bytes that snapshot history will pin if this edit is admitted.
    /// Measured outside the sender lock, against the same base admission checks.
    pub fn replaced_plan_bytes(&self) -> usize {
        if let Change::PutLook { looks, .. }
        | Change::RemoveLook { looks, .. }
        | Change::Patch {
            rebound: Some(looks),
            ..
        } = &self.change
        {
            self.base.looks.replaced_bytes(looks)
        } else {
            0
        }
    }
}

/// Keep the old stream in the patch checks until T9, including overlap with
/// independent switches on other outputs. Project already checks both sides of
/// a pending SetOutput. Explicit routes cannot guarantee a cross-universe stream.
fn check_pending_patch(
    patch: &Patch,
    mut project: Project,
    pending: &[Option<PriorOutput>; MAX_OUTPUTS],
    routed: &AttachedRoutes,
) -> Result<(), Refusal> {
    routed.check_patch(patch, &project)?;
    for (slot, old) in pending.iter().enumerate() {
        let Some(old) = old else { continue };
        let output = old.number;
        let next = project.output(output);
        project = old.restore(project);
        match next {
            Some(ProjectOutput::Kind { kind, universes }) => {
                project = project
                    .update(ProjectEvent::Client(Command::SetOutput {
                        output,
                        kind,
                        universes: RunNumbers {
                            first: universes.first(),
                            count: universes.count(),
                        },
                        rate: None,
                    }))?
                    .0;
            }
            Some(ProjectOutput::Attached { .. }) | None => {
                if routed.0.get(slot).is_some_and(Option::is_some) {
                    project = PriorOutput {
                        explicit: true,
                        ..*old
                    }
                    .restore(project);
                }
            }
        }
    }
    if pending.iter().any(Option::is_some) {
        routed.check_patch(patch, &project)?;
    }
    Ok(())
}

impl Base {
    /// Checks `command` against this base and makes what it changes: a calculation,
    /// which may take long (a patch measures its show file, a look compiles). `library`
    /// is where `Patch` looks up its fixture. Nothing is queued until it is admitted.
    pub fn prepare(&self, command: Command, library: &[Fixture]) -> Result<Prepared, Refusal> {
        let (project, effect) = self.project.update(ProjectEvent::Client(command))?;
        // Check an output edit against the run it will actually install. The
        // pending project stays conservative for other clients' patch edits.
        let checked = match effect {
            Some(
                ProjectEffect::Open {
                    output,
                    kind,
                    universes,
                    ..
                }
                | ProjectEffect::Restart {
                    output,
                    kind,
                    universes,
                    ..
                }
                | ProjectEffect::Keep {
                    output,
                    kind,
                    universes,
                    ..
                },
            ) => {
                let limits = outputs::limits(kind);
                limits.check(universes)?;
                self.routed.check_edit(
                    &self.patch,
                    &self.project,
                    output,
                    &[universes.span(limits.whole)],
                )?;
                Some(project.update(ProjectEvent::Opened { output, kind })?.0)
            }
            Some(ProjectEffect::Send(EngineCommand::RemoveOutput { .. })) => Some(project),
            Some(ProjectEffect::Send(_) | ProjectEffect::Look(_) | ProjectEffect::Patch(_))
            | None => None,
        };
        if let Some((fixture, _)) = checked.and_then(|p| self.patch.unstreamed(&p)) {
            return Err(Refusal::StreamsFixture { fixture });
        }
        let change = match effect {
            None => Change::Nothing,
            Some(ProjectEffect::Send(command)) => Change::Send(command),
            Some(ProjectEffect::Look(command)) => self.look(command)?,
            Some(ProjectEffect::Patch(engine_core::PatchCommand::Release { target, property })) => {
                Change::ReleaseProperty {
                    property,
                    runs: self.patch.runs(target)?,
                }
            }
            Some(ProjectEffect::Patch(command)) => {
                let (patch, effects) =
                    self.patch
                        .update_with_looks(command, library, self.looks.rows())?;
                // and a change of the patch that would make one is refused
                if let Some(patch) = &patch {
                    check_pending_patch(patch, project, &self.pending, &self.routed)?;
                }
                // as is one that would leave a saved look naming nothing
                let rebound = patch
                    .as_ref()
                    .map(|new| self.looks.rebind(new))
                    .transpose()?;
                let mut invalid = Vec::new();
                if let Some(new) = &patch {
                    // A press can start or stop any saved root while this prepare runs.
                    // Check its bindings, and those of earlier versions a stop could freeze.
                    for saved in self.looks.all() {
                        let plan = self.looks.plan(saved.look)?;
                        if !plan.bindings_match(new) {
                            invalid.push(plan);
                        }
                    }
                    for live in self.reservations.iter().flatten() {
                        for version in live.versions() {
                            if !version.plan.bindings_match(new) {
                                invalid.push(Arc::clone(&version.plan));
                            }
                        }
                    }
                }
                Change::Patch {
                    patch,
                    effects,
                    rebound,
                    invalid,
                }
            }
            Some(ProjectEffect::Keep {
                output,
                kind,
                universes,
                rate,
            }) => Change::Send(EngineCommand::Attach {
                output,
                universes,
                rate: rate.unwrap_or(outputs::default_rate(kind)),
            }),
            Some(
                ProjectEffect::Open {
                    output,
                    kind,
                    universes,
                    rate,
                }
                | ProjectEffect::Restart {
                    output,
                    kind,
                    universes,
                    rate,
                },
            ) => Change::Open {
                output,
                kind,
                universes,
                rate: rate.unwrap_or(outputs::default_rate(kind)),
            },
        };
        Ok(Prepared {
            base: self.clone(),
            project,
            change,
        })
    }

    fn look(&self, command: LookCommand) -> Result<Change, Refusal> {
        Ok(match command {
            LookCommand::Put(saved) => {
                let emptied_palette = (!saved.has_swatches()
                    && self
                        .looks
                        .get(saved.look)
                        .is_some_and(SavedLook::has_swatches))
                .then_some(saved.look);
                let looks = self.looks.put(saved, &self.patch)?;
                self.patch.fits_looks(looks.rows())?;
                Change::PutLook {
                    looks,
                    emptied_palette,
                }
            }
            LookCommand::RemoveLook { look } => Change::RemoveLook {
                look,
                looks: self.looks.remove(look)?,
            },
            LookCommand::StartLook {
                look,
                rank,
                when,
                usage,
            } => {
                let plan = self.looks.plan(look)?;
                if usage.spread > 100 || plan.is_step() && usage.repeat == Repeat::Once {
                    return Err(Refusal::InvalidTiming);
                }
                Change::StartLook {
                    look,
                    rank,
                    when,
                    usage,
                    plan,
                }
            }
            LookCommand::StopLook { play } => Change::StopLook { play },
            LookCommand::EndFlash { play } => Change::EndFlash { play },
            LookCommand::ReleaseHold { play } => Change::ReleaseHold { play },
            LookCommand::ReleaseAll => Change::ReleaseAll,
            LookCommand::UpdateGrab { grab, value } => Change::UpdateGrab { grab, value },
            LookCommand::EndGrab { grab } => Change::EndGrab { grab },
            LookCommand::SetMixRules { rules } => {
                self.patch.fits_looks(self.looks.rows())?;
                Change::Rules {
                    looks: self.looks.with_rules(rules),
                }
            }
        })
    }
}

/// The command number identifies the precise plan that supplied a frozen sample.
#[derive(Clone)]
struct Version {
    play: PlayId,
    sent: u64,
    plan: Arc<Plan>,
    usage: Use,
}

/// What the front reserves for a play: its own fact, never the tick's play state.
/// Table in `docs/engine.md` ("What the front reserves for a play").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hold {
    Playing,
    Kept,
}

#[derive(Clone, Copy, Debug)]
enum HoldEvent {
    Start,
    KeepSent,
    KeptRead,
    Released,
}

/// The transition function, a calculation. `None` is a free slot.
const fn next_hold(hold: Option<Hold>, event: HoldEvent) -> Option<Hold> {
    match (hold, event) {
        (_, HoldEvent::Start) => Some(Hold::Playing),
        (Some(Hold::Playing | Hold::Kept), HoldEvent::KeepSent | HoldEvent::KeptRead) => {
            Some(Hold::Kept)
        }
        (None, HoldEvent::KeepSent | HoldEvent::KeptRead) | (_, HoldEvent::Released) => None,
    }
}

impl Hold {
    /// The play state the core's rules ask about (`unkept`).
    const fn state(self) -> PlayState {
        match self {
            Self::Playing => PlayState::Playing,
            Self::Kept => PlayState::Kept,
        }
    }
}

/// Applies `event` to the play in `slot`. A hold that becomes free frees the slot and
/// returns the play it held.
fn apply_hold(slot: &mut Option<Live>, event: HoldEvent) -> Option<Live> {
    match next_hold(slot.as_ref().map(|live| live.hold), event) {
        Some(hold) => {
            if let Some(live) = slot {
                live.hold = hold;
            }
            None
        }
        None => slot.take(),
    }
}

#[derive(Clone)]
struct Live {
    play: PlayId,
    look: u16,
    rank: Rank,
    hold: Hold,
    current: Version,
    /// A once-use can finish just before a pending plan swap. The tick publishes
    /// the version it kept before acknowledging that FIFO prefix.
    earlier: Option<Arc<Vec<Version>>>,
}

impl Live {
    fn versions(&self) -> impl Iterator<Item = &Version> {
        core::iter::once(&self.current).chain(self.earlier.iter().flat_map(|v| v.iter()))
    }

    fn work(&self) -> usize {
        self.versions().map(|v| v.plan.work()).max().unwrap_or(0)
    }

    /// A once-use may finish at a FIFO event before the frame installs its replacement.
    fn work_after_edit(&self, plan: &Plan) -> usize {
        match self.current.usage.repeat {
            Repeat::Once => self.work().max(plan.work()),
            Repeat::Loop => plan.work(),
        }
    }

    /// Once released, only a possible natural end before the release can still sample.
    /// An explicit stop books its terminal sample when the Keep command is queued.
    fn work_before_release(&self) -> usize {
        if self.hold == Hold::Playing && self.current.usage.repeat == Repeat::Once {
            self.work()
        } else {
            0
        }
    }

    fn wake_work(&self) -> usize {
        match self.hold {
            Hold::Kept => self.work(),
            Hold::Playing => looks::play_work(self.work()),
        }
    }
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
    pub fn start(universes: Universes) -> io::Result<(Self, Sender)> {
        Self::start_with(universes, Box::new(outputs::open))
    }

    /// [`Engine::start`], with `open` making the devices `SetOutput` asks for.
    pub fn start_with(universes: Universes, open: Open) -> io::Result<(Self, Sender)> {
        Self::start_on(
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
        let (clock, trace, wakes) = wake::stepped();
        let (engine, sender) = Self::start_on(universes, open, clock, trace, None)?;
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
        let clock = Clock::wall();
        let (writer, reader) = snapshot::timing::TimingWriter::new(clock.clone());
        let (engine, sender) = Self::start_on(
            universes,
            Box::new(outputs::open),
            clock,
            Trace::none(),
            Some(writer),
        )?;
        Ok((engine, sender, reader))
    }

    fn start_on(
        universes: Universes,
        open: Open,
        clock: Clock,
        trace: Trace,
        #[cfg(feature = "test-support")] timings: Option<snapshot::timing::TimingWriter>,
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
        let (hand, queues) = snapshot::hand_over();
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
        let ticker = Ticker {
            shared: Arc::clone(&shared),
            commands: incoming,
            retired: outgoing,
            devices: std::array::from_fn(|_| None),
            switching: Box::new(std::array::from_fn(|_| None)),
            writers: Vec::new(),
            plans: std::array::from_fn(|_| None),
            scratch: {
                let first = engine_core::LightNumber::new(0, LIGHTS)
                    .and_then(|first| LightRun::new(first, 1, LIGHTS))
                    .ok_or_else(|| io::Error::other("empty lighting engine"))?;
                std::array::from_fn(|_| {
                    vec![
                        Entry {
                            lights: first,
                            values: Light::DARK
                        };
                        usize::from(MAX_LIGHTS)
                    ]
                    .into_boxed_slice()
                })
            },
            claims: vec![[None; engine_core::Property::ALL.len()]; usize::from(MAX_LIGHTS)]
                .into_boxed_slice(),
            // `run` puts in the priority, once the thread has it.
            now: Box::new(Snapshot::new(universes, Priority::RealTime)),
            hand,
            channels: channels(),
            carried: Box::new(Carried::new()),
            staged: Box::new([None; (lane::BOXES + 1) * MAX_OUTPUTS]),
            clock: clock.clone(),
            trace,
            #[cfg(feature = "test-support")]
            timings,
        };
        let life = Arc::clone(&shared.life);
        let (tick, priority) = rt_thread::spawn("tick", move |priority| {
            let _unwinds = TickGuard(&life);
            ticker.run(priority)
        })?;
        let (abandon, abandoned) = std::sync::mpsc::channel();
        let sender = Sender {
            open: Some(Opener(Arc::new(open.into()))),
            abandon,
            abandoned,
            lanes: std::array::from_fn(|_| None),
            generations: [0; MAX_OUTPUTS],
            previous_outputs: [None; MAX_OUTPUTS],
            retry,
            #[cfg(feature = "test-support")]
            lane_pause: None,
            commands: CommandQueue {
                fifo: commands,
                reserved: false,
            },
            project: Project::new(universes),
            routed: Arc::default(),
            patch: Arc::new(Patch::new(
                universes,
                LIGHTS,
                Whole(|kind| outputs::limits(kind).whole),
            )),
            looks: Arc::new(Looks::new()),
            plays: std::array::from_fn(|_| None),
            prefix_work: Default::default(),
            plan_bytes: 0,
            generation: 0,
            grabs: Grabs::new(),
            grabbed: 0,
            retired: 0,
            devices: 0,
            sent: 0,
            shared: Arc::clone(&shared),
            free: free.thread().clone(),
            clock,
            #[cfg(any(test, feature = "test-support"))]
            universes,
        };
        let engine = Self {
            shared,
            reader: Reader::new(queues, Snapshot::new(universes, priority)),
            tick: Some(tick),
            free: Some(free),
            retry: Some(retry_worker),
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
    /// two wakes. [`TickFailed`] once the tick thread is gone: no picture comes again.
    pub fn fresh(&mut self) -> Result<&Snapshot, TickFailed> {
        self.reader.fresh(&self.shared.life)
    }
}

impl Sender {
    /// An output still transferring its device cannot begin another output edit.
    fn check_handoff(&self, output: OutputNumber) -> Result<(), Refusal> {
        if self
            .lanes
            .get(output.slot())
            .and_then(Option::as_ref)
            .is_some_and(lane::Control::waits_to_serve)
        {
            return Err(Refusal::Opening);
        }
        Ok(())
    }

    /// Queues a command for the very next wake, or says why not. Nothing changes on a
    /// refusal. `SetOutput` opens its device here and waits for it: a caller that must
    /// not wait uses [`Sender::begin`], [`Opening::open`] and [`Sender::finish`]. With no
    /// library: `Patch` is refused (`NoSuchFixture`), use [`Sender::begin`] for it.
    pub fn send(&mut self, command: Command) -> Result<(), Unsent> {
        match self.begin(command, &[])? {
            Begun::Queued
            | Begun::Patched { .. }
            | Begun::Started { .. }
            | Begun::Grabbed { .. } => Ok(()),
            Begun::Opening(opening) => self.finish(opening.open()),
        }
    }

    /// Checks `command` against the project and sends what follows. `library` is where
    /// `Patch` looks up its fixture. [`Begun::Opening`] when a device must be opened
    /// first: until [`Sender::finish`] the output is `Opening`. Prepares and admits in
    /// one step: a caller that shares the front behind a lock prepares without it
    /// ([`Sender::base`], [`Base::prepare`], [`Sender::admit`]).
    pub fn begin(&mut self, command: Command, library: &[Fixture]) -> Result<Begun, Refusal> {
        self.take_back_abandoned();
        let prepared = self.base().prepare(command, library)?;
        self.admit(prepared)
    }

    /// What a command is checked against now: the project, the patch and the looks.
    /// Immutable handles pin the definitions and the versions a live play can keep.
    pub fn base(&self) -> Base {
        Base {
            reservations: self.plays.clone(),
            project: self.project,
            pending: self.pending_outputs(),
            routed: Arc::clone(&self.routed),
            patch: Arc::clone(&self.patch),
            looks: Arc::clone(&self.looks),
        }
    }

    /// Queues what `prepared` does, all of it or none of it. `Stale` when another change
    /// replaced its base since: nothing changes, prepare it again. Checks here only what
    /// a play or the queue decides: which plays a change touches, a play slot, room.
    pub fn admit(&mut self, prepared: Prepared) -> Result<Begun, Refusal> {
        let Prepared {
            base,
            project,
            change,
        } = prepared;
        // An abandoned opening undoes its edit: a base from before that is stale.
        self.take_back_abandoned();
        if !(base.project == self.project
            && Arc::ptr_eq(&base.routed, &self.routed)
            && Arc::ptr_eq(&base.patch, &self.patch)
            && Arc::ptr_eq(&base.looks, &self.looks))
        {
            return Err(Refusal::Stale);
        }
        drop(base);
        self.reap();
        let begun = match change {
            Change::Nothing => Begun::Queued,
            Change::Send(command) => {
                if let EngineCommand::Attach { output, .. }
                | EngineCommand::RemoveOutput { output } = command
                {
                    self.check_handoff(output)?;
                }
                self.push(command, Brings::Nothing)?;
                if let EngineCommand::RemoveOutput { output } = command {
                    self.set_attached_route(output, None);
                }
                Begun::Queued
            }
            Change::Open {
                output,
                kind,
                universes,
                rate,
            } => {
                // A receipt may be accepted while its visible switch still awaits
                // T9. The opener waits off the publisher before starting that lane.
                if self
                    .lanes
                    .get(output.slot())
                    .and_then(Option::as_ref)
                    .is_none_or(|old| old.cutover().state() != CutoverState::Returned)
                {
                    self.check_handoff(output)?;
                }
                if self.open.is_none() {
                    return Err(Refusal::Opening);
                }
                let route = Route::of_run(universes, outputs::limits(kind))
                    .map_err(|_| Refusal::CannotOpen)?;
                let source = if project.restarting(output) {
                    if outputs::exclusive(kind) {
                        DeviceSource::Exclusive
                    } else {
                        self.room(2, 2)?;
                        let ready = Arc::new(AtomicBool::new(false));
                        self.push(
                            EngineCommand::Attach {
                                output,
                                universes,
                                rate,
                            },
                            Brings::Probe(Arc::clone(&ready)),
                        )?;
                        DeviceSource::Reusable(ready)
                    }
                } else {
                    DeviceSource::Fresh
                };
                if self.project.output(output).is_some() {
                    self.room(1, 1)?;
                    self.commands.reserved = true;
                }
                let previous = self.lanes.get(output.slot()).cloned().flatten();
                match previous.as_ref().map(|old| old.cutover().apply(CutoverEvent::Begin)) {
                    // T1, or a new output; T12 after T8: the opener begins after T9
                    None | Some(CutoverEffect::Open | CutoverEffect::Wait) => {}
                    // stopped (T13): no switch begins
                    Some(_) => {
                        self.commands.reserved = false;
                        return Err(Refusal::Opening);
                    }
                }
                Begun::Opening(Opening {
                    lease: Lease {
                        open: self.open.take().ok_or(Refusal::Opening)?,
                        output,
                        previous,
                        lane: None,
                        back: self.abandon.clone(),
                        finished: false,
                    },
                    retry: self.retry.clone(),
                    #[cfg(feature = "test-support")]
                    lane_pause: self
                        .lane_pause
                        .take_if(|(at, _)| *at == output)
                        .map(|(_, pause)| pause),
                    output,
                    kind,
                    universes,
                    rate,
                    shared: Arc::clone(&self.shared),
                    free: self.free.clone(),
                    devices: self.devices,
                    clock: self.clock.clone(),
                    route,
                    source,
                })
            }
            Change::Patch {
                patch,
                effects,
                rebound,
                invalid,
            } => {
                if let Some(live) = self.plays.iter().flatten().find(|live| {
                    live.versions()
                        .any(|v| invalid.iter().any(|plan| Arc::ptr_eq(plan, &v.plan)))
                }) {
                    return Err(Refusal::LookInUse { look: live.look });
                }
                let begun = self.publish(effects, rebound.as_ref())?;
                if let Some(patch) = patch {
                    self.patch = Arc::new(patch);
                }
                if let Some(looks) = rebound {
                    self.looks = Arc::new(looks);
                }
                begun
            }
            Change::PutLook {
                looks,
                emptied_palette,
            } => {
                // A once-use can freeze an older plan before the queued replacement plays.
                if emptied_palette.is_some_and(|look| {
                    self.plays
                        .iter()
                        .flatten()
                        .flat_map(Live::versions)
                        .any(|v| v.plan.uses_palette(look))
                }) {
                    return Err(Refusal::EmptyPalette);
                }
                self.publish(Vec::new(), Some(&looks))?;
                self.looks = Arc::new(looks);
                Begun::Queued
            }
            Change::RemoveLook { look, looks } => {
                if let Some(live) = self
                    .plays
                    .iter()
                    .flatten()
                    .find(|p| p.versions().any(|v| v.plan.uses(look)))
                {
                    return Err(Refusal::LookInUse { look: live.look });
                }
                self.looks = Arc::new(looks);
                Begun::Queued
            }
            Change::StartLook {
                look,
                rank,
                when,
                plan,
                usage,
            } => {
                // a fresh start of the look at this rank, playing or kept, takes its slot
                let slot = self
                    .plays
                    .iter()
                    .position(|live| {
                        live.as_ref()
                            .is_some_and(|live| (live.look, live.rank) == (look, rank))
                    })
                    .or_else(|| self.plays.iter().position(Option::is_none))
                    .ok_or(Refusal::PlayFull)?;
                // no play is generation zero: each slot's `Shared::ended` starts there
                let generation = self.generation.wrapping_add(1).max(1);
                let play = PlayId::new(slot, generation).ok_or(Refusal::PlayFull)?;
                self.check_work(self.playing_work() + looks::play_work(plan.work()), 0)?;
                let bytes = plan.bytes() as u64;
                self.check_plan_bytes(bytes)?;
                let version = Version {
                    play,
                    sent: self.sent + 1,
                    plan,
                    usage,
                };
                self.push(
                    EngineCommand::Start {
                        play,
                        look,
                        rank,
                        when,
                    },
                    Brings::Plan(version.clone()),
                )?;
                self.plan_bytes += bytes;
                self.generation = generation;
                if let Some(live) = self.plays.get_mut(slot) {
                    if let Some(old) = live.as_ref() {
                        self.prefix_work
                            .push_back((self.sent, PrefixWork::Plan(old.wake_work())));
                    }
                    if let Some(hold) =
                        next_hold(live.as_ref().map(|l| l.hold), HoldEvent::Start)
                    {
                        *live = Some(Live {
                            play,
                            look,
                            rank,
                            hold,
                            current: version,
                            earlier: None,
                        });
                    }
                }
                Begun::Started { play }
            }
            Change::StopLook { play } => {
                match self.live(play) {
                    Some(live) if live.hold == Hold::Playing => {
                        match stops(live.rank, self.looks.rules()) {
                            End::Keep => {
                                self.push(EngineCommand::Keep { play }, Brings::Nothing)?;
                                if let Some(slot) = self.plays.get_mut(play.slot()) {
                                    if let Some(live) = slot {
                                        self.prefix_work
                                            .push_back((self.sent, PrefixWork::Plan(live.work())));
                                    }
                                    apply_hold(slot, HoldEvent::KeepSent);
                                }
                            }
                            End::Remove => self.release(play)?,
                        }
                    }
                    Some(_) | None => {}
                }
                Begun::Queued
            }
            Change::EndFlash { play } => {
                if self
                    .live(play)
                    .is_some_and(|live| live.rank == Rank::Manual)
                {
                    self.release(play)?;
                }
                Begun::Queued
            }
            Change::ReleaseHold { play } => {
                if self.live(play).is_some() {
                    self.release(play)?;
                }
                Begun::Queued
            }
            Change::ReleaseAll => {
                self.push(EngineCommand::ReleaseAll, Brings::Nothing)?;
                self.forget(|live| live.rank == Rank::Manual);
                self.grabs.clear();
                Begun::Queued
            }
            Change::ReleaseProperty { property, runs } => {
                let held = self.grabs.releasing(property, &runs);
                self.lights(LightChange::Release { property }, &held)?;
                self.grabs.let_go(Some(property), &held);
                Begun::Queued
            }
            Change::UpdateGrab { grab, value } => {
                if let Some(property) = self.grabs.property(grab) {
                    if property != value.property() {
                        return Err(Refusal::OtherProperty { property });
                    }
                    // only the lights it still holds take the value
                    let held = self.grabs.held(grab);
                    self.patch.supports(&held, value)?;
                    self.lights(LightChange::Update { value, grab }, &held)?;
                }
                Begun::Queued
            }
            Change::EndGrab { grab } => {
                if let Some(property) = self.grabs.property(grab) {
                    let end = stops(Rank::Manual, self.looks.rules());
                    let held = self.grabs.held(grab);
                    self.lights(
                        LightChange::End {
                            property,
                            grab,
                            end,
                        },
                        &held,
                    )?;
                    self.grabs.end(grab, property, &held, end);
                }
                Begun::Queued
            }
            Change::Rules { looks } => {
                let rules = looks.rules();
                self.push(EngineCommand::Rules { rules }, Brings::Nothing)?;
                self.forget(|live| unkept(live.rank, live.hold.state(), rules));
                if unkept(Rank::Manual, PlayState::Kept, rules) {
                    self.grabs.clear_kept();
                }
                self.looks = Arc::new(looks);
                Begun::Queued
            }
        };
        self.project = project;
        Ok(begun)
    }

    fn playing_work(&self) -> usize {
        self.plays.iter().flatten().map(Live::wake_work).sum()
    }

    /// Even a full set of plans leaves room to write one light property, end
    /// its grab and release it. Other commands share the unused play budget.
    fn check_work(&self, mut playing: usize, extra_lights: usize) -> Result<(), Refusal> {
        const ONE_GRAB: usize = 3;
        let mut lights = self.grabs.reserved() + extra_lights;
        for (_, work) in &self.prefix_work {
            match work {
                PrefixWork::Plan(visits) => playing += visits,
                PrefixWork::Lights(visits) => lights += visits,
            }
        }
        let remaining = looks::WORK_LIMIT
            .checked_sub(playing)
            .ok_or(Refusal::WorkLimit)?;
        if lights > remaining.max(ONE_GRAB) {
            return Err(Refusal::WorkLimit);
        }
        Ok(())
    }

    fn check_plan_bytes(&self, extra: u64) -> Result<(), Refusal> {
        let freed = self.shared.plan_bytes_freed.load(Ordering::Acquire);
        if self.plan_bytes.saturating_sub(freed) + extra > looks::PLAN_BYTES as u64 {
            return Err(Refusal::WorkLimit);
        }
        Ok(())
    }

    fn live(&self, play: PlayId) -> Option<&Live> {
        self.plays
            .get(play.slot())?
            .as_ref()
            .filter(|live| live.play == play)
    }

    /// Ends `play`, which is live: its plan goes back.
    fn release(&mut self, play: PlayId) -> Result<(), Refusal> {
        self.push(EngineCommand::Release { play }, Brings::Nothing)?;
        if let Some(old) = self
            .plays
            .get_mut(play.slot())
            .and_then(|slot| apply_hold(slot, HoldEvent::Released))
        {
            let terminal = old.work_before_release();
            if terminal > 0 {
                self.prefix_work
                    .push_back((self.sent, PrefixWork::Plan(terminal)));
            }
        }
        Ok(())
    }

    /// Reconcile ended and frozen versions before checking the next admission.
    fn reap(&mut self) {
        let applied = self.shared.applied.load(Ordering::Acquire);
        while self
            .prefix_work
            .front()
            .is_some_and(|(sent, _)| *sent <= applied)
        {
            self.prefix_work.pop_front();
        }
        for ((slot, ended), kept) in self
            .plays
            .iter_mut()
            .zip(&self.shared.ended)
            .zip(&self.shared.kept)
        {
            let Some(live) = slot else { continue };
            if live.play.generation == ended.load(Ordering::Acquire) {
                apply_hold(slot, HoldEvent::Released);
                self.retired += 1;
                continue;
            }
            let frozen = kept.load(Ordering::Acquire);
            let version = live.versions().find(|v| v.sent == frozen).cloned();
            if let Some(version) = version {
                live.current = version;
                live.earlier = None;
                apply_hold(slot, HoldEvent::KeptRead);
            } else if live.current.sent <= applied {
                live.earlier = None;
            } else if let Some(history) = &mut live.earlier {
                // Keep the plan the last completed prefix installed, plus its
                // queued successors. Continuous edits cannot pin every old plan.
                let installed = history.partition_point(|version| version.sent <= applied);
                if installed > 1 {
                    Arc::make_mut(history).drain(..installed - 1);
                }
            }
        }
    }

    fn forget(&mut self, ends: impl Fn(&Live) -> bool) {
        for slot in &mut self.plays {
            if slot.as_ref().is_some_and(&ends)
                && let Some(old) = apply_hold(slot, HoldEvent::Released)
            {
                let terminal = old.work_before_release();
                if terminal > 0 {
                    self.prefix_work
                        .push_back((self.sent, PrefixWork::Plan(terminal)));
                }
                self.retired += 1;
            }
        }
        self.free.unpark();
    }

    /// `QueueFull` unless `places` of the command queue are free now and `returns`
    /// more boxes may go back.
    fn room(&self, places: usize, returns: usize) -> Result<(), Refusal> {
        let held = self.plays.iter().flatten().count() as u64;
        if self.commands.slots() < places
            || returns > 0
                && on_their_way(&self.shared, self.retired)
                    + held
                    + returns as u64
                    + u64::from(self.commands.reserved)
                    > RETIRED as u64
        {
            self.free.unpark();
            return Err(Refusal::QueueFull);
        }
        Ok(())
    }

    /// Takes `patch` and `looks` in place of the empty patch and no looks, as a show file
    /// brings them: all of it in one chunk of the queue, or nothing. `patch` is made
    /// from [`Sender::patch`], so it has this engine's universes and lights.
    ///
    /// | The front | `patch` and `looks` | Result |
    /// |---|---|---|
    /// | not as it starts: a fixture patched, or Mixer settings other than the defaults | any | `Stale`: an open never replaces work |
    /// | as it starts | refused as a change of the patch, or a look names nothing in it | that refusal |
    /// | as it starts | fine, but no room in the queue | `QueueFull` |
    /// | as it starts | fine | writers and rules in one chunk, then the patch and looks |
    ///
    /// Every refusal leaves the front as it was.
    pub fn open(&mut self, patch: Patch, looks: Looks) -> Result<(), Refusal> {
        if !self.as_it_starts() {
            return Err(Refusal::Stale);
        }
        check_pending_patch(&patch, self.project, &self.pending_outputs(), &self.routed)?;
        let looks = looks.rebind(&patch)?;
        self.publish(
            vec![
                PatchEffect::Writers(patch.writers().to_vec()),
                PatchEffect::Send(EngineCommand::Rules {
                    rules: looks.rules(),
                }),
            ],
            None,
        )?;
        self.patch = Arc::new(patch);
        self.looks = Arc::new(looks);
        Ok(())
    }

    /// No fixture patched and the Mixer's default settings: nothing an open could
    /// replace. A saved look, a play and a live or kept grab each need a patched fixture
    /// (an unpatch lets the grabs of its lights go), so none is left either.
    fn as_it_starts(&self) -> bool {
        self.patch.fixtures().is_empty() && self.looks.rules() == MixRules::default()
    }

    /// The saved looks as they are now.
    pub fn looks(&self) -> Arc<Looks> {
        Arc::clone(&self.looks)
    }

    /// Publish every effect of one edit in one FIFO chunk. Starts admitted since
    /// prepare participate. Each replacement reserves a return even if the play
    /// naturally ends or keeps its older version before the chunk is applied.
    fn publish(
        &mut self,
        effects: Vec<PatchEffect>,
        looks: Option<&Looks>,
    ) -> Result<Begun, Refusal> {
        let places: usize = effects.iter().map(places).sum();
        let mut changed = Vec::new();
        let mut playing = self.playing_work();
        if let Some(looks) = looks {
            for live in self
                .plays
                .iter()
                .flatten()
                .filter(|p| p.hold == Hold::Playing)
            {
                let plan = looks.plan(live.look)?;
                if !Arc::ptr_eq(&plan, &live.current.plan) {
                    playing =
                        playing - live.wake_work() + looks::play_work(live.work_after_edit(&plan));
                    changed.push(Version {
                        play: live.play,
                        sent: self.sent + places as u64 + changed.len() as u64 + 1,
                        plan,
                        usage: live.current.usage,
                    });
                }
            }
        }
        let writers = effects
            .iter()
            .filter(|e| matches!(e, PatchEffect::Writers(_)))
            .count();
        let returns = writers + changed.len();
        self.room(places + changed.len(), returns)?;
        let bytes = changed.iter().map(|v| v.plan.bytes() as u64).sum();
        let lights = effects.iter().map(light_visits).sum();
        let mut book = lights;
        for effect in &effects {
            if let PatchEffect::Grab { value, runs } = effect {
                book += self.grabs.fits(value.property(), runs)?;
            }
        }
        if !changed.is_empty() || book > 0 {
            self.check_work(playing, book)?;
            self.check_plan_bytes(bytes)?;
        }
        let at = self.clock.now();
        let chunk = self
            .commands
            .write_chunk_uninit(places + changed.len())
            .map_err(|_| Refusal::QueueFull)?;
        let mut queued = Vec::with_capacity(places + changed.len());
        let mut begun = Begun::Queued;
        for effect in effects {
            match effect {
                PatchEffect::Patched { fixture } => begun = Begun::Patched { fixture },
                PatchEffect::Writers(writers) => queued.push(Sent {
                    command: EngineCommand::Writers,
                    at,
                    brings: Brings::Writers(writers),
                }),
                PatchEffect::Send(command) => {
                    queued.push(Sent {
                        command,
                        at,
                        brings: Brings::Nothing,
                    });
                    if let EngineCommand::Darken { lights } = command {
                        self.grabs.let_go(None, &[lights]);
                    }
                }
                PatchEffect::Runs { change, runs } => {
                    queued.extend(run_commands(change, &runs, at));
                }
                PatchEffect::Grab { value, runs } => {
                    let grab = GrabId {
                        generation: self.grabbed.wrapping_add(1),
                    };
                    queued.extend(run_commands(LightChange::Grab { value, grab }, &runs, at));
                    self.grabbed = grab.generation;
                    self.grabs.grab(grab, value.property(), runs);
                    begun = Begun::Grabbed { grab };
                }
            }
        }
        queued.extend(changed.iter().map(|v| Sent {
            command: EngineCommand::Plan { play: v.play },
            at,
            brings: Brings::Plan(v.clone()),
        }));
        self.sent += chunk.fill_from_iter(queued) as u64;
        self.retired += returns as u64;
        self.plan_bytes += bytes;
        if lights > 0 {
            self.prefix_work
                .push_back((self.sent, PrefixWork::Lights(lights)));
        }
        for version in changed {
            if let Some(Some(live)) = self.plays.get_mut(version.play.slot()) {
                if live.current.usage.repeat == Repeat::Once {
                    Arc::make_mut(live.earlier.get_or_insert_with(Default::default))
                        .push(live.current.clone());
                }
                live.current = version;
            }
        }
        if returns != 0 {
            self.free.unpark();
        }
        Ok(begun)
    }

    /// Queues `change` on the lights of `runs`, all in one wake: the runs travel in
    /// place, in places of the queue made visible at once, so nothing goes back and it
    /// needs only those places.
    fn lights(&mut self, change: LightChange, runs: &[LightRun]) -> Result<(), Refusal> {
        if runs.is_empty() {
            return Ok(());
        }
        let visits = run_visits(runs);
        if matches!(change, LightChange::Update { .. }) {
            self.check_work(self.playing_work(), visits)?;
        }
        let pieces = runs.len().div_ceil(RUNS_IN_PLACE);
        let chunk = self
            .commands
            .write_chunk_uninit(pieces)
            .map_err(|_| Refusal::QueueFull)?;
        let sent = chunk.fill_from_iter(run_commands(change, runs, self.clock.now()));
        self.sent += sent as u64;
        self.prefix_work
            .push_back((self.sent, PrefixWork::Lights(visits)));
        Ok(())
    }

    /// Holds the next start of this output's lane until the test releases it.
    #[cfg(feature = "test-support")]
    #[expect(
        clippy::disallowed_types,
        reason = "only the test and the blocking opener rendezvous"
    )]
    pub fn pause_lane_start(
        &mut self,
        output: OutputNumber,
        reached: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    ) {
        self.lane_pause = Some((output, lane::StartPause { reached, release }));
    }

    /// Undoes each output edit whose opening was dropped unfinished, as a failed open
    /// (output-cutover T4): the opener is back, the old lane serves on, the project is
    /// as before, a new lane goes to be freed.
    fn take_back_abandoned(&mut self) {
        while let Ok(Unfinished {
            open,
            output,
            previous,
            lane,
        }) = self.abandoned.try_recv()
        {
            self.open = Some(open);
            self.commands.reserved = false;
            if let Some(lane) = lane {
                free_elsewhere(Brings::Device(lane));
            }
            if let Some(old) = previous {
                old.cutover().apply(CutoverEvent::Refuse);
            }
            if let Ok((project, _)) = self.project.update(ProjectEvent::OpenFailed { output }) {
                self.project = project;
            }
        }
    }

    /// Publishes the prepared output and waits for the old adapter's last write.
    /// `Stopping` once the outputs stopped: nothing was queued (T13).
    pub fn finish(&mut self, opened: Opened) -> Result<(), Unsent> {
        match self.publish_output(opened)?.map(Handoff::wait) {
            Some(false) => Err(Unsent::Stopping),
            Some(true) | None => Ok(()),
        }
    }

    /// Queues the switch in one step. A shared front completes the returned handoff
    /// outside its publisher lock before it answers the command. Once the outputs
    /// stopped nothing is queued, the project goes back to what it was, and the
    /// handoff's [`Handoff::wait`] says `false` (T13).
    pub fn publish_output(&mut self, opened: Opened) -> Result<Option<Handoff>, Refusal> {
        let Opened {
            lease,
            output,
            kind,
            universes,
            rate,
            opened,
            reuse,
        } = opened;
        let (open, lane) = lease.finish();
        let lane = opened.and_then(|()| lane.ok_or_else(|| io::Error::other("no lane")));
        self.open = Some(open.clone());
        self.commands.reserved = false;
        if !self.shared.life.get().sends() {
            if let Ok(lane) = lane {
                free_elsewhere(Brings::Device(lane));
            }
            self.project = self.project.update(ProjectEvent::OpenFailed { output })?.0;
            return Ok(Some(Handoff {
                stopped: true,
                previous: None,
                next: None,
                retry: self.retry.clone(),
                reuse,
                shared: Arc::clone(&self.shared),
                unfinished_wake: None,
            }));
        }
        let command = EngineCommand::Attach {
            output,
            universes,
            rate,
        };
        let previous = self.lanes.get(output.slot()).cloned().flatten();
        let sent = lane.map_err(|_| Refusal::CannotOpen).and_then(|lane| {
            let next = lane.control.clone();
            let brings = if previous.is_some() {
                Brings::Reroute(lane, reuse)
            } else {
                Brings::Device(lane)
            };
            self.push(command, brings)?;
            Ok(previous.as_ref().map(|previous| {
                let unfinished_wake = self.shared.published();
                if unfinished_wake.is_none() {
                    previous.publish();
                }
                Handoff {
                    stopped: false,
                    previous: Some(previous.clone()),
                    next: Some(Recovery {
                        output,
                        kind,
                        open,
                        lane: next,
                    }),
                    retry: self.retry.clone(),
                    reuse,
                    shared: Arc::clone(&self.shared),
                    unfinished_wake,
                }
            }))
        });
        let event = if sent.is_ok() {
            self.set_attached_route(output, None);
            ProjectEvent::Opened { output, kind }
        } else {
            if let Some(old) = previous {
                old.cutover().apply(CutoverEvent::Refuse);
            }
            ProjectEvent::OpenFailed { output }
        };
        self.project = self.project.update(event)?.0;
        sent
    }

    /// Attaches a test device which takes every channel in the wire run. Tests only:
    /// the product opens every device from the project (`publish_output`).
    #[cfg(any(test, feature = "test-support"))]
    pub fn attach(
        &mut self,
        output: OutputNumber,
        device: Box<dyn Device>,
        universes: Run,
        rate: Rate,
    ) -> Result<(), Refusal> {
        let route =
            Route::of_run(universes, outputs::Limits::RAW).map_err(|_| Refusal::CannotOpen)?;
        self.attach_device(
            output,
            device,
            route,
            rate,
            ProjectEvent::Attached { output, universes },
        )
    }

    /// Attaches a device with its validated route. The project shows the route bounds.
    /// Tests only, as [`Engine::attach`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn attach_route(
        &mut self,
        output: OutputNumber,
        device: Box<dyn Device>,
        route: Route,
        rate: Rate,
    ) -> Result<(), Refusal> {
        device
            .limits()
            .route(route.spans())
            .map_err(|_| Refusal::CannotOpen)?;
        let bounds = route.bounds();
        self.attach_device(
            output,
            device,
            route,
            rate,
            ProjectEvent::Routed { output, bounds },
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    fn attach_device(
        &mut self,
        output: OutputNumber,
        device: Box<dyn Device>,
        route: Route,
        rate: Rate,
        event: ProjectEvent,
    ) -> Result<(), Refusal> {
        self.check_handoff(output)?;
        let universes = route.bounds();
        RunNumbers {
            first: universes.first(),
            count: universes.count(),
        }
        .check(self.universes)?;
        let (project, _) = self.project.update(event)?;
        self.routed
            .check_edit(&self.patch, &self.project, output, route.spans())?;
        if let Some((fixture, _)) = self.patch.unstreamed(&project) {
            return Err(Refusal::StreamsFixture { fixture });
        }
        let explicit =
            matches!(event, ProjectEvent::Routed { .. }).then(|| Arc::from(route.spans()));
        let previous = self.lanes.get(output.slot()).cloned().flatten();
        if let Some(old) = &previous
            && old.cutover().apply(CutoverEvent::Begin) != CutoverEffect::Open
        {
            return Err(Refusal::Opening);
        }
        let lane = Lane::start(
            Some(device),
            route,
            self.clock.clone(),
            Arc::clone(&self.shared.life),
            previous.as_ref(),
            #[cfg(feature = "test-support")]
            None,
        )
        .map_err(|_| {
            if let Some(old) = &previous {
                old.cutover().apply(CutoverEvent::Refuse);
            }
            Refusal::CannotOpen
        })?;
        let next = lane.control.clone();
        let brings = if previous.is_some() {
            Brings::Reroute(Box::new(lane), false)
        } else {
            Brings::Device(Box::new(lane))
        };
        if let Err(reason) = self.push(
            EngineCommand::Attach {
                output,
                universes,
                rate,
            },
            brings,
        ) {
            if let Some(old) = previous {
                old.cutover().apply(CutoverEvent::Refuse);
            }
            return Err(reason);
        }
        if let Some(previous) = previous {
            self.shared.finish_wake(self.shared.published());
            previous.publish();
            previous.finish_handoff(&next, false);
        }
        self.set_attached_route(output, explicit);
        self.project = project;
        Ok(())
    }

    fn set_attached_route(&mut self, output: OutputNumber, spans: Option<Arc<[Span]>>) {
        let current = self.routed.0.get(output.slot()).and_then(Option::as_deref);
        if current != spans.as_deref()
            && let Some(slot) = Arc::make_mut(&mut self.routed).0.get_mut(output.slot())
        {
            *slot = spans;
        }
    }

    /// The project with each route as of its last visible switch.
    pub fn project(&self) -> Project {
        let mut project = self.project;
        for (prior, lane) in self.previous_outputs.iter().zip(&self.lanes) {
            if let Some(old) = prior
                && lane.as_ref().is_some_and(lane::Control::waits_to_serve)
            {
                project = old.restore(project);
            }
        }
        project
    }

    fn pending_outputs(&self) -> [Option<PriorOutput>; MAX_OUTPUTS] {
        std::array::from_fn(|slot| {
            let prior = self.previous_outputs.get(slot).copied().flatten()?;
            self.lanes
                .get(slot)?
                .as_ref()?
                .waits_to_serve()
                .then_some(prior)
        })
    }

    /// Capture the descriptors that can be visible in a snapshot of this revision.
    pub fn project_for_snapshot(&self) -> ProjectView {
        ProjectView {
            project: self.project,
            previous: self.previous_outputs,
        }
    }

    /// A second client must prepare its output change against the next visible run.
    pub fn cutover_pending(&self, output: OutputNumber) -> bool {
        self.lanes
            .get(output.slot())
            .and_then(Option::as_ref)
            .is_some_and(|lane| lane.cutover().effect(CutoverEvent::Begin) == CutoverEffect::Wait)
    }

    /// Cancel pending transfers and stop sending. The tick can still supply a final report.
    pub fn stop_outputs(&self) {
        self.shared.life.apply(LifeEvent::StopOutputs);
        for lane in self.lanes.iter().flatten() {
            lane.cutover().stop();
        }
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

    /// Ends a change, once its commands are queued: the project, the patch and the looks
    /// as they are now are the next revision. A change that queues nothing (a rename of a
    /// look no play shows) is told apart from the one before by it. A wake reads the
    /// revision at its cutoff ([`Snapshot::revision`]): a change ended after it is not in
    /// that snapshot.
    pub fn revise(&mut self) {
        self.shared.revision.fetch_add(1, Ordering::Release);
    }

    /// The changes ended so far ([`Sender::revise`]).
    pub fn revision(&self) -> u64 {
        self.shared.revision.load(Ordering::Relaxed)
    }

    /// Queues `command` with what it `brings`. A device `Attach` brings and
    /// `RemoveOutput` send the old one back; `Attach` with no device keeps it. `Writers`
    /// send the old writers back.
    fn push(&mut self, command: EngineCommand, brings: Brings) -> Result<(), Refusal> {
        // The output this command changes, and whether the device there goes back.
        let (changes, sends_back) = match command {
            EngineCommand::Attach { output, .. } => (
                Some(output),
                matches!(brings, Brings::Device(_) | Brings::Reroute(_, _)),
            ),
            EngineCommand::RemoveOutput { output } => (Some(output), true),
            EngineCommand::Tick { .. }
            | EngineCommand::Handed { .. }
            | EngineCommand::Sent { .. }
            | EngineCommand::SetChannel { .. }
            | EngineCommand::ReleaseChannel { .. }
            | EngineCommand::ReleaseChannels
            | EngineCommand::ReleaseAll
            | EngineCommand::Blackout { .. }
            | EngineCommand::Writers
            | EngineCommand::Runs { .. }
            | EngineCommand::Lights { .. }
            | EngineCommand::Darken { .. }
            | EngineCommand::Start { .. }
            | EngineCommand::Keep { .. }
            | EngineCommand::Rules { .. }
            | EngineCommand::Plan { .. }
            | EngineCommand::Release { .. }
            | EngineCommand::Settle { .. }
            | EngineCommand::Tap
            | EngineCommand::ReleaseTempo
            | EngineCommand::Feed { .. } => (None, false),
        };
        // What goes back now: a start, the plan of a play in its slot; a new plan or a
        // release, the play's plan. A plan the tick thread keeps has its return kept from
        // its start on, so a release takes no more of the queue back: it needs only a
        // place in the command queue. (ReleaseAll and Rules: their plays go back through
        // `forget`, kept the same way.)
        let device =
            sends_back && changes.is_some_and(|output| self.project.output(output).is_some());
        let back = if let EngineCommand::Start { play, .. } = command {
            self.plays.get(play.slot()).is_some_and(Option::is_some)
        } else {
            device
                || matches!(brings, Brings::Probe(_))
                || matches!(
                    command,
                    EngineCommand::Plan { .. }
                        | EngineCommand::Release { .. }
                        | EngineCommand::Writers
                )
        };
        // one more box on its way back or held by the tick thread: all but a release
        let adds = back && !matches!(command, EngineCommand::Release { .. })
            || matches!(command, EngineCommand::Start { .. });
        if let Err(reason) = self.room(1, usize::from(adds)) {
            free_elsewhere(brings);
            return Err(reason);
        }
        let next = if let Brings::Device(lane) | Brings::Reroute(lane, _) = &brings {
            Some(Some(lane.control.clone()))
        } else {
            matches!(command, EngineCommand::RemoveOutput { .. }).then_some(None)
        };
        let cutover = matches!(brings, Brings::Reroute(_, _));
        let next_is_keep = matches!(brings, Brings::Nothing);
        let at = self.clock.now();
        if let Err(rtrb::PushError::Full(refused)) = self.commands.fifo.push(Sent {
            command,
            at,
            brings,
        }) {
            free_elsewhere(refused.brings);
            return Err(Refusal::QueueFull);
        }
        if let (Some(slot), Some(next)) = (
            changes.and_then(|output| self.lanes.get_mut(output.slot())),
            next,
        ) {
            *slot = next;
        }
        if let Some(output) = changes
            && !matches!(command, EngineCommand::RemoveOutput { .. })
            && let Some(generation) = self.generations.get_mut(output.slot())
            && let Some(prior) = self.previous_outputs.get_mut(output.slot())
            && (sends_back || next_is_keep)
        {
            *prior = self
                .project
                .output(output)
                .filter(|_| cutover)
                .map(|descriptor| PriorOutput {
                    number: output,
                    generation: *generation,
                    descriptor,
                    explicit: self
                        .routed
                        .0
                        .get(output.slot())
                        .is_some_and(Option::is_some),
                });
            *generation = generation.wrapping_add(1);
        }
        if back {
            self.retired += 1;
            self.devices += u64::from(device);
            self.free.unpark();
        }
        self.sent += 1;
        Ok(())
    }
}

fn run_commands(
    change: LightChange,
    runs: &[LightRun],
    at: Duration,
) -> impl Iterator<Item = Sent> {
    runs.chunks(RUNS_IN_PLACE).map(move |part| {
        let (command, brings) = if let [run] = part {
            (change.on(*run), Brings::Nothing)
        } else {
            let mut in_place = [None; RUNS_IN_PLACE];
            for (place, run) in in_place.iter_mut().zip(part) {
                *place = Some(*run);
            }
            (EngineCommand::Runs { change }, Brings::Runs(in_place))
        };
        Sent {
            command,
            at,
            brings,
        }
    })
}

fn run_visits(runs: &[LightRun]) -> usize {
    runs.iter().map(|run| usize::from(run.count())).sum()
}

fn light_visits(effect: &PatchEffect) -> usize {
    match effect {
        PatchEffect::Runs { runs, .. } | PatchEffect::Grab { runs, .. } => run_visits(runs),
        PatchEffect::Send(command) => {
            if let EngineCommand::Lights { lights, .. } | EngineCommand::Darken { lights } = command
            {
                usize::from(lights.count())
            } else {
                0
            }
        }
        PatchEffect::Writers(_) | PatchEffect::Patched { .. } => 0,
    }
}

/// Places of the command queue `effect` takes.
fn places(effect: &PatchEffect) -> usize {
    match effect {
        PatchEffect::Patched { .. } => 0,
        PatchEffect::Writers(_) | PatchEffect::Send(_) => 1,
        PatchEffect::Runs { runs, .. } | PatchEffect::Grab { runs, .. } => {
            runs.len().div_ceil(RUNS_IN_PLACE)
        }
    }
}

/// Frees a device the tick thread did not take, and stops its thread, on a thread of its
/// own: closing a port can take long, and the sender may be shared behind a lock. Without
/// a thread from the system, it is freed here after all. Writers are freed here.
fn free_elsewhere(brings: Brings) {
    if let Brings::Device(lane) | Brings::Reroute(lane, _) = brings {
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
    pub fn open(self) -> Opened {
        let started = Instant::now();
        let mut lease = self.lease;
        let prior_visible = lease.previous.as_ref().is_none_or(|old| {
            while old.waits_to_serve() {
                if !self.shared.life.get().sends()
                    || started.elapsed() >= FREE_WITHIN
                {
                    return false;
                }
                thread::sleep(Duration::from_millis(1));
            }
            // T1 once T9 showed the run this waited for; T12 when admission began it
            matches!(
                old.cutover().apply(CutoverEvent::Begin),
                CutoverEffect::Open | CutoverEffect::Wait
            )
        });
        let reuse = loop {
            let port = match &self.source {
                DeviceSource::Fresh => DeviceSource::Fresh,
                DeviceSource::Exclusive => DeviceSource::Exclusive,
                DeviceSource::Reusable(ready) => {
                    DeviceSource::Reusable(ready.load(Ordering::Acquire))
                }
            };
            match open_action(port, waits(&self.shared, self.devices), started.elapsed()) {
                OpenAction::Reuse => break true,
                OpenAction::Open => break false,
                OpenAction::Wait => {
                    self.free.unpark();
                    thread::sleep(Duration::from_millis(1));
                }
            }
        };
        // Opening the device and starting the thread may both fail. The accepted
        // lane keeps sending until finish publishes a fully prepared replacement.
        let lane = (|| {
            if !prior_visible || !self.shared.life.get().sends() {
                return Err(io::Error::other("output unavailable"));
            }
            let device = if reuse {
                None
            } else {
                Some(lease.open.device(self.kind)?)
            };
            let lane = Lane::start(
                device,
                self.route,
                self.clock,
                Arc::clone(&self.shared.life),
                lease.previous.as_ref(),
                #[cfg(feature = "test-support")]
                self.lane_pause,
            )?;
            Ok(Box::new(lane))
        })();
        if lane.is_err()
            && !matches!(self.source, DeviceSource::Fresh)
            && let Some(previous) = lease.previous.clone()
        {
            previous.watch_loss();
            if let Err(request) = self.retry.send(Recovery {
                output: self.output,
                kind: self.kind,
                open: lease.open.clone(),
                lane: previous,
            }) {
                request.0.lane.worker_gone();
            }
        }
        let opened = lane.map(|lane| {
            lease.lane = Some(lane);
        });
        Opened {
            lease,
            output: self.output,
            kind: self.kind,
            universes: self.universes,
            rate: self.rate,
            opened,
            reuse,
        }
    }
}

/// Whether one of the `devices` sent away so far is not freed yet: an opening waits.
fn waits(shared: &Shared, devices: u64) -> bool {
    devices > shared.devices_freed.load(Ordering::Acquire)
}

/// Of the `retired` boxes sent back so far, those not freed yet.
fn on_their_way(shared: &Shared, retired: u64) -> u64 {
    retired.saturating_sub(shared.freed.load(Ordering::Acquire))
}

/// A refused restart still has its old lane. If its adapter subsequently reports
/// failure, open the old kind off-thread and deliver it to that same lane.
#[derive(Clone)]
struct Recovery {
    output: OutputNumber,
    kind: OutputKind,
    open: Opener,
    lane: lane::Control,
}

#[expect(
    clippy::disallowed_types,
    reason = "only the blocking open worker submits retries"
)]
type Retry = std::sync::mpsc::Sender<Recovery>;

#[expect(
    clippy::disallowed_types,
    reason = "this is the blocking reopen worker"
)]
fn recover(requests: std::sync::mpsc::Receiver<Recovery>, shared: &Shared) {
    let mut pending: [Option<Recovery>; MAX_OUTPUTS] = std::array::from_fn(|_| None);
    while !shared.life.get().over() {
        match requests.recv_timeout(FREE_EVERY) {
            Ok(request) => {
                if !request.lane.closed()
                    && let Some(place) = pending.get_mut(request.output.slot())
                {
                    *place = Some(request);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        for place in &mut pending {
            if let Some(retry) = place {
                if retry.lane.closed() {
                    *place = None;
                } else if retry.lane.needs_open()
                    && let Ok(device) = retry.open.device(retry.kind)
                    && retry.lane.needs_open()
                    && !retry.lane.closed()
                {
                    retry.lane.install(device);
                }
            }
        }
    }
}

/// The free thread: frees the devices and writers the tick thread retired, closing ports
/// and sockets, on a steady beat or when the sender wakes it. Until the engine stops.
fn free(mut retired: rtrb::Consumer<Retired>, shared: &Shared) {
    loop {
        while let Ok(back) = retired.pop() {
            #[cfg(any(test, feature = "test-support"))]
            while shared.stalled.load(Ordering::Acquire) && !shared.life.get().over() {
                thread::park_timeout(Duration::from_millis(1));
            }
            let device = matches!(back, Retired::Device(_));
            let bytes = if let Retired::Plan(plan) = &back {
                plan.bytes() as u64
            } else {
                0
            };
            match back {
                Retired::Probe(ready) => ready.store(true, Ordering::Release),
                Retired::Device(_) | Retired::Plan(_) | Retired::Writers(_) => drop(back),
            }
            if bytes > 0 {
                shared.plan_bytes_freed.fetch_add(bytes, Ordering::Release);
            }
            shared.freed.fetch_add(1, Ordering::Release);
            if device {
                shared.devices_freed.fetch_add(1, Ordering::Release);
            }
        }
        if shared.life.get().over() {
            return; // what comes back later is freed with the tick thread's queue end
        }
        thread::park_timeout(FREE_EVERY);
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

/// Every channel at rest, on the heap. Its own frame: the 256 KiB a debug build passes
/// through the stack is gone again before the engine starts its threads.
#[inline(never)]
fn channels() -> Box<Channels> {
    Box::new(Channels::NEW)
}

/// What the tick thread owns. Every buffer is made before it starts.
struct Ticker {
    shared: Arc<Shared>,
    commands: rtrb::Consumer<Sent>,
    retired: rtrb::Producer<Retired>,
    /// Each output's own thread, by output number, beside `State::outputs`.
    devices: [Option<Box<Lane>>; MAX_OUTPUTS],
    switching: Box<[Option<Switch>; MAX_OUTPUTS]>,
    /// The patch's writers: lights to bytes, every frame.
    writers: Vec<Writer>,
    /// Each play's plan, by slot, beside `State::plays`.
    plans: [Option<Version>; PLAYS],
    scratch: [Box<[Entry]>; PLAYS],
    /// Who holds each property of each light this frame: the Mixer's buffer.
    claims: Box<[Claims]>,
    /// The state, the held levels and what was counted: what readers get a copy of.
    now: Box<Snapshot>,
    hand: Hand,
    /// Every channel, 8 bytes each: the frame that goes out and who set each byte.
    channels: Box<Channels>,
    /// When the commands were sent, until each output carried them.
    carried: Box<Carried>,
    /// Results waiting until all due outputs have been served. Room for every lane
    /// box plus one immediate result per output.
    staged: Box<[Option<Accepted>; (lane::BOXES + 1) * MAX_OUTPUTS]>,
    clock: Clock,
    trace: Trace,
    #[cfg(feature = "test-support")]
    timings: Option<snapshot::timing::TimingWriter>,
}

/// The tick owns both lanes from T6 until T9. The pending output has its own
/// deadline and press prefix, so its frames never advance the visible old run.
struct Switch {
    next: Box<Lane>,
    schedule: Output,
    reuse: bool,
    presses: u64,
    handed: u64,
    lateness: Times<LATENESS_BUCKETS>,
}

/// Capacity of each send-time history. Overwritten press stamps count as missing.
const CARRY: usize = 4 * QUEUE;

#[derive(Clone, Copy)]
struct Accepted {
    output: OutputNumber,
    prefix: lane::Prefix,
    at: Duration,
}

/// Send times indexed by the number of events recorded in this history.
struct Stamps {
    sent: [Duration; CARRY],
    total: u64,
}

impl Stamps {
    const EMPTY: Self = Self {
        sent: [Duration::ZERO; CARRY],
        total: 0,
    };

    fn push(&mut self, at: Duration) {
        if let Some(slot) = self.sent.get_mut(ring(self.total)) {
            *slot = at;
        }
        self.total = self.total.wrapping_add(1);
    }

    fn at(&self, index: u64) -> Option<Duration> {
        self.sent.get(ring(index)).copied()
    }
}

struct Carried {
    /// The scorecard times every applied command up to the lane handoff.
    commands: Stamps,
    /// Device-acceptance timing uses one stamp per command that changes what lights
    /// show: every command but `Attach` and `RemoveOutput`, which only manage outputs.
    presses: Stamps,
    /// Per output: the commands applied before its last frame, or before it was attached.
    carried: [u64; MAX_OUTPUTS],
    /// Prefix of each output's last accepted frame in this attachment.
    last_accepted: [u64; MAX_OUTPUTS],
}

impl Carried {
    const fn new() -> Self {
        Self {
            commands: Stamps::EMPTY,
            presses: Stamps::EMPTY,
            carried: [0; MAX_OUTPUTS],
            last_accepted: [0; MAX_OUTPUTS],
        }
    }

    /// A command sent at `at` was applied.
    fn applied(&mut self, command: &EngineCommand, at: Duration) {
        self.commands.push(at);
        if !matches!(
            command,
            EngineCommand::Attach { .. } | EngineCommand::RemoveOutput { .. }
        ) {
            self.presses.push(at);
        }
    }

    /// A new attachment starts at the current press prefix. The scorecard follows
    /// the device itself, so its handoff history continues when we keep the lane.
    fn attached(&mut self, output: usize, keeps_lane: bool) {
        if let Some(prefix) = self.last_accepted.get_mut(output) {
            *prefix = self.presses.total;
        }
        if !keeps_lane && let Some(carried) = self.carried.get_mut(output) {
            *carried = self.commands.total;
        }
    }

    /// Count through this frame's prefix at the time its device took it. Commands
    /// applied after the frame can already have overwritten some of its stamps.
    fn accepted(&mut self, frame: Accepted, now: &mut Snapshot) {
        let slot = frame.output.slot();
        let (Some(previous), Some(Some(output))) = (
            self.last_accepted.get_mut(slot),
            now.state.outputs.get(slot),
        ) else {
            return;
        };
        let rate = output.rate;
        let through = frame.prefix.presses;
        let pending = through.wrapping_sub(*previous);
        let newer = self.presses.total.wrapping_sub(through);
        let retained = pending.min((CARRY as u64).saturating_sub(newer));
        now.press_missing(slot, pending - retained);
        for distance in (1..=retained).rev() {
            if let Some(sent) = self.presses.at(through.wrapping_sub(distance)) {
                now.pressed(slot, frame.at.saturating_sub(sent), rate);
            }
        }
        *previous = through;
    }

    /// `output` handed over a frame at `handed`: each command applied since its last
    /// frame took `handed - sent` to get there.
    fn handed(&mut self, output: usize, handed: Duration, mut count: impl FnMut(Duration)) {
        let Some(carried) = self.carried.get_mut(output) else {
            return;
        };
        let oldest = self.commands.total.saturating_sub(CARRY as u64);
        for command in (*carried).max(oldest)..self.commands.total {
            if let Some(at) = self.commands.at(command) {
                count(handed.saturating_sub(at));
            }
        }
        *carried = self.commands.total;
    }
}

/// Where stamp number `n` lives in its ring.
fn ring(n: u64) -> usize {
    usize::try_from(n % CARRY as u64).unwrap_or(0)
}

impl Ticker {
    /// The real-time loop, an action: reads the clock, waits, hands frames over and
    /// the snapshot. The logic is in `engine-core`. Nothing here may lock, allocate,
    /// block, log or call out. Returns itself, so its devices are freed by whoever joins it.
    fn run(mut self, priority: Priority) -> Self {
        self.now.state = State::new(priority);
        while !self.shared.life.get().over() {
            // Sleep, then spin the last millisecond before an output: a sleep can wake
            // late, a spin cannot. A beat with no output only sleeps.
            let (sleep, spin) = wait(&self.scheduled(), self.clock.now());
            self.clock.wait(sleep, spin, &self.shared.life);
            // The cutoff (`wake.rs`): the picture asked for and the commands published
            // so far belong to this wake, then the frame time is read. A command
            // published after it waits for the next wake.
            let asked = self.hand.asked();
            self.shared.wake.fetch_add(1, Ordering::AcqRel);
            let published = self.commands.slots().min(QUEUE);
            let mut wake_actions = self.devices.each_ref().map(|lane| {
                lane.as_ref().map_or(CutoverEffect::Nothing, |old| {
                    old.control.cutover().effect(CutoverEvent::Wake)
                })
            });
            // read after the commands are counted: a change it counts has all its
            // commands in this wake, or its moment's commands are more than applied
            self.now
                .revised(self.shared.revision.load(Ordering::Acquire));
            let now = self.clock.cutoff(&self.shared.life);
            self.apply(EngineCommand::Tick { now });

            // Apply them right before the frame: a press is in the next one.
            for _ in 0..published {
                let Ok(sent) = self.commands.pop() else {
                    break;
                };
                // in time order: what the grid reached before it was sent comes first
                self.settle(sent.at);
                let rerouted = match (sent.command, &sent.brings) {
                    (EngineCommand::Attach { output, .. }, Brings::Reroute(_, _)) => Some(output),
                    _ => None,
                };
                match (sent.command, &sent.brings) {
                    (EngineCommand::Keep { play }, _) => self.stop(play),
                    (_, Brings::Reroute(_, _) | Brings::Probe(_)) => {}
                    (command, _) => self.apply(command),
                }
                if let EngineCommand::Attach { output, .. } = sent.command
                    && let Some(action) = wake_actions.get_mut(output.slot())
                {
                    match &sent.brings {
                        Brings::Device(lane) => {
                            *action = lane.control.cutover().effect(CutoverEvent::Wake)
                        }
                        Brings::Nothing
                        | Brings::Reroute(_, _)
                        | Brings::Probe(_)
                        | Brings::Writers(_)
                        | Brings::Runs(_)
                        | Brings::Plan(_) => {}
                    }
                }
                self.swap(sent.command, sent.brings);
                // T6 decided: frames go to the new lane (T7) or T9 shows it
                if let Some(output) = rerouted
                    && let Some(action) = wake_actions.get_mut(output.slot())
                {
                    *action = self
                        .devices
                        .get(output.slot())
                        .and_then(Option::as_ref)
                        .map_or(CutoverEffect::Nothing, |old| {
                            old.control.cutover().effect(CutoverEvent::Wake)
                        });
                }
                self.carried.applied(&sent.command, sent.at);
                self.now.applied_one();
                #[cfg(feature = "test-support")]
                if let Some(timings) = &mut self.timings {
                    timings.write(Timing::Applied {
                        number: self.now.applied(),
                        at: sent.at,
                    });
                }
            }
            self.settle(now);
            let clock = &self.now.state.clock;
            let elapsed = clock.beat_at(clock.at());
            for play in self
                .now
                .state
                .plays
                .iter()
                .filter(|p| p.state == PlayState::Playing)
            {
                let slot = play.play.slot();
                if let (Some(Some(version)), Some(out)) =
                    (self.plans.get(slot), self.scratch.get_mut(slot))
                {
                    version
                        .plan
                        .sample(elapsed.since(play.start), version.usage, out);
                }
            }
            let (plans, scratch) = (&self.plans, &self.scratch);
            self.now.show(
                |slot| {
                    plans
                        .get(slot)
                        .and_then(Option::as_ref)
                        .zip(scratch.get(slot))
                        .map_or(&[], |(v, out)| v.plan.entries(out))
                },
                &mut self.claims,
            );
            frame(
                &self.now.state,
                self.now.universes(),
                self.now.holds(),
                self.now.lights(),
                &self.writers,
                &mut self.channels,
            );
            self.back();
            self.show_returned(&wake_actions);
            let mut serve = [None; MAX_OUTPUTS];
            for (slot, number) in serve.iter_mut().zip(due(&self.scheduled(), now)) {
                *slot = Some(number);
            }
            for number in serve.into_iter().flatten() {
                if let Some(action) = wake_actions.get(number.slot()) {
                    self.serve(number, *action);
                }
                let (handed, now) = (self.clock.now(), &mut self.now);
                self.carried
                    .handed(number.slot(), handed, |time| now.carried(time));
            }
            self.shared.wake.fetch_add(1, Ordering::Release);
            // Each lane returns frames in order. Across its accepted prefixes this
            // wake, at most CARRY surviving send times can be visited.
            for frame in self.staged.iter_mut().filter_map(Option::take) {
                self.carried.accepted(frame, &mut self.now);
                self.trace.counted(frame.output);
            }
            self.shared
                .applied
                .store(self.now.applied(), Ordering::Release);
            if let Some(asked) = asked {
                self.hand.fill(asked, &self.now);
            }
        }
        for lane in self.devices.iter().flatten() {
            lane.control.cutover().stop();
        }
        self
    }

    /// Cross waiting launches and once ends before the next FIFO event. The last
    /// sample of a once-use is its left limit, regardless of how late this wake is.
    fn settle(&mut self, at: Duration) {
        self.apply(EngineCommand::Settle { at });
        let clock = &self.now.state.clock;
        let count = clock.beat_at(clock.at());
        for slot in 0..PLAYS {
            if let Some(play) = self.now.state.plays.at(slot)
                && play.state == PlayState::Playing
                && let Some(Some(version)) = self.plans.get(slot)
                && version.usage.repeat == Repeat::Once
                && version
                    .plan
                    .finished(count.since(play.start), version.usage)
            {
                match stops(play.rank, self.now.state.rules) {
                    End::Keep => self.stop(play.play),
                    End::Remove => self.ended(play.play),
                }
            }
        }
    }

    /// Freeze this source's sample at the event, even when a higher rank covers it.
    /// Waiting and resting plays have nothing to retain.
    fn stop(&mut self, id: PlayId) {
        let slot = id.slot();
        let Some(play) = self.now.state.plays.at(slot).filter(|p| p.play == id) else {
            return;
        };
        if play.state == PlayState::Kept {
            return;
        }
        if play.state == PlayState::Playing
            && let (Some(Some(version)), Some(out)) =
                (self.plans.get(slot), self.scratch.get_mut(slot))
            && version.plan.sample(
                self.now
                    .state
                    .clock
                    .beat_at(self.now.state.clock.at())
                    .since(play.start),
                version.usage,
                out,
            )
        {
            if let Some(kept) = self.shared.kept.get(slot) {
                kept.store(version.sent, Ordering::Release);
            }
            self.apply(EngineCommand::Keep { play: id });
        } else {
            self.ended(id);
        }
    }

    fn ended(&mut self, play: PlayId) {
        self.apply(EngineCommand::Release { play });
        self.replan(play.slot(), None);
        if let Some(ended) = self.shared.ended.get(play.slot()) {
            ended.store(play.generation, Ordering::Release);
        }
    }

    /// For `Attach` and `RemoveOutput`: puts the device brought at the output, sends the
    /// old one back to be freed and starts the output's lateness afresh. `Attach` without
    /// a device keeps the one there. For `Writers`: the writers brought take the place of
    /// the old ones, which go back. For `Runs`: each run brought takes the change, in this
    /// wake. Others change nothing.
    fn swap(&mut self, command: EngineCommand, brings: Brings) {
        if let EngineCommand::Release { play } = command {
            self.replan(play.slot(), None);
            return;
        }
        if let EngineCommand::ReleaseAll | EngineCommand::Rules { .. } = command {
            // Rules can clear a natural hold before the sender has observed it.
            for slot in 0..PLAYS {
                if !self.now.state.plays.has(slot) {
                    if let Some(Some(version)) = self.plans.get(slot)
                        && let Some(ended) = self.shared.ended.get(slot)
                    {
                        ended.store(version.play.generation, Ordering::Release);
                    }
                    self.replan(slot, None);
                }
            }
            return;
        }
        let device = match brings {
            Brings::Plan(version) => {
                let slot = version.play.slot();
                if self
                    .now
                    .state
                    .plays
                    .at(slot)
                    .is_some_and(|p| p.play == version.play && p.state != PlayState::Kept)
                {
                    self.replan(slot, Some(version));
                } else {
                    // Its reserved return carries the unused replacement itself.
                    let _ = self.retired.push(Retired::Plan(version.plan));
                }
                return;
            }
            Brings::Reroute(next, reuse) => {
                if let EngineCommand::Attach {
                    output,
                    universes,
                    rate,
                } = command
                    && let Some(Some(old)) = self.devices.get(output.slot())
                    && let Some(place) = self.switching.get_mut(output.slot())
                    && let Some(schedule) = self
                        .now
                        .state
                        .reroute(output, universes, rate)
                        .outputs
                        .get(output.slot())
                        .copied()
                        .flatten()
                {
                    old.control.publish();
                    if old.control.cutover().apply(CutoverEvent::Cutoff) != CutoverEffect::SwitchLane
                    {
                        // stopped (T13): no switch
                        let _ = self.retired.push(Retired::Device(next));
                        return;
                    }
                    *place = Some(Switch {
                        next,
                        schedule: Output {
                            status: Status::NotFound,
                            ..schedule
                        },
                        reuse,
                        presses: self.carried.presses.total,
                        handed: 0,
                        lateness: Times::ZERO,
                    });
                } else {
                    let _ = self.retired.push(Retired::Device(next));
                }
                return;
            }
            Brings::Device(device) => Some(device),
            Brings::Probe(probe) => {
                let _ = self.retired.push(Retired::Probe(probe));
                return;
            }
            Brings::Nothing => {
                if let EngineCommand::Attach { output, .. } = command {
                    self.carried.attached(output.slot(), true);
                    self.now.attached(output.slot());
                    return;
                }
                None
            }
            Brings::Writers(writers) => {
                let old = std::mem::replace(&mut self.writers, writers);
                let _ = self.retired.push(Retired::Writers(old));
                return;
            }
            Brings::Runs(runs) => {
                if let EngineCommand::Runs { change } = command {
                    for run in runs.into_iter().flatten() {
                        self.apply(change.on(run));
                    }
                }
                return;
            }
        };
        let (EngineCommand::Attach { output, .. } | EngineCommand::RemoveOutput { output }) =
            command
        else {
            return;
        };
        let Some(slot) = self.devices.get_mut(output.slot()) else {
            return;
        };
        self.carried.attached(output.slot(), false);
        if let Some(old) = std::mem::replace(slot, device) {
            // The queue has room for every device a wake can retire (see `start`).
            let _ = self.retired.push(Retired::Device(old));
        }
        self.now.attached(output.slot());
    }

    /// While a transfer drains, the held lane's rate owns the scheduler.
    fn scheduled(&self) -> State {
        let mut scheduled = self.now.state;
        if !self.shared.life.get().sends() {
            scheduled.outputs.fill(None);
            return scheduled;
        }
        for (out, switch) in scheduled.outputs.iter_mut().zip(self.switching.iter()) {
            if let Some(switch) = switch {
                *out = Some(switch.schedule);
            }
        }
        scheduled
    }

    /// T9. back() has counted the old boxes. Finish their press prefix before
    /// retiring the old lane or advancing its generation in the snapshot.
    fn show_returned(&mut self, actions: &[CutoverEffect; MAX_OUTPUTS]) {
        for (slot, action) in actions.iter().enumerate() {
            if *action != CutoverEffect::ShowNew {
                continue;
            }
            let Some(switch) = self.switching.get_mut(slot).and_then(Option::take) else {
                continue;
            };
            let Some(installed) = self.devices.get_mut(slot) else {
                continue;
            };
            for staged in &mut *self.staged {
                if staged.is_some_and(|sent| sent.output.slot() == slot)
                    && let Some(sent) = staged.take()
                {
                    self.carried.accepted(sent, &mut self.now);
                    self.trace.counted(sent.output);
                }
            }
            if let Some(old) = installed.replace(switch.next) {
                let _ = self.retired.push(Retired::Device(old));
            }
            let mut schedule = switch.schedule;
            if let Some(visible) = self.now.state.outputs.get_mut(slot) {
                if let Some(old) = *visible
                    && switch.reuse
                {
                    schedule.status = old.status;
                }
                *visible = Some(schedule);
            }
            self.now.cutover(
                slot,
                switch.reuse,
                switch.handed,
                &switch.lateness,
            );
            if let Some(prefix) = self.carried.last_accepted.get_mut(slot) {
                *prefix = switch.presses;
            }
            if let Some(new) = installed {
                // T9 commits: the serving lane number moves to the new lane. Stopped since
                // the peek (T13), each lane ends by the engine's life instead.
                new.control.cutover().apply(CutoverEvent::Wake);
            }
        }
    }

    /// Gathers a due output's route, and when its next is due, on to its own
    /// thread. What comes of it is counted when its thread hands the box back ([`Ticker::back`]). With no spare
    /// box, the device is still on older frames: this one is dropped and counted now.
    fn serve(&mut self, number: OutputNumber, action: CutoverEffect) {
        let slot = number.slot();
        if !self.shared.life.get().sends() {
            return;
        }
        let at = self.clock.now();
        let (output, next, lane, generation) = if action == CutoverEffect::SendNew {
            let Some(switch) = self.switching.get_mut(slot).and_then(Option::as_mut) else {
                return;
            };
            let output = switch.schedule;
            switch.schedule = output.handed(at);
            (
                output,
                switch.schedule.due,
                &mut switch.next,
                self.now.generation(slot).wrapping_add(1),
            )
        } else {
            if !matches!(action, CutoverEffect::SendOld | CutoverEffect::ShowNew) {
                return;
            }
            let Some(Some(output)) = self.now.state.outputs.get(slot).copied() else {
                return;
            };
            self.apply(EngineCommand::Handed { output: number, at });
            let Some(lane) = self.devices.get_mut(slot).and_then(Option::as_mut) else {
                return;
            };
            let next = self
                .now
                .state
                .outputs
                .get(slot)
                .and_then(|o| *o)
                .map_or(at, |o| o.due);
            (output, next, lane, self.now.generation(slot))
        };
        self.trace.handed(number, output.due, at);
        #[cfg(feature = "test-support")]
        if let Some(timings) = &mut self.timings {
            timings.write(Timing::Due {
                output: number,
                due: output.due,
                applied: self.now.applied(),
            });
        }
        let frame = self.channels.values();
        let prefix = lane::Prefix {
            presses: self.carried.presses.total,
            generation,
        };
        let handed = lane.hand(frame, output.due, next, prefix);
        if handed != lane::Hand::Full {
            if let Some(Some(switch)) = self.switching.get_mut(slot) {
                switch.handed = switch.handed.wrapping_add(1);
            } else {
                self.now.handed_one(slot);
            }
            #[cfg(all(test, feature = "test-support"))]
            self.shared
                .presses_at_handoff
                .store(self.now.presses_in_first_bucket(), Ordering::Relaxed);
        }
        let dropped = match handed {
            lane::Hand::Replaced(sent) => Some((sent.due, sent.at)),
            lane::Hand::Full => Some((output.due, at)),
            lane::Hand::Handed => None,
        };
        if let Some((due, at)) = dropped {
            let outcome = Outcome::Dropped;
            self.trace.came(number, outcome, due);
            #[cfg(feature = "test-support")]
            if let Some(timings) = &mut self.timings {
                timings.write(Timing::Finished {
                    output: number,
                    due,
                    at,
                    outcome,
                });
            }
            if let Some(Some(switch)) = self.switching.get_mut(slot) {
                switch.schedule = switch.schedule.sent(outcome, at);
                switch.lateness.count(at.saturating_sub(due));
            } else {
                self.now.late(slot, at.saturating_sub(due));
                self.now.sent(number, outcome, at, prefix.generation);
            }
        }
    }

    /// Counts what came of the frames each output's thread handed back since the last
    /// wake: the outcome, and how late the device was handed each.
    fn back(&mut self) {
        let Self {
            devices,
            staged,
            now,
            trace,
            #[cfg(feature = "test-support")]
            timings,
            ..
        } = self;
        let mut spare = staged.iter_mut();
        for (lane, number) in devices.iter_mut().zip(1..) {
            let (Some(lane), Ok(output)) = (lane, OutputNumber::try_from(number)) else {
                continue;
            };
            if lane.control.cutover().apply(CutoverEvent::FrameBack) != CutoverEffect::CountOld {
                continue;
            }
            let watching_loss = lane.control.watching_loss();
            lane.back(|sent| {
                trace.came(output, sent.outcome, sent.due);
                #[cfg(feature = "test-support")]
                if let Some(writer) = timings {
                    writer.write(Timing::Finished {
                        output,
                        due: sent.due,
                        at: sent.at,
                        outcome: sent.outcome,
                    });
                }
                now.late(output.slot(), sent.at.saturating_sub(sent.due));
                // The recovery worker watches this adapter after a failed open.
                // Its loss is visible even if it had only dropped frames before.
                if sent.outcome == Outcome::Failed
                    && watching_loss
                    && sent.prefix.generation == now.generation(output.slot())
                    && let Some(Some(out)) = now.state.outputs.get_mut(output.slot())
                    && out.status == Status::NotFound
                {
                    out.status = Status::Connecting;
                }
                now.sent(output, sent.outcome, sent.at, sent.prefix.generation);
                // Reconfiguration can keep the lane with an old frame still in flight:
                // its status, counters and lateness count, its presses and outage not.
                if sent.outcome == Outcome::Accepted
                    && sent.prefix.generation == now.generation(output.slot())
                    && let Some(place) = spare.next()
                {
                    *place = Some(Accepted {
                        output,
                        prefix: sent.prefix,
                        at: sent.at,
                    });
                }
            });
        }
    }

    /// `plan` in place of the plan of `slot`, which goes back.
    fn replan(&mut self, slot: usize, plan: Option<Version>) {
        let Some(at) = self.plans.get_mut(slot) else {
            return;
        };
        if let Some(old) = std::mem::replace(at, plan) {
            // The queue has room for every box a wake can retire (see `start`).
            let _ = self.retired.push(Retired::Plan(old.plan));
        }
    }

    fn apply(&mut self, command: EngineCommand) {
        self.now.update(command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// docs/engine.md, "What the front reserves for a play", one row per pair.
    #[test]
    fn every_hold_and_event_pair_follows_the_front_table() {
        use Hold::{Kept, Playing};
        use HoldEvent::{KeepSent, KeptRead, Released, Start};
        for (hold, event, expected) in [
            (None, Start, Some(Playing)),
            (None, KeepSent, None),
            (None, KeptRead, None),
            (None, Released, None),
            (Some(Playing), Start, Some(Playing)),
            (Some(Playing), KeepSent, Some(Kept)),
            (Some(Playing), KeptRead, Some(Kept)),
            (Some(Playing), Released, None),
            (Some(Kept), Start, Some(Playing)),
            (Some(Kept), KeepSent, Some(Kept)),
            (Some(Kept), KeptRead, Some(Kept)),
            (Some(Kept), Released, None),
        ] {
            assert_eq!(next_hold(hold, event), expected, "{hold:?} + {event:?}");
        }
    }

    /// docs/engine.md, Released: new mix rules that no longer keep a stopped play free
    /// its hold.
    #[cfg(feature = "test-support")]
    #[test]
    #[expect(
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test whose set-up fails fails"
    )]
    fn new_mix_rules_release_a_kept_hold_they_no_longer_keep() {
        let (_engine, mut sender, wakes) =
            Engine::start_stepped(Universes::ONE, Box::new(outputs::open)).unwrap();
        wakes.hold().unwrap();
        sender
            .send(Command::PatchStrip {
                name: Text::new("strip").unwrap(),
                pixels: engine_core::Pixels::try_from(1).unwrap(),
                color: Color::Rgb,
                at: ChannelNumbers {
                    universe: 1,
                    address: 1,
                },
            })
            .unwrap();
        sender
            .send(Command::PutStep {
                look: 1,
                name: Text::new("s").unwrap(),
                step: engine_core::Step {
                    target: Target::Light { light: 1 },
                    values: Light::DARK,
                },
            })
            .unwrap();
        let rules = |other_end| Command::SetMixRules {
            rules: MixRules {
                other_end,
                ..MixRules::default()
            },
        };
        sender.send(rules(OtherEnd::Stay)).unwrap();
        let start = Command::StartLook {
            look: 1,
            rank: Rank::Show,
            when: When::Now,
        };
        let Ok(Begun::Started { play }) = sender.begin(start, &[]) else {
            panic!("look 1 starts");
        };
        sender.send(Command::StopLook { play }).unwrap();
        let hold = |sender: &Sender| {
            sender
                .plays
                .get(play.slot())
                .and_then(|live| live.as_ref().map(|live| live.hold))
        };
        assert_eq!(hold(&sender), Some(Hold::Kept), "Stay keeps the stopped play");
        sender.send(rules(OtherEnd::Reveal)).unwrap();
        assert_eq!(hold(&sender), None, "Reveal releases it");
    }

    /// output-cutover.md, Lane recovery: a lost lane whose restart is refused after the
    /// recovery worker ended is Stranded (Opening::open's request cannot be sent).
    #[cfg(feature = "test-support")]
    #[test]
    #[expect(
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test whose set-up fails fails"
    )]
    fn a_refused_restart_with_the_recovery_worker_gone_strands_the_lost_lane() {
        use crate::recovery::State as RecoveryState;
        struct Lost;
        impl Device for Lost {
            fn take(&mut self, _: &[u8]) -> Outcome {
                Outcome::Failed
            }
        }

        let (engine, mut sender, wakes) = Engine::start_stepped(
            Universes::new(2).unwrap(),
            Box::new(|_| Ok(Box::new(Lost) as Box<dyn Device>)),
        )
        .unwrap();
        wakes.hold().unwrap();
        let output = OutputNumber::try_from(1).unwrap();
        let set = |count| Command::SetOutput {
            output,
            kind: engine_core::TEST_STREAM_KIND,
            universes: RunNumbers { first: 1, count },
            rate: None,
        };
        sender.send(set(1)).unwrap();
        let lane = sender.lanes.get(output.slot()).cloned().flatten().unwrap();
        let started = Instant::now();
        while lane.recovery_state() != RecoveryState::Lost {
            assert!(started.elapsed() < Duration::from_secs(10), "never lost");
            wakes.wake().unwrap();
        }
        let Ok(Begun::Opening(opening)) = sender.begin(set(2), &[]) else {
            panic!("the larger route needs a new lane");
        };
        wakes.wake().unwrap();
        drop(engine); // the recovery worker ends with the engine
        let opened = opening.open();
        assert_eq!(lane.recovery_state(), RecoveryState::Stranded);
        drop(opened);
    }

    #[test]
    fn opening_uses_the_reserved_device_or_waits_for_reclamation() {
        use DeviceSource::{Exclusive, Fresh, Reusable};
        use OpenAction::{Open, Reuse, Wait};
        let before = FREE_WITHIN - Duration::from_nanos(1);
        for (port, reclaiming, elapsed, expected) in [
            (Exclusive, true, before, Reuse),
            (Fresh, false, before, Open),
            (Fresh, true, before, Wait),
            (Fresh, true, FREE_WITHIN, Open),
            (Reusable(true), true, before, Reuse),
            (Reusable(true), true, FREE_WITHIN, Reuse),
            (Reusable(false), false, before, Wait),
            (Reusable(false), false, FREE_WITHIN, Open),
        ] {
            assert_eq!(open_action(port, reclaiming, elapsed), expected);
        }
    }

    #[cfg(feature = "test-support")]
    #[expect(
        clippy::unwrap_used,
        reason = "test inputs and stepped wakes must succeed"
    )]
    mod health {
        use super::*;
        use engine_core::health::{HealthSample, press_target};

        // The outcome is selected by the DMX byte that actually reaches the device.
        pub(super) struct ByLevel;

        impl Device for ByLevel {
            fn take(&mut self, frame: &[u8]) -> Outcome {
                match frame.first() {
                    Some(253) => Outcome::Dropped,
                    Some(254) => Outcome::Failed,
                    _ => Outcome::Accepted,
                }
            }
        }

        struct Desk {
            wakes: Wakes,
            engine: Engine,
            sender: Sender,
            start: Duration,
        }

        impl Desk {
            fn new() -> Self {
                Self::with_open(Universes::ONE, Box::new(outputs::open))
            }

            fn with_open(universes: Universes, open: Open) -> Self {
                let (engine, sender, wakes) = Engine::start_stepped(universes, open).unwrap();
                wakes.hold().unwrap();
                let start = wakes.wake().unwrap();
                Self {
                    wakes,
                    engine,
                    sender,
                    start,
                }
            }

            fn attach(&mut self, number: u8, rate: Rate) {
                let run = RunNumbers { first: 1, count: 1 }
                    .check(Universes::ONE)
                    .unwrap();
                self.sender
                    .attach(number.try_into().unwrap(), Box::new(ByLevel), run, rate)
                    .unwrap();
            }

            fn press(&mut self, level: u8) {
                self.sender
                    .send(Command::SetChannel {
                        channel: ChannelNumbers {
                            universe: 1,
                            address: 1,
                        },
                        value: level,
                    })
                    .unwrap();
            }

            // Keep the simulated time still until the tick has received every result.
            fn frames(&mut self, at: Duration, count: usize) {
                let started = Instant::now();
                let mut came = 0;
                while came < count {
                    assert!(
                        started.elapsed() < Duration::from_secs(10),
                        "{came} of {count} frames came back"
                    );
                    self.wakes.wake_at(at).unwrap();
                    came += self
                        .wakes
                        .events()
                        .iter()
                        .filter(|e| matches!(e, Event::Came { .. }))
                        .count();
                }
                assert_eq!(came, count);
            }

            fn sample(&mut self, at: Duration, number: u8) -> HealthSample {
                self.engine.latest(); // ask for the next wake
                self.wakes.wake_at(at).unwrap();
                let mut sample = HealthSample::new(Duration::MAX, u64::MAX, Status::NotFound);
                assert!(
                    self.engine
                        .latest()
                        .health_sample(number.try_into().unwrap(), &mut sample)
                );
                sample
            }
        }

        #[test]
        fn an_accepted_frame_counts_just_its_prefix_on_its_output() {
            let mut desk = Desk::new();
            let start = desk.start;
            desk.attach(1, Rate::Thirty);
            desk.attach(2, Rate::Sixty);
            desk.frames(start, 2);
            let slow = desk.sample(start, 1);
            let fast = desk.sample(start, 2);

            let at = start + Rate::Sixty.period();
            let latency = Duration::from_millis(3);
            desk.wakes.set_time(at - latency);
            desk.press(91);
            desk.wakes.wake_at(at).unwrap(); // the frame carrying 91 is now in the lane
            desk.press(17); // applied before that frame's result is collected
            desk.frames(at, 1);
            for (number, mut expected, added) in [(1, slow, 0), (2, fast, 1)] {
                *expected
                    .press
                    .get_mut(bucket::<LATENCY_BUCKETS>(latency))
                    .unwrap() += added;
                let sample = desk.sample(at, number);
                assert_eq!(sample.press, expected.press, "output {number}");
                assert_eq!(sample.press_worst, latency * added as u32);
                assert_eq!(
                    sample.press_under_target,
                    expected.press_under_target + added
                );
            }
        }

        #[test]
        fn attaching_another_output_does_not_count_as_a_press() {
            let mut desk = Desk::new();
            let rate = Rate::Sixty;
            let start = desk.start;
            desk.attach(1, rate);
            desk.frames(start, 1);
            let first = desk.sample(start, 1);
            assert_eq!(
                (first.press.iter().sum::<u64>(), first.press_under_target),
                (0, 0)
            );
            assert_eq!(
                desk.engine.latest().send_to_frame_p95(),
                Some(Duration::ZERO)
            );

            let due = start + rate.period();
            desk.wakes.set_time(due - Duration::from_millis(3));
            desk.press(91);
            desk.attach(2, rate);
            desk.frames(due, 2);

            for (output, presses) in [(1, 1), (2, 0)] {
                let sample = desk.sample(due, output);
                assert_eq!(
                    (sample.press.iter().sum::<u64>(), sample.press_under_target),
                    (presses, presses),
                    "output {output}"
                );
            }
        }

        #[test]
        fn removing_another_output_does_not_count_as_a_press() {
            let mut desk = Desk::new();
            let rate = Rate::Sixty;
            let start = desk.start;
            desk.attach(1, rate);
            desk.attach(2, rate);
            desk.frames(start, 2);

            let due = start + rate.period();
            desk.wakes.set_time(due - Duration::from_millis(3));
            desk.sender
                .send(Command::RemoveOutput {
                    output: 2.try_into().unwrap(),
                })
                .unwrap();
            desk.frames(due, 1);
            let sample = desk.sample(due, 1);
            assert_eq!(
                (sample.press.iter().sum::<u64>(), sample.press_under_target),
                (0, 0)
            );
        }

        #[test]
        fn attaching_another_output_is_not_a_press() {
            struct AcceptLevel(u8);
            impl Device for AcceptLevel {
                fn take(&mut self, frame: &[u8]) -> Outcome {
                    if frame.first() == Some(&self.0) {
                        Outcome::Accepted
                    } else {
                        Outcome::Failed
                    }
                }
            }

            let universes = Universes::new(2).unwrap();
            let mut desk = Desk::with_open(universes, Box::new(outputs::open));
            let start = desk.start;
            let rate = Rate::Sixty;
            let attach = |desk: &mut Desk, number: u8, level| {
                let run = RunNumbers {
                    first: u16::from(number),
                    count: 1,
                }
                .check(universes)
                .unwrap();
                desk.sender
                    .attach(
                        number.try_into().unwrap(),
                        Box::new(AcceptLevel(level)),
                        run,
                        rate,
                    )
                    .unwrap();
            };
            attach(&mut desk, 1, 91);
            desk.press(91);
            desk.frames(start, 1);
            let before = desk.sample(start, 1);
            assert_eq!(
                (before.press.iter().sum::<u64>(), before.press_under_target),
                (1, 1)
            );

            let due = start + rate.period();
            desk.wakes.set_time(due - Duration::from_millis(3));
            attach(&mut desk, 2, 0);
            desk.frames(due, 2);
            let running = desk.sample(due, 1);
            let new = desk.sample(due, 2);
            assert_eq!(
                (running.status, new.status),
                (Status::Running, Status::Connecting)
            );
            assert_eq!(
                (new.press.iter().sum::<u64>(), new.press_under_target),
                (0, 0)
            );
            assert_eq!(
                (
                    running.press,
                    running.press_worst,
                    running.press_under_target
                ),
                (before.press, before.press_worst, before.press_under_target)
            );
        }

        #[test]
        fn dropped_and_failed_frames_leave_the_press_for_the_next_acceptance() {
            let rate = Rate::Sixty;
            for (level, dropped, failed) in [(253, 1, 0), (254, 0, 1)] {
                let mut desk = Desk::new();
                let start = desk.start;
                desk.attach(1, rate);
                desk.frames(start, 1);
                let mut expected = desk.sample(start, 1).press;

                let due = start + rate.period();
                let sent = due - Duration::from_millis(3);
                desk.wakes.set_time(sent);
                desk.press(level);
                desk.frames(due, 1);
                let sample = desk.sample(due, 1);
                assert_eq!(sample.press, expected);
                assert_eq!((sample.dropped, sample.failed), (dropped, failed));
                assert_eq!(sample.frames_handed, 2, "level {level}");

                let next = due + rate.period();
                desk.wakes.set_time(next);
                desk.press(42);
                desk.frames(next, 1);
                for time in [next - sent, Duration::ZERO] {
                    *expected.get_mut(bucket::<LATENCY_BUCKETS>(time)).unwrap() += 1;
                }
                let sample = desk.sample(next, 1);
                assert_eq!(sample.press, expected, "level {level}");
                assert_eq!(sample.press_worst, next - sent);
            }
        }

        #[test]
        fn exact_press_target_and_existing_frame_counters_reach_the_snapshot() {
            for rate in [Rate::Thirty, Rate::FortyFour, Rate::Sixty] {
                let mut desk = Desk::new();
                let start = desk.start;
                desk.attach(1, rate);
                desk.frames(start, 1);
                let target = press_target(rate);
                let late = Duration::from_millis(7);
                let at = start + rate.period() + late;
                let ns = Duration::from_nanos(1);
                let mut expected = HealthSample::new(at, 1, Status::Running);
                for latency in [target + ns, target, target - ns] {
                    desk.wakes.set_time(at - latency);
                    desk.press(90);
                    *expected
                        .press
                        .get_mut(bucket::<LATENCY_BUCKETS>(latency))
                        .unwrap() += 1;
                }
                desk.frames(at, 1);
                // The attach is no press: only target minus one nanosecond is under it.
                expected.press_under_target = 1;
                expected.press_worst = target + ns;
                expected.frames_handed = 2;
                expected.off_beat = 1;
                expected.lateness_worst = late;
                for latency in [Duration::ZERO, late] {
                    *expected
                        .lateness
                        .get_mut(bucket::<LATENESS_BUCKETS>(latency))
                        .unwrap() += 1;
                }
                assert_eq!(desk.sample(at, 1), expected, "{rate:?}");
            }
        }

        #[test]
        fn only_entries_into_reconnecting_count_as_outages() {
            let mut desk = Desk::new();
            let mut at = desk.start;
            let rate = Rate::Sixty;
            desk.attach(1, rate);
            for (level, status, outages) in [
                (0, Status::Connecting, 0),
                (254, Status::Reconnecting { failures: 1 }, 1),
                (254, Status::Reconnecting { failures: 2 }, 1),
                (91, Status::Running, 1),
                (254, Status::Reconnecting { failures: 1 }, 2),
                (254, Status::Reconnecting { failures: 2 }, 2),
                (91, Status::Running, 2),
            ] {
                desk.wakes.set_time(at);
                desk.press(level);
                desk.frames(at, 1);
                let sample = desk.sample(at, 1);
                assert_eq!((sample.status, sample.outages), (status, outages));
                at += rate.period();
            }
        }

        #[test]
        fn a_returned_failure_counts_an_outage_only_in_its_own_generation() {
            #[derive(Clone, Default)]
            struct HoldFirst {
                entered: Arc<AtomicBool>,
                release: Arc<AtomicBool>,
            }

            impl Device for HoldFirst {
                fn take(&mut self, frame: &[u8]) -> Outcome {
                    if frame.first() == Some(&91) {
                        self.entered.store(true, Ordering::Release);
                        let began = Instant::now();
                        while !self.release.load(Ordering::Acquire) {
                            if began.elapsed() > Duration::from_secs(10) {
                                return Outcome::Failed;
                            }
                            thread::yield_now();
                        }
                    }
                    ByLevel.take(frame)
                }
            }

            let rate = outputs::default_rate(engine_core::TEST_KIND);
            let set_output = |rate| Command::SetOutput {
                output: 1.try_into().unwrap(),
                kind: engine_core::TEST_KIND,
                universes: RunNumbers { first: 1, count: 1 },
                rate,
            };
            for (reconfigure, generation, outages) in [(true, 2, 0), (false, 1, 1)] {
                let gate = HoldFirst::default();
                let device = gate.clone();
                let mut desk = Desk::with_open(
                    Universes::new(2).unwrap(),
                    Box::new(move |_| Ok(Box::new(device.clone()))),
                );
                desk.sender.send(set_output(None)).unwrap();
                desk.press(91);
                desk.wakes.wake_at(desk.start).unwrap();
                let began = Instant::now();
                while !gate.entered.load(Ordering::Acquire) {
                    assert!(began.elapsed() < Duration::from_secs(10), "first frame");
                    thread::yield_now();
                }

                // Queue the failure behind the held acceptance. Both return in order
                // after the optional reconfigure: NotFound -> Connecting -> Reconnecting.
                let due = desk.start + rate.period();
                desk.press(254);
                desk.wakes.wake_at(due).unwrap();
                if reconfigure {
                    desk.sender.send(set_output(Some(rate))).unwrap();
                }
                let before = desk.sample(due, 1);
                assert_eq!(
                    (before.generation, before.status),
                    (generation, Status::NotFound)
                );
                gate.release.store(true, Ordering::Release);
                desk.frames(due, 2);
                let sample = desk.sample(due, 1);
                assert_eq!(
                    (sample.status, sample.failed, sample.outages),
                    (Status::Reconnecting { failures: 1 }, 1, outages),
                    "generation {generation}"
                );
            }
        }

        #[test]
        fn a_frame_returned_after_reconfigure_adds_no_press_or_outage() {
            let rate = Rate::Sixty;
            for (old_level, status, failures) in
                [(91, Status::Connecting, 0), (254, Status::NotFound, 1)]
            {
                let mut desk = Desk::new();
                let start = desk.start;
                desk.attach(1, rate);
                desk.frames(start, 1);
                desk.press(254);
                let failed = start + rate.period();
                desk.frames(failed, 1); // an outage in the old generation
                desk.press(old_level);
                let old = failed + rate.period();
                desk.wakes.wake_at(old).unwrap(); // still in flight, or waiting in the return queue
                desk.press(42); // after the old frame's prefix, before re-attach
                // The command prepared by SetOutput when the same kind keeps its lane.
                let base = desk.sender.base();
                let keep = Prepared {
                    project: base.project,
                    base,
                    change: Change::Send(EngineCommand::Attach {
                        output: 1.try_into().unwrap(),
                        universes: RunNumbers { first: 1, count: 1 }
                            .check(Universes::ONE)
                            .unwrap(),
                        rate,
                    }),
                };
                desk.sender.admit(keep).unwrap();
                desk.press(7); // the new generation's first press
                desk.frames(old, 1);
                let sample = desk.sample(old, 1);
                assert_eq!(sample.generation, 2);
                // Status, counters and lateness take the old frame as before.
                assert_eq!(sample.status, status);
                assert_eq!(
                    (sample.lateness.iter().sum::<u64>(), sample.failed),
                    (1, failures)
                );
                assert_eq!(sample.press.iter().sum::<u64>(), 0);
                assert_eq!(sample.press_worst, Duration::ZERO);
                assert_eq!(
                    (
                        sample.press_under_target,
                        sample.press_missing,
                        sample.outages
                    ),
                    (0, 0, 0)
                );

                let next = old + rate.period();
                desk.frames(next, 1);
                let sample = desk.sample(next, 1);
                assert_eq!(
                    sample.press.iter().sum::<u64>(),
                    1,
                    "only the press after it"
                );
                assert_eq!(sample.press_worst, rate.period());
                assert_eq!(sample.press_under_target, 1);
            }
        }

        #[test]
        fn overwritten_presses_count_as_missing_once_and_reset_on_attach() {
            // Hold the frame carrying 91 while later commands overwrite its prefix.
            struct Hold(Arc<AtomicBool>);
            impl Device for Hold {
                fn take(&mut self, frame: &[u8]) -> Outcome {
                    if frame.first() == Some(&91) {
                        let started = Instant::now();
                        while !self.0.load(Ordering::Acquire) {
                            if started.elapsed() > Duration::from_secs(10) {
                                return Outcome::Failed;
                            }
                            thread::yield_now();
                        }
                    }
                    Outcome::Accepted
                }
            }
            let mut desk = Desk::new();
            let start = desk.start;
            let rate = Rate::Sixty;
            let release = Arc::new(AtomicBool::new(false));
            desk.sender
                .attach(
                    1.try_into().unwrap(),
                    Box::new(Hold(Arc::clone(&release))),
                    RunNumbers { first: 1, count: 1 }
                        .check(Universes::ONE)
                        .unwrap(),
                    rate,
                )
                .unwrap();
            desk.frames(start, 1);
            desk.press(91);
            let held = start + rate.period();
            desk.wakes.wake_at(held).unwrap();
            for press in 1..=CARRY + 3 {
                desk.press(42);
                if press % QUEUE == 0 {
                    desk.wakes.wake_at(held).unwrap();
                }
            }
            desk.wakes.wake_at(held).unwrap();
            release.store(true, Ordering::Release);
            desk.frames(held, 1);
            let early = desk.sample(held, 1);
            assert_eq!(early.press_missing, 1, "only 91 was in the accepted frame");
            assert_eq!(early.press.iter().sum::<u64>(), 0, "the attach is no press");
            let at = held + rate.period();
            desk.frames(at, 1);
            let next = at + rate.period();
            desk.frames(next, 1); // the same prefix a second time
            let sample = desk.sample(next, 1);
            assert_eq!(sample.press_missing, 4);
            assert_eq!(
                sample.press.get(bucket::<LATENCY_BUCKETS>(at - held)),
                Some(&(CARRY as u64))
            );
            assert_eq!(sample.press_under_target, CARRY as u64);
            desk.sender
                .send(Command::RemoveOutput {
                    output: 1.try_into().unwrap(),
                })
                .unwrap();
            desk.attach(1, rate);
            let replacement = desk.sample(next, 1);
            assert_eq!(replacement.generation, 2, "only attaches advance the slot");
            assert_eq!(replacement.press_missing, 0);
            assert_eq!(replacement.press_worst, Duration::ZERO);
        }

        #[test]
        fn presses_are_counted_after_every_due_hand_over() {
            // Counts the frames its device took.
            struct Took(Arc<std::sync::atomic::AtomicUsize>);
            impl Device for Took {
                fn take(&mut self, _: &[u8]) -> Outcome {
                    self.0.fetch_add(1, Ordering::Release);
                    Outcome::Accepted
                }
            }
            let mut desk = Desk::new();
            let rate = Rate::Sixty;
            let took = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            for number in [1, 2] {
                desk.sender
                    .attach(
                        number.try_into().unwrap(),
                        Box::new(Took(Arc::clone(&took))),
                        RunNumbers { first: 1, count: 1 }
                            .check(Universes::ONE)
                            .unwrap(),
                        rate,
                    )
                    .unwrap();
            }
            let mut at = desk.start;
            let mut before = 0;
            for frame in 0..20 {
                // Send and take at the same stepped time: every press goes in bucket zero.
                desk.wakes.set_time(at);
                desk.press(91);
                desk.engine.latest(); // request the snapshot at the end of this wake
                desk.wakes.wake_at(at).unwrap();
                let at_handoff = desk
                    .engine
                    .shared
                    .presses_at_handoff
                    .load(Ordering::Relaxed);
                assert_eq!(at_handoff, before, "frame {frame}");
                let picture = desk.engine.latest();
                before = 0;
                for number in [1, 2] {
                    let output = OutputNumber::try_from(number).unwrap();
                    let mut sample = HealthSample::new(at, 0, Status::NotFound);
                    assert!(picture.health_sample(output, &mut sample));
                    assert_eq!(sample.frames_handed, frame as u64 + 1);
                    before += sample.press.iter().sum::<u64>();
                }
                let events = desk.wakes.events();
                if before > at_handoff {
                    let handed = events
                        .iter()
                        .rposition(|e| matches!(e, Event::Handed { .. }))
                        .unwrap();
                    let counted = events
                        .iter()
                        .position(|e| matches!(e, Event::Counted { .. }))
                        .unwrap();
                    assert!(handed < counted, "frame {frame}: {events:?}");
                }
                // Both devices take this wake's frames, returned to the next wake.
                let started = Instant::now();
                while took.load(Ordering::Acquire) < 2 * (frame + 1) {
                    assert!(started.elapsed() < Duration::from_secs(10), "frame {frame}");
                    thread::yield_now();
                }
                at += rate.period();
            }
            assert!(before > 0, "no wake counted a press");
        }

        #[test]
        fn an_in_flight_frame_counts_only_its_partially_retained_prefix() {
            struct Hold(Arc<AtomicBool>);

            impl Device for Hold {
                fn take(&mut self, frame: &[u8]) -> Outcome {
                    if frame.first() == Some(&91) {
                        let started = Instant::now();
                        while !self.0.load(Ordering::Acquire) {
                            if started.elapsed() > Duration::from_secs(10) {
                                return Outcome::Failed;
                            }
                            thread::yield_now();
                        }
                    }
                    Outcome::Accepted
                }
            }

            let mut desk = Desk::new();
            let start = desk.start;
            let rate = Rate::Sixty;
            let release = Arc::new(AtomicBool::new(false));
            desk.sender
                .attach(
                    1.try_into().unwrap(),
                    Box::new(Hold(Arc::clone(&release))),
                    RunNumbers { first: 1, count: 1 }
                        .check(Universes::ONE)
                        .unwrap(),
                    rate,
                )
                .unwrap();
            desk.frames(start, 1);

            let held = start + rate.period();
            for (level, milliseconds) in [(11, 6), (22, 4), (33, 2), (91, 0)] {
                desk.wakes
                    .set_time(held - Duration::from_millis(milliseconds));
                desk.press(level);
            }
            desk.wakes.wake_at(held).unwrap();
            // The ring now has two stamps from the held frame and CARRY - 2
            // from commands that were applied after that frame.
            for press in 1..=CARRY - 2 {
                desk.press(42);
                if press % QUEUE == 0 {
                    desk.wakes.wake_at(held).unwrap();
                }
            }
            desk.wakes.wake_at(held).unwrap();
            release.store(true, Ordering::Release);
            desk.frames(held, 1);

            let sample = desk.sample(held, 1);
            let mut expected = [0; LATENCY_BUCKETS];
            for milliseconds in [0, 2] {
                *expected
                    .get_mut(bucket::<LATENCY_BUCKETS>(Duration::from_millis(
                        milliseconds,
                    )))
                    .unwrap() += 1;
            }
            assert_eq!(sample.press_missing, 2);
            assert_eq!(sample.press, expected);
            assert_eq!(sample.press_worst, Duration::from_millis(2));
            assert_eq!(sample.press_under_target, 2);
        }

        #[test]
        fn same_device_reconfigure_preserves_existing_return_accounting() {
            let number = OutputNumber::try_from(1).unwrap();
            let rate = outputs::default_rate(engine_core::TEST_KIND);
            let set_output = |rate| Command::SetOutput {
                output: number,
                kind: engine_core::TEST_KIND,
                universes: RunNumbers { first: 1, count: 1 },
                rate,
            };
            let mut samples = Vec::new();
            let mut expected = Vec::new();
            for (level, outcome, status, counts) in [
                (91, Outcome::Accepted, Status::Connecting, (1, 0, 0)),
                (253, Outcome::Dropped, Status::NotFound, (0, 1, 0)),
                (254, Outcome::Failed, Status::NotFound, (0, 0, 1)),
            ] {
                let (mut engine, mut sender, mut wakes) = Engine::start_stepped(
                    Universes::new(2).unwrap(),
                    Box::new(|_| Ok(Box::new(ByLevel))),
                )
                .unwrap();
                wakes.hold().unwrap();
                let start = wakes.wake().unwrap();
                sender.send(set_output(None)).unwrap();

                let wait_for_return = |engine: &mut Engine, wakes: &mut Wakes, at, due, outcome| {
                    let began = Instant::now();
                    loop {
                        engine.latest();
                        wakes.wake_at(at).unwrap();
                        if wakes.events().contains(&Event::Came {
                            output: number,
                            outcome,
                            due,
                        }) {
                            break;
                        }
                        assert!(began.elapsed() < Duration::from_secs(10), "frame {due:?}");
                    }
                };
                wait_for_return(&mut engine, &mut wakes, start, start, Outcome::Accepted);
                sender
                    .send(Command::SetChannel {
                        channel: ChannelNumbers {
                            universe: 1,
                            address: 1,
                        },
                        value: level,
                    })
                    .unwrap();
                let due = start + rate.period();
                let late = Duration::from_millis(4);
                let at = due + late;
                wakes.wake_at(at).unwrap();
                sender.send(set_output(Some(rate))).unwrap(); // keeps the lane: a new generation

                wait_for_return(&mut engine, &mut wakes, at, due, outcome);
                let snapshot = engine.latest();
                let output = snapshot.state.outputs.get(number.slot()).unwrap().unwrap();
                assert_eq!(
                    output.universes,
                    RunNumbers { first: 1, count: 1 }
                        .check(Universes::new(2).unwrap())
                        .unwrap()
                );
                let counters = output.counters;
                samples.push((
                    level,
                    output.status,
                    (counters.accepted, counters.dropped, counters.failed),
                    snapshot.lateness(number).map(|lateness| lateness.worst),
                ));
                expected.push((level, status, counts, Some(late)));
            }
            assert_eq!(samples, expected);
        }
    }

    #[cfg(feature = "test-support")]
    #[test]
    #[expect(
        clippy::unwrap_used,
        reason = "a missing frame or command fails this test"
    )]
    fn scorecard_stamps_the_admission_and_the_device_handoff() {
        struct Gated {
            // Each frame carries a different first byte. Hold its take until
            // the driver permits it to finish, including during a later wake.
            byte: Arc<AtomicU64>,
            permits: Arc<AtomicU64>,
            calls: u64,
        }
        impl Device for Gated {
            fn take(&mut self, frame: &[u8]) -> Outcome {
                self.calls += 1;
                let level = frame.first().copied().unwrap();
                self.byte.store(u64::from(level), Ordering::Release);
                let started = Instant::now();
                while self.permits.load(Ordering::Acquire) < self.calls {
                    if started.elapsed() > Duration::from_secs(10) {
                        return Outcome::Failed;
                    }
                    thread::yield_now();
                }
                Outcome::Accepted
            }
        }

        let (clock, trace, wakes) = wake::stepped();
        let (writer, mut timings) = snapshot::timing::TimingWriter::new(clock.clone());
        let (mut engine, mut sender) = Engine::start_on(
            Universes::ONE,
            Box::new(outputs::open),
            clock,
            trace,
            Some(writer),
        )
        .unwrap();
        wakes.hold().unwrap();
        let byte = Arc::new(AtomicU64::new(u64::MAX));
        let permits = Arc::new(AtomicU64::new(0));
        let heard = |value| {
            let started = Instant::now();
            while byte.load(Ordering::Acquire) != value {
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "device never saw {value}"
                );
                thread::yield_now();
            }
        };
        let device = Gated {
            byte: byte.clone(),
            permits: permits.clone(),
            calls: 0,
        };
        let output = OutputNumber::try_from(1).unwrap();
        let run = RunNumbers { first: 1, count: 1 }
            .check(Universes::ONE)
            .unwrap();
        let rate = Rate::Sixty;
        let first = timings.now() + Duration::from_secs(1);
        wakes.set_time(first);
        sender.attach(output, Box::new(device), run, rate).unwrap();
        wakes.wake_at(first).unwrap();
        heard(0);
        let due = first + rate.period();
        let send = |sender: &mut Sender, value| {
            sender
                .send(Command::SetChannel {
                    channel: ChannelNumbers {
                        universe: 1,
                        address: 1,
                    },
                    value,
                })
                .unwrap()
        };

        let admitted = first + Duration::from_millis(3);
        wakes.set_time(admitted);
        send(&mut sender, 91);
        assert_eq!(wakes.cut().unwrap(), due);
        let after_cut = due + Duration::from_millis(1);
        wakes.set_time(after_cut);
        send(&mut sender, 17);
        wakes.finish().unwrap();

        // Frame two waits behind the first device call. Its own device
        // handoff, completion and the next tick poll each have a distinct stamp.
        let handed = due + Duration::from_millis(5);
        wakes.set_time(handed);
        permits.store(1, Ordering::Release);
        heard(91);
        wakes.set_time(due + Duration::from_millis(8));
        permits.store(u64::MAX, Ordering::Release);
        let poll = due + rate.period();
        wakes.wake_at(poll).unwrap();
        heard(17); // frame two's result has been returned before this take
        engine.latest();
        wakes.wake_at(poll).unwrap();
        let mut health = engine_core::health::HealthSample::new(poll, 0, Status::NotFound);
        assert!(engine.latest().health_sample(output, &mut health));
        let latency = handed - admitted;
        assert_eq!(
            health.press.get(bucket::<LATENCY_BUCKETS>(latency)),
            Some(&1)
        );
        assert_eq!(health.press_worst, latency);

        let (mut commands, mut due_frames, mut finished) = (vec![], vec![], vec![]);
        while let Some(event) = timings.pop().unwrap() {
            match event {
                Timing::Applied { number, at } => commands.push((number, at)),
                Timing::Due {
                    output,
                    due,
                    applied,
                } => due_frames.push((output, due, applied)),
                Timing::Finished {
                    output,
                    due,
                    at,
                    outcome,
                } if due < poll => {
                    finished.push((output, due, at, outcome));
                }
                Timing::Finished { .. } => {}
            }
        }
        assert_eq!(commands, [(1, first), (2, admitted), (3, after_cut)]);
        assert_eq!(
            due_frames,
            [(output, first, 1), (output, due, 2), (output, poll, 3)]
        );
        assert_eq!(
            finished,
            [
                (output, first, first, Outcome::Accepted),
                (output, due, handed, Outcome::Accepted),
            ]
        );
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
    fn each_output_counts_a_command_when_its_own_frame_carries_it() {
        let ms = Duration::from_millis;
        #[derive(Clone, Copy, Debug)]
        enum Step {
            Attach(usize),
            /// The `Attach` command sent at this time is applied, then the output's lane is fresh.
            Attached(u8, u64),
            Sent(u64),
            Handed(usize, u64),
        }
        use Step::{Attach, Attached, Handed, Sent};
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
            // every command counts on a running output, a fresh one nothing from before it
            (
                vec![
                    Attached(0, 0),
                    Sent(10),
                    Attached(1, 11),
                    Handed(1, 12),
                    Handed(0, 13),
                ],
                vec![3, 2],
            ),
        ];
        let run = RunNumbers { first: 1, count: 1 }
            .check(Universes::ONE)
            .unwrap();
        for (steps, expected) in cases {
            let mut carried = Carried::new();
            let mut counted = vec![];
            for step in &steps {
                match *step {
                    Attach(output) => carried.attached(output, false),
                    Attached(slot, at) => {
                        let attach = EngineCommand::Attach {
                            output: OutputNumber::try_from(slot + 1).unwrap(),
                            universes: run,
                            rate: Rate::Sixty,
                        };
                        carried.applied(&attach, ms(at));
                        carried.attached(usize::from(slot), false);
                    }
                    Sent(at) => carried.applied(&EngineCommand::Tap, ms(at)),
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

    #[test]
    #[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
    fn a_command_is_a_press_unless_it_only_manages_outputs() {
        let output = OutputNumber::try_from(1).unwrap();
        let universes = RunNumbers { first: 1, count: 1 }
            .check(Universes::ONE)
            .unwrap();
        let channel = ChannelNumbers {
            universe: 1,
            address: 1,
        }
        .check(Universes::ONE)
        .unwrap();
        let lights = engine_core::LightNumber::new(0, LIGHTS)
            .and_then(|first| LightRun::new(first, 1, LIGHTS))
            .unwrap();
        let change = LightChange::Grab {
            value: Value::Intensity(1),
            grab: GrabId { generation: 1 },
        };
        let play = PlayId::new(0, 1).unwrap();
        // (command, presses it adds)
        let cases = [
            (EngineCommand::SetChannel { channel, value: 1 }, 1),
            (EngineCommand::ReleaseChannel { channel }, 1),
            (EngineCommand::ReleaseChannels, 1),
            (EngineCommand::ReleaseAll, 1),
            (EngineCommand::Blackout { on: true }, 1),
            (EngineCommand::Writers, 1),
            (EngineCommand::Lights { lights, change }, 1),
            (EngineCommand::Darken { lights }, 1),
            (EngineCommand::Runs { change }, 1),
            (
                EngineCommand::Start {
                    play,
                    look: 0,
                    rank: Rank::Manual,
                    when: When::default(),
                },
                1,
            ),
            (EngineCommand::Keep { play }, 1),
            (
                EngineCommand::Rules {
                    rules: MixRules::default(),
                },
                1,
            ),
            (EngineCommand::Plan { play }, 1),
            (EngineCommand::Release { play }, 1),
            (EngineCommand::Tap, 1),
            (EngineCommand::ReleaseTempo, 1),
            (
                EngineCommand::Feed {
                    bpm: Bpm::new(120).unwrap(),
                },
                1,
            ),
            (
                EngineCommand::Attach {
                    output,
                    universes,
                    rate: Rate::Sixty,
                },
                0,
            ),
            (EngineCommand::RemoveOutput { output }, 0),
        ];
        for (command, presses) in cases {
            let mut carried = Carried::new();
            carried.applied(&command, Duration::ZERO);
            assert_eq!(
                (carried.commands.total, carried.presses.total),
                (1, presses),
                "{command:?}"
            );
        }
    }

    /// Sends `command` until it is not refused for a full command queue: waits for the
    /// tick to drain it. What came of it once the queue had room.
    #[expect(clippy::unwrap_used, reason = "a test whose tick failed fails")]
    fn once_drained(
        engine: &mut Engine,
        sender: &mut Sender,
        command: Command,
    ) -> Result<(), Unsent> {
        loop {
            let refused = sender.send(command);
            if refused != Err(Unsent::Refused(Refusal::QueueFull)) {
                return refused;
            }
            let started = Instant::now();
            while engine.fresh().unwrap().applied() < sender.sent() {
                assert!(
                    started.elapsed() < Duration::from_secs(5),
                    "the tick drains"
                );
            }
            if sender.commands.slots() == QUEUE {
                return sender.send(command); // drained: the retirement queue decides
            }
        }
    }

    /// The first nine channels of what an attached device was handed last.
    struct Heard {
        frames: AtomicU64,
        levels: [std::sync::atomic::AtomicU8; 9],
    }

    struct Hears(Arc<Heard>);

    impl Device for Hears {
        fn take(&mut self, frame: &[u8]) -> Outcome {
            for (cell, level) in self.0.levels.iter().zip(frame) {
                cell.store(*level, Ordering::Relaxed);
            }
            self.0.frames.fetch_add(1, Ordering::Release);
            Outcome::Accepted
        }
    }

    impl Heard {
        /// The channels of a frame made after every command sent so far was applied.
        #[expect(clippy::unwrap_used, reason = "a test whose tick failed fails")]
        fn after(&self, engine: &mut Engine, sender: &Sender) -> [u8; 9] {
            let started = Instant::now();
            while engine.fresh().unwrap().applied() < sender.sent() {
                assert!(
                    started.elapsed() < Duration::from_secs(5),
                    "the tick applies"
                );
            }
            let frames = self.frames.load(Ordering::Acquire);
            // the frame being made as it was applied may be handed next: two on
            while self.frames.load(Ordering::Acquire) < frames + 2 {
                assert!(started.elapsed() < Duration::from_secs(5), "frames go out");
                thread::sleep(Duration::from_millis(1)); // poll
            }
            self.levels
                .each_ref()
                .map(|level| level.load(Ordering::Relaxed))
        }
    }

    /// A gesture that lets go of what is held, sent with every play slot taken and no
    /// return free.
    #[derive(Clone, Copy, Debug)]
    enum Gesture {
        ReleaseHold,
        StopLook,
        EndFlash,
        ReleaseProperty(Target),
        EndGrab,
        ReleaseAll,
    }

    #[test]
    #[expect(
        clippy::unwrap_used,
        clippy::panic,
        clippy::too_many_lines,
        reason = "a test that cannot set itself up fails; one table"
    )]
    fn with_reclamation_stalled_every_release_needs_only_a_place_in_the_queue() {
        // a strip of three pixels: every play red on the middle one, a grab green on the
        // grabbed lights
        let one = Target::Light { light: 1 };
        let ends = Target::Group {
            group: Text::new("ends").unwrap(),
        };
        let (red, green, dark) = ([255, 0, 0], [0, 255, 0], [0, 0, 0]);
        let frame = |pixels: [[u8; 3]; 3]| -> [u8; 9] { pixels.concat().try_into().unwrap() };
        // (Manual end, grabbed, gesture, DMX after, plays after)
        let cases = [
            (
                ManualEnd::UntilRelease,
                one,
                Gesture::ReleaseHold,
                frame([green, dark, dark]),
                0,
            ),
            (
                ManualEnd::UntilRelease,
                one,
                Gesture::StopLook,
                frame([green, red, dark]),
                PLAYS,
            ),
            (
                ManualEnd::UntilStop,
                one,
                Gesture::StopLook,
                frame([green, dark, dark]),
                0,
            ),
            (
                ManualEnd::UntilRelease,
                one,
                Gesture::EndFlash,
                frame([green, dark, dark]),
                0,
            ),
            (
                ManualEnd::UntilRelease,
                one,
                Gesture::ReleaseProperty(one),
                frame([dark, red, dark]),
                PLAYS,
            ),
            (
                ManualEnd::UntilRelease,
                ends,
                Gesture::ReleaseProperty(ends),
                frame([dark, red, dark]),
                PLAYS,
            ),
            (
                ManualEnd::UntilStop,
                one,
                Gesture::EndGrab,
                frame([dark, red, dark]),
                PLAYS,
            ),
            (
                ManualEnd::UntilStop,
                ends,
                Gesture::EndGrab,
                frame([dark, red, dark]),
                PLAYS,
            ),
            (
                ManualEnd::UntilRelease,
                ends,
                Gesture::ReleaseAll,
                frame([dark, dark, dark]),
                0,
            ),
        ];
        for (manual_end, grabbed, gesture, expected, plays_after) in cases {
            let case = format!("{manual_end:?} {grabbed:?} {gesture:?}");
            let (mut engine, mut sender) = Engine::start(Universes::new(1).unwrap()).unwrap();
            let heard = Arc::new(Heard {
                frames: AtomicU64::new(0),
                levels: Default::default(),
            });
            let run = RunNumbers { first: 1, count: 1 }
                .check(sender.universes)
                .unwrap();
            let output = OutputNumber::try_from(1).unwrap();
            sender
                .attach(
                    output,
                    Box::new(Hears(Arc::clone(&heard))),
                    run,
                    Rate::Sixty,
                )
                .unwrap();
            let strip = Command::PatchStrip {
                name: Text::new("strip").unwrap(),
                pixels: engine_core::Pixels::try_from(3).unwrap(),
                color: Color::Rgb,
                at: ChannelNumbers {
                    universe: 1,
                    address: 1,
                },
            };
            assert!(sender.begin(strip, &[]).is_ok(), "{case}");
            let group = Command::SetGroup {
                name: Text::new("ends").unwrap(),
                members: MemberList::Fits(
                    Members::new(&[Member::Light { light: 1 }, Member::Light { light: 3 }])
                        .unwrap(),
                ),
            };
            sender.send(group).unwrap();
            let rules = MixRules {
                manual_end,
                ..MixRules::default()
            };
            sender.send(Command::SetMixRules { rules }).unwrap();
            let full = Command::SetIntensity {
                target: Target::Fixture { fixture: 1 },
                intensity: u16::MAX,
            };
            sender.send(full).unwrap();
            let step = engine_core::Step {
                target: Target::Light { light: 2 },
                values: Light {
                    colour: Some(Rgb {
                        red: u16::MAX,
                        ..Rgb::BLACK
                    }),
                    ..Light::DARK
                },
            };
            let mut plays = Vec::new();
            for look in 1..=PLAYS as u16 {
                let name = Text::new("s").unwrap();
                sender.send(Command::PutStep { look, name, step }).unwrap();
                let start = Command::StartLook {
                    look,
                    rank: Rank::Manual,
                    when: When::Now,
                };
                match sender.begin(start, &[]) {
                    Ok(Begun::Started { play }) => plays.push(play),
                    _ => panic!("look {look} plays"),
                }
            }
            let grab = Command::SetColour {
                target: grabbed,
                red: 0,
                green: u16::MAX,
                blue: 0,
            };
            let Ok(Begun::Grabbed { grab }) = sender.begin(grab, &[]) else {
                panic!("{case}: grabbed");
            };
            let held = if matches!(grabbed, Target::Group { .. }) {
                frame([green, red, green])
            } else {
                frame([green, red, dark])
            };
            assert_eq!(heard.after(&mut engine, &sender), held, "{case}: before");
            engine.shared.stalled.store(true, Ordering::Release);
            // restarts send old plans back that are never freed, until no more may go
            let restart = Command::StartLook {
                look: 1,
                rank: Rank::Manual,
                when: When::Now,
            };
            while once_drained(&mut engine, &mut sender, restart).is_ok() {}
            assert_eq!(
                sender.send(restart),
                Err(Unsent::Refused(Refusal::QueueFull)),
                "{case}: the returns are full"
            );
            *plays.first_mut().unwrap() = PlayId::new(0, sender.generation).unwrap();
            let commands: Vec<Command> = match gesture {
                Gesture::ReleaseHold => plays
                    .iter()
                    .map(|&play| Command::ReleaseHold { play })
                    .collect(),
                Gesture::StopLook => plays
                    .iter()
                    .map(|&play| Command::StopLook { play })
                    .collect(),
                Gesture::EndFlash => plays
                    .iter()
                    .map(|&play| Command::EndFlash { play })
                    .collect(),
                Gesture::ReleaseProperty(target) => vec![Command::ReleaseProperty {
                    target,
                    property: Property::Colour,
                }],
                Gesture::EndGrab => vec![Command::EndGrab { grab }],
                Gesture::ReleaseAll => vec![Command::ReleaseAll],
            };
            for command in commands {
                assert_eq!(
                    once_drained(&mut engine, &mut sender, command),
                    Ok(()),
                    "{case}: {command:?}"
                );
            }
            assert_eq!(heard.after(&mut engine, &sender), expected, "{case}: after");
            let state = &engine.fresh().unwrap().state;
            let left = (0..PLAYS).filter(|&slot| state.plays.has(slot)).count();
            assert_eq!(left, plays_after, "{case}: plays");
            engine.shared.stalled.store(false, Ordering::Release);
        }
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
    fn a_play_started_as_the_generations_wrap_is_never_taken_for_a_cancelled_one() {
        let (mut engine, mut sender) = Engine::start(Universes::new(1).unwrap()).unwrap();
        let strip = Command::PatchStrip {
            name: Text::new("strip").unwrap(),
            pixels: engine_core::Pixels::try_from(1).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        };
        assert!(sender.begin(strip, &[]).is_ok());
        let step = engine_core::Step {
            target: Target::Light { light: 1 },
            values: Light::DARK,
        };
        let name = Text::new("s").unwrap();
        sender
            .send(Command::PutStep {
                look: 1,
                name,
                step,
            })
            .unwrap();
        // the last generation before the counter wraps
        sender.generation = u32::MAX;
        let start = Command::StartLook {
            look: 1,
            rank: Rank::Manual,
            when: When::Now,
        };
        assert!(matches!(
            sender.begin(start, &[]),
            Ok(Begun::Started { .. })
        ));
        let started = Instant::now();
        while engine.fresh().unwrap().applied() < sender.sent() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the tick applies"
            );
        }
        assert_eq!(
            sender.send(Command::RemoveLook { look: 1 }),
            Err(Unsent::Refused(Refusal::LookInUse { look: 1 }))
        );
        assert!(engine.fresh().unwrap().state.plays.has(0));
    }

    #[cfg(feature = "test-support")]
    #[test]
    #[expect(
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        reason = "an invalid fixture, missing play or missing frame fails this stepped test"
    )]
    fn linked_plans_at_the_fifo_cutoff() {
        use engine_core::{Repeat, Sequence, SequenceItem, Step, Use};

        let (mut engine, mut sender, wakes) =
            Engine::start_stepped(Universes::new(1).unwrap(), Box::new(outputs::open)).unwrap();
        wakes.hold().unwrap();
        let name = Text::new("sequence").unwrap();
        sender
            .send(Command::PatchStrip {
                name,
                pixels: engine_core::Pixels::try_from(1).unwrap(),
                color: Color::Rgb,
                at: ChannelNumbers {
                    universe: 1,
                    address: 1,
                },
            })
            .unwrap();
        let put = |look, intensity| Command::PutStep {
            look,
            name,
            step: Step {
                target: Target::Light { light: 1 },
                values: Light {
                    intensity,
                    ..Light::DARK
                },
            },
        };
        let sequence = |look, children: &[u16]| Command::PutSequence {
            look,
            name,
            sequence: Sequence::new(
                &children
                    .iter()
                    .map(|&child| SequenceItem {
                        look: child,
                        width: 16,
                        ..Default::default()
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        };
        let start = |look, rank, repeat| Command::StartUse {
            look,
            rank,
            when: When::Now,
            usage: Use {
                repeat,
                ..Default::default()
            },
        };
        sender
            .send(Command::SetInternalTempo {
                bpm: Bpm::new(60).unwrap(),
            })
            .unwrap();
        sender.send(put(1, Some(123))).unwrap();
        sender.send(put(2, None)).unwrap();
        sender.send(put(3, Some(654))).unwrap();
        for root in 10..10 + PLAYS as u16 {
            sender.send(sequence(root, &[1, 3])).unwrap();
        }
        let begun = |sender: &mut Sender, command| match sender.begin(command, &[]) {
            Ok(Begun::Started { play }) => play,
            _ => panic!("start refused"),
        };
        let time = wakes.wake().unwrap() + Duration::from_secs(1);
        wakes.set_time(time);
        let first = begun(&mut sender, start(10, Rank::Show, Repeat::Once));
        sender.send(put(1, Some(456))).unwrap();
        let mut ids = vec![first];
        for root in 11..10 + PLAYS as u16 {
            ids.push(begun(&mut sender, start(root, Rank::Manual, Repeat::Once)));
        }
        // Ask before each wake so latest() receives that exact frame.
        let picture = |engine: &mut Engine, at| {
            engine.latest();
            wakes.wake_at(at).unwrap();
            let snap = engine.latest();
            (
                snap.state.plays.iter().copied().collect::<Vec<_>>(),
                snap.lights()[0].intensity,
            )
        };
        let (plays, intensity) = picture(&mut engine, time);
        assert_eq!(plays.len(), PLAYS);
        assert_eq!(intensity, Some(456));
        assert_eq!(plays[0].play, first);
        // All ends preceded the edit's timestamp, with no wake at the end.
        // The queued plans must be returned even for a removed or already-kept slot.
        let end = time + Duration::from_secs(3);
        wakes.set_time(end);
        sender.send(put(3, Some(789))).unwrap();
        let (plays, intensity) = picture(&mut engine, end);
        assert_eq!(intensity, Some(654));
        assert_eq!(plays.len(), PLAYS - 1);
        assert!(
            plays
                .iter()
                .all(|p| p.state == PlayState::Kept && ids.contains(&p.play))
        );
        sender.send(Command::RemoveLook { look: 10 }).unwrap();
        assert_eq!(
            sender.send(Command::RemoveLook { look: 11 }),
            Err(Unsent::Refused(Refusal::LookInUse { look: 11 }))
        );
        for play in ids {
            sender.send(Command::ReleaseHold { play }).unwrap();
        }
        sender.send(sequence(11, &[1, 2])).unwrap();
        let play = begun(&mut sender, start(11, Rank::Manual, Repeat::Loop));
        assert_eq!(picture(&mut engine, end).1, Some(456));
        let rest = end + Duration::from_millis(1500);
        wakes.set_time(rest);
        sender.send(Command::StopLook { play }).unwrap();
        // The stop was in the rest, although the next frame would be in the first item again.
        assert_eq!(
            picture(&mut engine, end + Duration::from_secs(2)),
            (vec![], None)
        );
        sender.send(Command::RemoveLook { look: 11 }).unwrap();

        let from = end + Duration::from_secs(2);
        sender.send(sequence(11, &[1, 3, 2])).unwrap();
        let once = begun(&mut sender, start(11, Rank::Manual, Repeat::Once));
        assert_eq!(picture(&mut engine, from).1, Some(456));
        let shortened = from + Duration::from_millis(1500);
        assert_eq!(picture(&mut engine, shortened).1, Some(789));
        sender.send(sequence(11, &[1])).unwrap();
        let (plays, sample) = picture(&mut engine, shortened);
        assert_eq!(
            (plays[0].play, plays[0].state, sample),
            (once, PlayState::Kept, Some(456))
        );
        wakes.run();
    }

    #[cfg(feature = "test-support")]
    #[expect(
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::disallowed_types,
        reason = "the test client reads real packets and fails on setup or payload errors"
    )]
    mod output_restart {
        use super::*;
        use engine_core::Pixels;
        mod stream {
            include!("../../outputs/tests/common/stream.rs");
        }

        #[test]
        fn refused_restart_preserves_the_only_stream_for_a_patched_strip() {
            let (receiver, kind) = stream::receiver(Color::Rgb);
            let mut initial = true;
            let open = Box::new(move |kind| {
                if std::mem::take(&mut initial) {
                    outputs::open(kind)
                } else {
                    Err(io::Error::other("replacement device unavailable"))
                }
            });
            let universes = Universes::new(3).unwrap();
            let (mut engine, mut sender, wakes) = Engine::start_stepped(universes, open).unwrap();
            wakes.hold().unwrap();
            let output = OutputNumber::try_from(1).unwrap();
            let set = |count| Command::SetOutput {
                output,
                kind,
                universes: RunNumbers { first: 1, count },
                rate: Some(Rate::Sixty),
            };
            let at = ChannelNumbers {
                universe: 1,
                address: 512,
            };
            let hold = |value| Command::SetChannel { channel: at, value };
            sender.send(set(2)).unwrap();
            sender
                .send(Command::PatchStrip {
                    name: Text::new("Strip").unwrap(),
                    pixels: Pixels::try_from(2).unwrap(),
                    color: Color::Rgb,
                    at,
                })
                .unwrap();
            sender.send(hold(91)).unwrap();
            wakes.wake().unwrap();

            let mut packet = [0; 2048];
            assert_eq!(receiver.recv(&mut packet).unwrap(), 10 + 1023);
            assert_eq!(packet[10 + 511], 91);
            let before = sender.project().output(output);
            let old_run = RunNumbers { first: 1, count: 2 }.check(universes).unwrap();
            assert_eq!(sender.patch().unstreamed(&sender.project()), None);

            let Begun::Opening(restart) = sender.begin(set(3), &[]).unwrap() else {
                panic!("the longer run needs a restart");
            };
            // Hold the tick until the original device's hand-back times out. The fallback
            // opener then fails without ever obtaining the original, working device.
            assert_eq!(sender.finish(restart.open()), Err(Unsent::Refused(Refusal::CannotOpen)));
            sender.send(hold(73)).unwrap();
            engine.latest();
            wakes.wake().unwrap();
            let snapshot = engine.latest();
            assert_eq!(snapshot.applied(), sender.sent());
            assert_eq!(
                (
                    sender.project().output(output),
                    sender.patch().unstreamed(&sender.project()),
                    snapshot.state.outputs[output.slot()].map(|out| out.universes),
                ),
                (before, None, Some(old_run)),
                "the refused edit must leave the fixture on its previous output"
            );
            assert_eq!(receiver.recv(&mut packet).unwrap(), 10 + 1023);
            assert_eq!(packet[10 + 511], 73);
        }

        #[test]
        fn a_route_edit_cannot_drop_or_move_a_patched_byte() {
            let mut actual = Vec::new();
            // (fixture universe, run length, packet length, offset before the move)
            for (fixture_universe, count, len, offset) in [(1, 1, 510, 0), (2, 2, 1023, 512)] {
                let (receiver, kind) = stream::receiver(Color::Rgb);
                let universes = Universes::new(3).unwrap();
                let (_engine, mut sender, wakes) =
                    Engine::start_stepped(universes, Box::new(outputs::open)).unwrap();
                wakes.hold().unwrap();
                let output = OutputNumber::try_from(1).unwrap();
                let set = |first| Command::SetOutput {
                    output,
                    kind,
                    universes: RunNumbers { first, count },
                    rate: Some(Rate::Sixty),
                };
                let at = ChannelNumbers {
                    universe: fixture_universe,
                    address: 1,
                };
                sender.send(set(1)).unwrap();
                sender
                    .send(Command::PatchStrip {
                        name: Text::new("Strip").unwrap(),
                        pixels: Pixels::try_from(1).unwrap(),
                        color: Color::Rgb,
                        at,
                    })
                    .unwrap();
                sender
                    .send(Command::SetChannel {
                        channel: at,
                        value: 91,
                    })
                    .unwrap();
                wakes.wake().unwrap();
                let mut packet = [0; 2048];
                assert_eq!(receiver.recv(&mut packet).unwrap(), 10 + len);
                assert_eq!(packet[10 + offset], 91);

                let refusal = sender.send(set(2)).is_err();
                wakes.wake().unwrap();
                assert_eq!(receiver.recv(&mut packet).unwrap(), 10 + len);
                let in_packet = packet[10..10 + len].iter().position(|byte| *byte == 91);
                actual.push((refusal, in_packet));
            }
            assert_eq!(actual, [(true, Some(0)), (true, Some(512))]);
        }
    }

    #[cfg(feature = "test-support")]
    #[test]
    #[expect(clippy::unwrap_used, clippy::panic, reason = "test setup must succeed")]
    fn a_restart_with_a_full_fifo_commits_only_a_working_replacement() {
        // (the fresh open succeeds, finish answers, accepted universe count)
        let cases = [
            (true, Ok(()), 3),
            (false, Err(Unsent::Refused(Refusal::CannotOpen)), 2),
        ];
        for (fresh, answer, count) in cases {
            let universes = Universes::new(3).unwrap();
            let opens = Arc::new(AtomicU32::new(0));
            let open: Open = Box::new({
                let opens = Arc::clone(&opens);
                move |_| {
                    // the second open is the fresh one of the restart
                    if opens.fetch_add(1, Ordering::Relaxed) == 1 && !fresh {
                        return Err(io::Error::other("no device"));
                    }
                    Ok(Box::new(health::ByLevel))
                }
            });
            let (mut engine, mut sender, wakes) = Engine::start_stepped(universes, open).unwrap();
            wakes.hold().unwrap();
            let output = OutputNumber::try_from(1).unwrap();
            let set = |count| Command::SetOutput {
                output,
                kind: engine_core::TEST_STREAM_KIND,
                universes: RunNumbers { first: 1, count },
                rate: None,
            };
            sender.send(set(2)).unwrap();
            // This fixture uses the only output spanning both universes.
            sender
                .send(Command::PatchStrip {
                    name: Text::new("Strip").unwrap(),
                    pixels: engine_core::Pixels::try_from(2).unwrap(),
                    color: Color::Rgb,
                    at: ChannelNumbers {
                        universe: 1,
                        address: 512,
                    },
                })
                .unwrap();
            wakes.wake().unwrap();

            // Keep the tick paused until the hand-back has timed out. Other clients
            // may use every unreserved FIFO place while the worker is waiting.
            let Ok(Begun::Opening(restart)) = sender.begin(set(3), &[]) else {
                panic!("changing the route length must retire the current lane");
            };
            while sender.send(Command::Blackout { on: false }).is_ok() {}
            assert_eq!(sender.finish(restart.open()), answer, "fresh {fresh}");
            let run = RunNumbers { first: 1, count }.check(universes).unwrap();
            let expected = Some(ProjectOutput::Kind {
                kind: engine_core::TEST_STREAM_KIND,
                universes: run,
            });
            let before_cutover = RunNumbers { first: 1, count: 2 }.check(universes).unwrap();
            assert_eq!(
                sender.project().output(output),
                Some(ProjectOutput::Kind {
                    kind: engine_core::TEST_STREAM_KIND,
                    universes: before_cutover,
                }),
                "the old run is visible before T9"
            );
            engine.latest();
            wakes.wake().unwrap();
            assert_eq!(sender.project().output(output), expected, "fresh {fresh}");
            let snapshot = engine.latest();
            assert_eq!(snapshot.applied(), sender.sent());
            let state = snapshot.state.outputs.get(output.slot()).copied().flatten();
            assert_eq!(state.map(|o| o.universes), Some(run));
            assert_eq!(
                sender.devices,
                u64::from(fresh),
                "only a successful resize retires the old lane"
            );
            assert_eq!(sender.patch().unstreamed(&sender.project()), None);
            let started = Instant::now();
            while sender.devices != sender.shared.devices_freed.load(Ordering::Acquire) {
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "the old lane was never freed, fresh {fresh}"
                );
                sender.free.unpark();
                thread::sleep(Duration::from_millis(1)); // poll
            }
            assert_eq!(opens.load(Ordering::Relaxed), 2);
            let started = Instant::now();
            loop {
                wakes.wake().unwrap();
                if engine
                    .latest()
                    .state
                    .outputs
                    .get(output.slot())
                    .copied()
                    .flatten()
                    .is_some_and(|o| o.status == Status::Running)
                {
                    break;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "the output never ran, fresh {fresh}"
                );
            }
        }
    }

    /// A device that takes its time to close: until the test lets it.
    #[cfg(feature = "test-support")]
    struct Closing {
        closing: Arc<AtomicBool>,
        closes: Arc<AtomicBool>,
    }

    #[cfg(feature = "test-support")]
    impl Device for Closing {
        fn take(&mut self, _frame: &[u8]) -> Outcome {
            Outcome::Accepted
        }
    }

    #[cfg(feature = "test-support")]
    impl Drop for Closing {
        fn drop(&mut self) {
            self.closing.store(true, Ordering::Release);
            let started = Instant::now();
            while !self.closes.load(Ordering::Acquire)
                && started.elapsed() < Duration::from_secs(10)
            {
                thread::sleep(Duration::from_millis(1)); // poll
            }
        }
    }

    #[cfg(feature = "test-support")]
    #[test]
    #[expect(
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test that cannot set itself up fails"
    )]
    fn an_opening_waits_for_the_device_sent_back_not_for_a_cancelled_plan() {
        let (closing, closes) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let open: Open = Box::new(|_kind| Err(io::Error::other("the test attaches its device")));
        let (_engine, mut sender, wakes) =
            Engine::start_stepped(Universes::new(1).unwrap(), open).unwrap();
        let output = OutputNumber::try_from(1).unwrap();
        let run = RunNumbers { first: 1, count: 1 }
            .check(sender.universes)
            .unwrap();
        sender
            .attach(
                output,
                Box::new(Closing {
                    closing: Arc::clone(&closing),
                    closes: Arc::clone(&closes),
                }),
                run,
                Rate::Sixty,
            )
            .unwrap();
        let strip = Command::PatchStrip {
            name: Text::new("strip").unwrap(),
            pixels: engine_core::Pixels::try_from(1).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        };
        assert!(sender.begin(strip, &[]).is_ok());
        let step = engine_core::Step {
            target: Target::Light { light: 1 },
            values: Light::DARK,
        };
        let name = Text::new("s").unwrap();
        sender
            .send(Command::PutStep {
                look: 1,
                name,
                step,
            })
            .unwrap();
        let rules = MixRules {
            other_end: OtherEnd::Stay,
            ..MixRules::default()
        };
        sender.send(Command::SetMixRules { rules }).unwrap();
        // off the grid: the bar is ahead
        wakes.hold().unwrap();
        wakes.wake_at(Duration::from_millis(100)).unwrap();
        let start = Command::StartLook {
            look: 1,
            rank: Rank::Show,
            when: When::Bar,
        };
        let Ok(Begun::Started { play }) = sender.begin(start, &[]) else {
            panic!("look 1 waits");
        };
        wakes.wake().unwrap();
        // stopped and removed before the wake: an opening now has not reaped the cancel
        sender.send(Command::StopLook { play }).unwrap();
        sender.send(Command::RemoveOutput { output }).unwrap();
        // the plan goes back, then the device; the plan is freed, the device closes
        wakes.wake().unwrap();
        let started = Instant::now();
        while !closing.load(Ordering::Acquire) {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the device closes"
            );
            thread::sleep(Duration::from_millis(1)); // poll
        }
        assert!(
            waits(&sender.shared, sender.devices),
            "the port is still held"
        );
        closes.store(true, Ordering::Release);
        while waits(&sender.shared, sender.devices) {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the device is freed"
            );
            thread::sleep(Duration::from_millis(1)); // poll
        }
        wakes.run();
    }

    #[cfg(feature = "test-support")]
    #[test]
    fn linked_roots_book_aggregate_work_until_the_fifo_drains() {
        for repeat in [Repeat::Loop, Repeat::Once] {
            check_linked_roots(repeat);
        }
    }

    #[cfg(feature = "test-support")]
    #[expect(
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        reason = "an invalid fixture, missing play or missing frame fails this stepped test"
    )]
    fn check_linked_roots(repeat: Repeat) {
        use engine_core::{Sequence, SequenceItem, Step};

        let (mut engine, mut sender, wakes) =
            Engine::start_stepped(Universes::new(1).unwrap(), Box::new(outputs::open)).unwrap();
        wakes.hold().unwrap();
        let name = Text::new("linked fades").unwrap();
        sender
            .send(Command::PatchStrip {
                name,
                pixels: engine_core::Pixels::try_from(100).unwrap(),
                color: Color::Rgb,
                at: ChannelNumbers {
                    universe: 1,
                    address: 1,
                },
            })
            .unwrap();
        let leaf = |intensity| Command::PutStep {
            look: 1,
            name,
            step: Step {
                target: Target::Fixture { fixture: 1 },
                values: Light {
                    intensity: Some(intensity),
                    ..Light::DARK
                },
            },
        };
        sender.send(leaf(11)).unwrap();
        sender
            .send(Command::SetMixRules {
                rules: MixRules {
                    other_end: OtherEnd::Stay,
                    ..Default::default()
                },
            })
            .unwrap();
        // Two roots share three nested transitions over a hundred lights.
        // Each fits alone. Three plays fit with their terminal samples reserved.
        for (look, child) in [(2, 1), (3, 2), (4, 3), (5, 3), (6, 4)] {
            sender
                .send(Command::PutSequence {
                    look,
                    name,
                    sequence: Sequence::new(&[SequenceItem {
                        look: child,
                        transition: 100,
                        ..Default::default()
                    }])
                    .unwrap(),
                })
                .unwrap();
        }
        let start = |look, rank| Command::StartUse {
            look,
            rank,
            when: When::Now,
            usage: Use {
                repeat,
                ..Default::default()
            },
        };
        let started = |s: &mut Sender, cmd| match s.begin(cmd, &[]) {
            Ok(Begun::Started { play }) => play,
            _ => panic!("expected an admitted play"),
        };
        let clock = wakes.wake().unwrap() + Duration::from_secs(1);
        wakes.set_time(clock);
        let picture = |engine: &mut Engine| {
            engine.latest();
            wakes.wake_at(clock).unwrap();
            let snapshot = engine.latest();
            (
                snapshot.state.plays.iter().copied().collect::<Vec<_>>(),
                snapshot.lights()[..100]
                    .iter()
                    .map(|l| l.intensity)
                    .collect::<Vec<_>>(),
            )
        };
        let ids = [Rank::Manual, Rank::Show, Rank::Autopilot]
            .map(|rank| started(&mut sender, start(4, rank)));
        let before = picture(&mut engine);
        assert_eq!(before.0.iter().map(|p| p.play).collect::<Vec<_>>(), ids);
        assert_eq!(before.1, vec![Some(11); 100]);
        let saved = sender.looks().all();
        let tail = sender.sent();
        for (case, command) in [
            ("another linked root", start(5, Rank::Manual)),
            ("restart a reserved root", start(4, Rank::Manual)),
            (
                "expand every parent's shared sequence",
                Command::PutSequence {
                    look: 2,
                    name,
                    sequence: Sequence::new(
                        &[SequenceItem {
                            look: 1,
                            transition: 100,
                            ..Default::default()
                        }; 4],
                    )
                    .unwrap(),
                },
            ),
        ] {
            assert_eq!(sender.send(command), Err(Unsent::Refused(Refusal::WorkLimit)), "{case}");
            assert_eq!(sender.sent(), tail, "{case}");
            assert_eq!(sender.looks().all(), saved, "{case}");
            assert_eq!(picture(&mut engine), before, "{case}");
        }
        // Expanded groups share the FIFO prefix with these roots. One hundred
        // lights fit once, but a queueful would exhaust the remaining visits.
        let mut refusal = None;
        for _ in 0..QUEUE - PLAYS {
            let tail = sender.sent();
            if let Err(reason) = sender.send(Command::SetColour {
                target: Target::Fixture { fixture: 1 },
                red: 0,
                green: 0,
                blue: 0,
            }) {
                assert_eq!(sender.sent(), tail);
                refusal = Some(reason);
                break;
            }
        }
        assert_eq!(refusal, Some(Unsent::Refused(Refusal::WorkLimit)));
        for play in ids {
            if repeat == Repeat::Loop {
                sender.send(Command::StopLook { play }).unwrap();
            }
            sender.send(Command::ReleaseHold { play }).unwrap();
        }
        // A loop stopped under Stay and a once-use can sample before release.
        // Together with the queued group writes, a larger root still cannot fit.
        assert_eq!(sender.send(start(6, Rank::Manual)), Err(Unsent::Refused(Refusal::WorkLimit)));
        assert_eq!(picture(&mut engine), (vec![], vec![None; 100]));
        let retry = started(&mut sender, start(6, Rank::Manual));
        sender.send(leaf(22)).unwrap();
        let (plays, intensities) = picture(&mut engine);
        assert_eq!(plays.iter().map(|p| p.play).collect::<Vec<_>>(), [retry]);
        assert_eq!(intensities, vec![Some(22); 100]);
        wakes.run();
    }

    #[cfg(feature = "test-support")]
    #[test]
    #[expect(
        clippy::unwrap_used,
        clippy::panic,
        reason = "a missing fixture, stream or play fails this stepped admission test"
    )]
    fn a_saved_wave_can_start_alone_at_the_work_limit() {
        let universes = Universes::new(25).unwrap();
        let (mut engine, mut sender, wakes) =
            Engine::start_stepped(universes, Box::new(outputs::open)).unwrap();
        wakes.hold().unwrap();
        sender
            .attach(
                OutputNumber::try_from(1).unwrap(),
                Box::new(Hears(Arc::new(Heard {
                    frames: AtomicU64::new(0),
                    levels: Default::default(),
                }))),
                RunNumbers {
                    first: 1,
                    count: 25,
                }
                .check(universes)
                .unwrap(),
                Rate::Sixty,
            )
            .unwrap();
        let name = Text::new("wall").unwrap();
        // The panel fills exactly 24 universes. The extra pixel begins the next one.
        for (pixels, universe) in [(4096, 1), (1, 25)] {
            sender
                .send(Command::PatchStrip {
                    name,
                    pixels: engine_core::Pixels::try_from(pixels).unwrap(),
                    color: Color::Rgb,
                    at: ChannelNumbers {
                        universe,
                        address: 1,
                    },
                })
                .unwrap();
        }
        sender
            .send(Command::SetGroup {
                name,
                members: MemberList::Fits(
                    Members::new(&[
                        Member::Fixture { fixture: 1 },
                        Member::Fixture { fixture: 2 },
                    ])
                    .unwrap(),
                ),
            })
            .unwrap();
        let wave = |look, target| Command::PutPattern {
            look,
            name,
            pattern: Pattern::Wave {
                target,
                period: 1,
                palette: Palette::Inline(Swatches::new(&[Rgb::BLACK]).unwrap()),
            },
        };
        let wall = wave(1, WaveTarget::Matrix { fixture: 1 });
        assert!(
            edits(&wall),
            "Wave compilation belongs in off-lock edit preparation"
        );
        sender.send(wall).unwrap();
        sender
            .send(wave(2, WaveTarget::Matrix { fixture: 2 }))
            .unwrap();
        let start = |look| Command::StartLook {
            look,
            rank: Rank::Show,
            when: When::Now,
        };
        let clock = wakes.wake().unwrap();
        let Ok(Begun::Started { play }) = sender.begin(start(1), &[]) else {
            panic!("a saved 4096-light Wave must be playable on its own");
        };
        let applying_frame = |engine: &mut Engine| {
            engine.latest();
            wakes.wake_at(clock).unwrap();
            let snapshot = engine.latest();
            (
                snapshot
                    .state
                    .plays
                    .iter()
                    .map(|row| row.play)
                    .collect::<Vec<_>>(),
                snapshot.lights().get(4095).and_then(|light| light.colour),
            )
        };
        let before = applying_frame(&mut engine);
        assert_eq!(before, (vec![play], Some(Rgb::BLACK)));
        let saved = sender.looks().all();
        let tail = sender.sent();
        for command in [start(2), wave(3, WaveTarget::Group { group: name })] {
            assert_eq!(sender.send(command), Err(Unsent::Refused(Refusal::WorkLimit)));
            assert_eq!(sender.sent(), tail);
            assert_eq!(sender.looks().all(), saved);
            assert_eq!(applying_frame(&mut engine), before);
        }
        sender.send(Command::ReleaseHold { play }).unwrap();
        let Ok(Begun::Started { play: next }) = sender.begin(start(2), &[]) else {
            panic!("release frees the loop's frame reservation");
        };
        assert_eq!(applying_frame(&mut engine), (vec![next], None));

        // A stop that keeps the Manual Wave must still sample it at the stop.
        // Releasing it in that prefix only frees the following frame sample.
        sender.send(Command::ReleaseHold { play: next }).unwrap();
        let manual = Command::StartLook {
            look: 1,
            rank: Rank::Manual,
            when: When::Now,
        };
        let Ok(Begun::Started { play }) = sender.begin(manual, &[]) else {
            panic!("the Manual Wave starts");
        };
        let intensity = Command::SetIntensity {
            target: Target::Light { light: 1 },
            intensity: u16::MAX,
        };
        sender.send(intensity).unwrap();
        assert_eq!(applying_frame(&mut engine).0, [play]);
        sender.send(Command::StopLook { play }).unwrap();
        // Moving a sample from the live Wave into the stop event preserves
        // the headroom to write the same light property again.
        assert_eq!(sender.send(intensity), Ok(()));
        sender.send(Command::ReleaseHold { play }).unwrap();
        assert_eq!(sender.send(start(1)), Err(Unsent::Refused(Refusal::WorkLimit)));
        assert_eq!(applying_frame(&mut engine), (vec![], None));
        sender.send(start(1)).unwrap();
        assert_eq!(applying_frame(&mut engine).0.len(), 1);
        wakes.run();
    }

    #[cfg(feature = "test-support")]
    #[test]
    #[expect(
        clippy::unwrap_used,
        clippy::panic,
        reason = "an invalid fixture, missing play or missing frame fails this stepped test"
    )]
    fn a_kept_once_parent_retains_its_nested_look_ids_until_release() {
        use engine_core::{Repeat, Sequence, SequenceItem, Step, Use};

        let (mut engine, mut sender, wakes) =
            Engine::start_stepped(Universes::new(1).unwrap(), Box::new(outputs::open)).unwrap();
        wakes.hold().unwrap();
        let name = Text::new("held links").unwrap();
        sender
            .send(Command::PatchStrip {
                name,
                pixels: engine_core::Pixels::try_from(1).unwrap(),
                color: Color::Rgb,
                at: ChannelNumbers {
                    universe: 1,
                    address: 1,
                },
            })
            .unwrap();
        for (look, intensity) in [(1, 10), (2, 20)] {
            sender
                .send(Command::PutStep {
                    look,
                    name,
                    step: Step {
                        target: Target::Light { light: 1 },
                        values: Light {
                            intensity: Some(intensity),
                            ..Light::DARK
                        },
                    },
                })
                .unwrap();
        }
        let link = |look, child| Command::PutSequence {
            look,
            name,
            sequence: Sequence::new(&[SequenceItem {
                look: child,
                ..Default::default()
            }])
            .unwrap(),
        };
        sender.send(link(3, 1)).unwrap();
        sender.send(link(4, 3)).unwrap();
        let start = wakes.wake().unwrap();
        let Ok(Begun::Started { play }) = sender.begin(
            Command::StartUse {
                look: 4,
                rank: Rank::Manual,
                when: When::Now,
                usage: Use {
                    repeat: Repeat::Once,
                    ..Default::default()
                },
            },
            &[],
        ) else {
            panic!("the parent starts");
        };
        wakes.wake_at(start).unwrap();
        engine.latest();
        wakes.wake_at(start + Duration::from_secs(2)).unwrap();
        assert_eq!(
            engine.latest().state.plays.iter().next().unwrap().state,
            PlayState::Kept
        );
        // Both saved sequences now reference step 2. The frozen generation reached
        // sequence 3 and step 1 through the old definitions.
        sender.send(link(4, 2)).unwrap();
        sender.send(link(3, 2)).unwrap();
        for look in [1, 3] {
            assert_eq!(
                sender.send(Command::RemoveLook { look }),
                Err(Unsent::Refused(Refusal::LookInUse { look: 4 }))
            );
        }
        sender.send(Command::ReleaseHold { play }).unwrap();
        for look in [1, 3] {
            sender.send(Command::RemoveLook { look }).unwrap();
        }
        wakes.run();
    }
}
