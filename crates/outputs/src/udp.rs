//! One lane for every protocol over UDP (DDP, Art-Net, sACN, IDN). Each is a [`Protocol`]:
//! a calculation that cuts a frame into packets and writes each into its own buffer. The
//! lane is the action: it sends each packet on a connected, non-blocking socket, ends the
//! frame at the first failed send, keeps the health machine and the socket thread that
//! opens and drops sockets off the output thread. Both machines: `docs/design/outputs.md`.

use crate::{Device, Limits, Outcome};
use std::io::{self, ErrorKind};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// A protocol's packets: everything it knows, nothing it sends.
pub(crate) trait Protocol: Send {
    /// What an output of this protocol is set with.
    type Settings;
    /// Where one packet of a frame lies in the stream.
    type Cut: Copy;

    fn new(settings: Self::Settings) -> Self;

    fn limits(&self) -> Limits;

    /// Starts the next frame of `channels` channels: its packets, in the order they go out.
    fn frame(&mut self, channels: usize) -> impl Iterator<Item = Self::Cut> + use<Self>;

    /// Writes the packet at `cut` into the protocol's own buffer. `None` cannot happen: a cut
    /// of the frame fits the buffer and the stream.
    fn packet(&mut self, cut: Self::Cut, stream: &[u8]) -> Option<&[u8]>;

    /// Ends the stream for good: how many closing frames `frame` and `packet` make now,
    /// each sent as a frame of no channels. Most protocols just go quiet.
    fn stop(&mut self) -> u8 {
        0
    }

    /// A send found nobody there or a broken socket. Most protocols carry on.
    fn lost(&mut self) {}
}

/// Frames that must go out without a refusal before a device that refused counts as there
/// again. The host reports a refusal once per answer and rate-limits them, so one clean
/// frame proves nothing.
pub(crate) const CLEAN: u32 = 30;
/// How long the socket thread waits before it tries to open a socket again.
const REOPEN_EVERY: Duration = Duration::from_millis(500);

/// What a failed send means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    /// The link is busy: the frame is dropped, nothing is wrong.
    Busy,
    /// The host answers that nothing listens or the device cannot be reached.
    DeviceNotThere,
    /// The socket is no good any more (a new address, a network that went): open another.
    SocketBroken,
}

#[cfg(target_os = "linux")]
const NO_BUFFERS: i32 = 105;
#[cfg(target_os = "macos")]
const NO_BUFFERS: i32 = 55;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const NO_BUFFERS: i32 = -1;

/// The host found no answer on the link (no ARP reply): the device is off or gone.
#[cfg(target_os = "linux")]
const HOST_DOWN: i32 = 112;
#[cfg(target_os = "macos")]
const HOST_DOWN: i32 = 64;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const HOST_DOWN: i32 = -1;

/// The table of send errors. A calculation.
fn fault(error: &io::Error) -> Fault {
    match error.raw_os_error() {
        Some(NO_BUFFERS) => return Fault::Busy,
        Some(HOST_DOWN) => return Fault::DeviceNotThere,
        _ => {}
    }
    let kind = error.kind();
    if kind == ErrorKind::WouldBlock {
        Fault::Busy
    } else if matches!(
        kind,
        ErrorKind::ConnectionRefused | ErrorKind::HostUnreachable
    ) {
        Fault::DeviceNotThere
    } else {
        Fault::SocketBroken
    }
}

/// How a UDP output is doing. The engine sees it only as each frame's [`Outcome`].
///
/// ```text
/// [*] --> Running --not there--> Proving --CLEAN frames sent--> Running
///            |                      |  ^------- not there ------+
///            +---broken--> Reopening <--broken--+
///                             |
///                             +--new socket--> Proving
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Health {
    /// Frames go out. The device does not answer: running means sent.
    Running,
    /// The device refused; `clean` frames went out since without a refusal.
    Proving { clean: u32 },
    /// The socket broke; a new one was asked for and has not come yet.
    Reopening,
}

/// What happened in one `take`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    /// Every packet of the frame went out whole.
    Sent,
    /// A send failed.
    Failed(Fault),
    /// The socket thread handed over a new socket.
    NewSocket,
}

/// What a transition asks the caller to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    Nothing,
    /// Wake the socket thread to open a new socket.
    NewSocket,
}

