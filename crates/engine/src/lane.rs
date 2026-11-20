//! Each output sends on a thread of its own, so the tick is never late behind a send
//! (#174). The tick copies the frame into a spare box and hands it over; the output's
//! thread sends the newest box and hands every box back with what came of it.
//!
//! ```text
//! tick thread                                output's thread, real-time, one per output
//!   copy the frame into a spare box --frames--> the newest: device.take
//!                                               older ones: passed over, Dropped
//!   spare again, what came of it <-----back---- the box, with its outcome and time
//! ```
//!
//! A box goes round: spare (tick) → waiting → sending or passed over → back → spare.
//! [`BOXES`] boxes per output, made with the lane, off the tick thread.
//!
//! | The tick hands over a frame with | What came of it |
//! |---|---|
//! | a spare box | the device's outcome, once the box is back |
//! | no spare box: the device is still on older frames | `Dropped`, at once ([`Hand::Full`]) |
//!
//! The tick wakes the output's thread when it hands it a frame (`unpark`, wait-free). The
//! thread never sleeps longer than until [`LEAD`] before its next frame is due (each box
//! says when), then it looks every [`LOOK`]: a thread that slept long reaches the CPU
//! late when woken. Measured at 16 outputs of 64 universes, under load:
//!
//! | How the output's thread waits | p99 lateness, output 1 / output 16 |
//! |---|---|
//! | parked until the tick wakes it at its hand-over | 100 / 1200 µs |
//! | parked until the tick wakes it after every hand-over | 100 / 800 µs |
//! | the tick never wakes it: it sleeps until its due, then looks | 700 / 800 µs |
//! | **as here: woken at its hand-over, asleep at most until [`LEAD`] before its due** | 60 / 400 µs |
//!
//! The output's thread:
//!
//! ```text
//! [*] --> Looking --frame there--> Sending --device took it--> Sleeping
//! Sleeping --woken, or LEAD before its next frame--> Looking --nothing yet, LOOK later--> Looking
//! any --stop--> [*]     (the device goes back to whoever stops it)
//! ```
//!
//! Its one transition function is [`step`]: what the thread does each time it looks at
//! its queue and the clock. The thread only does what `step` says.

use crate::wake::Clock;
use crate::{Device, Levels, Outcome, Universes};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Boxes per output: one being sent, one waiting, one the tick fills. A device that takes
/// longer than a frame skips to the newest frame instead of sending stale ones.
pub(crate) const BOXES: usize = 3;

/// The name of every output's thread.
const THREAD: &str = "output";

/// How often an output's thread looks for its frame from [`LEAD`] before it is due, if
/// the tick's wake has not come yet.
const LOOK: Duration = Duration::from_micros(200);
/// How long before its next frame is due an output's thread stops sleeping and starts to
/// look, so it is quick to run when the frame comes.
const LEAD: Duration = Duration::from_millis(3);

/// A frame on its way to an output's device and back.
struct Frame {
    /// Made for every universe of the engine: a run of any length fits.
    levels: Box<[Levels]>,
    /// The universes of `levels` this frame has.
    len: usize,
    /// When it was due, since the engine started.
    due: Duration,
    /// When the output's next frame is due.
    next: Duration,
    prefix: Prefix,
    /// Outcome and handoff time, filled by the output's thread.
    sent: Option<(Outcome, Duration)>,
}

/// The commands in a frame and the attachment they went to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Prefix {
    pub applied: u64,
    pub generation: u64,
}

/// What came of a frame on the output's thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sent {
    pub outcome: Outcome,
    /// When the device was handed it (or it was passed over), since the engine started.
    pub at: Duration,
    /// When it was due.
    pub due: Duration,
    pub prefix: Prefix,
}

/// What came of handing a frame to the output's thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hand {
    /// On its way: what comes of it comes back as [`Sent`].
    Handed,
    /// No spare box: the device is still on older frames. This frame is dropped.
    Full,
}

