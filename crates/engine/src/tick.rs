//! The mutable tick state, scheduler, and cutover accounting. Nothing in this module
//! waits for the front: commands arrive through one-way queues.

use crate::front::{Brings, Retired, Sent, Version};
use crate::lane::Lane;
use crate::publication::Hand;
use crate::snapshot::{OutputHealth, Publication, Times};
use crate::*;
use engine_core::{ChannelSet, Held};
use rig::WriterPlan;

/// Every channel at rest, on the heap. Its own frame: the 256 KiB a debug build passes
/// through the stack is gone again before the engine starts its threads.
#[inline(never)]
fn channels() -> Box<Channels> {
    Box::new(Channels::NEW)
}

/// What the tick thread owns. Every buffer is made before it starts.
pub(crate) struct Ticker {
    shared: Arc<Shared>,
    commands: rtrb::Consumer<Sent>,
    retired: rtrb::Producer<Retired>,
    /// Each output's own thread, by output number, beside `State::outputs`.
    devices: [Option<Box<Lane>>; MAX_OUTPUTS],
    switching: Box<[Option<Switch>; MAX_OUTPUTS]>,
    /// The patch's writers: lights to bytes, every frame.
    writers: WriterPlan,
    /// Each play's plan, by slot, beside `State::plays`.
    plans: [Option<Version>; PLAYS],
    scratch: [Box<[Entry]>; PLAYS],
    /// Who holds each property of each light this frame: the Mixer's buffer.
    claims: Box<[Claims]>,
    state: State,
    universes: Universes,
    held: Box<[Levels; MAX_UNIVERSES as usize]>,
    holds: Box<[ChannelSet; MAX_UNIVERSES as usize]>,
    lights: Box<[Light]>,
    grabs: Box<[Held]>,
    health: Box<[OutputHealth; MAX_OUTPUTS]>,
    latency: Times<LATENESS_BUCKETS>,
    applied: u64,
    revision: u64,
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
    timings: Option<timing::TimingWriter>,
}

pub(crate) struct TickerStart {
    pub universes: Universes,
    pub shared: Arc<Shared>,
    pub commands: rtrb::Consumer<Sent>,
    pub retired: rtrb::Producer<Retired>,
    pub hand: Hand,
    pub clock: Clock,
    pub trace: Trace,
    #[cfg(feature = "test-support")]
    pub timings: Option<timing::TimingWriter>,
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

impl Ticker {
    fn revised(&mut self, revision: u64) {
        self.revision = revision;
    }

    fn applied(&self) -> u64 {
        self.applied
    }

    fn applied_one(&mut self) {
        self.applied = self.applied.wrapping_add(1);
    }

    fn update(&mut self, command: EngineCommand) {
        let count = usize::from(self.universes.count());
        let held = self.held.get_mut(..count).unwrap_or_default();
        let holds = self.holds.get_mut(..count).unwrap_or_default();
        self.state = update(self.state, held, holds, &mut self.grabs, command);
    }

    fn sent(
        &mut self,
        output: OutputNumber,
        (outcome, watch): (Outcome, Watch),
        at: Duration,
        generation: u64,
    ) {
        let slot = output.slot();
        let current = generation == self.generation(slot);
        let before = self.state.attached_output(output).map(Output::connection);
        self.update(EngineCommand::Sent {
            output,
            outcome,
            watch: if current { watch } else { Watch::Unwatched },
            at,
        });
        if current
            && !matches!(before, Some(Status::Reconnecting { .. }))
            && matches!(
                self.state.attached_output(output).map(Output::connection),
                Some(Status::Reconnecting { .. })
            )
            && let Some(health) = self.health.get_mut(slot)
        {
            health.outages = health.outages.wrapping_add(1);
        }
    }

    fn late(&mut self, output: usize, time: Duration) {
        if let Some(health) = self.health.get_mut(output) {
            health.lateness.count(time);
        }
    }

    fn handed_one(&mut self, output: usize) {
        if let Some(health) = self.health.get_mut(output) {
            health.handed = health.handed.wrapping_add(1);
        }
    }

    fn attached(&mut self, output: usize) {
        if let Some(health) = self.health.get_mut(output) {
            let present =
                OutputNumber::from_slot(output).is_some_and(|number| self.state.has_output(number));
            let generation = health.generation.wrapping_add(u64::from(present));
            *health = OutputHealth::ZERO;
            health.generation = generation;
        }
    }

