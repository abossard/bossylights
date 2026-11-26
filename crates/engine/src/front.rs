//! Command admission and installation. This is the engine front: it owns the project,
//! patch, plays reserved for admission, and output openings before the tick sees them.

use crate::blackout::{self, Blackout};
use crate::grabs::{Change as GrabSet, Changes, Grabs};
use crate::lane::Lane;
use crate::*;
use looks::Plan;
use rig::{FieldPlan, PatchEffect, Whole};

mod opening;
use opening::{
    Abandon, Abandoned, DeviceSource, Lease, Opener, Recovery, Retry, Unfinished, free_elsewhere,
    on_their_way,
};
pub use opening::{Handoff, Open, Opened, Opening};
pub(crate) use opening::{LaneDelivery, free, recover};

/// Who sends: one per connection ([`Sender::hand`]), and the engine's own. What a hand
/// holds is its own, and goes in the next frame once the hand drops
/// ([`Sender::drop_hand`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hand(pub(crate) u64);

impl Hand {
    /// The engine's own: [`Sender::begin`], [`Sender::send`] and [`Sender::admit`] send
    /// with it. Never given to a connection.
    pub const ENGINE: Self = Self(0);
}

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
    /// The last hand given to a connection.
    hands: u64,
    /// Who keeps Blackout on.
    blackout: Blackout,
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

/// A command, when it was sent, and what it brings.
pub(crate) struct Sent {
    pub(crate) command: EngineCommand,
    pub(crate) at: Duration,
    pub(crate) brings: Brings,
}

/// A replacement reserves one command and one retirement so its prepared lane can
/// always be committed. Other commands, including multi-command edits, share the rest.
struct CommandQueue {
    fifo: rtrb::Producer<Sent>,
    reserved: bool,
    /// One place for the let-go of each hand that holds something: a hand's let-go is
    /// never refused.
    held: usize,
}

impl CommandQueue {
    fn slots(&self) -> usize {
        self.fifo
            .slots()
            .saturating_sub(usize::from(self.reserved) + self.held)
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
pub(crate) enum Brings {
    Nothing,
    /// An opened device on its own thread, for `Attach`.
    Device(LaneDelivery),
    /// An accepted route change to the same adapter, in its new lane.
    Reroute(LaneDelivery, bool),
    /// Tell the opener when prior attachments passed the tick and reclaimer.
    Probe(Arc<AtomicBool>),
    /// The field table of the patch, for `Fields`.
    Fields(FieldPlan),
    /// The grab changes of one press, for `Grabs`: nothing goes back, the front keeps
    /// them until the tick applied them.
    Grabs(Changes),
    /// The plan of a play, for `Start` and `Plan`.
    Plan(Version),
}

/// What goes back from the tick thread, to be freed on the free thread.
#[expect(
    dead_code,
    reason = "held only to be dropped on the free thread, never read"
)]
pub(crate) enum Retired {
    Probe(Arc<AtomicBool>),
    Device(Box<Lane>),
    Fields(FieldPlan),
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
}

