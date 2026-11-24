//! Output opening, handoff, recovery, and reclamation for the front.

use super::*;
use std::panic::{self, AssertUnwindSafe};
use std::time::Instant;

/// Opens the device for an output kind, off the tick thread. The engine's own is
/// `outputs::open`; a test brings one that opens fake devices.
pub type Open = Box<dyn FnMut(OutputKind) -> io::Result<Box<dyn Device>> + Send>;

pub(crate) struct LaneDelivery {
    pub(crate) lane: Box<Lane>,
    pub(crate) control: lane::Control,
}

/// Only opening workers call the configured factory. A retry can share it with a
/// later SetOutput without holding the publisher, the tick, or an adapter's thread.
#[derive(Clone)]
#[expect(
    clippy::disallowed_types,
    reason = "only blocking device-open workers take this lock"
)]
pub(super) struct Opener(pub(super) Arc<std::sync::Mutex<Open>>);

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
    pub(super) retry: Retry,
    pub(super) lease: Lease,
    pub(super) output: OutputNumber,
    pub(super) kind: OutputKind,
    pub(super) universes: Run,
    pub(super) rate: Rate,
    pub(super) shared: Arc<Shared>,
    pub(super) free: Thread,
    /// The devices sent away before this one opens: a port they hold must be free.
    pub(super) devices: u64,
    pub(super) route: Route,
    pub(super) clock: Clock,
    #[cfg(feature = "test-support")]
    pub(super) lane_pause: Option<lane::StartPause>,
    /// Can the installed lane be reused, or does this opening need a fresh device?
    pub(super) source: DeviceSource,
}

/// An exclusive port moves at admission. A socket can also be opened afresh if
/// the previous attachment has not passed through the tick and reclaimer in time.
pub(super) enum DeviceSource<Probe = Arc<AtomicBool>> {
    Fresh,
    Exclusive,
    Reusable(Probe),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OpenAction {
    Wait,
    Open,
    Reuse,
}

/// A socket can fall back to a fresh open at the limit. An exclusive device
/// is already reserved for this output and transfers at T8.
pub(super) fn open_action(
    port: DeviceSource<bool>,
    reclaiming: bool,
    elapsed: Duration,
) -> OpenAction {
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
    pub(super) lease: Lease,
    pub(super) output: OutputNumber,
    pub(super) kind: OutputKind,
    pub(super) universes: Run,
    pub(super) rate: Rate,
    pub(super) opened: Result<(), OpenFailure>,
    pub(super) reuse: bool,
}

/// The one opener, lent to an output edit with what the edit holds. Finished by
/// [`Sender::publish_output`]. Dropped unfinished (the caller let go of an [`Opening`]
/// or an [`Opened`]), it goes back to the front, which undoes the edit like a failed
/// open (output-cutover T4): the opener back, the old lane serving again, the project
/// as before. A new lane it held is freed off the caller's thread.
pub(super) struct Lease {
    pub(super) open: Opener,
    pub(super) output: OutputNumber,
    pub(super) previous: Option<lane::Control>,
    pub(super) lane: Option<LaneDelivery>,
    pub(super) back: Abandon,
    pub(super) finished: bool,
}

/// What an unfinished [`Lease`] brings back to the front.
pub(super) struct Unfinished {
    pub(super) open: Opener,
    pub(super) output: OutputNumber,
    pub(super) previous: Option<lane::Control>,
    pub(super) lane: Option<LaneDelivery>,
}

#[expect(
    clippy::disallowed_types,
    reason = "only a dropped opening sends, off the tick, and only the front receives"
)]
pub(super) type Abandon = std::sync::mpsc::Sender<Unfinished>;
#[expect(
    clippy::disallowed_types,
    reason = "only a dropped opening sends, off the tick, and only the front receives"
)]
pub(super) type Abandoned = std::sync::mpsc::Receiver<Unfinished>;

impl Lease {
    /// The opener and the new lane, for publication.
    pub(super) fn finish(mut self) -> (Opener, Option<LaneDelivery>) {
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
    pub(super) stopped: bool,
    pub(super) previous: Option<lane::Control>,
    pub(super) next: Option<Recovery>,
    pub(super) retry: Retry,
    pub(super) reuse: bool,
    pub(super) shared: Arc<Shared>,
    pub(super) unfinished_wake: Option<u64>,
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

/// Frees a device the tick thread did not take, and stops its thread, on a thread of its
/// own: closing a port can take long, and the sender may be shared behind a lock. Without
/// a thread from the system, it is freed here after all. Writers are freed here.
pub(super) fn free_elsewhere(brings: Brings) {
    if let Brings::Device(delivery) | Brings::Reroute(delivery, _) = brings {
        drop(
            thread::Builder::new()
                .name("free a refused device".into())
                .spawn(move || drop(delivery.lane)),
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
                if !self.shared.life.get().sends() || started.elapsed() >= FREE_WITHIN {
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
                return Err(OpenFailure::Unavailable);
            }
            let device = if reuse {
                None
            } else {
                let device = lease.open.device(self.kind);
                Some(device.map_err(|error| OpenFailure::Device {
                    with: error.raw_os_error(),
                })?)
            };
            let (lane, control) = Lane::start(
                device,
                self.route,
                self.clock,
                Arc::clone(&self.shared.life),
                lease.previous.as_ref(),
                #[cfg(feature = "test-support")]
                self.lane_pause,
            )
            .map_err(|_| OpenFailure::Thread)?;
            Ok(LaneDelivery {
                lane: Box::new(lane),
                control,
            })
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
        let opened = lane.map(|delivery| {
            lease.lane = Some(delivery);
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
pub(super) fn waits(shared: &Shared, devices: u64) -> bool {
    devices > shared.devices_freed.load(Ordering::Acquire)
}

/// Of the `retired` boxes sent back so far, those not freed yet.
pub(super) fn on_their_way(shared: &Shared, retired: u64) -> u64 {
    retired.saturating_sub(shared.freed.load(Ordering::Acquire))
}

/// A refused restart still has its old lane. If its adapter subsequently reports
/// failure, open the old kind off-thread and deliver it to that same lane.
#[derive(Clone)]
pub(crate) struct Recovery {
    pub(super) output: OutputNumber,
    pub(super) kind: OutputKind,
    pub(super) open: Opener,
    pub(super) lane: lane::Control,
}

#[expect(
    clippy::disallowed_types,
    reason = "only the blocking open worker submits retries"
)]
pub(crate) type Retry = std::sync::mpsc::Sender<Recovery>;

#[expect(
    clippy::disallowed_types,
    reason = "this is the blocking reopen worker"
)]
pub(crate) fn recover(requests: std::sync::mpsc::Receiver<Recovery>, shared: &Shared) {
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
pub(crate) fn free(mut retired: rtrb::Consumer<Retired>, shared: &Shared) {
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
