//! The hand-over: how the state leaves the tick thread, as one picture of one wake.
//!
//! ```text
//! reader --- an empty box ---> asks -----> tick thread: takes the box as a wake begins,
//!                                          copies its state into it as the wake ends
//! reader <-- the box, filled -- pictures <-+
//! ```
//!
//! Two boxes go round: the reader holds the newest picture, the other one is asked for
//! or on its way back. The queues are `rtrb`, wait-free, and each has room for every box:
//! the tick thread never waits, never fails a push, never frees a box. With no box asked
//! for, it sends no picture.

use crate::{
    EngineCommand, Histogram, LATENCY_BUCKETS, LATENESS_BUCKETS, Levels, MAX_LIGHTS, MAX_OUTPUTS,
    MAX_UNIVERSES, Outcome, OutputNumber, Priority, Rate, State, Status, Universes, bucket,
    percentile, update,
};
use engine_core::health::{HealthSample, press_is_under_target};
use engine_core::{ChannelSet, Claims, Entry, Held, Light, show};
use std::thread;
use std::time::Duration;

/// Boxes going round.
const BOXES: usize = 2;

/// Everything the tick thread holds, as one wake left it. The tick thread works on its
/// own `Snapshot` and hands over a copy: a new field (held values per light, say) is in
/// every picture with no change in the hand-over. Every field is fixed in size, as all
/// state on the tick thread is. The values per light live on the heap, made there once:
/// a copy on the stack would not fit a thread's stack. [`Snapshot::copy_to`] copies
/// without allocating; `clone` allocates, off the tick thread only.
#[derive(Clone)]
pub struct Snapshot {
    pub state: State,
    universes: Universes,
    /// The levels of the raw channels. Past the engine's universes they stay zero.
    held: [Levels; MAX_UNIVERSES as usize],
    /// Which raw channels are held.
    holds: [ChannelSet; MAX_UNIVERSES as usize],
    /// What each light shows, by light number: [`MAX_LIGHTS`] of them, its direct holds
    /// and the plays mixed.
    lights: Box<[Light]>,
    /// The direct holds per light, by light number: [`MAX_LIGHTS`] of them. The tick
    /// thread's own: no reader reads them, so [`Snapshot::copy_to`] leaves them.
    grabs: Box<[Held]>,
    /// Timing and counts for each output since its last attach.
    output_health: [OutputHealth; MAX_OUTPUTS],
    /// The time from a send to the frame that carries it, per output that carries it.
    latency: Times<LATENCY_BUCKETS>,
    /// Commands applied, so far: the first this many the sender queued.
    applied: u64,
    /// The sender's revision at this wake's cutoff.
    revision: u64,
}

/// How late an output's device was handed its frames, so far: on the output's own
/// thread, as the send begins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lateness {
    pub p99: Duration,
    pub worst: Duration,
}

impl Snapshot {
    /// A new engine's: nothing held, no outputs, nothing counted.
    pub(crate) fn new(universes: Universes, priority: Priority) -> Self {
        Self {
            state: State::new(priority),
            universes,
            held: [Levels::ZERO; MAX_UNIVERSES as usize],
            holds: [ChannelSet::NONE; MAX_UNIVERSES as usize],
            lights: vec![Light::DARK; usize::from(MAX_LIGHTS)].into_boxed_slice(),
            grabs: vec![Held::NONE; usize::from(MAX_LIGHTS)].into_boxed_slice(),
            output_health: [OutputHealth::ZERO; MAX_OUTPUTS],
            latency: Times::ZERO,
            applied: 0,
            revision: 0,
        }
    }

    /// The held levels of the engine's universes.
    pub fn universes(&self) -> &[Levels] {
        self.held
            .get(..usize::from(self.universes.count()))
            .unwrap_or_default()
    }

    /// Which raw channels of the engine's universes are held.
    pub fn holds(&self) -> &[ChannelSet] {
        self.holds
            .get(..usize::from(self.universes.count()))
            .unwrap_or_default()
    }

    /// The held values per light, by light number.
    pub fn lights(&self) -> &[Light] {
        &self.lights
    }