/// The one transition function of a UDP output: a calculation.
fn next(health: Health, event: Event) -> (Health, Ask) {
    use Fault::{Busy, DeviceNotThere, SocketBroken};
    match (health, event) {
        (Health::Reopening, Event::NewSocket) => (Health::Proving { clean: 0 }, Ask::Nothing),
        (Health::Reopening, _) => (Health::Reopening, Ask::Nothing),
        (_, Event::Failed(SocketBroken)) => (Health::Reopening, Ask::NewSocket),
        (_, Event::Failed(DeviceNotThere)) => (Health::Proving { clean: 0 }, Ask::Nothing),
        (Health::Proving { clean }, Event::Sent) if clean + 1 >= CLEAN => {
            (Health::Running, Ask::Nothing)
        }
        (Health::Proving { clean }, Event::Sent) => {
            (Health::Proving { clean: clean + 1 }, Ask::Nothing)
        }
        (same, Event::Failed(Busy) | Event::NewSocket) | (same @ Health::Running, Event::Sent) => {
            (same, Ask::Nothing)
        }
    }
}

/// Whether `event` lost the device: nobody there or a broken socket. A busy link only
/// drops the frame.
const fn lost(event: Event) -> bool {
    match event {
        Event::Failed(Fault::DeviceNotThere | Fault::SocketBroken) => true,
        Event::Sent | Event::Failed(Fault::Busy) | Event::NewSocket => false,
    }
}

/// What the engine is told after `event` left the output `health`.
const fn outcome(health: Health, event: Event) -> Outcome {
    match (health, event) {
        (_, Event::Failed(Fault::Busy)) => Outcome::Dropped,
        (Health::Running, _) => Outcome::Accepted,
        (Health::Proving { .. } | Health::Reopening, _) => Outcome::Failed,
    }
}

/// What a UDP output sends through: a connected UDP socket, or a test's own.
pub(crate) trait Link: Send {
    fn send(&mut self, packet: &[u8]) -> io::Result<usize>;
}

impl Link for UdpSocket {
    fn send(&mut self, packet: &[u8]) -> io::Result<usize> {
        UdpSocket::send(self, packet)
    }
}

/// Opens a new link when the socket broke. Runs on the socket thread.
pub(crate) type Reopen = Box<dyn FnMut() -> io::Result<Box<dyn Link>> + Send>;

fn socket(address: SocketAddrV4) -> io::Result<Box<dyn Link>> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    socket.connect(address)?;
    socket.set_nonblocking(true)?;
    Ok(Box::new(socket))
}

/// What the output thread or `drop` asks of the socket thread. One value: a stop is
/// never overwritten by an ask for a socket.
const ASK_NOTHING: u8 = 0;
const ASK_SOCKET: u8 = 1;
const ASK_STOP: u8 = 2;

struct Shared {
    ask: AtomicU8,
    /// The socket thread is in its loop: whatever its start-up allocated is done.
    ready: AtomicBool,
}

/// What the socket thread is doing. Every pair: `docs/design/outputs.md`.
///
/// ```text
/// [*] --> Idle --asked--> Opening --opened--> Idle
/// Opening --open failed--> Retrying --wait over--> Opening
/// any --stop--> Stopping
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Waits for an ask.
    Idle,
    /// Opens a new socket now.
    Opening,
    /// The last open failed `with`; waits [`REOPEN_EVERY`], then opens again. The cause
    /// stays here: no reader outside this crate yet (#265 carries causes to the API).
    Retrying { with: ErrorKind },
    /// Ends: frees the spent sockets and returns.
    Stopping,
}

/// What the socket thread heard or did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    /// The output thread asked for a new socket.
    Asked,
    /// A new socket opened and was handed over.
    Opened,
    /// Opening failed.
    OpenFailed(ErrorKind),
    /// [`REOPEN_EVERY`] passed, or a wake cut it short.
    WaitOver,
    /// `drop` asked it to stop.
    Stop,
}

/// The one transition function of the socket thread: a calculation.
const fn next_phase(phase: Phase, turn: Turn) -> Phase {
    match (phase, turn) {
        (_, Turn::Stop) | (Phase::Stopping, _) => Phase::Stopping,
        (Phase::Idle, Turn::Asked) | (Phase::Retrying { .. }, Turn::WaitOver) => Phase::Opening,
        (Phase::Opening, Turn::Opened) => Phase::Idle,
        (Phase::Opening, Turn::OpenFailed(with)) => Phase::Retrying { with },
        // Asked once per broken socket; an ask while opening is that same ask.
        (same @ (Phase::Idle | Phase::Opening | Phase::Retrying { .. }), _) => same,
    }
}