    fn cutover(
        &mut self,
        output: usize,
        reuse: bool,
        handed: u64,
        lateness: &Times<LATENESS_BUCKETS>,
    ) {
        let prior = self.health.get(output).filter(|_| reuse).map(|old| {
            (
                old.prior_runs.wrapping_add(1),
                old.prior_outages.wrapping_add(old.outages),
            )
        });
        self.attached(output);
        if let Some(health) = self.health.get_mut(output) {
            (health.prior_runs, health.prior_outages) = prior.unwrap_or_default();
            health.handed = handed;
            health.lateness = *lateness;
        }
    }

    fn generation(&self, output: usize) -> u64 {
        self.health
            .get(output)
            .map_or(0, |health| health.generation)
    }

    fn count_accepted(&mut self, frame: Accepted) {
        let output = self.state.attached_output(frame.output);
        let (carried, health) = (&mut self.carried, &mut self.health);
        carried.accepted(frame, output, |counted| match counted {
            Counted::Missing { output, count } => {
                if let Some(health) = health.get_mut(output) {
                    health.press_missing = health.press_missing.wrapping_add(count);
                }
            }
            Counted::Pressed {
                output,
                latency,
                schedule,
            } => {
                if let Some(health) = health.get_mut(output) {
                    health.press.count(latency);
                    health.press_under_target = health
                        .press_under_target
                        .wrapping_add(u64::from(schedule.press_met(latency)));
                }
            }
        });
    }

