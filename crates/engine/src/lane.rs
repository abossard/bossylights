//! Each output sends on a thread of its own, so the tick is never late behind a send
//! (#174). The tick gathers the route into a spare box and hands it over; the output's
//! thread sends the newest box and hands every box back with what came of it.
//!
//! ```text
//! tick thread                                output's thread, real-time, one per output
//!   gather route into a spare box   --frames--> the newest: device.take
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
//! Prepared --previous device finished its route--> Looking
//! Looking --accepted handoff--> Returned --retired--> [*]
//! any --stop--> [*]     (the device goes back to whoever stops it)
//! ```
//!
//! [`step`] schedules sends while the lane is serving. At a route switch the
//! worker finishes its last send, gives up the device and parks until reclamation.

use crate::wake::Clock;
use crate::{Device, Levels, Outcome, Run};
use engine_core::{RouteFault, Span, bounds, gather};
use outputs::Limits;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
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

/// The checked channel spans that form one output's byte stream.
#[derive(Debug)]
pub struct Route {
    spans: Box<[Span]>,
    length: u32,
    bounds: Run,
}

impl Route {
    /// Checks the assembled stream against the device's limits before owning its spans.
    pub fn new(limits: Limits, spans: &[Span]) -> Result<Self, RouteFault> {
        let length = limits.route(spans)?;
        let bounds = bounds(spans).ok_or(RouteFault::PastChannels)?;
        Ok(Self {
            spans: spans.into(),
            length,
            bounds,
        })
    }

    /// API v11 supplies one run. Its last partial pixel is omitted.
    pub fn of_run(run: Run, limits: Limits) -> Result<Self, RouteFault> {
        Self::new(limits, &[run.span(limits.whole)])
    }

    /// The first through last universes the route touches, as API v11 reports them.
    pub fn bounds(&self) -> Run {
        self.bounds
    }

    pub(crate) fn spans(&self) -> &[Span] {
        &self.spans
    }
}

/// A frame on its way to an output's device and back.
struct Frame {
    /// Holds exactly the bytes this route gathers.
    bytes: Box<[u8]>,
    /// When it was due, since the engine started.
    due: Duration,
    /// When the output's next frame is due.
    next: Duration,
    prefix: Prefix,
    /// Outcome and handoff time, filled by the output's thread.
    sent: Option<(Outcome, Duration)>,
}

/// The number of presses applied through this frame and the attachment it went to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Prefix {
    pub presses: u64,
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
    failed: AtomicBool,
    /// A refused restart registered this device with the recovery worker.
    watch_loss: AtomicBool,
    /// A replacement waits for the adapter before serving. Once it hands that
    /// adapter back, it stays parked until the free thread reclaims the lane.
    phase: AtomicU8,
}

/// Only an opener or the free thread joins a lane. The tick keeps its wait-free wake handle.
#[derive(Clone)]
pub(crate) struct Control(Arc<Worker>);

type DeviceBox = Box<dyn Device>;
type Returning = JoinHandle<Option<DeviceBox>>;

#[expect(
    clippy::disallowed_types,
    reason = "only the opener and the free thread join an output worker"
)]
struct Worker {
    shared: Arc<Shared>,
    wake: thread::Thread,
    port: std::sync::Mutex<Delivery>,
}

impl Control {
    /// The opener and recovery worker transfer devices through these queues.
    /// The tick and output worker never acquire the port lock.
    pub(crate) fn install(&self, device: DeviceBox) {
        let mut delivery = self.0.port.lock().unwrap_or_else(|e| e.into_inner());
        let reconnects_itself = device.reconnects_itself();
        if !self.closed() && delivery.thread.is_some() && delivery.next.push(device).is_ok() {
            delivery.reconnects_itself = reconnects_itself;
        }
    }

    pub(crate) fn watch_loss(&self) {
        self.0.shared.watch_loss.store(true, Ordering::Release);
    }

    pub(crate) fn watching_loss(&self) -> bool {
        self.0.shared.watch_loss.load(Ordering::Acquire)
    }

    pub(crate) fn needs_open(&self) -> bool {
        let mut delivery = self.0.port.lock().unwrap_or_else(|e| e.into_inner());
        drop(delivery.old.pop());
        !delivery.reconnects_itself
            && !delivery.next.is_full()
            && self.0.shared.failed.load(Ordering::Acquire)
    }

    /// Set during preparation, before the first frame can be handed to this lane.
    pub(crate) fn hold_for_handoff(&self) {
        self.0
            .shared
            .phase
            .store(AWAITING_DEVICE, Ordering::Release);
    }

    pub(crate) fn awaiting_device(&self) -> bool {
        self.0.shared.phase.load(Ordering::Acquire) == AWAITING_DEVICE
    }