    /// Fills the cumulative measurements for this output at the snapshot's wake time.
    /// Returns false if the slot has no output.
    pub fn health_sample(&self, output: OutputNumber, sample: &mut HealthSample) -> bool {
        let slot = output.slot();
        let (Some(Some(device)), Some(health)) =
            (self.state.outputs.get(slot), self.output_health.get(slot))
        else {
            return false;
        };
        *sample = HealthSample {
            at: self.state.now,
            generation: health.generation,
            status: device.status,
            lateness: health.lateness.counts,
            lateness_worst: health.lateness.worst,
            press: health.press.counts,
            press_worst: health.press.worst,
            press_under_target: health.press_under_target,
            press_missing: health.press_missing,
            outages: health.outages,
            prior_runs: health.prior_runs,
            prior_outages: health.prior_outages,
            frames_handed: health.handed,
            dropped: device.counters.dropped,
            failed: device.counters.failed,
            off_beat: device.counters.off_beat,
        };
        true
    }

    /// How late this output's device was handed its frames. `None` before its first.
    pub fn lateness(&self, output: OutputNumber) -> Option<Lateness> {
        let times = &self.output_health.get(output.slot())?.lateness;
        Some(Lateness {
            p99: times.percentile(99)?,
            worst: times.worst,
        })
    }

    /// Frames handed to this output's own thread since it was attached, this wake's
    /// too. What came of each is counted in its counters once its thread is done with it,
    /// a wake or so later: the frame this picture's levels went out in is this number.
    pub fn handed(&self, output: OutputNumber) -> u64 {
        self.output_health
            .get(output.slot())
            .map_or(0, |h| h.handed)
    }

    /// The stepped ordering test samples the histogram at handoff and at the end of the wake.
    #[cfg(all(test, feature = "test-support"))]
    pub(crate) fn presses_in_first_bucket(&self) -> u64 {
        self.output_health
            .iter()
            .filter_map(|health| health.press.counts.first())
            .sum()
    }

    /// The time from a send to the frame that carries it, for 95 of 100 commands:
    /// counted once per output, when that output's next frame is handed over. `None`
    /// before the first frame that carried a command.
    pub fn send_to_frame_p95(&self) -> Option<Duration> {
        self.latency.percentile(95)
    }

    /// How many commands this wake and the ones before applied: the first this many the
    /// sender queued ([`crate::Sender::sent`]). Pairs a snapshot with the project it was
    /// taken under.
    pub fn applied(&self) -> u64 {
        self.applied
    }

    /// The changes the sender ended before this wake's cutoff ([`crate::Sender::revise`]).
    /// With [`Snapshot::applied`], pairs a snapshot with the project it was taken under:
    /// a change that queued no command is in it only if it ended before the cutoff.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Makes `to` this snapshot without allocating: both have every light.
    pub(crate) fn copy_to(&self, to: &mut Self) {
        let Self {
            state,
            universes,
            held,
            holds,
            lights,
            grabs: _,
            output_health,
            latency,
            applied,
            revision,
        } = self;
        to.state = *state;
        to.universes = *universes;
        to.held = *held;
        to.holds = *holds;
        if to.lights.len() == lights.len() {
            to.lights.copy_from_slice(lights);
        }
        to.output_health.copy_from_slice(output_health);
        to.latency = *latency;
        to.applied = *applied;
        to.revision = *revision;
    }

    /// The sender's revision read at this wake's cutoff.
    pub(crate) fn revised(&mut self, revision: u64) {
        self.revision = revision;
    }

    /// One more command was applied.
    pub(crate) fn applied_one(&mut self) {
        self.applied = self.applied.wrapping_add(1);
    }

    /// The only change to the state, the held levels and the held values per light: through `engine_core::update`.
    pub(crate) fn update(&mut self, command: EngineCommand) {
        let count = usize::from(self.universes.count());
        let held = self.held.get_mut(..count).unwrap_or_default();
        let holds = self.holds.get_mut(..count).unwrap_or_default();
        self.state = update(self.state, held, holds, &mut self.grabs, command);
    }