impl PriorOutput {
    /// The project as it was before this output's switch: the project goes back to what
    /// the switch replaced (`Restore`). With nothing to go back to, it stays.
    fn restore(self, project: Project) -> Project {
        project
            .update(ProjectEvent::Restore {
                output: self.number,
            })
            .map_or(project, |(restored, _)| restored)
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
        self.project.outputs() == other.project.outputs()
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
    /// Who keeps Blackout on: the front decides with who asks.
    Blackout(engine_core::BlackoutAsk),
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

    #[cfg(all(test, feature = "test-support"))]
    pub(crate) fn keep_output(
        base: Base,
        output: OutputNumber,
        universes: Run,
        rate: Rate,
    ) -> Self {
        Self {
            project: base.project,
            base,
            change: Change::Send(EngineCommand::Attach {
                output,
                universes,
                rate,
            }),
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
                // a route attached now: the old run is no stream to count on either
                if routed.0.get(slot).is_some_and(Option::is_some)
                    && let Some(
                        ProjectOutput::Kind { universes, .. }
                        | ProjectOutput::Attached { universes },
                    ) = project.output(output)
                {
                    project = project
                        .update(ProjectEvent::Routed {
                            output,
                            bounds: universes,
                        })?
                        .0;
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
                    rate,
                }
                | ProjectEffect::Restart {
                    output,
                    kind,
                    universes,
                    rate,
                }
                | ProjectEffect::Keep {
                    output,
                    kind,
                    universes,
                    rate,
                },
            ) => {
                let limits = outputs::limits(kind);
                limits.check(universes, rate)?;
                self.routed.check_edit(
                    &self.patch,
                    &self.project,
                    output,
                    &[universes.span(limits.whole)],
                )?;
                Some(project.update(ProjectEvent::Opened { output, kind })?.0)
            }
            Some(ProjectEffect::Send(EngineCommand::RemoveOutput { .. })) => Some(project),
            Some(
                ProjectEffect::Send(_)
                | ProjectEffect::Blackout(_)
                | ProjectEffect::Look(_)
                | ProjectEffect::Patch(_),
            )
            | None => None,
        };
        if let Some((fixture, _)) = checked.and_then(|p| self.patch.unstreamed(&p)) {
            return Err(Refusal::StreamsFixture { fixture });
        }
        let change = match effect {
            None => Change::Nothing,
            Some(ProjectEffect::Send(command)) => Change::Send(command),
            Some(ProjectEffect::Blackout(ask)) => Change::Blackout(ask),
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
                if plan.is_step() && usage.repeat == Repeat::Once {
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
pub(crate) struct Version {
    pub(crate) play: PlayId,
    pub(crate) sent: u64,
    pub(crate) plan: Arc<Plan>,
    pub(crate) usage: Use,
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

impl Sender {
    pub(crate) fn new(
        universes: Universes,
        open: Open,
        retry: Retry,
        commands: rtrb::Producer<Sent>,
        shared: Arc<Shared>,
        free: Thread,
        clock: Clock,
    ) -> Self {
        let (abandon, abandoned) = std::sync::mpsc::channel();
        Self {
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
                held: 0,
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
            hands: 0,
            blackout: Blackout::new(),
            retired: 0,
            devices: 0,
            sent: 0,
            shared,
            free,
            clock,
            #[cfg(any(test, feature = "test-support"))]
            universes,
        }
    }

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
        self.begin_as(Hand::ENGINE, command, library)
    }

    /// [`Sender::begin`], sent by `hand`: what it holds is its own.
    pub fn begin_as(
        &mut self,
        hand: Hand,
        command: Command,
        library: &[Fixture],
    ) -> Result<Begun, Refusal> {
        self.take_back_abandoned();
        let prepared = self.base().prepare(command, library)?;
        self.admit_as(hand, prepared)
    }

    /// A new hand, for one connection.
    pub fn hand(&mut self) -> Hand {
        self.hands += 1;
        Hand(self.hands)
    }

    /// `hand` is gone: what it holds goes in the next frame. Its let-go kept a place in
    /// the queue since it first held something, so it is never refused for room.
    pub fn drop_hand(&mut self, hand: Hand) -> Result<(), Refusal> {
        self.blackout(hand, blackout::Event::Drops)
    }

    /// Tells the tick who keeps Blackout on after `event` from `hand`, when that changes.
    /// A hand that starts holding keeps a place for its let-go too; refused
    /// (`QueueFull`), nothing changes.
    fn blackout(&mut self, hand: Hand, event: blackout::Event) -> Result<(), Refusal> {
        let step = self.blackout.step(hand, event);
        match step {
            blackout::Step::Same => {}
            blackout::Step::Latch(hold) => {
                self.push(EngineCommand::Blackout { hold }, Brings::Nothing)?
            }
            blackout::Step::Holds(hold) => {
                if self.commands.slots() < 2 {
                    return Err(Refusal::QueueFull);
                }
                self.push(EngineCommand::Blackout { hold }, Brings::Nothing)?;
                self.commands.held += 1;
            }
            blackout::Step::LetsGo(hold) => {
                self.commands.held -= 1;
                if let Err(refused) = self.push(EngineCommand::Blackout { hold }, Brings::Nothing) {
                    self.commands.held += 1;
                    return Err(refused);
                }
            }
        }
        self.blackout.take(hand, step);
        Ok(())
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
        self.admit_as(Hand::ENGINE, prepared)
    }

    /// [`Sender::admit`], sent by `hand`.
    pub fn admit_as(&mut self, hand: Hand, prepared: Prepared) -> Result<Begun, Refusal> {
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
            Change::Blackout(ask) => {
                self.blackout(hand, blackout::Event::Ask(ask))?;
                Begun::Queued
            }
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
                // prepare checked the run against the kind's limits already
                let route = Route::of_run(universes, outputs::limits(kind)).map_err(|_| {
                    Refusal::TooManyUniverses {
                        most: outputs::limits(kind).universes,
                    }
                })?;
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
                match previous
                    .as_ref()
                    .map(|old| old.cutover().apply(CutoverEvent::Begin))
                {
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
                let begun = self.publish(effects, rebound.as_ref(), patch.as_ref())?;
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
                self.publish(Vec::new(), Some(&looks), None)?;
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
                    if let Some(hold) = next_hold(live.as_ref().map(|l| l.hold), HoldEvent::Start) {
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
                self.grabs.sent(self.sent, None);
                Begun::Queued
            }
            Change::ReleaseProperty { property, runs } => {
                let held = self.grabs.releasing(property, &runs);
                let patch = Arc::clone(&self.patch);
                self.grab_changes(run_visits(&held), false, |grabs| {
                    grabs.let_go(Some(property), &held, &patch)
                })?;
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
                    let patch = Arc::clone(&self.patch);
                    self.grab_changes(run_visits(&held), true, |grabs| {
                        grabs.update(grab, value, &patch)
                    })?;
                }
                Begun::Queued
            }
            Change::EndGrab { grab } => {
                if self.grabs.property(grab).is_some() {
                    let end = stops(Rank::Manual, self.looks.rules());
                    let held = self.grabs.held(grab);
                    self.grab_changes(run_visits(&held), false, |grabs| grabs.end(grab, end))?;
                }
                Begun::Queued
            }
            Change::Rules { looks } => {
                let rules = looks.rules();
                self.push(EngineCommand::Rules { rules }, Brings::Nothing)?;
                self.forget(|live| unkept(live.rank, live.hold.state(), rules));
                if unkept(Rank::Manual, PlayState::Kept, rules) {
                    self.grabs.clear_kept();
                    self.grabs.sent(self.sent, None);
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
        self.grabs.passed(applied);
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
                PatchEffect::Fields(patch.prepare()),
                PatchEffect::Send(EngineCommand::Rules {
                    rules: looks.rules(),
                }),
            ],
            None,
            Some(&patch),
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
        patch: Option<&Patch>,
    ) -> Result<Begun, Refusal> {
        let grabbing = effects.iter().any(|e| {
            matches!(
                e,
                PatchEffect::Grab { .. } | PatchEffect::Release { .. } | PatchEffect::Gone { .. }
            )
        });
        let places = effects.iter().map(places).sum::<usize>() + usize::from(grabbing);
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
        let tables = effects
            .iter()
            .filter(|e| matches!(e, PatchEffect::Fields(_)))
            .count();
        let returns = tables + changed.len();
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
        let mut sets = Vec::new();
        let patch = patch.unwrap_or(self.patch.as_ref());
        for effect in effects {
            match effect {
                PatchEffect::Patched { fixture } => begun = Begun::Patched { fixture },
                PatchEffect::Fields(table) => queued.push(Sent {
                    command: EngineCommand::Fields,
                    at,
                    brings: Brings::Fields(table),
                }),
                PatchEffect::Send(command) => queued.push(Sent {
                    command,
                    at,
                    brings: Brings::Nothing,
                }),
                PatchEffect::Gone { lights } => {
                    sets.extend(self.grabs.let_go(None, &[lights], patch));
                }
                PatchEffect::Release { property, runs } => {
                    let held = self.grabs.releasing(property, &runs);
                    sets.extend(self.grabs.let_go(Some(property), &held, patch));
                }
                PatchEffect::Grab { value, runs } => {
                    let grab = GrabId {
                        generation: self.grabbed.wrapping_add(1),
                    };
                    self.grabbed = grab.generation;
                    sets.extend(self.grabs.hold(grab, value, runs, patch));
                    begun = Begun::Grabbed { grab };
                }
            }
        }
        // one command for every grab change of the edit, in the place booked for it
        let sets: Option<Changes> = grabbing.then(|| sets.into());
        if let Some(sets) = &sets {
            queued.push(Sent {
                command: EngineCommand::Grabs,
                at,
                brings: Brings::Grabs(Arc::clone(sets)),
            });
        }
        queued.extend(changed.iter().map(|v| Sent {
            command: EngineCommand::Plan { play: v.play },
            at,
            brings: Brings::Plan(v.clone()),
        }));
        self.sent += chunk.fill_from_iter(queued) as u64;
        if grabbing {
            self.grabs.sent(self.sent, sets);
        }
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

    /// Queues the grab changes `make` makes in the book, all in one wake, in one place of
    /// the queue: the book keeps every box until the tick applied them. `visits` are
    /// booked for the lights they touch, checked against the work limit when `check`.
    /// Nothing changes when there is no room.
    fn grab_changes(
        &mut self,
        visits: usize,
        check: bool,
        make: impl FnOnce(&mut Grabs) -> Vec<GrabSet>,
    ) -> Result<(), Refusal> {
        if visits == 0 {
            return Ok(());
        }
        if check {
            self.check_work(self.playing_work(), visits)?;
        }
        let chunk = self
            .commands
            .write_chunk_uninit(1)
            .map_err(|_| Refusal::QueueFull)?;
        let sets: Changes = make(&mut self.grabs).into();
        let sent = chunk.fill_from_iter(std::iter::once(Sent {
            command: EngineCommand::Grabs,
            at: self.clock.now(),
            brings: Brings::Grabs(Arc::clone(&sets)),
        }));
        self.sent += sent as u64;
        self.grabs.sent(self.sent, Some(sets));
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
        let lane = opened.and_then(|()| lane.ok_or(OpenFailure::Unavailable));
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
        let sent = lane
            .map_err(|why| Refusal::CannotOpen { why })
            .and_then(|delivery| {
                let next = delivery.control.clone();
                let brings = if previous.is_some() {
                    Brings::Reroute(delivery, reuse)
                } else {
                    Brings::Device(delivery)
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
        let route = Route::of_run(universes, outputs::Limits::RAW).map_err(|_| REFUSED_ROUTE)?;
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
            .map_err(|_| REFUSED_ROUTE)?;
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
        let (lane, control) = Lane::start(
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
            Refusal::CannotOpen {
                why: OpenFailure::Thread,
            }
        })?;
        let next = control.clone();
        let delivery = LaneDelivery {
            lane: Box::new(lane),
            control,
        };
        let brings = if previous.is_some() {
            Brings::Reroute(delivery, false)
        } else {
            Brings::Device(delivery)
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

    /// Every light a grab holds, live or kept, after every admitted command: what the
    /// picture shows.
    pub fn held_lights(&self) -> Vec<engine_core::HeldLight> {
        self.grabs.held_lights()
    }

    /// One more for each admitted command that changed the grabs: [`Sender::held_lights`]
    /// changes only with it.
    pub fn grabs_revision(&self) -> u64 {
        self.grabs.revision()
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
            | EngineCommand::Master { .. }
            | EngineCommand::Fields
            | EngineCommand::Grab { .. }
            | EngineCommand::Grabs
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
                        | EngineCommand::Fields
                )
        };
        // one more box on its way back or held by the tick thread: all but a release
        let adds = back && !matches!(command, EngineCommand::Release { .. })
            || matches!(command, EngineCommand::Start { .. });
        if let Err(reason) = self.room(1, usize::from(adds)) {
            free_elsewhere(brings);
            return Err(reason);
        }
        let next = if let Brings::Device(delivery) | Brings::Reroute(delivery, _) = &brings {
            Some(Some(delivery.control.clone()))
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
                .map(|_| PriorOutput {
                    number: output,
                    generation: *generation,
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

fn run_visits(runs: &[LightRun]) -> usize {
    runs.iter().map(|run| usize::from(run.count())).sum()
}

/// The lights an effect's grab changes visit.
fn light_visits(effect: &PatchEffect) -> usize {
    match effect {
        PatchEffect::Release { runs, .. } | PatchEffect::Grab { runs, .. } => run_visits(runs),
        PatchEffect::Gone { lights } => usize::from(lights.count()),
        PatchEffect::Fields(_) | PatchEffect::Send(_) | PatchEffect::Patched { .. } => 0,
    }
}

/// Places of the command queue `effect` takes of its own. The grab changes of an edit
/// share one more.
fn places(effect: &PatchEffect) -> usize {
    match effect {
        PatchEffect::Fields(_) | PatchEffect::Send(_) => 1,
        PatchEffect::Patched { .. }
        | PatchEffect::Release { .. }
        | PatchEffect::Gone { .. }
        | PatchEffect::Grab { .. } => 0,
    }
}

#[cfg(test)]
mod tests;