/// The link of an output whose socket did not open yet: every send says so, without
/// allocating, and the output stays reopening until the socket thread hands one over.
struct NoSocket;

impl Link for NoSocket {
    fn send(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(ErrorKind::NotConnected.into())
    }
}

/// A UDP output: a connected, non-blocking UDP socket and its [`Protocol`]. A
/// broken socket is replaced by one the socket thread opens, and dropped there, off the
/// output thread. The socket thread is the one owner of every reopen, also of a socket
/// that did not open at the start.
pub(crate) struct Udp<P: Protocol> {
    pub(crate) link: Box<dyn Link>,
    protocol: P,
    health: Health,
    renewed: rtrb::Consumer<Box<dyn Link>>,
    /// The socket a new one replaced, back to the socket thread to be closed and freed.
    spent: rtrb::Producer<Box<dyn Link>>,
    shared: Arc<Shared>,
    thread: JoinHandle<()>,
}

impl<P: Protocol> Udp<P> {
    /// Opens a socket to `address`. A socket the system refuses now is opened by the
    /// socket thread later: frames fail until then. Fails on port 0 and when the system
    /// refuses a thread. Off the output thread.
    pub(crate) fn open(address: SocketAddrV4, settings: P::Settings) -> io::Result<Self> {
        if address.port() == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "no port"));
        }
        Self::with(
            socket(address).ok(),
            Box::new(move || socket(address)),
            settings,
        )
    }

    pub(crate) fn with(
        link: Option<Box<dyn Link>>,
        mut reopen: Reopen,
        settings: P::Settings,
    ) -> io::Result<Self> {
        let (link, health, first) = match link {
            Some(link) => (link, Health::Running, Phase::Idle),
            None => (
                Box::new(NoSocket) as Box<dyn Link>,
                Health::Reopening,
                Phase::Opening,
            ),
        };
        let shared = Arc::new(Shared {
            ask: AtomicU8::new(ASK_NOTHING),
            ready: AtomicBool::new(false),
        });
        let (mut give, renewed) = rtrb::RingBuffer::new(1);
        let (spent, mut close) = rtrb::RingBuffer::<Box<dyn Link>>::new(1);
        let thread = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name(SOCKET_THREAD.into())
                .spawn(move || {
                    thread::park_timeout(Duration::ZERO); // sets up the park state
                    shared.ready.store(true, Ordering::Release);
                    let heard = || match shared.ask.load(Ordering::Acquire) {
                        ASK_STOP => Some(Turn::Stop),
                        ASK_SOCKET => Some(Turn::Asked),
                        _ => None,
                    };
                    let mut phase = first;
                    loop {
                        let turn = match (phase, heard()) {
                            (Phase::Stopping, _) => break,
                            (_, Some(Turn::Stop)) => Turn::Stop,
                            (Phase::Idle, Some(asked)) => {
                                let _ = shared.ask.compare_exchange(
                                    ASK_SOCKET,
                                    ASK_NOTHING,
                                    Ordering::AcqRel,
                                    Ordering::Acquire,
                                );
                                asked
                            }
                            (Phase::Idle, None) => {
                                thread::park();
                                continue;
                            }
                            (Phase::Opening, _) => {
                                // The socket the last new one replaced: `take` handed it
                                // back before it asked again.
                                while close.pop().is_ok() {}
                                match reopen() {
                                    Ok(link) => {
                                        let _ = give.push(link);
                                        Turn::Opened
                                    }
                                    Err(error) => Turn::OpenFailed(error.kind()),
                                }
                            }
                            (Phase::Retrying { .. }, _) => {
                                thread::park_timeout(REOPEN_EVERY);
                                Turn::WaitOver
                            }
                        };
                        phase = next_phase(phase, turn);
                    }
                    while close.pop().is_ok() {}
                })?
        };
        while !shared.ready.load(Ordering::Acquire) {
            thread::yield_now();
        }
        Ok(Self {
            link,
            protocol: P::new(settings),
            health,
            renewed,
            spent,
            shared,
            thread,
        })
    }

    /// Takes the new socket, if one came, and hands the old one back to be dropped.
    fn renew(&mut self) {
        if let Ok(link) = self.renewed.pop() {
            let old = std::mem::replace(&mut self.link, link);
            // Never full: one socket comes per ask, and the socket thread empties `spent`
            // before it opens the next.
            let _ = self.spent.push(old);
            self.health = next(self.health, Event::NewSocket).0;
        }
    }

    /// Sends the frame as packets; stops at the first that fails.
    fn send(&mut self, stream: &[u8]) -> Event {
        for cut in self.protocol.frame(stream.len()) {
            let Some(packet) = self.protocol.packet(cut, stream) else {
                return Event::Failed(Fault::SocketBroken);
            };
            match self.link.send(packet) {
                Ok(sent) if sent == packet.len() => {}
                Ok(_) => return Event::Failed(Fault::DeviceNotThere),
                Err(error) => return Event::Failed(fault(&error)),
            }
        }
        Event::Sent
    }
}

