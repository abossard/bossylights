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
pub use engine_core::{
    Bpm, Brightness, GrabId, Light, ManualEnd, MixRules, OtherEnd, PlayId, PlayState, Property,
    Rank, Repeat, Rgb, Saved, SavedLook, Sequence, SequenceItem, Source, Speed, Use, Value, When,
};
use engine_core::{
    EngineCommand, Histogram, LATENCY_BUCKETS, LATENESS_BUCKETS, MAX_LIGHTS, bucket, due, frame,
    percentile, update, wait,
};
pub use fixture_library::Fixture;
pub use looks::Looks;
pub use rig::Patch;
mod grabs;
use grabs::Grabs;
mod snapshot;

use engine_core::{
    Claims, End, Entry, LightChange, LightRun, Lights, LookCommand, PLAYS, ProjectEffect,
    ProjectEvent, Writer, stops, unkept,
};
use looks::Plan;
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
/// Runs one place of the command queue carries: a change on a group of the most runs
/// ([`Members::MOST`]) takes 86 places, a third of the queue, all in one wake.
const RUNS_IN_PLACE: usize = 6;
/// How often the free thread frees retired devices when nobody wakes it.
const FREE_EVERY: Duration = Duration::from_millis(100);
/// How long `SetOutput` waits for the devices on their way back before it opens.
const FREE_WITHIN: Duration = Duration::from_secs(1);

/// Whether `command` is an edit: it changes what the show file keeps (the patch, the
/// groups, the looks), and its prepare may take long (a patch measures its show file, a
/// look compiles). A front shared behind a lock prepares an edit without it
/// ([`Sender::base`]). Every other command is a press: [`Sender::begin`] under the lock,
/// queued at once for the very next wake.
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
    universes: Universes,
}

/// The engine front: owns the [`Project`], checks every client command against it,
/// sends the tick thread what follows and opens the outputs' devices, so the tick thread
/// never does. The only sender: each queue has one producer and one consumer. A command
/// checked against an older [`Base`] is refused `Stale` when admitted.
pub struct Sender {
    /// `None` while an [`Opening`] has it: one device opens at a time.
    open: Option<Open>,
    commands: rtrb::Producer<Sent>,
    project: Project,
    /// The fixtures, their lights and writers. Shared with readers as it is: a change
    /// makes a new one.
    patch: Arc<Patch>,
    /// The saved looks. Shared with readers as they are: a change makes new ones.
    looks: Arc<Looks>,
    /// The plays queued for the tick thread, by slot: the tick thread's plays once it
    /// applied every command sent.
    plays: [Option<Live>; PLAYS],
    /// Visits booked for expanded light commands and retiring plans until their FIFO prefix completes.
    prefix_work: std::collections::VecDeque<(u64, usize)>,
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
    devices: u64,
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
    Device(Box<Lane>),
    Writers(Vec<Writer>),
    Plan(Arc<Plan>),
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
    patch: Arc<Patch>,
    looks: Arc<Looks>,
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
    UpdateGrab {
        grab: GrabId,
        value: Value,
    },
    EndGrab {
        grab: GrabId,
    },
}