    /// What came of a frame of the attach `generation`. Counts an outage on the transition
    /// into reconnecting, using the core's status, only for the output's current generation.
    pub(crate) fn sent(
        &mut self,
        output: OutputNumber,
        outcome: Outcome,
        at: Duration,
        generation: u64,
    ) {
        let slot = output.slot();
        let status = |state: &State| state.outputs.get(slot)?.as_ref().map(|o| o.status);
        let before = status(&self.state);
        self.update(EngineCommand::Sent {
            output,
            outcome,
            at,
        });
        if generation == self.generation(slot)
            && matches!(
                (before, status(&self.state)),
                (
                    Some(Status::Connecting | Status::Running),
                    Some(Status::Reconnecting { .. })
                )
            )
            && let Some(health) = self.output_health.get_mut(slot)
        {
            health.outages = health.outages.wrapping_add(1);
        }
    }

    /// What each light shows now: its direct holds and the plays, `plan` giving each
    /// play's entries by slot.
    pub(crate) fn show<'a>(&mut self, plan: impl Fn(usize) -> &'a [Entry], claims: &mut [Claims]) {
        show(
            &self.grabs,
            &self.state.plays,
            plan,
            self.state.rules,
            &mut self.lights,
            claims,
        );
    }

    /// A frame of `output` was handed over this late.
    pub(crate) fn late(&mut self, output: usize, time: Duration) {
        if let Some(health) = self.output_health.get_mut(output) {
            health.lateness.count(time);
        }
    }

    /// A frame went to `output`'s own thread.
    pub(crate) fn handed_one(&mut self, output: usize) {
        if let Some(health) = self.output_health.get_mut(output) {
            health.handed = health.handed.wrapping_add(1);
        }
    }

    /// An attach starts the next generation. A removal clears this slot's measurements.
    pub(crate) fn attached(&mut self, output: usize) {
        if let Some(health) = self.output_health.get_mut(output) {
            let present = self.state.outputs.get(output).is_some_and(Option::is_some);
            let generation = health.generation.wrapping_add(u64::from(present));
            // Assigned whole, then the generation: `OutputHealth { generation, ..ZERO }`
            // builds a 12 KiB temporary on the tick's stack in a debug build.
            *health = OutputHealth::ZERO;
            health.generation = generation;
        }
    }

    /// T9. Old results were counted before this reset. Carry the device's outage
    /// history on reuse and install the new lane's pre-switch measurements.
    pub(crate) fn cutover(
        &mut self,
        output: usize,
        reuse: bool,
        handed: u64,
        lateness: &Histogram<LATENESS_BUCKETS>,
        worst: Duration,
    ) {
        let prior = self.output_health.get(output).filter(|_| reuse).map(|old| {
            (
                old.prior_runs.wrapping_add(1),
                old.prior_outages.wrapping_add(old.outages),
            )
        });
        self.attached(output);
        if let Some(health) = self.output_health.get_mut(output) {
            (health.prior_runs, health.prior_outages) = prior.unwrap_or_default();
            health.handed = handed;
            health.lateness.counts.copy_from_slice(lateness);
            health.lateness.worst = worst;
        }
    }

    pub(crate) fn generation(&self, output: usize) -> u64 {
        self.output_health.get(output).map_or(0, |h| h.generation)
    }

    /// One command reached its first accepted frame at this latency.
    pub(crate) fn pressed(&mut self, output: usize, latency: Duration, rate: Rate) {
        if let Some(health) = self.output_health.get_mut(output) {
            health.press.count(latency);
            health.press_under_target = health
                .press_under_target
                .wrapping_add(u64::from(press_is_under_target(latency, rate)));
        }
    }

    /// The ring had already overwritten these commands' send times.
    pub(crate) fn press_missing(&mut self, output: usize, count: u64) {
        if let Some(health) = self.output_health.get_mut(output) {
            health.press_missing = health.press_missing.wrapping_add(count);
        }
    }

    /// A command took this long from its send to an output's frame.
    pub(crate) fn carried(&mut self, time: Duration) {
        self.latency.count(time);
    }
}

#[derive(Clone, Copy)]
struct OutputHealth {
    lateness: Times<LATENESS_BUCKETS>,
    press: Times<LATENCY_BUCKETS>,
    generation: u64,
    handed: u64,
    press_under_target: u64,
    press_missing: u64,
    outages: u64,
    prior_runs: u64,
    prior_outages: u64,
}

