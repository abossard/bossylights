//! ENTTEC DMX USB Pro over the serial driver the system already has. The adapter makes
//! the DMX timing itself; the host sends one "send DMX" message per frame.
//!
//! | Byte | Message |
//! |---|---|
//! | 0 | start of message `0x7E` |
//! | 1 | label 6: send DMX |
//! | 2 to 3 | data length, little endian: 513 |
//! | 4 | DMX start code 0 |
//! | 5 to 516 | 512 channels: the routed stream, zero-padded |
//! | 517 | end of message `0xE7` |
//!
//! Who does what:
//!
//! | Thread | Does |
//! |---|---|
//! | output thread | [`DmxUsbPro::take`]: puts the message in a one-slot wait-free queue and wakes the port thread. Never calls the serial driver: a vanishing device holds the driver up for many milliseconds |
//! | port thread, one per adapter | owns the port: at each new port asks the widget its settings (label 3), then writes each message, never more often than the link drains. While the adapter is not there yet, or after a write fails or the port takes no byte for [`STALLED_FOR`] (then it closes the port first), it looks for the adapter every [`LOOK_EVERY`] |

use crate::{Adapter, Device, Limits, Outcome, Reading};
use engine_core::ADDRESSES;
use engine_core::health::{AfterHandoff, HandoffPart, HandoffSpan};
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use serialport::{
    ClearBuffer, DataBits, FlowControl, Parity, SerialPort, SerialPortType, StopBits, TTYPort,
};
use std::io::{self, ErrorKind};
use std::ops::RangeInclusive;
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The adapter's USB identity: an FTDI chip that calls itself "DMX USB PRO".
const VENDOR: u16 = 0x0403;
const PRODUCT: u16 = 0x6001;
const NAME: &str = "DMX USB PRO";
/// The link from the USB chip to the widget is a real UART. 44 frames need 22.8 kB/s;
/// 57 600 baud queued frames minutes behind, out of sight. 500 000 is about 2x headroom.
const BAUD: u32 = 500_000;
/// One "send DMX" message.
pub const MESSAGE: usize = 4 + 1 + ADDRESSES + 1;
/// The time the link takes for one message, 11 bits a byte, plus a quarter. A frame
/// that comes sooner is dropped instead of queued.
const LINK_TIME: Duration =
    Duration::from_nanos(MESSAGE as u64 * 11 * 1_000_000_000 * 5 / (BAUD as u64 * 4));
/// One message on the link, 11 bits a byte (8 data, 2 stop).
const ON_THE_LINK: Duration =
    Duration::from_nanos(MESSAGE as u64 * 11 * 1_000_000_000 / BAUD as u64);
/// DMX512: 250 000 bits a second, 11 bits a slot.
const SLOT: Duration = Duration::from_micros(44);
/// One step of the widget's break and mark after break (API p.3).
const STEP: Duration = Duration::from_nanos(10_670);
/// Label 3, "get widget parameters", asking for no user memory (API p.3).
const ASK: [u8; 7] = [0x7E, 3, 2, 0, 0, 0, 0xE7];
/// Tries at each new port: the widget ignores the first "get parameters" after another
/// program's run, and answers the second in 3 ms (`docs/spikes/dmx-timing.md`).
const ASKS: u8 = 2;
/// How long a try waits for the answer: FTDI's latency timer holds the widget's bytes up
/// to 16 ms by default; 3 ms measured.
const ANSWER_WITHIN: Duration = Duration::from_millis(50);
// Below FTDI's 16 ms latency timer (AN232B-04, its default) no answer would ever come in time.
const _: () = assert!(ANSWER_WITHIN.as_millis() > 16);
/// How often the port thread looks for a lost adapter.
const LOOK_EVERY: Duration = Duration::from_millis(500);
/// How often an idle port thread checks whether it should stop.
const WATCH_EVERY: Duration = Duration::from_millis(100);
/// How long the port thread waits for a full driver queue before it tries again.
const QUEUE_FULL: Duration = Duration::from_millis(1);
/// How long a message may stay unfinished after its first write that took no byte, and
/// how long a drain may take from its start, before the port counts as lost. Half a
/// second: a busy machine or a USB hiccup can hold the queue for a few messages without
/// the adapter being gone, and a person sees a stall within about that time anyway. Later
/// bytes never restart the clock, so a port that trickles still reaches it.
const STALLED_FOR: Duration = Duration::from_millis(500);
/// The most a route finish takes: the message (or label 3 request) being written when it
/// is asked, the one queued behind it, then the drain. Each is at most [`STALLED_FOR`], one [`QUEUE_FULL`]
/// past it and one [`LINK_TIME`]. Not covered: a listing of USB ports that hangs while
/// the adapter is lost. The opener then stops waiting, and `take` keeps dropping frames
/// until the port thread answered, so no frame of the next route meets the old one.
pub const FINISH_WITHIN: Duration = Duration::from_nanos(
    3 * (STALLED_FOR.as_nanos() + QUEUE_FULL.as_nanos() + LINK_TIME.as_nanos()) as u64,
);

/// The widget's settings, as label 3 answers them (API p.3): break and mark after break
/// in steps of 10.67 µs, and its rate in DMX frames a second, 0 for as fast as it can.
/// Only settings in the API's ranges exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    break_steps: u8,
    mark_steps: u8,
    rate: u8,
}

impl Settings {
    /// What a span counts while nothing was read: the API's defaults.
    const DEFAULT: Self = Self {
        break_steps: 9,
        mark_steps: 1,
        rate: 40,
    };

    /// Settings in the API's ranges: break 9 to 127, mark 1 to 127, rate 0 to 40.
    pub const fn new(break_steps: u8, mark_steps: u8, rate: u8) -> Option<Self> {
        match (break_steps, mark_steps, rate) {
            (9..=127, 1..=127, 0..=40) => Some(Self {
                break_steps,
                mark_steps,
                rate,
            }),
            _ => None,
        }
    }

    /// One word for the port thread to share. 0 is nothing read: its break, 0, is out of
    /// range.
    const fn packed(self) -> u32 {
        u32::from_be_bytes([0, self.break_steps, self.mark_steps, self.rate])
    }

    /// The settings a [`Settings::packed`] word holds; 0 holds none.
    const fn unpacked(word: u32) -> Option<Self> {
        let [_, break_steps, mark_steps, rate] = word.to_be_bytes();
        Self::new(break_steps, mark_steps, rate)
    }

    /// Break and mark after break: the time before the start code.
    fn before_start(self) -> Duration {
        STEP * (u32::from(self.break_steps) + u32::from(self.mark_steps))
    }

    /// The longest wait for the widget's next frame: one period at its rate, or the frame
    /// on the wire when that is longer; at rate 0 only the frame.
    fn period(self) -> Duration {
        let frame = self.before_start() + SLOT * (ADDRESSES as u32 + 1);
        match self.rate {
            0 => frame,
            rate => frame.max(Duration::from_nanos(
                1_000_000_000_u64.div_ceil(u64::from(rate)),
            )),
        }
    }
}

/// The settings in the first whole label 3 message of `heard`, if they are in the API's
/// ranges: firmware (2 bytes), break, mark, rate, then user memory (API p.3).
fn settings_of(heard: &[u8]) -> Option<Settings> {
    heard
        .iter()
        .enumerate()
        .filter(|&(_, &byte)| byte == 0x7E)
        .find_map(|(at, _)| {
            let message = heard.get(at..)?;
            let (&[_, 3, lsb, msb], rest) = message.split_first_chunk::<4>()? else {
                return None;
            };
            let length = usize::from(u16::from_le_bytes([lsb, msb]));
            let (data, end) = rest.split_at_checked(length)?;
            let (&[_, _, break_steps, mark_steps, rate], _) = data.split_first_chunk::<5>()?;
            (end.first() == Some(&0xE7)).then_some(())?;
            Settings::new(break_steps, mark_steps, rate)
        })
}