impl Base {
    /// Checks `command` against this base and makes what it changes: a calculation,
    /// which may take long (a patch measures its show file, a look compiles). `library`
    /// is where `Patch` looks up its fixture. Nothing is queued until it is admitted.
    pub fn prepare(&self, command: Command, library: &[Fixture]) -> Result<Prepared, Refusal> {
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
        let change = match effect {
            None => Change::Nothing,
            Some(ProjectEffect::Send(command)) => Change::Send(command),
            Some(ProjectEffect::Look(command)) => self.look(command)?,
            Some(ProjectEffect::Patch(command)) => {
                let (patch, effects) = self.patch.update(command, library)?;
                // and a change of the patch that would make one is refused
                if let Some((_, reason)) = patch.as_ref().and_then(|p| p.unstreamed(&project)) {
                    return Err(reason);
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
            Some(ProjectEffect::Open {
                output,
                kind,
                universes,
                rate,
            }) => Change::Open {
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
            LookCommand::PutStep { look, name, step } => {
                self.patch.fits()?;
                Change::PutLook {
                    looks: self.looks.put(Saved { look, name, step }, &self.patch)?,
                }
            }
            LookCommand::PutSequence {
                look,
                name,
                sequence,
            } => {
                self.patch.fits()?;
                let saved = SavedLook {
                    look,
                    name,
                    definition: engine_core::Definition::Sequence(sequence),
                };
                Change::PutLook {
                    looks: self.looks.put(saved, &self.patch)?,
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
                self.patch.fits()?;
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

#[derive(Clone)]
struct Live {
    play: PlayId,
    look: u16,
    rank: Rank,
    state: PlayState,
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
}

/// Between the threads of the engine. Atomics only.
struct Shared {
    stop: AtomicBool,
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
    #[cfg(test)]
    stalled: AtomicBool,
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
            plan_bytes_freed: AtomicU64::new(0),
            devices_freed: AtomicU64::new(0),
            revision: AtomicU64::new(0),
            ended: [const { AtomicU32::new(0) }; PLAYS],
            kept: [const { AtomicU64::new(0) }; PLAYS],
            applied: AtomicU64::new(0),
            #[cfg(test)]
            stalled: AtomicBool::new(false),
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
        let prepared = self.base().prepare(command, library)?;
        self.admit(prepared)
    }

    /// What a command is checked against now: the project, the patch and the looks.
    /// Immutable handles pin the definitions and the versions a live play can keep.
    pub fn base(&self) -> Base {
        Base {
            reservations: self.plays.clone(),
            project: self.project,
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
        if !(base.project == self.project
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
                self.push(command, Brings::Nothing)?;
                Begun::Queued
            }
            Change::Open {
                output,
                kind,
                universes,
                rate,
            } => Begun::Opening(Opening {
                output,
                kind,
                universes,
                rate,
                open: self.open.take().ok_or(Refusal::Opening)?,
                shared: Arc::clone(&self.shared),
                free: self.free.clone(),
                devices: self.devices,
            }),
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
            Change::PutLook { looks } => {
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
                self.check_work(plan.work(), 0)?;
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
                            .push_back((self.sent, looks::play_work(old.work())));
                    }
                    *live = Some(Live {
                        play,
                        look,
                        rank,
                        state: PlayState::Playing,
                        current: version,
                        earlier: None,
                    });
                }
                Begun::Started { play }
            }
            Change::StopLook { play } => {
                match self.live(play) {
                    Some(live) if live.state == PlayState::Playing => {
                        match stops(live.rank, self.looks.rules()) {
                            End::Keep => {
                                self.push(EngineCommand::Keep { play }, Brings::Nothing)?;
                                if let Some(Some(live)) = self.plays.get_mut(play.slot()) {
                                    live.state = PlayState::Kept;
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
            Change::UpdateGrab { grab, value } => {
                if let Some((property, runs)) = self.grabs.get(grab) {
                    if property != value.property() {
                        return Err(Refusal::OtherProperty { property });
                    }
                    // only the lights it still holds take the value
                    self.patch.supports(&self.grabs.held(grab), value)?;
                    let runs = runs.to_vec();
                    self.lights(LightChange::Update { value, grab }, &runs)?;
                }
                Begun::Queued
            }
            Change::EndGrab { grab } => {
                if let Some((property, runs)) = self.grabs.get(grab) {
                    let end = LightChange::End {
                        property,
                        grab,
                        end: stops(Rank::Manual, self.looks.rules()),
                    };
                    let runs = runs.to_vec();
                    self.lights(end, &runs)?;
                    self.grabs.end(grab);
                }
                Begun::Queued
            }
            Change::Rules { looks } => {
                let rules = looks.rules();
                self.push(EngineCommand::Rules { rules }, Brings::Nothing)?;
                self.forget(|live| unkept(live.rank, live.state, rules));
                self.looks = Arc::new(looks);
                Begun::Queued
            }
        };
        self.project = project;
        Ok(begun)
    }

    /// Charge even a version replaced twice in one wake. A play has room for its
    /// frame and for the terminal sample of a stop or natural end.
    fn check_work(&self, new_plans: usize, lights: usize) -> Result<(), Refusal> {
        let playing: usize = self.plays.iter().flatten().map(Live::work).sum();
        let prefix: usize = self.prefix_work.iter().map(|(_, work)| work).sum();
        if looks::play_work(playing + new_plans) + prefix + lights > looks::WORK_LIMIT {
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
        if let Some(old) = self.plays.get_mut(play.slot()).and_then(Option::take) {
            self.prefix_work
                .push_back((self.sent, looks::play_work(old.work())));
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
                *slot = None;
                self.retired += 1;
                continue;
            }
            let frozen = kept.load(Ordering::Acquire);
            let version = live.versions().find(|v| v.sent == frozen).cloned();
            if let Some(version) = version {
                live.current = version;
                live.earlier = None;
                live.state = PlayState::Kept;
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
                && let Some(old) = slot.take()
            {
                self.prefix_work
                    .push_back((self.sent, looks::play_work(old.work())));
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
                && on_their_way(&self.shared, self.retired) + held + returns as u64 > RETIRED as u64
        {
            self.free.unpark();
            return Err(Refusal::QueueFull);
        }
        Ok(())
    }

    /// Takes `looks` in place of the saved looks, as a show file brings them: refused
    /// when one names nothing in the patch. No play uses them yet.
    pub fn load_looks(&mut self, looks: Looks) -> Result<(), Refusal> {
        let looks = looks.rebind(&self.patch)?;
        self.push(
            EngineCommand::Rules {
                rules: looks.rules(),
            },
            Brings::Nothing,
        )?;
        self.looks = Arc::new(looks);
        Ok(())
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
        if let Some(looks) = looks {
            for live in self
                .plays
                .iter()
                .flatten()
                .filter(|p| p.state == PlayState::Playing)
            {
                let plan = looks.plan(live.look)?;
                if !Arc::ptr_eq(&plan, &live.current.plan) {
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
        if !changed.is_empty()
            || effects
                .iter()
                .any(|e| matches!(e, PatchEffect::Grab { .. }))
        {
            self.check_work(changed.iter().map(|v| v.plan.work()).sum(), lights)?;
            self.check_plan_bytes(bytes)?;
        }
        for effect in &effects {
            if let PatchEffect::Grab { value, runs } = effect {
                self.grabs.fits(value.property(), runs)?;
            }
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
                    if let EngineCommand::Lights {
                        lights,
                        change: LightChange::Release { property },
                    } = command
                    {
                        self.grabs.let_go(Some(property), &[lights]);
                    } else if let EngineCommand::Darken { lights } = command {
                        self.grabs.let_go(None, &[lights]);
                    }
                }
                PatchEffect::Runs { change, runs } => {
                    queued.extend(run_commands(change, &runs, at));
                    if let LightChange::Release { property } = change {
                        self.grabs.let_go(Some(property), &runs);
                    }
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
            self.prefix_work.push_back((self.sent, lights));
        }
        for version in changed {
            if let Some(Some(live)) = self.plays.get_mut(version.play.slot()) {
                self.prefix_work
                    .push_back((self.sent, looks::play_work(live.current.plan.work())));
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
        let visits = run_visits(runs);
        if matches!(change, LightChange::Update { .. }) {
            self.check_work(0, visits)?;
        }
        let pieces = runs.len().div_ceil(RUNS_IN_PLACE);
        let chunk = self
            .commands
            .write_chunk_uninit(pieces)
            .map_err(|_| Refusal::QueueFull)?;
        let sent = chunk.fill_from_iter(run_commands(change, runs, self.clock.now()));
        self.sent += sent as u64;
        self.prefix_work.push_back((self.sent, visits));
        Ok(())
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
        self.publish(vec![PatchEffect::Writers(patch.writers().to_vec())], None)?;
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
            EngineCommand::Attach { output, .. } => {
                (Some(output), matches!(brings, Brings::Device(_)))
            }
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
        let held = self.plays.iter().flatten().count() as u64;
        if adds && on_their_way(&self.shared, self.retired) + held >= RETIRED as u64 {
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
        while waits(&self.shared, self.devices) && started.elapsed() < FREE_WITHIN {
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

/// Whether one of the `devices` sent away so far is not freed yet: an opening waits.
fn waits(shared: &Shared, devices: u64) -> bool {
    devices > shared.devices_freed.load(Ordering::Acquire)
}

/// Of the `retired` boxes sent back so far, those not freed yet.
fn on_their_way(shared: &Shared, retired: u64) -> u64 {
    retired.saturating_sub(shared.freed.load(Ordering::Acquire))
}

/// The free thread: frees the devices and writers the tick thread retired, closing ports
/// and sockets, on a steady beat or when the sender wakes it. Until the engine stops.
fn free(mut retired: rtrb::Consumer<Retired>, shared: &Shared) {
    loop {
        #[cfg(test)]
        while shared.stalled.load(Ordering::Acquire) && !shared.stop.load(Ordering::Relaxed) {
            thread::park_timeout(Duration::from_millis(1));
        }
        while let Ok(back) = retired.pop() {
            let device = matches!(back, Retired::Device(_));
            let bytes = if let Retired::Plan(plan) = &back {
                plan.bytes() as u64
            } else {
                0
            };
            drop(back);
            if bytes > 0 {
                shared.plan_bytes_freed.fetch_add(bytes, Ordering::Release);
            }
            shared.freed.fetch_add(1, Ordering::Release);
            if device {
                shared.devices_freed.fetch_add(1, Ordering::Release);
            }
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
    /// Each play's plan, by slot, beside `State::plays`.
    plans: [Option<Version>; PLAYS],
    scratch: [Box<[Entry]>; PLAYS],
    /// Who holds each property of each light this frame: the Mixer's buffer.
    claims: Box<[Claims]>,
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
            // read after the commands are counted: a change it counts has all its
            // commands in this wake, or its moment's commands are more than applied
            self.now
                .revised(self.shared.revision.load(Ordering::Acquire));
            let now = self.clock.cutoff(&self.shared.stop);
            self.apply(EngineCommand::Tick { now });

            // Apply them right before the frame: a press is in the next one.
            for _ in 0..published {
                let Ok(sent) = self.commands.pop() else {
                    break;
                };
                // in time order: what the grid reached before it was sent comes first
                self.settle(sent.at);
                if let EngineCommand::Keep { play } = sent.command {
                    self.stop(play);
                } else {
                    self.apply(sent.command);
                }
                self.swap(sent.command, sent.brings);
                self.carried.applied(sent.at);
                self.now.applied_one();
            }
            self.settle(now);
            let elapsed = self.now.state.clock.elapsed_now();
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
                        .sample(elapsed.saturating_sub(play.start), version.usage, out);
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
            self.shared
                .applied
                .store(self.now.applied(), Ordering::Release);
            if let Some(asked) = asked {
                self.hand.fill(asked, &self.now);
            }
        }
        self
    }

    /// Cross waiting launches and once ends before the next FIFO event. The last
    /// sample of a once-use is its left limit, regardless of how late this wake is.
    fn settle(&mut self, at: Duration) {
        self.apply(EngineCommand::Settle { at });
        let count = self.now.state.clock.elapsed_now();
        for slot in 0..PLAYS {
            if let Some(play) = self.now.state.plays.at(slot)
                && play.state == PlayState::Playing
                && let Some(Some(version)) = self.plans.get(slot)
                && version
                    .plan
                    .finished(count.saturating_sub(play.start), version.usage)
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
                    .elapsed_now()
                    .saturating_sub(play.start),
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
        let (device, writers) = match brings {
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
            Brings::Nothing => (None, None),
            Brings::Device(device) => (Some(device), None),
            Brings::Writers(writers) => (None, Some(writers)),
            Brings::Runs(runs) => {
                if let EngineCommand::Runs { change } = command {
                    for run in runs.into_iter().flatten() {
                        self.apply(change.on(run));
                    }
                }
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

    /// Sends `command` until it is not refused for a full command queue: waits for the
    /// tick to drain it. What came of it once the queue had room.
    fn once_drained(
        engine: &mut Engine,
        sender: &mut Sender,
        command: Command,
    ) -> Result<(), Refusal> {
        loop {
            let refused = sender.send(command);
            if refused != Err(Refusal::QueueFull) {
                return refused;
            }
            let started = Instant::now();
            while engine.fresh().applied() < sender.sent() {
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
        fn take(&mut self, frame: &[Levels]) -> Outcome {
            let levels = frame.iter().flat_map(|universe| universe.0.iter());
            for (cell, level) in self.0.levels.iter().zip(levels) {
                cell.store(*level, Ordering::Relaxed);
            }
            self.0.frames.fetch_add(1, Ordering::Release);
            Outcome::Accepted
        }
    }

    impl Heard {
        /// The channels of a frame made after every command sent so far was applied.
        fn after(&self, engine: &mut Engine, sender: &Sender) -> [u8; 9] {
            let started = Instant::now();
            while engine.fresh().applied() < sender.sent() {
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
                Err(Refusal::QueueFull),
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
            let state = &engine.fresh().state;
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
        while engine.fresh().applied() < sender.sent() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the tick applies"
            );
        }
        assert_eq!(
            sender.send(Command::RemoveLook { look: 1 }),
            Err(Refusal::LookInUse { look: 1 })
        );
        assert!(engine.fresh().state.plays.has(0));
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
            Err(Refusal::LookInUse { look: 11 })
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

    /// A device that takes its time to close: until the test lets it.
    #[cfg(feature = "test-support")]
    struct Closing {
        closing: Arc<AtomicBool>,
        closes: Arc<AtomicBool>,
    }

    #[cfg(feature = "test-support")]
    impl Device for Closing {
        fn take(&mut self, _frame: &[Levels]) -> Outcome {
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
    #[expect(
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        reason = "an invalid fixture, missing play or missing frame fails this stepped test"
    )]
    fn linked_roots_book_aggregate_work_until_the_fifo_drains() {
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
        // Two roots share three nested loop transitions over a hundred lights.
        // Each fits alone. Three plays fit with their terminal samples reserved.
        for (look, child) in [(2, 1), (3, 2), (4, 3), (5, 3)] {
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
        let start = |look, rank| Command::StartLook {
            look,
            rank,
            when: When::Now,
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
            ("replace every parent's shared leaf", leaf(22)),
        ] {
            assert_eq!(sender.send(command), Err(Refusal::WorkLimit), "{case}");
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
        assert_eq!(refusal, Some(Refusal::WorkLimit));
        for play in ids {
            sender.send(Command::ReleaseHold { play }).unwrap();
        }
        // Their release is queued. An additional root must still fit that prefix.
        assert_eq!(sender.send(start(5, Rank::Manual)), Err(Refusal::WorkLimit));
        assert_eq!(picture(&mut engine), (vec![], vec![None; 100]));
        let retry = started(&mut sender, start(5, Rank::Manual));
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
                Err(Refusal::LookInUse { look: 4 })
            );
        }
        sender.send(Command::ReleaseHold { play }).unwrap();
        for look in [1, 3] {
            sender.send(Command::RemoveLook { look }).unwrap();
        }
        wakes.run();
    }
}