    #[cfg(all(test, feature = "test-support"))]
    fn presses_in_first_bucket(&self) -> u64 {
        self.health
            .iter()
            .filter_map(|health| health.press.counts.first())
            .sum()
    }
}

/// Capacity of each send-time history. Overwritten press stamps count as missing.
const CARRY: usize = 4 * QUEUE;

#[derive(Clone, Copy)]
struct Accepted {
    output: OutputNumber,
    prefix: lane::Prefix,
    at: Duration,
}

enum Counted {
    Missing {
        output: usize,
        count: u64,
    },
    Pressed {
        output: usize,
        latency: Duration,
        schedule: Output,
    },
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
    fn accepted(&mut self, frame: Accepted, output: Option<Output>, mut each: impl FnMut(Counted)) {
        let slot = frame.output.slot();
        let (Some(previous), Some(output)) = (self.last_accepted.get_mut(slot), output) else {
            return;
        };
        let through = frame.prefix.presses;
        let pending = through.wrapping_sub(*previous);
        let newer = self.presses.total.wrapping_sub(through);
        let retained = pending.min((CARRY as u64).saturating_sub(newer));
        each(Counted::Missing {
            output: slot,
            count: pending - retained,
        });
        for distance in (1..=retained).rev() {
            if let Some(sent) = self.presses.at(through.wrapping_sub(distance)) {
                each(Counted::Pressed {
                    output: slot,
                    latency: frame.at.saturating_sub(sent),
                    schedule: output,
                });
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

    fn cutover(&mut self, output: usize, accepted: u64) {
        if let Some(prefix) = self.last_accepted.get_mut(output) {
            *prefix = accepted;
        }
    }
}

/// Where stamp number `n` lives in its ring.
fn ring(n: u64) -> usize {
    usize::try_from(n % CARRY as u64).unwrap_or(0)
}

impl Ticker {
    pub(crate) fn new(start: TickerStart) -> io::Result<Self> {
        let TickerStart {
            universes,
            shared,
            commands,
            retired,
            hand,
            clock,
            trace,
            #[cfg(feature = "test-support")]
            timings,
        } = start;
        let first = engine_core::LightNumber::new(0, LIGHTS)
            .and_then(|first| LightRun::new(first, 1, LIGHTS))
            .ok_or_else(|| io::Error::other("empty lighting engine"))?;
        Ok(Self {
            shared,
            commands,
            retired,
            devices: std::array::from_fn(|_| None),
            switching: Box::new(std::array::from_fn(|_| None)),
            writers: WriterPlan::default(),
            plans: std::array::from_fn(|_| None),
            scratch: std::array::from_fn(|_| {
                vec![
                    Entry {
                        lights: first,
                        values: Light::DARK
                    };
                    usize::from(MAX_LIGHTS)
                ]
                .into_boxed_slice()
            }),
            claims: vec![[None; Property::ALL.len()]; usize::from(MAX_LIGHTS)].into_boxed_slice(),
            state: State::new(Priority::RealTime),
            universes,
            held: Box::new([Levels::ZERO; MAX_UNIVERSES as usize]),
            holds: Box::new([ChannelSet::NONE; MAX_UNIVERSES as usize]),
            lights: vec![Light::DARK; usize::from(MAX_LIGHTS)].into_boxed_slice(),
            grabs: vec![Held::NONE; usize::from(MAX_LIGHTS)].into_boxed_slice(),
            health: Box::new([OutputHealth::ZERO; MAX_OUTPUTS]),
            latency: Times::ZERO,
            applied: 0,
            revision: 0,
            hand,
            channels: channels(),
            carried: Box::new(Carried::new()),
            staged: Box::new([None; (lane::BOXES + 1) * MAX_OUTPUTS]),
            clock,
            trace,
            #[cfg(feature = "test-support")]
            timings,
        })
    }

    /// The real-time loop, an action: reads the clock, waits, hands frames over and
    /// the snapshot. The logic is in `engine-core`. Nothing here may lock, allocate,
    /// block, log or call out. Returns itself, so its devices are freed by whoever joins it.
    pub(crate) fn run(mut self, priority: Priority) -> Self {
        self.state = State::new(priority);
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
                lane.as_ref()
                    .map_or(CutoverEffect::Nothing, |lane| lane.wake_action())
            });
            // read after the commands are counted: a change it counts has all its
            // commands in this wake, or its moment's commands are more than applied
            self.revised(self.shared.revision.load(Ordering::Acquire));
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
                        Brings::Device(delivery) => *action = delivery.lane.wake_action(),
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
                        .map_or(CutoverEffect::Nothing, |lane| lane.wake_action());
                }
                self.carried.applied(&sent.command, sent.at);
                self.applied_one();
                #[cfg(feature = "test-support")]
                {
                    let applied = self.applied;
                    if let Some(timings) = &mut self.timings {
                        timings.write(Timing::Applied {
                            number: applied,
                            at: sent.at,
                        });
                    }
                }
            }
            self.settle(now);
            for play in self
                .state
                .active_plays()
                .filter(|p| p.state == PlayState::Playing)
            {
                let slot = play.play.slot();
                if let (Some(Some(version)), Some(out)) =
                    (self.plans.get(slot), self.scratch.get_mut(slot))
                {
                    version
                        .plan
                        .sample(self.state.elapsed_since(play.start), version.usage, out);
                }
            }
            {
                let Self {
                    state,
                    universes,
                    held,
                    holds,
                    lights,
                    grabs,
                    plans,
                    scratch,
                    claims,
                    writers,
                    channels,
                    ..
                } = &mut self;
                state.show_plays(
                    grabs,
                    |slot| {
                        plans
                            .get(slot)
                            .and_then(Option::as_ref)
                            .zip(scratch.get(slot))
                            .map_or(&[], |(v, out)| v.plan.entries(out))
                    },
                    lights,
                    claims,
                );
                let count = usize::from(universes.count());
                writers.frame(
                    state,
                    held.get(..count).unwrap_or_default(),
                    holds.get(..count).unwrap_or_default(),
                    lights,
                    channels,
                );
            }
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
                let handed = self.clock.now();
                let (carried, latency) = (&mut self.carried, &mut self.latency);
                carried.handed(number.slot(), handed, |time| latency.count(time));
            }
            self.shared.wake.fetch_add(1, Ordering::Release);
            // Each lane returns frames in order. Across its accepted prefixes this
            // wake, at most CARRY surviving send times can be visited.
            for slot in 0..self.staged.len() {
                if let Some(frame) = self.staged.get_mut(slot).and_then(Option::take) {
                    self.count_accepted(frame);
                    self.trace.counted(frame.output);
                }
            }
            self.shared.applied.store(self.applied(), Ordering::Release);
            if let Some(asked) = asked {
                let Self {
                    ref mut hand,
                    ref state,
                    ref universes,
                    ref held,
                    ref holds,
                    ref lights,
                    ref health,
                    ref latency,
                    ref applied,
                    ref revision,
                    ..
                } = self;
                hand.fill(
                    asked,
                    Publication {
                        state: *state,
                        universes: *universes,
                        held,
                        holds,
                        lights,
                        output_health: health.as_ref(),
                        latency: *latency,
                        applied: *applied,
                        revision: *revision,
                    },
                );
            }
        }
        for lane in self.devices.iter().flatten() {
            lane.stop_cutover();
        }
        self
    }

    /// Cross waiting launches and once ends before the next FIFO event. The last
    /// sample of a once-use is its left limit, regardless of how late this wake is.
    fn settle(&mut self, at: Duration) {
        self.apply(EngineCommand::Settle { at });
        for slot in 0..PLAYS {
            if let Some(play) = self.state.play_at(slot)
                && play.state == PlayState::Playing
                && let Some(Some(version)) = self.plans.get(slot)
                && version.usage.repeat == Repeat::Once
                && version
                    .plan
                    .finished(self.state.elapsed_since(play.start), version.usage)
            {
                match stops(play.rank, self.state.rules) {
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
        let Some(play) = self.state.play_at(slot).filter(|p| p.play == id) else {
            return;
        };
        if play.state == PlayState::Kept {
            return;
        }
        if play.state == PlayState::Playing
            && let (Some(Some(version)), Some(out)) =
                (self.plans.get(slot), self.scratch.get_mut(slot))
            && version
                .plan
                .sample(self.state.elapsed_since(play.start), version.usage, out)
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
                if !self.state.has_play(slot) {
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
                if self.state.takes_plan(version.play) {
                    self.replan(slot, Some(version));
                } else {
                    // Its reserved return carries the unused replacement itself.
                    let _ = self.retired.push(Retired::Plan(version.plan));
                }
                return;
            }
            Brings::Reroute(delivery, reuse) => {
                let next = delivery.lane;
                if let EngineCommand::Attach {
                    output,
                    universes,
                    rate,
                } = command
                    && let Some(Some(old)) = self.devices.get(output.slot())
                    && let Some(place) = self.switching.get_mut(output.slot())
                    && let Some(schedule) = self.state.rerouted_output(output, universes, rate)
                {
                    old.publish_cutoff();
                    if !old.cutoff() {
                        // stopped (T13): no switch
                        let _ = self.retired.push(Retired::Device(next));
                        return;
                    }
                    *place = Some(Switch {
                        next,
                        schedule: schedule.fresh_lane(),
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
            Brings::Device(delivery) => Some(delivery.lane),
            Brings::Probe(probe) => {
                let _ = self.retired.push(Retired::Probe(probe));
                return;
            }
            Brings::Nothing => {
                if let EngineCommand::Attach { output, .. } = command {
                    self.carried.attached(output.slot(), true);
                    self.attached(output.slot());
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
        self.attached(output.slot());
    }

    /// While a transfer drains, the held lane's rate owns the scheduler.
    fn scheduled(&self) -> State {
        let mut scheduled = self.state;
        if !self.shared.life.get().sends() {
            return scheduled.without_output_schedules();
        }
        for (slot, switch) in self.switching.iter().enumerate() {
            if let (Some(number), Some(switch)) = (OutputNumber::from_slot(slot), switch) {
                scheduled = scheduled.with_output_schedule(number, switch.schedule);
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
            for staged in 0..self.staged.len() {
                if self
                    .staged
                    .get(staged)
                    .is_some_and(|sent| sent.is_some_and(|sent| sent.output.slot() == slot))
                    && let Some(sent) = self.staged.get_mut(staged).and_then(Option::take)
                {
                    self.count_accepted(sent);
                    self.trace.counted(sent.output);
                }
            }
            let Some(installed) = self.devices.get_mut(slot) else {
                continue;
            };
            if let Some(old) = installed.replace(switch.next) {
                let _ = self.retired.push(Retired::Device(old));
            }
            if let Some(number) = OutputNumber::from_slot(slot) {
                self.state =
                    self.state
                        .commit_output_schedule(number, switch.schedule, switch.reuse);
            }
            self.cutover(slot, switch.reuse, switch.handed, &switch.lateness);
            self.carried.cutover(slot, switch.presses);
            if let Some(new) = self.devices.get(slot).and_then(Option::as_ref) {
                // T9 commits: the serving lane number moves to the new lane. Stopped since
                // the peek (T13), each lane ends by the engine's life instead.
                new.show_new();
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
            let generation = self.generation(slot).wrapping_add(1);
            let Some(switch) = self.switching.get_mut(slot).and_then(Option::as_mut) else {
                return;
            };
            let output = switch.schedule;
            switch.schedule = output.handed(at);
            (
                output,
                switch.schedule.deadline(),
                &mut switch.next,
                generation,
            )
        } else {
            if !matches!(action, CutoverEffect::SendOld | CutoverEffect::ShowNew) {
                return;
            }
            let Some(output) = self.state.attached_output(number) else {
                return;
            };
            self.apply(EngineCommand::Handed { output: number, at });
            let generation = self.generation(slot);
            let Some(lane) = self.devices.get_mut(slot).and_then(Option::as_mut) else {
                return;
            };
            let next = self
                .state
                .attached_output(number)
                .map_or(at, Output::deadline);
            (output, next, lane, generation)
        };
        #[cfg(feature = "test-support")]
        let applied = self.applied;
        self.trace.handed(number, output.deadline(), at);
        #[cfg(feature = "test-support")]
        if let Some(timings) = &mut self.timings {
            timings.write(Timing::Due {
                output: number,
                due: output.deadline(),
                applied,
            });
        }
        let frame = self.channels.values();
        let prefix = lane::Prefix {
            presses: self.carried.presses.total,
            generation,
        };
        let handed = lane.hand(frame, output.deadline(), next, prefix);
        if handed != lane::Hand::Full {
            if let Some(Some(switch)) = self.switching.get_mut(slot) {
                switch.handed = switch.handed.wrapping_add(1);
            } else {
                self.handed_one(slot);
            }
            #[cfg(all(test, feature = "test-support"))]
            self.shared
                .presses_at_handoff
                .store(self.presses_in_first_bucket(), Ordering::Relaxed);
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
                switch.schedule = switch.schedule.sent(outcome, Watch::Unwatched, at);
                switch.lateness.count(at.saturating_sub(due));
            } else {
                self.late(slot, at.saturating_sub(due));
                self.sent(number, (outcome, Watch::Unwatched), at, prefix.generation);
            }
        }
    }

    /// Counts what came of the frames each output's thread handed back since the last
    /// wake: the outcome, and how late the device was handed each.
    fn back(&mut self) {
        for slot in 0..MAX_OUTPUTS {
            let Some(number) = u8::try_from(slot + 1)
                .ok()
                .and_then(|number| OutputNumber::try_from(number).ok())
            else {
                continue;
            };
            let mut returned: [Option<(lane::Sent, Watch)>; lane::BOXES] = [None; lane::BOXES];
            if let Some(lane) = self.devices.get_mut(slot).and_then(Option::as_mut) {
                let mut place = returned.iter_mut();
                lane.returned(|sent, watch| {
                    if let Some(cell) = place.next() {
                        *cell = Some((sent, watch));
                    }
                });
            }
            for (sent, watch) in returned.into_iter().flatten() {
                let output = number;
                self.trace.came(output, sent.outcome, sent.due);
                #[cfg(feature = "test-support")]
                if let Some(writer) = &mut self.timings {
                    writer.write(Timing::Finished {
                        output,
                        due: sent.due,
                        at: sent.at,
                        outcome: sent.outcome,
                    });
                }
                self.late(slot, sent.at.saturating_sub(sent.due));
                // A watched lane's failed frame is a lost device, even if it had only
                // dropped frames before (core's Status::after).
                self.sent(
                    output,
                    (sent.outcome, watch),
                    sent.at,
                    sent.prefix.generation,
                );
                // Reconfiguration can keep the lane with an old frame still in flight:
                // its status, counters and lateness count, its presses and outage not.
                if sent.outcome == Outcome::Accepted
                    && sent.prefix.generation == self.generation(slot)
                    && let Some(place) = self.staged.iter_mut().find(|place| place.is_none())
                {
                    *place = Some(Accepted {
                        output,
                        prefix: sent.prefix,
                        at: sent.at,
                    });
                }
            }
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
        self.update(command);
    }
}

#[cfg(test)]
mod tests;