/// What follows the handoff, part by part. `patched`: the output's lowest and highest
/// patched slot. With none patched, a channel set by hand can still reach a light on any
/// slot, so the wire part spans all 512 and its source says why. `read`: the widget's
/// settings, if it answered; else its defaults count, and the sources say so.
///
/// | Stage | What | Span, source |
/// |---|---|---|
/// | `HostToAdapter` | the port thread's wake, then the USB transfer | not known: neither has a time in its spec |
/// | `SerialLink` | the message on the serial line to the widget | `Baud`: [`MESSAGE`] bytes, 11 bits each, at [`BAUD`] |
/// | `AdapterFrame` | the wait for the widget's next DMX frame, on its own timer | 0 to one period. `AdapterRead`, `AdapterReadAsFastAsItCan` (rate 0: the frame on the wire), `AdapterDefault` |
/// | `DmxWire` | the frame on the wire, from the end of the lowest patched slot to the end of the highest | 44 µs a slot, after the break and mark. `Dmx512AdapterRead`, `Dmx512AdapterDefault`, each with `NothingPatched`: the same over the whole universe |
/// | `SlotGap` | the widget's gaps between slots and between frames | not known: not in its API |
pub(crate) fn after_handoff(
    patched: Option<RangeInclusive<u32>>,
    read: Option<Settings>,
) -> AfterHandoff {
    let ((lowest, highest), wire) = match (patched, read) {
        (Some(slots), Some(_)) => (slots.into_inner(), "Dmx512AdapterRead"),
        (Some(slots), None) => (slots.into_inner(), "Dmx512AdapterDefault"),
        (None, Some(_)) => ((1, ADDRESSES as u32), "Dmx512AdapterReadNothingPatched"),
        (None, None) => ((1, ADDRESSES as u32), "Dmx512AdapterDefaultNothingPatched"),
    };
    let frame = match read {
        Some(Settings { rate: 0, .. }) => "AdapterReadAsFastAsItCan",
        Some(_) => "AdapterRead",
        None => "AdapterDefault",
    };
    let settings = read.unwrap_or(Settings::DEFAULT);
    // Slot n's last bit is (n + 1) slots after the mark: the start code is slot 0.
    let slot_end = |slot: u32| settings.before_start() + SLOT * (slot + 1);
    let not_known = |stage| HandoffPart {
        stage,
        span: HandoffSpan::NotKnown,
    };
    let computed = |stage, low, high, source| HandoffPart {
        stage,
        span: crate::computed(low, high, source),
    };
    AfterHandoff::new([
        not_known("HostToAdapter"),
        computed("SerialLink", ON_THE_LINK, ON_THE_LINK, "Baud"),
        computed("AdapterFrame", Duration::ZERO, settings.period(), frame),
        computed("DmxWire", slot_end(lowest), slot_end(highest), wire),
        not_known("SlotGap"),
    ])
}

/// Writes the first 512 stream bytes, with zeroes in any unused slots.
fn message(stream: &[u8], out: &mut [u8; MESSAGE]) {
    let [lsb, msb] = ((ADDRESSES + 1) as u16).to_le_bytes();
    let (head, rest) = out.split_at_mut(5);
    head.copy_from_slice(&[0x7E, 6, lsb, msb, 0]);
    let (channels, end) = rest.split_at_mut(ADDRESSES);
    let bytes = stream.get(..ADDRESSES).unwrap_or(stream);
    let (routed, padding) = channels.split_at_mut(bytes.len());
    routed.copy_from_slice(bytes);
    padding.fill(0);
    end.copy_from_slice(&[0xE7]);
}

/// Finds a port and opens it. Runs once in [`DmxUsbPro::open`], then on the port thread.
pub type Find = Box<dyn FnMut() -> io::Result<TTYPort> + Send>;

/// A DMX USB Pro, as the output thread holds it: no port, only the queue to its thread.
pub struct DmxUsbPro {
    message: [u8; MESSAGE],
    to_port: rtrb::Producer<[u8; MESSAGE]>,
    port_thread: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
    /// The number of the last route finish asked.
    asked: u32,
    /// Wakes the opener once the port thread answered a route finish.
    #[expect(
        clippy::disallowed_types,
        reason = "only the opener waits here, off the output thread"
    )]
    answers: mpsc::Receiver<()>,
}

/// What the output thread, the port thread and a [`Reading`] share.
#[derive(Default)]
pub(crate) struct Shared {
    /// Set while there is no port: not found yet, or lost.
    lost: AtomicBool,
    stop: AtomicBool,
    /// The route finish the opener asked and the port thread has not answered: its
    /// number, 0 for none. `take` drops every frame while one is open.
    finish: AtomicU32,
    /// The system error that lost the port, or refused to open it, last; 0 if none.
    lost_with: AtomicI32,
    /// The settings the widget on the current port answered, [`Settings::packed`]; 0 if
    /// none yet.
    settings: AtomicU32,
}

impl Shared {
    /// The settings the widget on the current port answered, if it did.
    pub(crate) fn settings(&self) -> Option<Settings> {
        Settings::unpacked(self.settings.load(Ordering::Acquire))
    }
}

/// Why the port was lost last: the system's error, as the port thread saw it.
pub struct LostWith(Arc<Shared>);

impl LostWith {
    /// The last error that lost the port or refused to open it, if one did.
    pub fn get(&self) -> Option<io::Error> {
        match self.0.lost_with.load(Ordering::Relaxed) {
            0 => None,
            code => Some(io::Error::from_raw_os_error(code)),
        }
    }
}

impl DmxUsbPro {
    /// Looks for the port once, here, then hands it to its own thread. An adapter that
    /// is not there, or that refuses to open, is no error: the thread looks for it every
    /// [`LOOK_EVERY`], and frames fail until it is found. Fails only when the system
    /// refuses a thread. Call it off the output thread.
    pub fn open(mut find: Find) -> io::Result<Self> {
        let found = find();
        let link = next(Link::NotFound, seen_of(&found), Instant::now());
        let port = found.ok();
        let (to_port, messages) = rtrb::RingBuffer::new(1);
        let (answer, answers) = mpsc::sync_channel(1);
        let shared = Arc::new(Shared {
            lost: AtomicBool::new(port.is_none()),
            lost_with: AtomicI32::new(cause(link).map_or(0, |with| with as i32)),
            ..Shared::default()
        });
        let port_thread = PortThread {
            find,
            port,
            link,
            messages,
            unsent: None,
            ask: Ask::Try(1),
            shared: Arc::clone(&shared),
            answer,
        };
        let handle = thread::Builder::new()
            .name("dmx usb pro port".into())
            .spawn(move || port_thread.run())?;
        Ok(Self {
            message: [0; MESSAGE],
            to_port,
            port_thread: Some(handle),
            shared,
            asked: 0,
            answers,
        })
    }

    /// Why the port was lost last; readable from any thread.
    pub fn lost_with(&self) -> LostWith {
        LostWith(Arc::clone(&self.shared))
    }
}

impl Device for DmxUsbPro {
    fn reconnects_itself(&self) -> bool {
        true
    }

    /// The settings the widget answers at each new port.
    fn reading(&self) -> Reading {
        Reading(Arc::downgrade(&self.shared))
    }

    /// Asks the port thread to finish the route and waits for its answer, at most
    /// [`FINISH_WITHIN`].
    fn finish_route(&mut self) {
        let Some(worker) = &self.port_thread else {
            return;
        };
        self.asked = self.asked.wrapping_add(1).max(1);
        let asked = self.asked;
        self.shared.finish.store(asked, Ordering::Release);
        worker.thread().unpark();
        let until = Instant::now() + FINISH_WITHIN;
        loop {
            let heard = match self
                .answers
                .recv_timeout(until.saturating_duration_since(Instant::now()))
            {
                Ok(()) => Wake::Answer,
                Err(RecvTimeoutError::Timeout) => Wake::Timeout,
                Err(RecvTimeoutError::Disconnected) => Wake::Closed,
            };
            let open = self.shared.finish.load(Ordering::Acquire) == asked;
            match waited(open, heard) {
                Wait::Waiting => {}
                Wait::Done | Wait::GaveUp => return,
            }
        }
    }