impl OutputHealth {
    const ZERO: Self = Self {
        lateness: Times::ZERO,
        press: Times::ZERO,
        generation: 0,
        handed: 0,
        press_under_target: 0,
        press_missing: 0,
        outages: 0,
        prior_runs: 0,
        prior_outages: 0,
    };
}

/// Times counted in a histogram, and the latest one exactly: the histogram's last bucket
/// holds everything later.
#[derive(Clone, Copy)]
struct Times<const N: usize> {
    counts: Histogram<N>,
    worst: Duration,
}

#[cfg(all(test, target_pointer_width = "64"))]
#[test]
fn snapshot_has_one_press_histogram_and_six_counts_per_output() {
    const BEFORE: usize = 77_208; // Inline bytes with the existing lateness and global latency.
    let added = MAX_OUTPUTS * (size_of::<Times<LATENCY_BUCKETS>>() + 6 * size_of::<u64>());
    assert_eq!(size_of::<Snapshot>(), BEFORE + added);
}

#[cfg(test)]
#[test]
#[expect(clippy::unwrap_used, reason = "the test's output and run are valid")]
fn only_a_frame_of_the_current_generation_counts_an_outage() {
    for (generation, outages) in [(1, 1), (0, 0)] {
        let output = OutputNumber::try_from(1).unwrap();
        let mut now = Snapshot::new(Universes::ONE, Priority::RealTime);
        now.update(EngineCommand::Attach {
            output,
            universes: crate::RunNumbers { first: 1, count: 1 }
                .check(Universes::ONE)
                .unwrap(),
            rate: Rate::Sixty,
        });
        now.attached(output.slot());
        now.sent(output, Outcome::Accepted, Duration::ZERO, 1);
        now.sent(output, Outcome::Failed, Duration::ZERO, generation);
        let health = now.output_health.first().unwrap();
        assert_eq!((health.generation, health.outages), (1, outages));
    }
}

impl<const N: usize> Times<N> {
    const ZERO: Self = Self {
        counts: [0; N],
        worst: Duration::ZERO,
    };

    fn count(&mut self, time: Duration) {
        if let Some(count) = self.counts.get_mut(bucket::<N>(time)) {
            *count += 1;
        }
        self.worst = self.worst.max(time);
    }

    fn percentile(&self, percent: u64) -> Option<Duration> {
        percentile(&self.counts, percent, self.worst)
    }
}

/// The tick thread's end of the hand-over.
pub(crate) struct Hand {
    asks: rtrb::Consumer<Box<Snapshot>>,
    pictures: rtrb::Producer<Box<Snapshot>>,
}

/// A box the reader asked to be filled, taken as a wake begins.
pub(crate) struct Asked(Box<Snapshot>);

impl Hand {
    /// The box a reader sent, if any. Taken before the wake applies its commands, so
    /// its picture carries every command sent before the ask.
    pub(crate) fn asked(&mut self) -> Option<Asked> {
        self.asks.pop().ok().map(Asked)
    }

    /// Copies `now` into the box and sends it back.
    pub(crate) fn fill(&mut self, Asked(mut picture): Asked, now: &Snapshot) {
        now.copy_to(&mut picture);
        // The queue has room for every box: this push cannot fail, so no box is freed here.
        let _ = self.pictures.push(picture);
    }
}

/// The reader's end of the hand-over: the newest picture and the box that asks for the
/// next one.
pub(crate) struct Reader {
    asks: rtrb::Producer<Box<Snapshot>>,
    pictures: rtrb::Consumer<Box<Snapshot>>,
    latest: Box<Snapshot>,
    /// The other box, while it is here. `None` while it is asked for or on its way back.
    spare: Option<Box<Snapshot>>,
}

/// Queues, before the reader has its boxes.
pub(crate) struct Queues {
    asks: rtrb::Producer<Box<Snapshot>>,
    pictures: rtrb::Consumer<Box<Snapshot>>,
}

/// Both ends of a new hand-over. The queues are made here, the boxes by the reader.
pub(crate) fn hand_over() -> (Hand, Queues) {
    let (asks, asked) = rtrb::RingBuffer::new(BOXES);
    let (filled, pictures) = rtrb::RingBuffer::new(BOXES);
    let hand = Hand {
        asks: asked,
        pictures: filled,
    };
    (hand, Queues { asks, pictures })
}

