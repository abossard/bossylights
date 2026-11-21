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

use crate::cutover::{Effect, Event, Machine, Work};
use crate::wake::Clock;
use crate::{Device, Levels, Outcome, Run};
use engine_core::{RouteFault, Span, bounds, gather};
use outputs::Limits;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
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
    /// The new frame is held. This older held frame was superseded on the tick.
    Replaced(Sent),
    /// No spare box: the device is still on older frames. This frame is dropped.
    Full,
}

/// What the output's thread does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Gives the device back and ends.
    Stop,
    /// Gives the device back for T8 and waits for reclamation.
    Return,
    /// Pass over old boxes, retain the newest, and wait for T8 or more boxes.
    Hold { passed_over: usize },
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
fn step(work: Work, waiting: usize, stop: bool, now: Duration, next: Duration) -> Step {
    match (work, stop, waiting) {
        (_, true, _) | (Work::Stop, _, _) => Step::Stop,
        (Work::Hold, false, _) => Step::Hold {
            passed_over: waiting.saturating_sub(1),
        },
        (Work::Drain, false, 0) => Step::Return,
        (Work::Send | Work::Drain, false, 1..) => Step::Send {
            passed_over: waiting - 1,
        },
        (Work::Send, false, 0) if now.saturating_add(LEAD) < next => Step::Sleep(next - LEAD - now),
        (Work::Send, false, 0) => Step::Look,
    }
}

/// The new worker can be held for arbitrarily many due frames. Two preallocated
/// buffers let the tick write one while the opener seals the other at T8. There
/// is one producer; it never waits for a reader or for a frame box to come back.
struct Held {
    buffers: [Box<[AtomicU8]>; 2],
    published: AtomicU8,
}

impl Held {
    const EMPTY: u8 = 2;
    const SEALED: u8 = 4;
    const TAKEN: u8 = 8;

    fn new(length: usize) -> Self {
        Self {
            buffers: std::array::from_fn(|_| (0..length).map(|_| AtomicU8::new(0)).collect()),
            published: AtomicU8::new(Self::EMPTY),
        }
    }

    fn write(&self, bytes: &[u8]) -> bool {
        let previous = self.published.load(Ordering::Acquire);
        let [first, second] = &self.buffers;
        let (buffer, index) = match previous {
            0 => (second, 1),
            1 | Self::EMPTY => (first, 0),
            _ => return false,
        };
        for (to, byte) in buffer.iter().zip(bytes) {
            to.store(*byte, Ordering::Relaxed);
        }
        // If T8 raced this write, the old frame was sealed. Hand this frame
        // over on the ordinary queue after it, using the already-gathered box.
        self.published
            .compare_exchange(previous, index, Ordering::Release, Ordering::Relaxed)
            .is_ok()
    }

    fn seal(&self) {
        self.published.fetch_or(Self::SEALED, Ordering::AcqRel);
    }

    /// There is one reader, and it takes the selected frame exactly once.
    fn take(&self, out: &mut [u8]) -> bool {
        let published = self.published.load(Ordering::Acquire);
        let Some(index) = published.checked_sub(Self::SEALED) else {
            return false;
        };
        let Some(buffer) = self.buffers.get(usize::from(index)) else {
            return false;
        };
        for (byte, from) in out.iter_mut().zip(buffer) {
            *byte = from.load(Ordering::Relaxed);
        }
        self.published.store(Self::TAKEN, Ordering::Relaxed);
        true
    }
}

struct HeldFrame {
    latest: Option<Sent>,
    result: rtrb::Consumer<(Outcome, Duration)>,
}

struct HeldReader {
    bytes: Box<[u8]>,
    result: rtrb::Producer<(Outcome, Duration)>,
}

/// Between the tick's end and the output's thread. Atomics only.
struct Shared {
    stop: AtomicBool,
    /// The thread is in its loop: whatever its start-up allocated is done.
    ready: AtomicBool,
    failed: AtomicBool,
    /// A refused restart registered this device with the recovery worker.
    watch_loss: AtomicBool,
    /// All lanes of this output observe the same cutover. Only T9 advances the
    /// number of the serving lane. A future lane holds its boxes until T8.
    cutover: Arc<Machine>,
    number: u64,
    clock: Clock,
    held: Option<Held>,
}