/// The name of the thread that opens and drops sockets.
pub(crate) const SOCKET_THREAD: &str = "udp-socket";

impl<P: Protocol> Drop for Udp<P> {
    fn drop(&mut self) {
        self.shared.ask.store(ASK_STOP, Ordering::Release);
        self.thread.thread().unpark();
    }
}

impl<P: Protocol> Device for Udp<P> {
    fn reconnects_itself(&self) -> bool {
        true
    }

    fn limits(&self) -> Limits {
        self.protocol.limits()
    }

    /// Sends the protocol's closing frames back to back. Best effort: a busy link drops
    /// the rest of a closing frame as it would any frame's.
    fn terminate(&mut self) {
        for _ in 0..self.protocol.stop() {
            let _ = self.take(&[]);
        }
    }

    /// Sends the frame as packets. A busy link drops the rest of the frame. A device that
    /// refused stays failed until `CLEAN` frames went out without a refusal. Running means
    /// frames were sent: the device does not answer.
    fn take(&mut self, stream: &[u8]) -> Outcome {
        self.renew();
        let event = self.send(stream);
        if lost(event) {
            self.protocol.lost();
        }
        let (health, ask) = next(self.health, event);
        self.health = health;
        if ask == Ask::NewSocket {
            let _ = self.shared.ask.compare_exchange(
                ASK_NOTHING,
                ASK_SOCKET,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            self.thread.thread().unpark();
        }
        outcome(health, event)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
mod tests {
    use super::*;

    // (the error of a send, what it means)
    #[test]
    fn a_failed_send_means_busy_not_there_or_broken() {
        let cases = [
            (io::Error::from(ErrorKind::WouldBlock), Fault::Busy),
            (io::Error::from_raw_os_error(NO_BUFFERS), Fault::Busy),
            (
                io::Error::from(ErrorKind::ConnectionRefused),
                Fault::DeviceNotThere,
            ),
            (
                io::Error::from(ErrorKind::HostUnreachable),
                Fault::DeviceNotThere,
            ),
            (
                io::Error::from_raw_os_error(HOST_DOWN),
                Fault::DeviceNotThere,
            ),
            (
                io::Error::from(ErrorKind::AddrNotAvailable),
                Fault::SocketBroken,
            ),
            (
                io::Error::from(ErrorKind::NetworkUnreachable),
                Fault::SocketBroken,
            ),
            (io::Error::from(ErrorKind::BrokenPipe), Fault::SocketBroken),
            (io::Error::other("anything else"), Fault::SocketBroken),
        ];
        for (error, expected) in cases {
            assert_eq!(fault(&error), expected, "{error:?}");
        }
    }

    #[test]
    fn only_nobody_there_or_a_broken_socket_loses_the_device() {
        // (event, lost)
        let cases = [
            (Event::Sent, false),
            (Event::Failed(Fault::Busy), false),
            (Event::Failed(Fault::DeviceNotThere), true),
            (Event::Failed(Fault::SocketBroken), true),
            (Event::NewSocket, false),
        ];
        for (event, expected) in cases {
            assert_eq!(lost(event), expected, "{event:?}");
        }
    }

    #[test]
    fn every_state_and_event_of_a_ddp_output() {
        use Event::{Failed, NewSocket, Sent};
        use Fault::{Busy, DeviceNotThere, SocketBroken};
        use Health::{Proving, Reopening, Running};
        let last = CLEAN - 1;
        // (health, event, new health, ask, what the engine is told)
        let cases = [
            (Running, Sent, Running, Ask::Nothing, Outcome::Accepted),
            (
                Running,
                Failed(Busy),
                Running,
                Ask::Nothing,
                Outcome::Dropped,
            ),
            (
                Running,
                Failed(DeviceNotThere),
                Proving { clean: 0 },
                Ask::Nothing,
                Outcome::Failed,
            ),
            (
                Running,
                Failed(SocketBroken),
                Reopening,
                Ask::NewSocket,
                Outcome::Failed,
            ),
            (Running, NewSocket, Running, Ask::Nothing, Outcome::Accepted), // cannot happen
            (
                Proving { clean: 0 },
                Sent,
                Proving { clean: 1 },
                Ask::Nothing,
                Outcome::Failed,
            ),
            (
                Proving { clean: last },
                Sent,
                Running,
                Ask::Nothing,
                Outcome::Accepted,
            ),
            (
                Proving { clean: 5 },
                Failed(Busy),
                Proving { clean: 5 },
                Ask::Nothing,
                Outcome::Dropped,
            ),
            (
                Proving { clean: last },
                Failed(DeviceNotThere),
                Proving { clean: 0 },
                Ask::Nothing,
                Outcome::Failed,
            ),
            (
                Proving { clean: 5 },
                Failed(SocketBroken),
                Reopening,
                Ask::NewSocket,
                Outcome::Failed,
            ),
            (
                Proving { clean: 5 },
                NewSocket,
                Proving { clean: 5 },
                Ask::Nothing,
                Outcome::Failed,
            ), // cannot happen
            (Reopening, Sent, Reopening, Ask::Nothing, Outcome::Failed),
            (
                Reopening,
                Failed(Busy),
                Reopening,
                Ask::Nothing,
                Outcome::Dropped,
            ),
            (
                Reopening,
                Failed(DeviceNotThere),
                Reopening,
                Ask::Nothing,
                Outcome::Failed,
            ),
            (
                Reopening,
                Failed(SocketBroken),
                Reopening,
                Ask::Nothing, // asked once
                Outcome::Failed,
            ),
            (
                Reopening,
                NewSocket,
                Proving { clean: 0 },
                Ask::Nothing,
                Outcome::Failed,
            ),
        ];
        for (health, event, new, ask, told) in cases {
            assert_eq!(next(health, event), (new, ask), "{health:?} {event:?}");
            assert_eq!(outcome(new, event), told, "{health:?} {event:?}");
        }
    }

    /// Every pair of the socket thread table in `docs/design/outputs.md`.
    #[test]
    fn every_phase_and_turn_of_the_socket_thread() {
        use Phase::{Idle, Opening, Retrying, Stopping};
        let retrying = Retrying {
            with: ErrorKind::AddrNotAvailable,
        };
        let failed = Turn::OpenFailed(ErrorKind::NetworkUnreachable);
        let unreachable = Retrying {
            with: ErrorKind::NetworkUnreachable,
        };
        // (phase, turn, phase after)
        let cases = [
            (Idle, Turn::Asked, Opening),
            (Idle, Turn::Opened, Idle),   // n/a
            (Idle, failed, Idle),         // n/a
            (Idle, Turn::WaitOver, Idle), // n/a
            (Idle, Turn::Stop, Stopping),
            (Opening, Turn::Asked, Opening), // the same ask
            (Opening, Turn::Opened, Idle),
            (Opening, failed, unreachable),
            (Opening, Turn::WaitOver, Opening), // n/a
            (Opening, Turn::Stop, Stopping),
            (retrying, Turn::Asked, retrying),  // the same ask
            (retrying, Turn::Opened, retrying), // n/a
            (retrying, failed, retrying),       // n/a
            (retrying, Turn::WaitOver, Opening),
            (retrying, Turn::Stop, Stopping),
            (Stopping, Turn::Asked, Stopping),
            (Stopping, Turn::Opened, Stopping),
            (Stopping, failed, Stopping),
            (Stopping, Turn::WaitOver, Stopping),
            (Stopping, Turn::Stop, Stopping),
        ];
        for (phase, turn, after) in cases {
            assert_eq!(next_phase(phase, turn), after, "{phase:?} {turn:?}");
        }
    }

    /// A protocol of `n` packets a frame: packet `k` is the one byte `k`.
    struct Numbered(usize, [u8; 1]);
    impl Protocol for Numbered {
        type Settings = usize;
        type Cut = usize;
        fn new(packets: usize) -> Self {
            Self(packets, [0])
        }
        fn limits(&self) -> Limits {
            Limits::RAW
        }
        fn frame(&mut self, _: usize) -> impl Iterator<Item = usize> + use<> {
            0..self.0
        }
        fn packet(&mut self, cut: usize, _: &[u8]) -> Option<&[u8]> {
            self.1 = [u8::try_from(cut).ok()?];
            Some(&self.1)
        }
    }

    /// A link whose send number `at` (from 1) fails `with`; `None` sends one byte short.
    struct FailsAt {
        at: usize,
        with: Option<ErrorKind>,
        tried: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl Link for FailsAt {
        fn send(&mut self, packet: &[u8]) -> io::Result<usize> {
            if self.tried.fetch_add(1, Ordering::SeqCst) + 1 != self.at {
                return Ok(packet.len());
            }
            match self.with {
                Some(kind) => Err(kind.into()),
                None => Ok(packet.len() - 1),
            }
        }
    }

    #[test]
    fn the_first_failed_send_ends_the_frame() {
        use ErrorKind::{AddrNotAvailable, ConnectionRefused, WouldBlock};
        // (send that fails, how, sends tried, what the engine is told, health after)
        let cases = [
            (9, Some(WouldBlock), 4, Outcome::Accepted, Health::Running),
            (2, Some(WouldBlock), 2, Outcome::Dropped, Health::Running),
            (
                3,
                Some(ConnectionRefused),
                3,
                Outcome::Failed,
                Health::Proving { clean: 0 },
            ),
            (1, None, 1, Outcome::Failed, Health::Proving { clean: 0 }),
            (
                4,
                Some(AddrNotAvailable),
                4,
                Outcome::Failed,
                Health::Reopening,
            ),
        ];
        for (at, with, tried, told, health) in cases {
            let sends = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let link = FailsAt {
                at,
                with,
                tried: Arc::clone(&sends),
            };
            let reopen: Reopen = Box::new(|| Err(io::Error::other("not needed")));
            let mut udp = Udp::<Numbered>::with(Some(Box::new(link)), reopen, 4).unwrap();
            assert_eq!(udp.take(&[]), told, "{at} {with:?}");
            assert_eq!(sends.load(Ordering::SeqCst), tried, "{at} {with:?}");
            assert_eq!(udp.health, health, "{at} {with:?}");
        }
    }

    #[test]
    fn terminate_sends_nothing_for_a_protocol_without_a_stop() {
        let sends = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let link = FailsAt {
            at: usize::MAX,
            with: None,
            tried: Arc::clone(&sends),
        };
        let reopen: Reopen = Box::new(|| Err(io::Error::other("not needed")));
        let mut udp = Udp::<Numbered>::with(Some(Box::new(link)), reopen, 4).unwrap();
        udp.terminate();
        assert_eq!(sends.load(Ordering::SeqCst), 0);
    }

    /// A link that is always broken and counts where it was dropped: on the socket thread
    /// or elsewhere.
    struct Broken(Arc<[std::sync::atomic::AtomicUsize; 2]>);
    impl Link for Broken {
        fn send(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(ErrorKind::AddrNotAvailable.into())
        }
    }
    impl Drop for Broken {
        fn drop(&mut self) {
            let on_socket_thread = thread::current().name() == Some(SOCKET_THREAD);
            if let Some(count) = self.0.get(usize::from(!on_socket_thread)) {
                count.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    #[test]
    fn every_spent_socket_is_freed_on_the_socket_thread() {
        let dropped: Arc<[std::sync::atomic::AtomicUsize; 2]> = Arc::default();
        let opened = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let reopen: Reopen = {
            let (dropped, opened) = (Arc::clone(&dropped), Arc::clone(&opened));
            Box::new(move || {
                opened.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(Broken(Arc::clone(&dropped))) as Box<dyn Link>)
            })
        };
        let first = Box::new(Broken(Arc::clone(&dropped)));
        let mut udp = Udp::<Numbered>::with(Some(first), reopen, 1).unwrap();
        let started = std::time::Instant::now();
        while opened.load(Ordering::SeqCst) < 3 {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "never reopened"
            ); // guard
            udp.take(&[]);
            thread::yield_now();
        }
        let [on_socket_thread, elsewhere] = &*dropped;
        assert_eq!(
            elsewhere.load(Ordering::SeqCst),
            0,
            "a socket freed on the output thread"
        );
        assert!(
            on_socket_thread.load(Ordering::SeqCst) >= 1,
            "spent sockets freed"
        );
    }
}