    fn limits(&self) -> Limits {
        Limits::USB
    }

    /// Hands the message to the port thread. While it has not taken the last one yet, or
    /// a route finish is open, this frame is dropped; while the port is lost, it fails.
    fn take(&mut self, stream: &[u8]) -> Outcome {
        let lost = self.shared.lost.load(Ordering::Acquire);
        let finishing = self.shared.finish.load(Ordering::Acquire) != 0;
        if let Some(outcome) = gate(lost, finishing, self.to_port.is_full()) {
            return outcome;
        }
        message(stream, &mut self.message);
        match self.to_port.push(self.message) {
            Ok(()) => {
                if let Some(port_thread) = &self.port_thread {
                    port_thread.thread().unpark();
                }
                Outcome::Accepted
            }
            Err(_) => Outcome::Dropped,
        }
    }
}

/// Whether `take` answers before it queues: a calculation. A lost port fails; an open
/// route finish or a full queue drops the frame; `None` queues it.
const fn gate(lost: bool, finishing: bool, full: bool) -> Option<Outcome> {
    match (lost, finishing, full) {
        (true, _, _) => Some(Outcome::Failed),
        (false, true, _) | (false, false, true) => Some(Outcome::Dropped),
        (false, false, false) => None,
    }
}

impl Drop for DmxUsbPro {
    /// Stops the port thread and waits until it closed the port, so the port is free
    /// for the next opener. Never on the output thread: the engine frees devices off it.
    /// Each wait of the port thread watches `stop`, so this takes at most a link time
    /// or a look for the adapter. Before the port closes, the message being written is
    /// finished and what the driver holds is sent, so the next opener's first frame is
    /// whole: at most two [`STALLED_FOR`] more when the port takes no byte.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(port_thread) = self.port_thread.take() {
            port_thread.thread().unpark();
            drop(port_thread.join());
        }
    }
}

/// What woke the opener waiting for a route finish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wake {
    /// The port thread answered a finish, this one or one the opener gave up on.
    Answer,
    /// [`FINISH_WITHIN`] passed.
    Timeout,
    /// The port thread ended.
    Closed,
}

/// Where the opener's wait stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wait {
    Waiting,
    Done,
    GaveUp,
}

/// The opener's wait: a calculation. `open`: its finish is not answered yet.
const fn waited(open: bool, heard: Wake) -> Wait {
    match (open, heard) {
        (true, Wake::Answer) => Wait::Waiting,
        (true, Wake::Timeout) => Wait::GaveUp,
        (false, Wake::Answer | Wake::Timeout) | (_, Wake::Closed) => Wait::Done,
    }
}

/// The one thread that calls the serial driver for this adapter.
struct PortThread {
    find: Find,
    port: Option<TTYPort>,
    link: Link,
    messages: rtrb::Consumer<[u8; MESSAGE]>,
    /// The message whose write lost the port. Written first once the adapter is found
    /// again, then the one `take` queued meanwhile. A route finish discards both.
    unsent: Option<[u8; MESSAGE]>,
    /// Where asking the widget on the current port its settings stands.
    ask: Ask,
    shared: Arc<Shared>,
    #[expect(
        clippy::disallowed_types,
        reason = "the port thread answers the opener, never the output thread"
    )]
    answer: mpsc::SyncSender<()>,
}

impl PortThread {
    fn lose(&mut self, with: Errno) {
        self.link = Link::Lost { with };
        self.shared.lost_with.store(with as i32, Ordering::Relaxed);
        self.shared.settings.store(0, Ordering::Release);
        self.shared.lost.store(true, Ordering::Release);
        self.port = None;
    }

    /// Runs until `drop` asks it to stop, as [`Life`] says: then sends what the driver
    /// holds and closes the port.
    fn run(mut self) {
        let mut life = Life::Running;
        loop {
            let has_port = self.port.is_some();
            let stop = self.shared.stop.load(Ordering::Relaxed);
            let finish = self.shared.finish.load(Ordering::Acquire);
            let (after, answer) = match (life, hears(life, stop, finish)) {
                (_, Some(heard)) => next_life(life, heard, has_port),
                (Life::Running, None) => {
                    self.step();
                    (life, None)
                }
                (Life::Finishing { .. }, None) => {
                    if self.finish_turn() {
                        next_life(life, Heard::Finished, self.port.is_some())
                    } else {
                        (life, None)
                    }
                }
                (Life::Draining, None) => {
                    if let Some(port) = &self.port {
                        drain(port);
                    }
                    next_life(life, Heard::Drained, has_port)
                }
                // Dropping `self` closes the port.
                (Life::Closed, None) => return,
            };
            if let Some(asked) = answer {
                self.answer(asked);
            }
            life = after;
        }
    }

    /// Marks finish `asked` answered, unless the opener asked a newer one meanwhile,
    /// and wakes the opener. A wake already waiting is enough.
    fn answer(&self, asked: u32) {
        let _ = self
            .shared
            .finish
            .compare_exchange(asked, 0, Ordering::AcqRel, Ordering::Acquire);
        let _ = self.answer.try_send(());
    }

    /// One turn of a route finish. On a healthy port the last accepted message is
    /// written first. A failed write belongs to the old route and never replays on the
    /// new one. Returns whether the route is finished.
    fn finish_turn(&mut self) -> bool {
        if self.port.is_some()
            && self.unsent.is_none()
            && let Ok(message) = self.messages.pop()
        {
            self.write(&message);
            return false;
        }
        self.unsent = None;
        while self.messages.pop().is_ok() {}
        if let Some(port) = &self.port
            && let Some(with) = loses_after(drain(port))
        {
            let _ = port.clear(ClearBuffer::Output);
            self.lose(with);
        }
        true
    }

    /// One turn while running: looks for the adapter, asks the widget its settings, or
    /// writes one whole message.
    fn step(&mut self) {
        if self.port.is_none() {
            // An adapter not found or lost is gone for a while; then look again.
            // `drop` and a route finish wake it; a wake left over from `take` does not
            // cut the wait short.
            let look_at = Instant::now() + LOOK_EVERY;
            let asked = || {
                cuts_look_short(
                    self.shared.stop.load(Ordering::Relaxed),
                    self.shared.finish.load(Ordering::Acquire),
                )
            };
            while Instant::now() < look_at && !asked() {
                thread::park_timeout(look_at.saturating_duration_since(Instant::now()));
            }
            if asked() {
                return;
            }
            let found = (self.find)();
            self.link = next(self.link, seen_of(&found), Instant::now());
            if let Some(with) = cause(self.link) {
                self.shared.lost_with.store(with as i32, Ordering::Relaxed);
            }
            if let Ok(port) = found {
                self.port = Some(port);
                self.ask = Ask::Try(1);
                self.shared.lost.store(false, Ordering::Release);
            }
            return;
        }
        if let Ask::Try(n) = self.ask {
            self.ask = after_try(n, self.try_ask());
            return;
        }
        // The one that failed was taken before the one queued.
        let Some(message) = self.unsent.take().or_else(|| self.messages.pop().ok()) else {
            thread::park_timeout(WATCH_EVERY);
            return;
        };
        self.write(&message);
    }