/// What the output's thread does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Gives the device back and ends.
    Stop,
    /// Hands the device the newest frame; the `passed_over` older ones go back dropped.
    Send { passed_over: usize },
    /// Sleeps this long, until [`LEAD`] before its next frame, or until the tick wakes it
    /// or it is stopped.
    Sleep(Duration),
    /// Its frame is due soon or now and not there yet: looks again in [`LOOK`], or when
    /// the tick wakes it.
    Look,
}

/// The transition function of the output's thread: what it does with `waiting` frames
/// in its queue, `stop` asked or not, at `now`, its next frame due at `next` (since the
/// engine started; zero before its first frame). A calculation.
fn step(waiting: usize, stop: bool, now: Duration, next: Duration) -> Step {
    match (stop, waiting) {
        (true, _) => Step::Stop,
        (false, 1..) => Step::Send {
            passed_over: waiting - 1,
        },
        (false, 0) if now.saturating_add(LEAD) < next => Step::Sleep(next - LEAD - now),
        (false, 0) => Step::Look,
    }
}

/// Between the tick's end and the output's thread. Atomics only.
struct Shared {
    stop: AtomicBool,
    /// The thread is in its loop: whatever its start-up allocated is done.
    ready: AtomicBool,
}

/// The tick's end of an output's thread. Made and dropped off the tick thread: dropping
/// it stops the thread and frees the device there.
pub(crate) struct Lane {
    frames: rtrb::Producer<Frame>,
    back: rtrb::Consumer<Frame>,
    spare: [Option<Frame>; BOXES],
    shared: Arc<Shared>,
    thread: Option<JoinHandle<Box<dyn Device>>>,
}

impl Lane {
    /// Starts `device`'s own thread with real-time priority, and boxes for every universe
    /// of the engine. Off the tick thread: it allocates and starts a thread. Returns once
    /// the thread is in its loop. Fails only when the system refuses a thread.
    pub(crate) fn start(
        device: Box<dyn Device>,
        universes: Universes,
        clock: Clock,
    ) -> io::Result<Self> {
        let count = usize::from(universes.count());
        let (frames, waiting) = rtrb::RingBuffer::new(BOXES);
        let (done, back) = rtrb::RingBuffer::new(BOXES);
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            ready: AtomicBool::new(false),
        });
        let (thread, _) = rt_thread::spawn(THREAD, {
            let shared = Arc::clone(&shared);
            move |_| run(device, waiting, done, &shared, &clock)
        })?;
        while !shared.ready.load(Ordering::Acquire) {
            thread::yield_now();
        }
        let spare = std::array::from_fn(|_| {
            Some(Frame {
                levels: vec![Levels::ZERO; count].into_boxed_slice(),
                len: 0,
                due: Duration::ZERO,
                next: Duration::ZERO,
                prefix: Prefix::default(),
                sent: None,
            })
        });
        Ok(Self {
            frames,
            back,
            spare,
            shared,
            thread: Some(thread),
        })
    }

    /// Hands `frame`, due at `due`, to the output's thread; the output's next frame is
    /// due at `next`. Never waits, never allocates: copies into a spare box and wakes the
    /// thread (`unpark`, wait-free).
    pub(crate) fn hand(
        &mut self,
        frame: &[Levels],
        due: Duration,
        next: Duration,
        prefix: Prefix,
    ) -> Hand {
        let Some(mut free) = self.spare.iter_mut().find_map(Option::take) else {
            return Hand::Full;
        };
        let len = frame.len().min(free.levels.len());
        if let (Some(to), Some(from)) = (free.levels.get_mut(..len), frame.get(..len)) {
            to.copy_from_slice(from);
        }
        free.len = len;
        free.due = due;
        free.next = next;
        free.prefix = prefix;
        free.sent = None;
        match self.frames.push(free) {
            Ok(()) => {
                if let Some(thread) = &self.thread {
                    thread.thread().unpark();
                }
                Hand::Handed
            }
            // Cannot happen: the queue has room for every box.
            Err(rtrb::PushError::Full(free)) => {
                self.keep(free);
                Hand::Full
            }
        }
    }

    /// What came of each frame back from the output's thread since the last call, in the
    /// order they were sent. Their boxes are spare again.
    pub(crate) fn back(&mut self, mut each: impl FnMut(Sent)) {
        while let Ok(mut frame) = self.back.pop() {
            if let Some((outcome, at)) = frame.sent.take() {
                each(Sent {
                    outcome,
                    at,
                    due: frame.due,
                    prefix: frame.prefix,
                });
            }
            self.keep(frame);
        }
    }

    /// Keeps a box as spare. There is always a place: there are [`BOXES`] boxes.
    fn keep(&mut self, frame: Frame) {
        if let Some(place) = self.spare.iter_mut().find(|place| place.is_none()) {
            *place = Some(frame);
        }
    }
}