/// Only an opener or the free thread joins a lane. The tick keeps its wait-free wake handle.
#[derive(Clone)]
pub(crate) struct Control(Arc<Worker>);

type DeviceBox = Box<dyn Device>;

/// A device that unwound is missing for recovery. Keep both allocations with
/// the adapter until the opener or the reclaimer takes ownership of them.
struct Adapter {
    device: DeviceBox,
    unwind: Option<Box<dyn std::any::Any + Send>>,
}

impl Adapter {
    fn new(device: DeviceBox) -> Self {
        Self {
            device,
            unwind: None,
        }
    }

    fn take(&mut self, bytes: &[u8]) -> Outcome {
        if self.unwind.is_some() {
            return Outcome::Failed;
        }
        match catch_unwind(AssertUnwindSafe(|| self.device.take(bytes))) {
            Ok(outcome) => outcome,
            Err(payload) => {
                self.unwind = Some(payload);
                Outcome::Failed
            }
        }
    }

    /// Only called off the output and tick threads.
    fn finish(mut self) -> Option<DeviceBox> {
        if self.unwind.is_some() {
            return None;
        }
        catch_unwind(AssertUnwindSafe(|| self.device.finish_route())).ok()?;
        Some(self.device)
    }
}

// Scratch belongs to the opener. Returning it through join keeps its destruction
// off the output thread, including when stop interrupts a pending transfer.
type Finished = (Option<Adapter>, Option<HeldReader>);
type Returning = JoinHandle<Finished>;

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
        if self.closed() {
            return;
        }
        delivery.install(device);
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

    pub(crate) fn cutover(&self) -> &Machine {
        &self.0.shared.cutover
    }

    pub(crate) fn waits_to_serve(&self) -> bool {
        self.cutover().waiting_for_visibility(self.0.shared.number)
    }

    /// T5: the publisher's wake fence, or the tick applying the switch,
    /// establishes that no more old frames can arrive. The first one wakes it.
    pub(crate) fn publish(&self) {
        if self.cutover().apply(Event::Publish) == Effect::DrainOld {
            self.0.wake.unpark();
        }
    }

    /// T8 runs on the opener. The old worker acknowledges an empty frame queue
    /// by moving its device into the return slot, even when the device is absent.
    /// The old worker stays parked so reclamation happens on the free thread.
    pub(crate) fn finish_handoff(
        &self,
        next: &Self,
        reuse: bool,
        stopping: &AtomicBool,
    ) -> Option<bool> {
        let returned = loop {
            if stopping.load(Ordering::Acquire) {
                return None;
            }
            {
                let mut port = self.0.port.lock().unwrap_or_else(|e| e.into_inner());
                if let Ok(device) = port.returned.pop() {
                    break device;
                }
                // A worker already stopped cannot produce another message.
                if port.thread.as_ref().is_none_or(JoinHandle::is_finished) {
                    break None;
                }
            }
            thread::sleep(Duration::from_millis(1));
        };

        let returned = returned.and_then(Adapter::finish);
        let missing = returned.is_none();
        if let Some(device) = returned {
            if reuse {
                // T8 is the only producer until the new lane is released.
                let mut port = next.0.port.lock().unwrap_or_else(|e| e.into_inner());
                port.install(device);
            } else {
                self.0
                    .port
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .retired = Some(device);
            }
        }
        let watch = reuse && (missing || self.watching_loss());
        if watch {
            next.watch_loss();
        }
        if reuse {
            next.0.shared.failed.store(
                missing || self.0.shared.failed.load(Ordering::Acquire),
                Ordering::Release,
            );
        }
        if let Some(held) = &next.0.shared.held {
            held.seal();
        }
        if stopping.load(Ordering::Acquire)
            || self.cutover().apply(Event::DeviceReturned) != Effect::ReleaseAndAccept
        {
            return None;
        }
        next.0.wake.unpark();
        Some(watch)
    }

    pub(crate) fn closed(&self) -> bool {
        let shared = &self.0.shared;
        shared.stop.load(Ordering::Acquire)
            || matches!(shared.cutover.work(shared.number), Work::Drain | Work::Stop)
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
        if let Ok((Some(adapter), _scratch)) = thread.join() {
            drop(adapter.finish());
        }
    }
}

