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
use engine_core::{Effect, Light};
use std::thread;
use std::time::Duration;

/// Boxes going round.
const BOXES: usize = 2;

/// Everything the tick thread holds, as one wake left it. The tick thread works on its
/// own `Snapshot` and hands over a copy: a new field (held values per light, say) is in
/// every picture with no change in the hand-over. Every field is fixed in size, as all
/// state on the tick thread is.
#[derive(Clone, Copy)]
pub struct Snapshot {
    pub state: State,
    universes: Universes,
    /// The held levels. Past the engine's universes they stay zero.
    held: [Levels; MAX_UNIVERSES as usize],
    /// The held values per light, by light number.
    lights: [Light; MAX_LIGHTS as usize],
    /// How late each output's frames were handed over, since it was attached.
    lateness: [Times<LATENESS_BUCKETS>; MAX_OUTPUTS],
    /// The time from a send to the frame that carries it, per output that carries it.
    latency: Times<LATENCY_BUCKETS>,
}

/// How late an output's frames were handed over, so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lateness {
    pub p99: Duration,
    pub worst: Duration,
}

impl Snapshot {
    /// A new engine's: nothing held, no outputs, nothing counted.
    pub(crate) const fn new(universes: Universes, priority: Priority) -> Self {
        Self {
            state: State::new(priority),
            universes,
            held: [Levels::ZERO; MAX_UNIVERSES as usize],
            lights: [Light::DARK; MAX_LIGHTS as usize],
            lateness: [Times::ZERO; MAX_OUTPUTS],
            latency: Times::ZERO,
        }
    }

    /// The held levels of the engine's universes.
    pub fn universes(&self) -> &[Levels] {
        self.held
            .get(..usize::from(self.universes.count()))
            .unwrap_or_default()
    }

    /// The held values per light, by light number.
    pub fn lights(&self) -> &[Light] {
        &self.lights
    }

    /// How late this output's frames were handed over. `None` before its first.
    pub fn lateness(&self, output: OutputNumber) -> Option<Lateness> {
        let times = self.lateness.get(output.slot())?;
        Some(Lateness {
            p99: times.percentile(99)?,
            worst: times.worst,
        })
    }

    /// The time from a send to the frame that carries it, for 95 of 100 commands:
    /// counted once per output, when that output's next frame is handed over. `None`
    /// before the first frame that carried a command.
    pub fn send_to_frame_p95(&self) -> Option<Duration> {
        self.latency.percentile(95)
    }

    /// The only change to the state, the held levels and the held values per light: through `engine_core::update`.
    pub(crate) fn update(&mut self, command: EngineCommand) -> Option<Effect> {
        let count = usize::from(self.universes.count());
        let held = self.held.get_mut(..count).unwrap_or_default();
        let (state, effect) = update(self.state, held, &mut self.lights, command);
        self.state = state;
        effect
    }

    /// A frame of `output` was handed over this late.
    pub(crate) fn late(&mut self, output: usize, time: Duration) {
        if let Some(times) = self.lateness.get_mut(output) {
            times.count(time);
        }
    }

    /// A new device at `output`: its lateness starts afresh.
    pub(crate) fn attached(&mut self, output: usize) {
        if let Some(times) = self.lateness.get_mut(output) {
            *times = Times::ZERO;
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
        *picture = *now;
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
            latest: Box::new(first),
            spare: Some(Box::new(first)),
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