    /// One try at the widget's settings: clears the input, writes label 3, then reads
    /// what comes for at most [`ANSWER_WITHIN`]. A stop or a finish cuts the wait.
    fn try_ask(&mut self) -> Tried {
        let Some(port) = &self.port else {
            return Tried::Lost;
        };
        let _ = port.clear(ClearBuffer::Input);
        self.link = write_all(port, &ASK, self.link);
        if let Some(with) = cause(self.link) {
            self.lose(with);
            return Tried::Lost;
        }
        let until = Instant::now() + ANSWER_WITHIN;
        let mut heard = [0; 64];
        let mut length = 0;
        loop {
            if cuts_look_short(
                self.shared.stop.load(Ordering::Relaxed),
                self.shared.finish.load(Ordering::Acquire),
            ) {
                return Tried::Cut;
            }
            if let Some(free) = heard.get_mut(length..)
                && let Ok(read) = nix::unistd::read(port.as_raw_fd(), free)
            {
                length += read;
            }
            if let Some(settings) = settings_of(heard.get(..length).unwrap_or_default()) {
                self.shared
                    .settings
                    .store(settings.packed(), Ordering::Release);
                return Tried::Answered;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Tried::Nothing;
            }
            thread::park_timeout(left.min(QUEUE_FULL));
        }
    }

    /// Writes one whole message, then waits out the rest of its link time. A write that
    /// loses the port keeps the message as `unsent`.
    #[expect(
        clippy::disallowed_methods,
        reason = "the port thread sleeps; the output thread never does"
    )]
    fn write(&mut self, message: &[u8; MESSAGE]) {
        let Some(port) = &self.port else {
            return;
        };
        let started = Instant::now();
        self.link = write_all(port, message, self.link);
        match self.link {
            Link::Writing | Link::Stalled { .. } => {
                thread::sleep(LINK_TIME.saturating_sub(started.elapsed()));
            }
            Link::Lost { with } | Link::CannotOpen { with } => {
                self.unsent = Some(*message);
                self.lose(with);
            }
            // A write never leads here: the link was found before it.
            Link::NotFound => {}
        }
    }
}

/// Where asking the widget on the current port its settings stands. Every pair:
/// `docs/design/outputs.md`, "Ask".
///
/// ```text
/// [*] --> Try(1) --nothing--> Try(2) --nothing--> Done
/// Try(n) --answered--> Read; Try(n) --cut--> Try(n); a port found --> Try(1)
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    /// Try `n` comes next, counted from 1.
    Try(u8),
    /// The widget answered: its settings are kept. No more tries on this port.
    Read,
    /// [`ASKS`] tries heard nothing: the port stays in use. No more tries on this port.
    Done,
}

/// What one try heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tried {
    /// Settings in the API's ranges, now kept.
    Answered,
    /// None in time, out of range or garbled.
    Nothing,
    /// A stop or a finish ended the wait.
    Cut,
    /// The request's write lost the port.
    Lost,
}

/// The next ask after try `n` heard `tried`: a calculation. A cut try does not count; a
/// lost port is asked again from try 1 once it is found.
const fn after_try(n: u8, tried: Tried) -> Ask {
    match tried {
        Tried::Answered => Ask::Read,
        Tried::Nothing if n >= ASKS => Ask::Done,
        Tried::Nothing => Ask::Try(n + 1),
        Tried::Cut | Tried::Lost => Ask::Try(n),
    }
}

/// How a drain ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drain {
    /// The driver holds nothing more.
    Empty,
    /// The driver still held bytes [`STALLED_FOR`] after the drain began.
    Stuck,
    /// The driver could not say what it holds: nothing is known to be sent.
    Failed(Errno),
}

/// The error a route finish loses its port with after `drained`: a calculation. A drain
/// that did not end empty may hold old-route bytes: the driver is cleared, the port lost.
const fn loses_after(drained: Drain) -> Option<Errno> {
    match drained {
        Drain::Empty => None,
        Drain::Stuck => Some(Errno::ETIMEDOUT),
        Drain::Failed(with) => Some(with),
    }
}

/// One look at what the driver holds during a drain that began at `started`: a
/// calculation. `None`: look again.
fn drain_step(held: Result<u32, Errno>, started: Instant, now: Instant) -> Option<Drain> {
    match held {
        Ok(0) => Some(Drain::Empty),
        Err(with) => Some(Drain::Failed(with)),
        Ok(_) if now.saturating_duration_since(started) >= STALLED_FOR => Some(Drain::Stuck),
        Ok(_) => None,
    }
}

/// Waits until the driver sent all it holds, then one link time for bytes still on their
/// way (in the adapter; on Linux, inside a pseudo terminal, which reports none held).
/// Closing sooner may throw away the end of the last message.
#[expect(
    clippy::disallowed_methods,
    reason = "the port thread sleeps; the output thread never does"
)]
fn drain(port: &TTYPort) -> Drain {
    let started = Instant::now();
    let drained = loop {
        let held = port
            .bytes_to_write()
            .map_err(|error| errno_of(&io::Error::from(error)));
        if let Some(drained) = drain_step(held, started, Instant::now()) {
            break drained;
        }
        thread::sleep(QUEUE_FULL);
    };
    thread::sleep(LINK_TIME);
    drained
}

/// Writes the whole message, straight to the system: a full driver queue is waited out
/// here, on the port thread, until [`STALLED_FOR`] passes after the first write that took
/// no byte. Half a message would garble the link, so a stop waits for the rest too.
/// Returns the link as it is after the write.
#[expect(
    clippy::disallowed_methods,
    reason = "the port thread sleeps; the output thread never does"
)]
fn write_all(port: &TTYPort, message: &[u8], mut link: Link) -> Link {
    let mut rest = message;
    while !rest.is_empty() {
        let seen = match nix::unistd::write(port.as_raw_fd(), rest) {
            Ok(0) | Err(Errno::EAGAIN | Errno::EINTR) => Seen::NoBytes,
            Ok(written) => {
                rest = rest.get(written..).unwrap_or_default();
                Seen::Bytes
            }
            Err(error) => Seen::Error(error),
        };
        link = next(link, seen, Instant::now());
        match link {
            Link::Writing | Link::Stalled { .. } if seen == Seen::NoBytes => {
                thread::sleep(QUEUE_FULL);
            }
            Link::Writing | Link::Stalled { .. } => {}
            Link::NotFound | Link::CannotOpen { .. } | Link::Lost { .. } => return link,
        }
    }
    next(link, Seen::Whole, Instant::now())
}

/// The port thread's link to the adapter. Every pair: `docs/design/outputs.md`.
///
/// ```text
/// [*] --> NotFound --found--> Writing --no byte--> Stalled --whole--> Writing
/// NotFound --refused--> CannotOpen --found--> Writing
/// Stalled --STALLED_FOR after its first no byte--> Lost --found again--> Writing
/// any write error --> Lost; a refused look --> CannotOpen
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Link {
    /// No port: the adapter is not there (never found yet, or gone after it refused).
    NotFound,
    /// No port: the adapter is there, but the system refused to open it with `with`
    /// (no permission, or another program holds it).
    CannotOpen { with: Errno },
    /// The current message is going out, or the last one went out whole.
    Writing,
    /// A write of the current message took no byte at `since`: the driver queue is full.
    Stalled { since: Instant },
    /// No port: closed after `with`, looked for every [`LOOK_EVERY`].
    Lost { with: Errno },
}

/// What the port thread saw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seen {
    /// A write took bytes.
    Bytes,
    /// A write took none: the driver queue is full.
    NoBytes,
    /// The last byte of the message was written.
    Whole,
    /// A write failed.
    Error(Errno),
    /// The finder opened a port.
    Found,
    /// The finder found no adapter.
    Missing,
    /// The finder found the adapter, but the system refused to open it.
    Refused(Errno),
}

/// What a find says, as the link sees it: a calculation.
fn seen_of<T>(found: &io::Result<T>) -> Seen {
    match found {
        Ok(_) => Seen::Found,
        Err(error) => {
            let with = errno_of(error);
            if matches!(with, Errno::ENOENT | Errno::ENODEV | Errno::ENXIO) {
                Seen::Missing
            } else {
                Seen::Refused(with)
            }
        }
    }
}

