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
    MAX_UNIVERSES, OutputNumber, Priority, State, Universes, bucket, percentile, update,
};
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
    /// How late each output's device was handed its frames, since it was attached.
    lateness: [Times<LATENESS_BUCKETS>; MAX_OUTPUTS],
    /// Frames handed to each output's own thread, since it was attached.
    handed: [u64; MAX_OUTPUTS],
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
            lateness: [Times::ZERO; MAX_OUTPUTS],
            handed: [0; MAX_OUTPUTS],
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

    /// How late this output's device was handed its frames. `None` before its first.
    pub fn lateness(&self, output: OutputNumber) -> Option<Lateness> {
        let times = self.lateness.get(output.slot())?;
        Some(Lateness {
            p99: times.percentile(99)?,
            worst: times.worst,
        })
    }

    /// Frames handed to this output's own thread since it was attached, this wake's
    /// too. What came of each is counted in its counters once its thread is done with it,
    /// a wake or so later: the frame this picture's levels went out in is this number.
    pub fn handed(&self, output: OutputNumber) -> u64 {
        self.handed.get(output.slot()).copied().unwrap_or(0)
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
            lateness,
            handed,
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
        to.lateness = *lateness;
        to.handed = *handed;
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
        if let Some(times) = self.lateness.get_mut(output) {
            times.count(time);
        }
    }

    /// A frame went to `output`'s own thread.
    pub(crate) fn handed_one(&mut self, output: usize) {
        if let Some(handed) = self.handed.get_mut(output) {
            *handed = handed.wrapping_add(1);
        }
    }

    /// A new device at `output`: its lateness and frames start afresh.
    pub(crate) fn attached(&mut self, output: usize) {
        if let Some(times) = self.lateness.get_mut(output) {
            *times = Times::ZERO;
        }
        if let Some(handed) = self.handed.get_mut(output) {
            *handed = 0;
        }
    }

    /// A command took this long from its send to an output's frame.
    pub(crate) fn carried(&mut self, time: Duration) {
        self.latency.count(time);
    }
}

/// Times counted in a histogram, and the latest one exactly: the histogram's last bucket
/// holds everything later.
#[derive(Clone, Copy)]
struct Times<const N: usize> {
    counts: Histogram<N>,
    worst: Duration,
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