    /// The accepted switch. Only the worker can give up its device, between sends.
    pub(crate) fn handoff(&self) {
        let _ = self.0.shared.phase.compare_exchange(
            SERVING,
            HANDING_OVER,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.0.wake.unpark();
    }

    /// Outside the publisher, tick and lanes. Finish any write in the old adapter
    /// before enabling the new worker, even when it already has a fresh device.
    /// Returns whether the opener must watch the new lane for a lost device.
    pub(crate) fn finish_handoff(&self, next: &Self, reuse: bool) -> bool {
        while self.0.shared.phase.load(Ordering::Acquire) != RETURNED {
            thread::sleep(Duration::from_millis(1));
        }
        let mut port = self.0.port.lock().unwrap_or_else(|e| e.into_inner());
        let returned = port.returned.pop().ok();
        let missing = reuse && returned.is_none();
        if let Some(mut device) = returned {
            device.finish_route();
            if reuse {
                // A waiting lane with no device cannot have requested a retry.
                let mut delivery = next.0.port.lock().unwrap_or_else(|e| e.into_inner());
                delivery.reconnects_itself = device.reconnects_itself();
                let _ = delivery.next.push(device);
            } else if !self.0.shared.stop.load(Ordering::Acquire) {
                port.retired = Some(device);
            }
        }
        let watch = reuse && (missing || self.watching_loss());
        if reuse {
            let failed = missing || self.0.shared.failed.load(Ordering::Acquire);
            next.0.shared.failed.store(failed, Ordering::Release);
        }
        if watch {
            next.watch_loss();
        }
        next.0.shared.phase.store(SERVING, Ordering::Release);
        next.0.wake.unpark();
        watch
    }

    pub(crate) fn closed(&self) -> bool {
        self.0.shared.stop.load(Ordering::Acquire)
            || matches!(
                self.0.shared.phase.load(Ordering::Acquire),
                HANDING_OVER | RETURNED
            )
    }

    /// Retire on the free thread after the last send. The worker can transfer its
    /// device to a concurrent handoff even when reclamation starts first.
    fn stop(&self) {
        self.0.shared.stop.store(true, Ordering::Release);
        self.0.wake.unpark();
        let mut port = self.0.port.lock().unwrap_or_else(|e| e.into_inner());
        let Some(thread) = port.thread.take() else {
            return;
        };
        drop(port.old.pop());
        drop(port.retired.take());
        // Release the delivery lock before joining. The handoff can now drain
        // the adapter even while the free thread reclaims the old worker.
        drop(port);
        if let Ok(Some(mut device)) = thread.join() {
            device.finish_route();
        }
    }
}

/// A lane can start before the old device changes hands. Replacements, including a
/// reopened adapter, travel in one direction; displaced devices are freed off thread.
struct Delivery {
    reconnects_itself: bool,
    thread: Option<Returning>,
    next: rtrb::Producer<DeviceBox>,
    old: rtrb::Consumer<DeviceBox>,
    returned: rtrb::Consumer<DeviceBox>,
    /// A replaced device that finished its last write. Freed with the old lane.
    retired: Option<DeviceBox>,
}

struct Devices {
    current: Option<DeviceBox>,
    next: rtrb::Consumer<DeviceBox>,
    old: rtrb::Producer<DeviceBox>,
    returning: rtrb::Producer<DeviceBox>,
}

impl Devices {
    fn receive(&mut self, failed: &AtomicBool) {
        if !self.old.is_full()
            && let Ok(device) = self.next.pop()
            && let Some(old) = self.current.replace(device)
        {
            let _ = self.old.push(old);
            // Only the worker knows when a replacement actually takes ownership.
            failed.store(false, Ordering::Release);
        }
    }
}

/// The tick keeps the last accepted lane and its boxes throughout a pending restart.
pub(crate) struct Lane {
    spans: Box<[Span]>,
    frames: rtrb::Producer<Frame>,
    back: rtrb::Consumer<Frame>,
    spare: [Option<Frame>; BOXES],
    pub(crate) control: Control,
}

/// Rendezvous on the thread preparing an output. Dropping the release end lets it run.
#[cfg(feature = "test-support")]
#[expect(
    clippy::disallowed_types,
    reason = "only the test and the blocking opener rendezvous"
)]
pub(crate) struct StartPause {
    pub reached: std::sync::mpsc::Sender<()>,
    pub release: std::sync::mpsc::Receiver<()>,
}