/// The system's error code for `error`. The serial crate keeps only the kind of an open
/// error, so a kind stands for the code std maps to it (`decode_error_kind`).
fn errno_of(error: &io::Error) -> Errno {
    match (error.raw_os_error(), error.kind()) {
        (Some(code), _) => Errno::from_i32(code),
        (None, ErrorKind::PermissionDenied) => Errno::EACCES,
        (None, ErrorKind::NotFound) => Errno::ENOENT,
        (None, ErrorKind::ResourceBusy) => Errno::EBUSY,
        (None, _) => Errno::EIO,
    }
}

/// The error a link without a port keeps, if it keeps one.
const fn cause(link: Link) -> Option<Errno> {
    match link {
        Link::CannotOpen { with } | Link::Lost { with } => Some(with),
        Link::NotFound | Link::Writing | Link::Stalled { .. } => None,
    }
}

/// The one transition function of the link: a calculation, time comes in as `now`.
fn next(link: Link, seen: Seen, now: Instant) -> Link {
    let stalled_since = |since: Instant| {
        if now.saturating_duration_since(since) >= STALLED_FOR {
            Link::Lost {
                with: Errno::ETIMEDOUT,
            }
        } else {
            Link::Stalled { since }
        }
    };
    match (link, seen) {
        (_, Seen::Error(with)) => Link::Lost { with },
        (
            Link::NotFound | Link::CannotOpen { .. } | Link::Lost { .. } | Link::Writing,
            Seen::Found,
        )
        | (Link::Writing | Link::Stalled { .. }, Seen::Whole)
        | (Link::Writing, Seen::Bytes) => Link::Writing,
        (Link::NotFound | Link::CannotOpen { .. }, Seen::Missing) => Link::NotFound,
        (Link::NotFound | Link::CannotOpen { .. } | Link::Lost { .. }, Seen::Refused(with)) => {
            Link::CannotOpen { with }
        }
        (
            gone @ (Link::NotFound | Link::CannotOpen { .. } | Link::Lost { .. }),
            Seen::Bytes | Seen::NoBytes | Seen::Whole,
        )
        | (gone @ Link::Lost { .. }, Seen::Missing) => gone,
        (Link::Writing, Seen::NoBytes) => Link::Stalled { since: now },
        (Link::Stalled { since }, Seen::Bytes | Seen::NoBytes) => stalled_since(since),
        // A port is never looked for while it is open.
        (same @ (Link::Writing | Link::Stalled { .. }), Seen::Missing | Seen::Refused(_))
        | (same @ Link::Stalled { .. }, Seen::Found) => same,
    }
}

/// The port thread's life. A stop never cuts a message: it is looked at between
/// messages, and [`write_all`] returns only once the whole message is written or the
/// port is lost. Every pair: `docs/design/outputs.md`.
///
/// ```text
/// [*] --> Running --finish asked--> Finishing --finished, answer--> Running
/// Running, Finishing --stop, a port--> Draining --drained--> Closed
/// Running, Finishing --stop, no port--> Closed
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Life {
    /// Writes messages and looks for a lost adapter.
    Running,
    /// Ends the old route: its last accepted message goes out, nothing older replays,
    /// the driver drains. `asked` is the number the opener waits for.
    Finishing { asked: u32 },
    /// Stopped: the driver sends what it holds, so the next opener's first frame is whole.
    Draining,
    /// Done: the port is closed.
    Closed,
}

/// What the port thread heard about its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Heard {
    /// `drop` asked it to stop.
    Stop,
    /// The opener asked for route finish number `n`.
    Finish(u32),
    /// The route finish is done.
    Finished,
    /// [`drain`] returned.
    Drained,
}

/// What the port thread hears at the top of a turn in `life`, given what `drop` and the
/// opener asked: a calculation. `None`: do the life's own work this turn. A stop wins
/// over a finish; a finish is listened for only while running.
const fn hears(life: Life, stop: bool, finish: u32) -> Option<Heard> {
    match (life, stop, finish) {
        (Life::Running | Life::Finishing { .. }, true, _) => Some(Heard::Stop),
        (Life::Running, false, 0)
        | (Life::Finishing { .. }, false, _)
        | (Life::Draining | Life::Closed, _, _) => None,
        (Life::Running, false, asked) => Some(Heard::Finish(asked)),
    }
}

/// Whether a wait to look for the adapter again ends now: `drop` or a route finish
/// asked. A calculation.
const fn cuts_look_short(stop: bool, finish: u32) -> bool {
    stop || finish != 0
}

/// The one transition function of the port thread's life: a calculation. `has_port`
/// tells whether there is a port to drain. The second value is the route finish to
/// answer, if any.
const fn next_life(life: Life, heard: Heard, has_port: bool) -> (Life, Option<u32>) {
    match (life, heard) {
        (Life::Running | Life::Finishing { .. }, Heard::Stop) if has_port => (Life::Draining, None),
        (Life::Running | Life::Finishing { .. }, Heard::Stop)
        | (Life::Draining, Heard::Drained)
        | (Life::Closed, _) => (Life::Closed, None),
        (Life::Running | Life::Finishing { .. }, Heard::Finish(asked)) => {
            (Life::Finishing { asked }, None)
        }
        (Life::Finishing { asked }, Heard::Finished) => (Life::Running, Some(asked)),
        (same @ (Life::Running | Life::Finishing { .. }), Heard::Finished | Heard::Drained)
        | (same @ Life::Draining, Heard::Stop | Heard::Finish(_) | Heard::Finished) => (same, None),
    }
}

/// The finder for real adapters: the `adapter`th DMX USB Pro by USB identity when it
/// first finds one, from then on that same adapter by its serial.
/// Opened at [`BAUD`], 8 data bits, 2 stop bits, writes that never wait.
pub fn find(adapter: Adapter) -> Find {
    let mut wanted = None;
    Box::new(move || {
        let listed = adapters()?;
        let not_found = || io::Error::new(ErrorKind::NotFound, "no such DMX USB Pro");
        let wanted = match &mut wanted {
            Some(wanted) => wanted,
            empty @ None => empty.insert(pick(&listed, adapter).ok_or_else(not_found)?),
        };
        open_port(port_of(&listed, wanted).ok_or_else(not_found)?)
    })
}

/// A DMX USB Pro as the system lists it now.
#[derive(Clone)]
struct Listed {
    serial: String,
    port: String,
}

/// The adapter an output drives: its USB serial, fixed when its finder first ran, so a
/// lost adapter never comes back as another. Kept in memory only: never logged, printed
/// or put in the state.
struct Wanted(String);

/// The `adapter`th in the list.
fn pick(listed: &[Listed], adapter: Adapter) -> Option<Wanted> {
    listed
        .get(usize::from(adapter.number() - 1))
        .map(|listed| Wanted(listed.serial.clone()))
}

/// The port of the wanted adapter in the list as it is now, wherever it is in it.
fn port_of<'a>(listed: &'a [Listed], wanted: &Wanted) -> Option<&'a str> {
    listed
        .iter()
        .find(|listed| listed.serial == wanted.0)
        .map(|listed| listed.port.as_str())
}

/// The adapter number of the port with this name, as [`find`] counts.
pub fn adapter_of(port: &str) -> io::Result<Adapter> {
    let at = adapters()?
        .iter()
        .position(|listed| listed.port == port)
        .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "that port is no DMX USB Pro"))?;
    u8::try_from(at + 1)
        .ok()
        .and_then(|number| Adapter::try_from(number).ok())
        .ok_or_else(|| io::Error::other("too many adapters"))
}