impl Reader {
    /// The reader, with `first` as its picture until the tick thread sends one.
    pub(crate) fn new(queues: Queues, first: Snapshot) -> Self {
        Self {
            asks: queues.asks,
            pictures: queues.pictures,
            spare: Some(Box::new(first.clone())),
            latest: Box::new(first),
        }
    }

    /// The newest picture the tick thread sent back, and asks for the next. Never waits.
    pub(crate) fn latest(&mut self) -> &Snapshot {
        self.take_back();
        self.ask();
        &self.latest
    }

    /// A picture of a wake that took the ask after this call: it carries every command
    /// sent before. Waits for it, about two wakes.
    pub(crate) fn fresh(&mut self) -> &Snapshot {
        // A box asked for before may come back from an older wake: wait for it first.
        self.wait_for_the_spare();
        self.ask();
        self.wait_for_the_spare();
        &self.latest
    }

    fn wait_for_the_spare(&mut self) {
        while self.spare.is_none() {
            if !self.take_back() {
                thread::sleep(Duration::from_millis(1)); // poll: the tick thread never calls out
            }
        }
    }

    /// Takes a picture that came back as the newest. Says whether one did.
    fn take_back(&mut self) -> bool {
        let Ok(picture) = self.pictures.pop() else {
            return false;
        };
        self.spare = Some(std::mem::replace(&mut self.latest, picture));
        true
    }

    /// Sends the spare box to be filled, if it is here.
    fn ask(&mut self) {
        if let Some(spare) = self.spare.take() {
            // The queue has room for every box.
            if let Err(rtrb::PushError::Full(spare)) = self.asks.push(spare) {
                self.spare = Some(spare);
            }
        }
    }
}

#[cfg(feature = "test-support")]
pub(crate) mod timing {
    use crate::{Clock, Outcome, OutputNumber};
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    /// Events for the scorecard in the engine's monotonic epoch. A returned frame names
    /// its output and deadline, so a later wake cannot charge it for newer commands.
    #[derive(Clone, Copy, Debug)]
    pub enum Timing {
        /// The tick applied this FIFO entry, stamped at admission.
        Applied { number: u64, at: Duration },
        /// The frame for this deadline contains exactly this many applied entries.
        Due {
            output: OutputNumber,
            due: Duration,
            applied: u64,
        },
        /// The device was handed this frame (or it was dropped) at this time.
        Finished {
            output: OutputNumber,
            due: Duration,
            at: Duration,
            outcome: Outcome,
        },
    }

    /// The scorecard's reader. Overflow invalidates a measurement, even across a final drop.
    pub struct Timings {
        clock: Clock,
        events: rtrb::Consumer<Timing>,
        lost: Arc<AtomicBool>,
    }

    impl Timings {
        /// Now in the epoch of the events.
        pub fn now(&self) -> Duration {
            self.clock.now()
        }

        /// Discard the setup traffic before starting a measurement.
        pub fn discard(&mut self) {
            while self.events.pop().is_ok() {}
            self.lost.store(false, Ordering::Release);
        }

        /// Take the next event, failing the run if the bounded stream overflowed.
        pub fn pop(&mut self) -> io::Result<Option<Timing>> {
            if self.lost.load(Ordering::Acquire) {
                return Err(io::Error::other("scorecard timing reader fell behind"));
            }
            Ok(self.events.pop().ok())
        }
    }

    /// Optional wait-free tap for measurements on the wall clock. Absent in product builds.
    pub(crate) struct TimingWriter {
        events: rtrb::Producer<Timing>,
        lost: Arc<AtomicBool>,
    }

    impl TimingWriter {
        pub(crate) fn new(clock: Clock) -> (Self, Timings) {
            let (events, reader) = rtrb::RingBuffer::new(8192);
            let lost = Arc::new(AtomicBool::new(false));
            (
                Self {
                    events,
                    lost: lost.clone(),
                },
                Timings {
                    events: reader,
                    lost,
                    clock,
                },
            )
        }

        pub(crate) fn write(&mut self, event: Timing) {
            if self.events.push(event).is_err() {
                self.lost.store(true, Ordering::Release);
            }
        }
    }
}