/// A lane can start before the old device changes hands. Replacements, including a
/// reopened adapter, travel in one direction; displaced devices are freed off thread.
struct Delivery {
    reconnects_itself: bool,
    thread: Option<Returning>,
    next: rtrb::Producer<DeviceBox>,
    old: rtrb::Consumer<Adapter>,
    returned: rtrb::Consumer<Option<Adapter>>,
    /// A replaced device that finished its last write. Freed with the old lane.
    retired: Option<DeviceBox>,
}

impl Delivery {
    fn install(&mut self, device: DeviceBox) {
        let reconnects_itself = device.reconnects_itself();
        if self.next.push(device).is_ok() {
            self.reconnects_itself = reconnects_itself;
        }
    }
}

struct Devices {
    current: Option<Adapter>,
    next: rtrb::Consumer<DeviceBox>,
    old: rtrb::Producer<Adapter>,
    returning: rtrb::Producer<Option<Adapter>>,
}

impl Devices {
    fn receive(&mut self, failed: &AtomicBool) {
        if !self.old.is_full()
            && let Ok(device) = self.next.pop()
            && let Some(old) = self.current.replace(Adapter::new(device))
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
    held: Option<HeldFrame>,
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
        previous: Option<&Control>,
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
            cutover: previous.map_or_else(
                || Arc::new(Machine::new()),
                |old| Arc::clone(&old.0.shared.cutover),
            ),
            number: previous.map_or(0, |old| old.0.shared.number + 1),
            clock,
            held: previous.map(|_| Held::new(route.length as usize)),
        });
        let (held, held_reader) = previous.map_or((None, None), |_| {
            let (result, read) = rtrb::RingBuffer::new(1);
            (
                Some(HeldFrame {
                    latest: None,
                    result: read,
                }),
                Some(HeldReader {
                    bytes: vec![0; route.length as usize].into(),
                    result,
                }),
            )
        });
        let (next, receive) = rtrb::RingBuffer::new(1);
        let (retire, old) = rtrb::RingBuffer::new(1);
        let (returning, returned) = rtrb::RingBuffer::new(1);
        let reconnects_itself = device.as_ref().is_some_and(|d| d.reconnects_itself());
        let devices = Devices {
            current: device.map(Adapter::new),
            next: receive,
            old: retire,
            returning,
        };
        let (thread, _) = rt_thread::spawn(THREAD, {
            let shared = Arc::clone(&shared);
            move |_| run(devices, held_reader, waiting, done, &shared)
        })?;
        while !shared.ready.load(Ordering::Acquire) {
            if thread.is_finished() {
                return Err(io::Error::other("output worker exited during startup"));
            }
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
            held,
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
        let shared = &self.control.0.shared;
        if let (Some(held), Some(receipt)) = (&shared.held, &mut self.held)
            && held.write(&free.bytes)
        {
            let at = shared.clock.now();
            let previous = receipt.latest.replace(Sent {
                due,
                at,
                prefix,
                outcome: Outcome::Dropped,
            });
            self.keep(free);
            return previous.map_or(Hand::Handed, |sent| Hand::Replaced(Sent { at, ..sent }));
        }
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
        if let Some(held) = &mut self.held
            && let Some(sent) = held.latest
        {
            // This receipt precedes every ordinary box. A worker can finish
            // both between two polls, so don't pass a pending held receipt.
            let Ok((outcome, at)) = held.result.pop() else {
                return;
            };
            held.latest = None;
            each(Sent {
                outcome,
                at,
                ..sent
            });
        }
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

/// The output's thread, an action: does what [`step`] says until it is stopped, then
/// gives the device back. Never allocates or frees; the only call outward is `take`.
fn run(
    mut devices: Devices,
    mut held_reader: Option<HeldReader>,
    mut waiting: rtrb::Consumer<Frame>,
    mut done: rtrb::Producer<Frame>,
    shared: &Shared,
) -> Finished {
    let clock = &shared.clock;
    thread::park_timeout(Duration::ZERO); // sets up the park state
    shared.ready.store(true, Ordering::Release);
    let mut next = Duration::ZERO;
    loop {
        let stop = shared.stop.load(Ordering::Acquire);
        // Acquire the phase before counting frames. Publication orders the last
        // old handover before this queue read, including a box behind a write.
        let work = shared.cutover.work(shared.number);
        if !stop
            && matches!(work, Work::Send | Work::Drain)
            && let (Some(held), Some(reader)) = (&shared.held, &mut held_reader)
            && held.take(&mut reader.bytes)
        {
            let at = clock.now();
            let outcome = send(&mut devices, shared, &reader.bytes);
            let _ = reader.result.push((outcome, at));
            continue;
        }
        let action = step(work, waiting.slots(), stop, clock.now(), next);
        if let Step::Hold { passed_over } | Step::Send { passed_over } = action {
            for _ in 0..passed_over {
                if let Ok(frame) = waiting.pop() {
                    back(&mut done, frame, Outcome::Dropped, clock.now());
                }
            }
        }
        match action {
            Step::Return => {
                devices.receive(&shared.failed);
                let _ = devices.returning.push(devices.current.take());
                // The queues and clock stay alive until reclamation.
                while !shared.stop.load(Ordering::Acquire) {
                    thread::park();
                }
                return (None, held_reader);
            }
            Step::Stop => {
                devices.receive(&shared.failed);
                return (devices.current, held_reader);
            }
            Step::Hold { .. } => thread::park(),
            Step::Sleep(time) => thread::park_timeout(time),
            Step::Look => thread::park_timeout(LOOK),
            Step::Send { .. } => {
                if let Ok(frame) = waiting.pop() {
                    next = frame.next;
                    let at = clock.now();
                    let outcome = send(&mut devices, shared, &frame.bytes);
                    back(&mut done, frame, outcome, at);
                }
            }
        }
    }
}

fn send(devices: &mut Devices, shared: &Shared, bytes: &[u8]) -> Outcome {
    devices.receive(&shared.failed);
    let outcome = devices
        .current
        .as_mut()
        .map_or(Outcome::Failed, |device| device.take(bytes));
    match outcome {
        Outcome::Failed if shared.cutover.apply(Event::SendFailed) == Effect::RecordFailure => {
            shared.failed.store(true, Ordering::Release);
        }
        Outcome::Accepted
            if shared.failed.load(Ordering::Acquire)
                && shared.cutover.apply(Event::AdapterAvailable) == Effect::RecordAvailable =>
        {
            shared.failed.store(false, Ordering::Release);
        }
        Outcome::Failed | Outcome::Accepted | Outcome::Dropped => {}
    }
    outcome
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
                step(Work::Send, waiting, stop, now, next),
                does,
                "{waiting} waiting, stop {stop}, at {now:?}, next {next:?}"
            );
        }
    }

    #[test]
    fn only_an_empty_old_lane_returns_its_device_and_a_held_lane_keeps_the_newest() {
        for (work, waiting, expected) in [
            (Work::Drain, 0, Step::Return),
            (Work::Drain, 1, Step::Send { passed_over: 0 }),
            (
                Work::Drain,
                BOXES,
                Step::Send {
                    passed_over: BOXES - 1,
                },
            ),
            (Work::Hold, 0, Step::Hold { passed_over: 0 }),
            (Work::Hold, 1, Step::Hold { passed_over: 0 }),
            (Work::Hold, 2, Step::Hold { passed_over: 1 }),
            (
                Work::Hold,
                BOXES,
                Step::Hold {
                    passed_over: BOXES - 1,
                },
            ),
            (Work::Stop, 0, Step::Stop),
            (Work::Stop, BOXES, Step::Stop),
        ] {
            assert_eq!(
                step(work, waiting, false, Duration::ZERO, Duration::ZERO),
                expected
            );
            assert_eq!(
                step(work, waiting, true, Duration::ZERO, Duration::ZERO),
                Step::Stop
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

    struct Constant(Outcome);

    impl Device for Constant {
        fn take(&mut self, _: &[u8]) -> Outcome {
            self.0
        }
    }

    fn start(device: Option<Outcome>, previous: Option<&Control>) -> Lane {
        let route = Route::new(Limits::RAW, &[Span { start: 0, len: 1 }]).unwrap();
        Lane::start(
            device.map(|outcome| Box::new(Constant(outcome)) as DeviceBox),
            route,
            Clock::wall(),
            previous,
            #[cfg(feature = "test-support")]
            None,
        )
        .unwrap()
    }

    fn take(lane: &mut Lane) -> Outcome {
        assert_eq!(
            lane.hand(
                &levels(73),
                Duration::ZERO,
                Duration::ZERO,
                Prefix::default()
            ),
            Hand::Handed
        );
        wait_back(lane, 1).first().unwrap().0
    }

    #[test]
    fn only_a_reused_device_carries_its_loss_and_recovery_watch() {
        use Outcome::{Accepted, Failed};
        // Old device, watched, reuse, watch after T8, outcome after T8, needs reopen.
        for (device, watched, reuse, watch, outcome, needs_open) in [
            (Some(Accepted), false, true, false, Accepted, false),
            (Some(Failed), false, true, false, Failed, true),
            (Some(Failed), true, true, true, Failed, true),
            (None, false, true, true, Failed, true),
            (Some(Failed), true, false, false, Accepted, false),
        ] {
            let mut old = start(device, None);
            assert!(!old.control.closed());
            if device.is_some() {
                assert_eq!(Some(take(&mut old)), device);
            }
            if watched {
                old.control.watch_loss();
            }
            assert_eq!(old.control.cutover().apply(Event::Begin), Effect::Open);
            let mut next = start((!reuse).then_some(Accepted), Some(&old.control));
            assert!(next.control.waits_to_serve());
            old.control.publish();
            assert!(old.control.closed());
            // The opener must release on a held clock, even on a missing adapter.
            let prior = old.control.clone();
            let prepared = next.control.clone();
            let (answer, finished) = std::sync::mpsc::channel();
            let stopping = Arc::new(AtomicBool::new(false));
            let stop = Arc::clone(&stopping);
            thread::spawn(move || {
                let _ = answer.send(prior.finish_handoff(&prepared, reuse, &stop));
            });
            let result = finished.recv_timeout(Duration::from_secs(10));
            stopping.store(true, Ordering::Release);
            assert_eq!(result.unwrap(), Some(watch));
            assert_eq!(next.control.watching_loss(), watch);
            assert_eq!(next.control.needs_open(), reuse && device.is_none());

            // A queued reused adapter already occupies the recovery slot.
            // Taking it installs its reconnect policy and exposes its outcome.
            assert_eq!(take(&mut next), outcome);
            assert_eq!(next.control.needs_open(), needs_open);
            if needs_open {
                next.control.install(Box::new(Constant(Accepted)));
                assert!(!next.control.needs_open(), "one reopen in flight");
                assert_eq!(take(&mut next), Accepted);
                assert!(!next.control.needs_open(), "recovered");
            }
            assert_eq!(old.control.cutover().apply(Event::Wake), Effect::ShowNew);
            assert!(old.control.closed());
            assert!(!next.control.closed());
            assert!(!next.control.waits_to_serve());
        }
    }

    #[test]
    fn publication_wakes_an_old_lane_asleep_until_its_next_frame() {
        // The old lane sent its frame and sleeps until LEAD before the next, an hour away.
        let mut old = start(Some(Outcome::Accepted), None);
        let hour = Duration::from_secs(3600);
        assert_eq!(
            old.hand(&levels(73), Duration::ZERO, hour, Prefix::default()),
            Hand::Handed
        );
        assert_eq!(wait_back(&mut old, 1).first().unwrap().0, Outcome::Accepted);
        old.control.cutover().apply(Event::Begin);
        let next = start(None, Some(&old.control));
        old.control.publish();
        let (prior, prepared) = (old.control.clone(), next.control.clone());
        let (answer, finished) = std::sync::mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopping);
        thread::spawn(move || {
            let _ = answer.send(prior.finish_handoff(&prepared, true, &stop));
        });
        let result = finished.recv_timeout(Duration::from_secs(10));
        stopping.store(true, Ordering::Release);
        assert_eq!(result.unwrap(), Some(false), "T8 waited for the hour");
    }

    #[test]
    fn a_held_lane_supersedes_every_older_frame_and_sends_its_newest_on_return() {
        struct Record(Arc<AtomicU8>);
        impl Device for Record {
            fn take(&mut self, bytes: &[u8]) -> Outcome {
                self.0.store(
                    bytes.first().copied().unwrap_or_default(),
                    Ordering::Release,
                );
                Outcome::Accepted
            }
        }

        for count in [6_u8, 7] {
            let marker = Arc::new(AtomicU8::new(0));
            let old = start(None, None);
            old.control.install(Box::new(Record(Arc::clone(&marker))));
            old.control.cutover().apply(Event::Begin);
            let mut new = start(None, Some(&old.control));
            old.control.publish();
            old.control.cutover().apply(Event::Cutoff);
            let prefix = |n: u8| Prefix {
                presses: u64::from(n),
                generation: 2,
            };
            // No worker or tick collection is needed between handovers. Even
            // and odd frame counts exercise both sides of the held buffer.
            for press in 1..=count {
                let handed = new.hand(
                    &levels(press),
                    Duration::ZERO,
                    Duration::ZERO,
                    prefix(press),
                );
                if press == 1 {
                    assert_eq!(handed, Hand::Handed);
                } else {
                    assert!(matches!(handed, Hand::Replaced(sent)
                        if sent.outcome == Outcome::Dropped && sent.prefix == prefix(press - 1)));
                }
            }
            assert_eq!(marker.load(Ordering::Acquire), 0);
            assert_eq!(
                old.control
                    .finish_handoff(&new.control, true, &AtomicBool::new(false)),
                Some(false)
            );
            assert_eq!(
                wait_back(&mut new, 1),
                [(Outcome::Accepted, Duration::ZERO, prefix(count))]
            );
            assert_eq!(marker.load(Ordering::Acquire), count);
        }
    }

    #[test]
    fn an_unwound_adapter_returns_every_old_box_and_the_reused_lane_can_recover() {
        struct Broken;
        impl Device for Broken {
            #[expect(clippy::panic, reason = "this adapter unwinds inside a write")]
            fn take(&mut self, _: &[u8]) -> Outcome {
                panic!("adapter lost its worker");
            }
        }

        let mut old = start(None, None);
        old.control.install(Box::new(Broken));
        assert_eq!(take(&mut old), Outcome::Failed);
        assert_eq!(take(&mut old), Outcome::Failed);
        old.control.cutover().apply(Event::Begin);
        let mut new = start(None, Some(&old.control));
        old.control.publish();
        assert_eq!(
            old.control
                .finish_handoff(&new.control, true, &AtomicBool::new(false)),
            Some(true)
        );
        assert!(new.control.needs_open());
        new.control.install(Box::new(Constant(Outcome::Accepted)));
        assert_eq!(take(&mut new), Outcome::Accepted);
    }

    #[test]
    fn a_dropped_first_new_frame_preserves_the_reused_devices_need_to_reopen() {
        struct Lost {
            first: bool,
        }

        impl Device for Lost {
            fn take(&mut self, _: &[u8]) -> Outcome {
                if std::mem::take(&mut self.first) {
                    Outcome::Failed
                } else {
                    Outcome::Dropped
                }
            }
        }

        let mut old = start(None, None);
        old.control.install(Box::new(Lost { first: true }));
        assert_eq!(take(&mut old), Outcome::Failed);
        old.control.cutover().apply(Event::Begin);
        let mut new = start(None, Some(&old.control));
        old.control.publish();
        assert_eq!(
            old.control
                .finish_handoff(&new.control, true, &AtomicBool::new(false)),
            Some(false)
        );
        assert_eq!(take(&mut new), Outcome::Dropped);
        assert!(new.control.needs_open());
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
            None,
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