impl Drop for Lane {
    /// Stops the thread once its send is done, and frees the device here. Never on the
    /// tick thread: the engine retires a lane to its free thread.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            drop(thread.join());
        }
    }
}

/// The output's thread, an action: does what [`step`] says until it is stopped, then
/// gives the device back. Never allocates or frees; the only call outward is `take`.
fn run(
    mut device: Box<dyn Device>,
    mut waiting: rtrb::Consumer<Frame>,
    mut done: rtrb::Producer<Frame>,
    shared: &Shared,
    clock: &Clock,
) -> Box<dyn Device> {
    thread::park_timeout(Duration::ZERO); // sets up the park state
    shared.ready.store(true, Ordering::Release);
    let mut next = Duration::ZERO;
    loop {
        let stop = shared.stop.load(Ordering::Acquire);
        match step(waiting.slots(), stop, clock.now(), next) {
            Step::Stop => return device,
            Step::Sleep(time) => thread::park_timeout(time),
            Step::Look => thread::park_timeout(LOOK),
            Step::Send { passed_over } => {
                for _ in 0..passed_over {
                    if let Ok(frame) = waiting.pop() {
                        back(&mut done, frame, Outcome::Dropped, clock.now());
                    }
                }
                if let Ok(frame) = waiting.pop() {
                    next = frame.next;
                    let at = clock.now();
                    let outcome = device.take(frame.levels.get(..frame.len).unwrap_or_default());
                    back(&mut done, frame, outcome, at);
                }
            }
        }
    }
}