/// Every DMX USB Pro, in the order of their USB serial numbers.
fn adapters() -> io::Result<Vec<Listed>> {
    let mut ports: Vec<(String, String)> = serialport::available_ports()?
        .into_iter()
        .filter_map(|port| match port.port_type {
            SerialPortType::UsbPort(usb)
                if usb.vid == VENDOR
                    && usb.pid == PRODUCT
                    && usb.product.as_deref().is_none_or(|p| p == NAME)
                    && !port.port_name.starts_with("/dev/tty.") =>
            {
                Some((usb.serial_number.unwrap_or_default(), port.port_name))
            }
            SerialPortType::UsbPort(_)
            | SerialPortType::PciPort
            | SerialPortType::BluetoothPort
            | SerialPortType::Unknown => None,
        })
        .collect();
    ports.sort();
    let listed = |(serial, port)| Listed { serial, port };
    Ok(ports.into_iter().map(listed).collect())
}

/// Opens a port for the adapter, exclusive, writes never wait.
fn open_port(name: &str) -> io::Result<TTYPort> {
    let port = serialport::new(name, BAUD)
        .data_bits(DataBits::Eight)
        .stop_bits(StopBits::Two)
        .parity(Parity::None)
        .flow_control(FlowControl::None)
        .timeout(Duration::ZERO)
        .open_native()?;
    never_wait(&port)?;
    Ok(port)
}