impl Lane {
    /// Starts `device`'s own thread with real-time priority and boxes sized to the route.
    /// Off the tick thread: it allocates and starts a thread. Returns once
    /// the thread is in its loop. Fails only when the system refuses a thread.
    pub(crate) fn start(
        device: Option<Box<dyn Device>>,
        route: Route,
        clock: Clock,
        #[cfg(feature = "test-support")] pause: Option<StartPause>,
    ) -> io::Result<Self> {
        #[cfg(feature = "test-support")]
        if let Some(pause) = pause {
            let _ = pause.reached.send(());
            let _ = pause.release.recv();
        }
        let (frames, waiting) = rtrb::RingBuffer::new(BOXES);
        let (done, back) = rtrb::RingBuffer::new(BOXES);
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            watch_loss: AtomicBool::new(false),
            phase: AtomicU8::new(SERVING),
        });
        let (next, receive) = rtrb::RingBuffer::new(1);
        let (retire, old) = rtrb::RingBuffer::new(1);
        let (returning, returned) = rtrb::RingBuffer::new(1);
        let reconnects_itself = device.as_ref().is_some_and(|d| d.reconnects_itself());
        let devices = Devices {
            current: device,
            next: receive,
            old: retire,
            returning,
        };
        let (thread, _) = rt_thread::spawn(THREAD, {
            let shared = Arc::clone(&shared);
            move |_| run(devices, waiting, done, &shared, &clock)
        })?;
        while !shared.ready.load(Ordering::Acquire) {
            thread::yield_now();
        }
        let spare = std::array::from_fn(|_| {
            Some(Frame {
                bytes: vec![0; route.length as usize].into_boxed_slice(),
                due: Duration::ZERO,
                next: Duration::ZERO,
                prefix: Prefix::default(),
                sent: None,
            })
        });
        let control = Control(Arc::new(Worker {
            shared,
            wake: thread.thread().clone(),
            port: Delivery {
                reconnects_itself,
                thread: None,
                next,
                old,
                returned,
                retired: None,
            }
            .into(),
        }));
        // Publish the join handle here. This also initializes the platform mutex
        // during setup, before the lane enters service.
        control
            .0
            .port
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .thread = Some(thread);
        Ok(Self {
            spans: route.spans,
            frames,
            back,
            spare,
            control,
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
        gather(frame, &self.spans, &mut free.bytes);
        free.due = due;
        free.next = next;
        free.prefix = prefix;
        free.sent = None;
        match self.frames.push(free) {
            Ok(()) => {
                self.control.0.wake.unpark();
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
    fn drop(&mut self) {
        self.control.stop();
    }
}

// AWAITING_DEVICE -> SERVING -> HANDING_OVER -> RETURNED. An initial lane
// starts at SERVING. Only the worker returns the device, after its last send.
const AWAITING_DEVICE: u8 = 3;
const SERVING: u8 = 0;
const HANDING_OVER: u8 = 1;
const RETURNED: u8 = 2;

/// Also release the opener if a supplied device unwinds on its worker.
struct EndOfRun<'a>(&'a AtomicU8);

impl Drop for EndOfRun<'_> {
    fn drop(&mut self) {
        self.0.store(RETURNED, Ordering::Release);
    }
}

/// The output's thread, an action: does what [`step`] says until it is stopped, then
/// gives the device back. Never allocates or frees; the only call outward is `take`.
fn run(
    mut devices: Devices,
    mut waiting: rtrb::Consumer<Frame>,
    mut done: rtrb::Producer<Frame>,
    shared: &Shared,
    clock: &Clock,
) -> Option<DeviceBox> {
    thread::park_timeout(Duration::ZERO); // sets up the park state
    let _finished = EndOfRun(&shared.phase);
    shared.ready.store(true, Ordering::Release);
    let mut next = Duration::ZERO;
    let mut finished_last_send = false;
    loop {
        let stop = shared.stop.load(Ordering::Acquire);
        let mut queued = waiting.slots();
        let handoff = shared.phase.load(Ordering::Acquire);
        if handoff == AWAITING_DEVICE && !stop {
            thread::park();
            continue;
        }
        if handoff == HANDING_OVER {
            // The cutoff that requested the handoff may have queued its last
            // old frame after our first look. It has now finished handing frames.
            queued = waiting.slots();
        }
        if handoff == HANDING_OVER && (finished_last_send || queued == 0) {
            devices.receive(&shared.failed);
            if let Some(device) = devices.current.take() {
                // One worker, one handoff and one reserved place.
                let _ = devices.returning.push(device);
            }
            shared.phase.store(RETURNED, Ordering::Release);
            continue;
        }
        if !stop && handoff == RETURNED {
            thread::park();
            continue;
        }
        match step(queued, stop && handoff != HANDING_OVER, clock.now(), next) {
            Step::Stop => {
                devices.receive(&shared.failed);
                return devices.current;
            }
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
                    devices.receive(&shared.failed);
                    let outcome = devices
                        .current
                        .as_mut()
                        .map_or(Outcome::Failed, |device| device.take(&frame.bytes));
                    // A send already in the device when the switch arrived is
                    // its last. Otherwise send the newest outstanding old frame.
                    finished_last_send = shared.phase.load(Ordering::Acquire) == HANDING_OVER;
                    if outcome != Outcome::Dropped {
                        shared
                            .failed
                            .store(outcome == Outcome::Failed, Ordering::Release);
                    }
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
        fn take(&mut self, stream: &[u8]) -> Outcome {
            while !self.open.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(1)); // poll
            }
            self.taken.fetch_add(1, Ordering::Release);
            match stream.first() {
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
        let route = Route::new(Limits::RAW, &[Span { start: 0, len: 1 }]).unwrap();
        let mut lane = Lane::start(
            Some(Box::new(gate)),
            route,
            Clock::wall(),
            #[cfg(feature = "test-support")]
            None,
        )
        .unwrap();
        // Every frame is due at once: the thread sends what it finds.
        let prefix = |n| Prefix {
            presses: 100 + n,
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