/// Sends a box back to the tick with what came of its frame. The queue has room for
/// every box, so it is never full.
fn back(done: &mut rtrb::Producer<Frame>, mut frame: Frame, outcome: Outcome, at: Duration) {
    frame.sent = Some((outcome, at));
    let _ = done.push(frame);
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    #[test]
    fn the_output_thread_does_what_its_queue_and_the_clock_say() {
        let (ms, us) = (Duration::from_millis, Duration::from_micros);
        let send = |passed_over| Step::Send { passed_over };
        // (frames waiting, stop asked, now, next due, does)
        let table = [
            (0, false, ms(10), ms(26), Step::Sleep(ms(13))), // sent: sleeps until LEAD before
            (0, false, ms(22) + us(999), ms(26), Step::Sleep(us(1))),
            (0, false, ms(23), ms(26), Step::Look), // LEAD before
            (0, false, ms(26), ms(26), Step::Look), // due: the tick hands it soon
            (0, false, ms(27), ms(26), Step::Look), // the tick is late
            (0, false, ms(0), ms(0), Step::Look),   // before its first frame
            (1, false, ms(25), ms(26), send(0)),    // handed early: sent at once
            (1, false, ms(26), ms(26), send(0)),
            (2, false, ms(26), ms(26), send(1)), // the device was slow: the newest wins
            (BOXES, false, ms(26), ms(26), send(BOXES - 1)),
            (0, true, ms(10), ms(26), Step::Stop),
            (1, true, ms(26), ms(26), Step::Stop), // stopped: what waits is not sent
            (BOXES, true, ms(26), ms(26), Step::Stop),
        ];
        for (waiting, stop, now, next, does) in table {
            assert_eq!(
                step(waiting, stop, now, next),
                does,
                "{waiting} waiting, stop {stop}, at {now:?}, next {next:?}"
            );
        }
    }

    /// A device that waits until it is let go, then takes the frame, and counts frames.
    struct Gate {
        open: Arc<AtomicBool>,
        taken: Arc<AtomicUsize>,
    }

    impl Device for Gate {
        fn take(&mut self, frame: &[Levels]) -> Outcome {
            while !self.open.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(1)); // poll
            }
            self.taken.fetch_add(1, Ordering::Release);
            match frame.first().map(|universe| universe.0[0]) {
                Some(0) => Outcome::Failed,
                _ => Outcome::Accepted,
            }
        }
    }

    fn levels(first: u8) -> [Levels; 1] {
        let mut universe = Levels::ZERO;
        universe.0[0] = first;
        [universe]
    }

    /// Until `n` frames came back, collects what came of them.
    fn wait_back(lane: &mut Lane, n: usize) -> Vec<(Outcome, Duration, Prefix)> {
        let mut came = vec![];
        let started = Instant::now();
        while came.len() < n {
            assert!(started.elapsed() < Duration::from_secs(10), "{came:?}");
            lane.back(|sent| came.push((sent.outcome, sent.due, sent.prefix)));
            thread::sleep(Duration::from_millis(1)); // poll
        }
        came
    }

    #[test]
    fn a_busy_device_skips_to_the_newest_frame_and_a_full_lane_drops_at_once() {
        let ms = Duration::from_millis;
        let open = Arc::new(AtomicBool::new(false));
        let taken = Arc::new(AtomicUsize::new(0));
        let gate = Gate {
            open: Arc::clone(&open),
            taken: Arc::clone(&taken),
        };
        let mut lane = Lane::start(Box::new(gate), Universes::ONE, Clock::wall()).unwrap();
        // Every frame is due at once: the thread sends what it finds.
        let prefix = |n| Prefix {
            applied: 100 + n,
            generation: 10 + n,
        };
        let hand =
            |lane: &mut Lane, first, due| lane.hand(&levels(first), ms(due), ms(0), prefix(due));
        // Frame 1 is taken and held at the gate; then two wait and the boxes run out.
        assert_eq!(hand(&mut lane, 1, 1), Hand::Handed);
        let started = Instant::now();
        while lane.frames.slots() < BOXES {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "frame 1 never taken"
            );
            thread::sleep(Duration::from_millis(1)); // poll
        }
        // (frame, due in ms, handed over)
        let handed = [
            (2, 2, Hand::Handed),
            (3, 3, Hand::Handed),
            (4, 4, Hand::Full),
        ];
        for (first, due, expected) in handed {
            assert_eq!(hand(&mut lane, first, due), expected, "frame {first}");
        }
        open.store(true, Ordering::Release);
        // 1 sent, 2 passed over for 3, 3 sent; a frame of zeros the device fails
        let came = wait_back(&mut lane, 3);
        let expected = [
            (Outcome::Accepted, ms(1), prefix(1)),
            (Outcome::Dropped, ms(2), prefix(2)),
            (Outcome::Accepted, ms(3), prefix(3)),
        ];
        assert_eq!(came, expected);
        assert_eq!(hand(&mut lane, 0, 5), Hand::Handed, "boxes are spare again");
        assert_eq!(
            wait_back(&mut lane, 1),
            [(Outcome::Failed, ms(5), prefix(5))]
        );
        drop(lane);
        assert_eq!(
            taken.load(Ordering::Acquire),
            3,
            "frames 1, 3 and 5 reached the device"
        );
    }
}