/// Makes writes on `port` return at once, and its close never wait for the queue to
/// drain; also for a port a test made.
pub fn never_wait(port: &TTYPort) -> io::Result<()> {
    fcntl(port.as_raw_fd(), FcntlArg::F_SETFL(OFlag::O_NONBLOCK))
        .map(|_| ())
        .map_err(io::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// Every pair of the link table in `docs/design/outputs.md`.
    #[test]
    fn every_link_and_what_the_port_thread_saw() {
        let t0 = Instant::now();
        let stalled = Link::Stalled { since: t0 };
        let lost = |with| Link::Lost { with };
        let refused = |with| Link::CannotOpen { with };
        let (io, gone, eacces) = (Errno::EIO, Errno::ENXIO, Errno::EACCES);
        let (half, over) = (t0 + STALLED_FOR / 2, t0 + STALLED_FOR);
        // (link, what the port thread saw, when, the link after)
        let cases = [
            (Link::NotFound, Seen::Bytes, t0, Link::NotFound),
            (Link::NotFound, Seen::NoBytes, t0, Link::NotFound),
            (Link::NotFound, Seen::Whole, t0, Link::NotFound),
            (Link::NotFound, Seen::Error(io), t0, lost(io)),
            (Link::NotFound, Seen::Found, t0, Link::Writing), // plugged in after the output was set
            (Link::NotFound, Seen::Missing, t0, Link::NotFound),
            (Link::NotFound, Seen::Refused(eacces), t0, refused(eacces)),
            (refused(eacces), Seen::Bytes, t0, refused(eacces)),
            (refused(eacces), Seen::NoBytes, t0, refused(eacces)),
            (refused(eacces), Seen::Whole, t0, refused(eacces)),
            (refused(eacces), Seen::Error(io), t0, lost(io)),
            (refused(eacces), Seen::Found, t0, Link::Writing),
            (refused(eacces), Seen::Missing, t0, Link::NotFound),
            (
                refused(eacces),
                Seen::Refused(Errno::EBUSY),
                t0,
                refused(Errno::EBUSY),
            ),
            (Link::Writing, Seen::Bytes, t0, Link::Writing),
            (Link::Writing, Seen::NoBytes, t0, stalled),
            (Link::Writing, Seen::Whole, t0, Link::Writing),
            (Link::Writing, Seen::Error(io), t0, lost(io)),
            (Link::Writing, Seen::Found, t0, Link::Writing), // n/a: no look with a port
            (Link::Writing, Seen::Missing, t0, Link::Writing), // n/a
            (Link::Writing, Seen::Refused(eacces), t0, Link::Writing), // n/a
            (stalled, Seen::Bytes, half, stalled),           // bytes never restart the clock
            (stalled, Seen::Bytes, over, lost(Errno::ETIMEDOUT)), // a trickle still ends
            (stalled, Seen::NoBytes, half, stalled),
            (stalled, Seen::NoBytes, over, lost(Errno::ETIMEDOUT)),
            (stalled, Seen::Whole, over, Link::Writing),
            (stalled, Seen::Error(gone), t0, lost(gone)),
            (stalled, Seen::Found, t0, stalled),           // n/a
            (stalled, Seen::Missing, t0, stalled),         // n/a
            (stalled, Seen::Refused(eacces), t0, stalled), // n/a
            (lost(io), Seen::Bytes, t0, lost(io)),
            (lost(io), Seen::NoBytes, t0, lost(io)),
            (lost(io), Seen::Whole, t0, lost(io)),
            (lost(io), Seen::Error(gone), t0, lost(gone)),
            (lost(io), Seen::Found, t0, Link::Writing),
            (lost(io), Seen::Missing, t0, lost(io)), // still unplugged: the cause stays
            (lost(io), Seen::Refused(eacces), t0, refused(eacces)),
        ];
        for (link, seen, now, after) in cases {
            assert_eq!(next(link, seen, now), after, "{link:?} {seen:?}");
        }
    }

    /// Expected codes: open(2) (ENOENT, ENXIO, ENODEV: no such file or device; EACCES:
    /// no permission), TIOCEXCL in tty_ioctl(4) and termios(4) (EBUSY: held exclusive),
    /// std's `decode_error_kind` for the kind of each code, and serialport's
    /// `From<Error> for io::Error`, which keeps the kind and drops the code.
    #[test]
    fn a_find_error_is_an_absent_adapter_or_a_refusal() {
        let os = |errno: Errno| io::Error::from_raw_os_error(errno as i32);
        let kind = |kind: ErrorKind| io::Error::new(kind, "serialport");
        // (find error, its code, what the link sees)
        let cases = [
            (os(Errno::ENOENT), Errno::ENOENT, Seen::Missing),
            (os(Errno::ENXIO), Errno::ENXIO, Seen::Missing),
            (os(Errno::ENODEV), Errno::ENODEV, Seen::Missing),
            (
                os(Errno::EACCES),
                Errno::EACCES,
                Seen::Refused(Errno::EACCES),
            ),
            (os(Errno::EBUSY), Errno::EBUSY, Seen::Refused(Errno::EBUSY)),
            (kind(ErrorKind::NotFound), Errno::ENOENT, Seen::Missing),
            (
                kind(ErrorKind::PermissionDenied),
                Errno::EACCES,
                Seen::Refused(Errno::EACCES),
            ),
            (
                kind(ErrorKind::ResourceBusy),
                Errno::EBUSY,
                Seen::Refused(Errno::EBUSY),
            ),
            (
                kind(ErrorKind::Other),
                Errno::EIO,
                Seen::Refused(Errno::EIO),
            ),
        ];
        for (error, code, seen) in cases {
            assert_eq!(errno_of(&error), code, "{error:?}");
            assert_eq!(seen_of::<()>(&Err(error)), seen, "{code:?}");
        }
        assert_eq!(seen_of(&Ok(())), Seen::Found);
    }

    /// Every pair of the life table in `docs/design/outputs.md`.
    #[test]
    fn every_life_and_what_the_port_thread_heard() {
        let finishing = |asked| Life::Finishing { asked };
        // (life, what the port thread heard, a port, the life after, finish answered)
        let cases = [
            (Life::Running, Heard::Stop, true, Life::Draining, None),
            (Life::Running, Heard::Stop, false, Life::Closed, None),
            (Life::Running, Heard::Finish(1), true, finishing(1), None),
            (Life::Running, Heard::Finish(1), false, finishing(1), None),
            (Life::Running, Heard::Finished, true, Life::Running, None), // n/a
            (Life::Running, Heard::Drained, true, Life::Running, None),  // n/a
            // Stop while finishing: only after the opener gave up waiting, as it holds
            // the device while it waits.
            (finishing(1), Heard::Stop, true, Life::Draining, None),
            (finishing(1), Heard::Stop, false, Life::Closed, None),
            (finishing(1), Heard::Finish(2), true, finishing(2), None), // n/a: not listened for, kept in `finish`
            (finishing(1), Heard::Finished, true, Life::Running, Some(1)),
            (finishing(1), Heard::Finished, false, Life::Running, Some(1)),
            (finishing(1), Heard::Drained, true, finishing(1), None), // n/a
            (Life::Draining, Heard::Stop, true, Life::Draining, None),
            (Life::Draining, Heard::Finish(1), true, Life::Draining, None), // n/a: drop owns the device
            (Life::Draining, Heard::Finished, true, Life::Draining, None),  // n/a
            (Life::Draining, Heard::Drained, true, Life::Closed, None),
            (Life::Draining, Heard::Drained, false, Life::Closed, None),
            (Life::Closed, Heard::Stop, true, Life::Closed, None),
            (Life::Closed, Heard::Finish(1), true, Life::Closed, None),
            (Life::Closed, Heard::Finished, true, Life::Closed, None),
            (Life::Closed, Heard::Drained, true, Life::Closed, None),
        ];
        for (life, heard, has_port, after, answer) in cases {
            assert_eq!(
                next_life(life, heard, has_port),
                (after, answer),
                "{life:?} {heard:?} {has_port}"
            );
        }
    }

    /// Expected: TIOCOUTQ answers the bytes in the output queue (tty_ioctl(4) on Linux,
    /// `sys/ttycom.h` and termios(4) on macOS). An error answers nothing about them.
    #[test]
    fn a_drain_is_empty_stuck_or_failed() {
        let t0 = Instant::now();
        let (half, over) = (t0 + STALLED_FOR / 2, t0 + STALLED_FOR);
        // (driver holds, now, the answer)
        let cases = [
            (Ok(0), t0, Some(Drain::Empty)),
            (Ok(0), over, Some(Drain::Empty)),
            (Ok(518), t0, None),
            (Ok(1), half, None),
            (Ok(1), over, Some(Drain::Stuck)),
            (Err(Errno::EIO), t0, Some(Drain::Failed(Errno::EIO))),
            (Err(Errno::ENXIO), half, Some(Drain::Failed(Errno::ENXIO))),
        ];
        for (held, now, answer) in cases {
            assert_eq!(drain_step(held, t0, now), answer, "{held:?}");
        }
    }

    #[test]
    fn take_fails_on_a_lost_port_and_drops_while_a_finish_is_open_or_the_queue_is_full() {
        // (lost, a finish open, queue full, take answers before it queues)
        let cases = [
            (false, false, false, None),
            (false, false, true, Some(Outcome::Dropped)),
            (false, true, false, Some(Outcome::Dropped)),
            (false, true, true, Some(Outcome::Dropped)),
            (true, false, false, Some(Outcome::Failed)),
            (true, false, true, Some(Outcome::Failed)),
            (true, true, false, Some(Outcome::Failed)),
            (true, true, true, Some(Outcome::Failed)),
        ];
        for (lost, finishing, full, answer) in cases {
            assert_eq!(
                gate(lost, finishing, full),
                answer,
                "{lost} {finishing} {full}"
            );
        }
    }

    /// The finishing table in `docs/design/outputs.md`: a drain that did not end empty
    /// loses the port.
    #[test]
    fn a_route_finish_loses_the_port_unless_the_drain_ended_empty() {
        // (drain answer, the port is lost with)
        let cases = [
            (Drain::Empty, None),
            (Drain::Stuck, Some(Errno::ETIMEDOUT)),
            (Drain::Failed(Errno::EIO), Some(Errno::EIO)),
            (Drain::Failed(Errno::ENXIO), Some(Errno::ENXIO)),
        ];
        for (drained, lost) in cases {
            assert_eq!(loses_after(drained), lost, "{drained:?}");
        }
    }

    #[test]
    fn a_turn_hears_stop_first_and_a_finish_only_while_running() {
        let finishing = Life::Finishing { asked: 1 };
        // (life, stop asked, finish asked, heard)
        let cases = [
            (Life::Running, false, 0, None),
            (Life::Running, false, 2, Some(Heard::Finish(2))),
            (Life::Running, true, 0, Some(Heard::Stop)),
            (Life::Running, true, 2, Some(Heard::Stop)),
            (finishing, false, 1, None),
            (finishing, false, 2, None), // a newer ask waits for the next finish
            (finishing, true, 1, Some(Heard::Stop)), // after the opener gave up
            (Life::Draining, true, 0, None),
            (Life::Draining, false, 1, None),
            (Life::Closed, true, 1, None),
        ];
        for (life, stop, finish, heard) in cases {
            assert_eq!(hears(life, stop, finish), heard, "{life:?} {stop} {finish}");
        }
        // (stop asked, finish asked, the wait to look again ends now)
        let looks = [
            (false, 0, false),
            (true, 0, true),
            (false, 3, true),
            (true, 3, true),
        ];
        for (stop, finish, ends) in looks {
            assert_eq!(cuts_look_short(stop, finish), ends, "{stop} {finish}");
        }
    }

    /// The bound's parts, as `docs/design/outputs.md` counts them: when the finish is
    /// asked a message may be in flight, one more queued (`take` and `finish_route` both
    /// hold the device, so no third), then the drain. Each can take a stall, one sleep
    /// past it, and a link time.
    #[test]
    fn finish_within_covers_a_write_a_queued_write_and_a_drain() {
        let worst = STALLED_FOR + QUEUE_FULL + LINK_TIME;
        // (part, its worst)
        let parts = [("in flight", worst), ("queued", worst), ("drain", worst)];
        let total: Duration = parts.iter().map(|(_, worst)| *worst).sum();
        assert!(FINISH_WITHIN >= total, "{FINISH_WITHIN:?} < {total:?}");
        assert!(
            FINISH_WITHIN < total + QUEUE_FULL,
            "{FINISH_WITHIN:?} pads past {total:?}"
        );
    }

    /// Every pair of the opener's wait in `docs/design/outputs.md`.
    #[test]
    fn the_opener_waits_for_its_own_answer_at_most_finish_within() {
        // (its finish still open, what woke it, the wait after)
        let cases = [
            (true, Wake::Answer, Wait::Waiting), // an answer to a finish it gave up on
            (true, Wake::Timeout, Wait::GaveUp),
            (true, Wake::Closed, Wait::Done),
            (false, Wake::Answer, Wait::Done),
            (false, Wake::Timeout, Wait::Done),
            (false, Wake::Closed, Wait::Done),
        ];
        for (open, heard, after) in cases {
            assert_eq!(waited(open, heard), after, "{open} {heard:?}");
        }
    }

    #[test]
    fn a_lost_adapter_is_found_again_by_itself_never_by_its_place() {
        let listed = |serial: &str, port: &str| Listed {
            serial: serial.into(),
            port: port.into(),
        };
        let (a, b) = (listed("A1", "port a"), listed("B2", "port b"));
        let a_again = listed("A1", "port a, plugged in again");
        // (listed when the output is set, its adapter, listed on a reconnect, port found)
        let cases = [
            (
                vec![a.clone(), b.clone()],
                1,
                vec![a.clone(), b.clone()],
                Some("port a"),
            ),
            (
                vec![a.clone(), b.clone()],
                2,
                vec![a.clone(), b.clone()],
                Some("port b"),
            ),
            (
                vec![a.clone(), b.clone()],
                2,
                vec![b.clone()],
                Some("port b"),
            ), // now first
            (
                vec![b.clone()],
                1,
                vec![a.clone(), b.clone()],
                Some("port b"),
            ), // now second
            (
                vec![a.clone()],
                1,
                vec![a_again],
                Some("port a, plugged in again"),
            ),
            (vec![a.clone(), b.clone()], 1, vec![], None), // gone
            (vec![a.clone(), b.clone()], 1, vec![b.clone()], None), // gone, B in its place
            (vec![a.clone()], 2, vec![a.clone(), b.clone()], None), // never there
        ];
        for (n, (at_set, adapter, now, port)) in cases.into_iter().enumerate() {
            let found = Adapter::try_from(adapter)
                .ok()
                .and_then(|adapter| pick(&at_set, adapter))
                .and_then(|wanted| port_of(&now, &wanted));
            assert_eq!(found, port, "case {n}");
        }
    }

    /// A port thread on the port side of a pseudo terminal, its shared state, and the
    /// widget's side, whose reads wait at most a second.
    fn on_a_pseudo_terminal() -> io::Result<(PortThread, Arc<Shared>, TTYPort)> {
        let (mut widget, port) = TTYPort::pair()?;
        never_wait(&port)?;
        widget.set_timeout(Duration::from_secs(1))?;
        let (_, messages) = rtrb::RingBuffer::new(1);
        let (answer, _) = mpsc::sync_channel(1);
        let shared = Arc::new(Shared::default());
        let port_thread = PortThread {
            find: Box::new(|| Err(io::Error::from(ErrorKind::NotFound))),
            port: Some(port),
            link: Link::Writing,
            messages,
            unsent: None,
            ask: Ask::Try(1),
            shared: Arc::clone(&shared),
            answer,
        };
        Ok((port_thread, shared, widget))
    }

    /// A try writes label 3 whole. A stop or a finish already asked ends its wait at once
    /// (the Ask table's "a stop or a finish" column); with neither, a silent widget is
    /// heard as nothing once [`ANSWER_WITHIN`] passed. Nothing is kept either way.
    #[test]
    fn a_stop_or_a_finish_cuts_a_try_and_a_silent_widget_is_nothing() -> io::Result<()> {
        // (case, stop, finish asked, heard)
        let cases = [
            ("a stop", true, 0, Tried::Cut),
            ("a finish", false, 1, Tried::Cut),
            ("neither: silent", false, 0, Tried::Nothing),
        ];
        for (case, stop, finish, heard) in cases {
            let (mut port_thread, shared, mut widget) = on_a_pseudo_terminal()?;
            shared.stop.store(stop, Ordering::Relaxed);
            shared.finish.store(finish, Ordering::Release);
            assert_eq!(port_thread.try_ask(), heard, "{case}");
            let mut asked = [0; ASK.len()];
            widget.read_exact(&mut asked)?;
            assert_eq!(
                asked,
                [0x7E, 3, 2, 0, 0, 0, 0xE7],
                "{case}: label 3 (API p.3)"
            );
            assert_eq!(shared.settings(), None, "{case}");
        }
        Ok(())
    }

    /// An answer that comes in two parts is read whole: the second part, written once the
    /// port has read the first, joins it.
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "the widget's side polls between the parts"
    )]
    fn an_answer_in_two_parts_is_read_whole() -> io::Result<()> {
        let (mut port_thread, shared, mut widget) = on_a_pseudo_terminal()?;
        // The port's own input queue, on a second handle: empty once the port read it.
        let input = port_thread
            .port
            .as_ref()
            .ok_or(ErrorKind::NotFound)?
            .try_clone_native()?;
        let widget = thread::spawn(move || -> io::Result<TTYPort> {
            let mut asked = [0; ASK.len()];
            widget.read_exact(&mut asked)?;
            let answer = [0x7E, 3, 5, 0, 44, 1, 20, 4, 30, 0xE7];
            let (first, rest) = answer.split_at(4);
            widget.write_all(first)?;
            let started = Instant::now();
            while input.bytes_to_read()? > 0 && started.elapsed() < Duration::from_secs(1) {
                thread::sleep(Duration::from_millis(1)); // poll
            }
            widget.write_all(rest)?;
            Ok(widget)
        });
        assert_eq!(port_thread.try_ask(), Tried::Answered);
        assert_eq!(shared.settings(), Settings::new(20, 4, 30));
        drop(widget.join());
        Ok(())
    }

    /// API p.2 (a message) and p.3 (label 3's answer and its ranges).
    #[test]
    fn an_answer_holds_settings_only_whole_with_label_3_and_in_range() {
        let answer = |break_steps, mark_steps, rate| {
            vec![0x7E, 3, 5, 0, 44, 1, break_steps, mark_steps, rate, 0xE7]
        };
        let with_memory = vec![0x7E, 3, 7, 0, 44, 1, 20, 4, 30, 0xAA, 0xBB, 0xE7];
        let cases = [
            ("in range", answer(20, 4, 30), Settings::new(20, 4, 30)),
            ("the lowest", answer(9, 1, 0), Settings::new(9, 1, 0)),
            (
                "the highest",
                answer(127, 127, 40),
                Settings::new(127, 127, 40),
            ),
            ("break 8", answer(8, 4, 30), None),
            ("break 128", answer(128, 4, 30), None),
            ("mark 0", answer(20, 0, 30), None),
            ("mark 128", answer(20, 128, 30), None),
            ("rate 41", answer(20, 4, 41), None),
            ("label 6", vec![0x7E, 6, 5, 0, 44, 1, 20, 4, 30, 0xE7], None),
            ("not whole yet", vec![0x7E, 3, 5, 0, 44, 1, 20, 4, 30], None),
            (
                "no end byte",
                vec![0x7E, 3, 5, 0, 44, 1, 20, 4, 30, 0xE6],
                None,
            ),
            (
                "data under 5 bytes",
                vec![0x7E, 3, 4, 0, 44, 1, 20, 4, 0xE7],
                None,
            ),
            (
                "user memory after the rate",
                with_memory,
                Settings::new(20, 4, 30),
            ),
            (
                "bytes before the start",
                [vec![0, 0x7E, 0xFF], answer(20, 4, 30)].concat(),
                Settings::new(20, 4, 30),
            ),
            ("nothing", vec![], None),
        ];
        for (case, heard, expected) in cases {
            assert_eq!(settings_of(&heard), expected, "{case}");
            if let Some(settings) = expected {
                assert_eq!(
                    Settings::unpacked(settings.packed()),
                    expected,
                    "{case}, shared"
                );
            }
        }
        assert_eq!(Settings::unpacked(0), None, "nothing read");
    }

    /// Every cell of the Ask table (`docs/design/outputs.md`). A lost port is asked again
    /// from try 1 once it is found: `step` sets it.
    #[test]
    fn every_try_and_what_it_heard() {
        // (try, heard, next)
        let cases = [
            (1, Tried::Answered, Ask::Read),
            (1, Tried::Nothing, Ask::Try(2)),
            (1, Tried::Cut, Ask::Try(1)),
            (1, Tried::Lost, Ask::Try(1)),
            (2, Tried::Answered, Ask::Read),
            (2, Tried::Nothing, Ask::Done),
            (2, Tried::Cut, Ask::Try(2)),
            (2, Tried::Lost, Ask::Try(2)),
        ];
        for (n, tried, next) in cases {
            assert_eq!(after_try(n, tried), next, "try {n}, {tried:?}");
        }
    }

    #[test]
    fn a_frame_becomes_one_send_dmx_message() {
        let counting = (0..ADDRESSES).map(|n| (n % 251) as u8).collect();
        // (frame, channel 1, channel 512)
        let cases = [
            (vec![], 0, 0),
            (counting, 0, (511 % 251) as u8),
            ([[9; ADDRESSES], [3; ADDRESSES]].concat(), 9, 9),
        ];
        for (frame, first, last) in cases {
            let mut out = [0xAA; MESSAGE];
            message(&frame, &mut out);
            assert_eq!(out[..5], [0x7E, 6, 0x01, 0x02, 0], "{} bytes", frame.len());
            assert_eq!((out[5], out[516]), (first, last));
            assert_eq!(out[517], 0xE7);
        }
    }
}
