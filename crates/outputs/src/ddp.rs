//! DDP over UDP, as WLED reads it. A frame is one stream of channel bytes;
//! it goes out in packets of at most [`MAX_DATA`] bytes, the push flag on the last one.
//!
//! | Byte | Header field |
//! |---|---|
//! | 0 | flags: version 1 (`0x40`), push (`0x01`) on the last packet of a frame |
//! | 1 | sequence, 1 to 15, the same for every packet of a frame |
//! | 2 | data type: RGB 8 bit `0x0B`, RGBW 8 bit `0x1B` |
//! | 3 | destination: 1, the default output |
//! | 4 to 7 | offset of this packet's data in the frame, bytes, big endian |
//! | 8 to 9 | length of this packet's data, bytes, big endian |

use crate::{Color, Device, Outcome};
use std::io::{self, ErrorKind};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::num::NonZeroU16;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const HEADER: usize = 10;
/// Data bytes per packet: 480 RGB or 360 RGBW pixels, as WLED expects.
const MAX_DATA: usize = 1440;
const VERSION: u8 = 0x40;
const PUSH: u8 = 0x01;
const DESTINATION: u8 = 1;

/// Frames that must go out without a refusal before a device that refused counts as there
/// again. The host reports a refusal once per answer and rate-limits them, so one clean
/// frame proves nothing.
const CLEAN: u32 = 30;
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

/// How a DDP output is doing. The engine sees it only as each frame's [`Outcome`].
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
    /// Frames go out. DDP has no answer from the device: running means sent.
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

/// The one transition function of a DDP output: a calculation.
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

/// What the engine is told after `event` left the output `health`.
const fn outcome(health: Health, event: Event) -> Outcome {
    match (health, event) {
        (_, Event::Failed(Fault::Busy)) => Outcome::Dropped,
        (Health::Running, _) => Outcome::Accepted,
        (Health::Proving { .. } | Health::Reopening, _) => Outcome::Failed,
    }
}

/// What a DDP output sends through: a connected UDP socket, or a test's own.
trait Link: Send {
    fn send(&mut self, packet: &[u8]) -> io::Result<usize>;
}

impl Link for UdpSocket {
    fn send(&mut self, packet: &[u8]) -> io::Result<usize> {
        UdpSocket::send(self, packet)
    }
}

/// Opens a new link when the socket broke. Runs on the socket thread.
type Reopen = Box<dyn FnMut() -> io::Result<Box<dyn Link>> + Send>;

fn socket(address: SocketAddrV4) -> io::Result<Box<dyn Link>> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    socket.connect(address)?;
    socket.set_nonblocking(true)?;
    Ok(Box::new(socket))
}

struct Shared {
    want: AtomicBool,
    stop: AtomicBool,
    /// The socket thread is in its loop: whatever its start-up allocated is done.
    ready: AtomicBool,
}

/// A DDP output: a connected, non-blocking UDP socket and one packet's buffer. A broken
/// socket is replaced by one the socket thread opens, and dropped there, off the output
/// thread.
pub(crate) struct Ddp {
    link: Box<dyn Link>,
    color: Color,
    sequence: u8,
    packet: [u8; HEADER + MAX_DATA],
    health: Health,
    renewed: rtrb::Consumer<Box<dyn Link>>,
    /// The socket a new one replaced, back to the socket thread to be closed and freed.
    spent: rtrb::Producer<Box<dyn Link>>,
    shared: Arc<Shared>,
    thread: JoinHandle<()>,
}

impl Ddp {
    /// Opens a socket to `address`. Off the output thread.
    pub(crate) fn open(address: SocketAddrV4, color: Color) -> io::Result<Self> {
        if address.port() == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "no port"));
        }
        Self::with(socket(address)?, Box::new(move || socket(address)), color)
    }

    fn with(link: Box<dyn Link>, mut reopen: Reopen, color: Color) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            want: AtomicBool::new(false),
            stop: AtomicBool::new(false),
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
                    while !shared.stop.load(Ordering::Acquire) {
                        if !shared.want.swap(false, Ordering::AcqRel) {
                            thread::park();
                            continue;
                        }
                        // The socket the last new one replaced: `take` handed it back
                        // before it asked again.
                        while close.pop().is_ok() {}
                        match reopen() {
                            Ok(link) => {
                                let _ = give.push(link);
                            }
                            Err(_) => {
                                shared.want.store(true, Ordering::Release);
                                thread::park_timeout(REOPEN_EVERY);
                            }
                        }
                    }
                    while close.pop().is_ok() {}
                })?
        };
        while !shared.ready.load(Ordering::Acquire) {
            thread::yield_now();
        }
        Ok(Self {
            link,
            color,
            sequence: 0,
            packet: [0; HEADER + MAX_DATA],
            health: Health::Running,
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
        self.sequence = next_sequence(self.sequence);
        let total = data_length(stream.len(), self.color);
        for (offset, length, last) in packets(total) {
            let (header, data) = self.packet.split_at_mut(HEADER);
            header.copy_from_slice(&self::header(
                self.sequence,
                self.color,
                offset,
                length,
                last,
            ));
            let (Some(data), Some(bytes)) =
                (data.get_mut(..length), stream.get(offset..offset + length))
            else {
                // Each packet fits the buffer and is within the whole-pixel stream.
                return Event::Failed(Fault::SocketBroken);
            };
            data.copy_from_slice(bytes);
            let Some(packet) = self.packet.get(..HEADER + length) else {
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
const SOCKET_THREAD: &str = "ddp-socket";

impl Drop for Ddp {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.thread.thread().unpark();
    }
}

impl Device for Ddp {
    /// Sends the frame as packets. A busy link drops the rest of the frame. A device that
    /// refused stays failed until `CLEAN` frames went out without a refusal. Running means
    /// frames were sent: DDP has no answer from the device.
    fn take(&mut self, stream: &[u8]) -> Outcome {
        self.renew();
        let event = self.send(stream);
        let (health, ask) = next(self.health, event);
        self.health = health;
        if ask == Ask::NewSocket {
            self.shared.want.store(true, Ordering::Release);
            self.thread.thread().unpark();
        }
        outcome(health, event)
    }
}

/// The next sequence number: 1 to 15, then 1 again. 0 means "no sequence" to WLED.
fn next_sequence(sequence: u8) -> u8 {
    sequence % 15 + 1
}

/// The channels of one pixel.
pub(crate) const fn pixel(color: Color) -> NonZeroU16 {
    let size = match color {
        Color::Rgb => NonZeroU16::new(3),
        Color::Rgbw => NonZeroU16::new(4),
    };
    match size {
        Some(size) => size,
        None => NonZeroU16::MIN,
    }
}

/// The bytes of whole pixels in `channels` channels.
fn data_length(channels: usize, color: Color) -> usize {
    let size = usize::from(pixel(color).get());
    channels / size * size
}

/// The packets for `total` bytes: offset, length, and whether it is the last.
fn packets(total: usize) -> impl Iterator<Item = (usize, usize, bool)> {
    (0..total).step_by(MAX_DATA).map(move |offset| {
        let length = MAX_DATA.min(total - offset);
        (offset, length, offset + length == total)
    })
}

/// The header of one packet.
fn header(sequence: u8, color: Color, offset: usize, length: usize, last: bool) -> [u8; HEADER] {
    let kind = match color {
        Color::Rgb => 0x0B,
        Color::Rgbw => 0x1B,
    };
    let [o0, o1, o2, o3] = u32::try_from(offset).unwrap_or(u32::MAX).to_be_bytes();
    let [l0, l1] = u16::try_from(length).unwrap_or(u16::MAX).to_be_bytes();
    let flags = if last { VERSION | PUSH } else { VERSION };
    [flags, sequence, kind, DESTINATION, o0, o1, o2, o3, l0, l1]
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
mod tests {
    use super::*;

    #[test]
    fn the_sequence_runs_1_to_15() {
        // (now, next)
        let cases = [(0, 1), (1, 2), (14, 15), (15, 1)];
        for (now, next) in cases {
            assert_eq!(next_sequence(now), next, "{now}");
        }
    }

    #[test]
    fn a_frame_splits_into_packets_of_whole_pixels() {
        // (channels, color, packets: offset, length, last)
        let cases = [
            (512, Color::Rgb, vec![(0, 510, true)]),
            (512, Color::Rgbw, vec![(0, 512, true)]),
            (1536, Color::Rgb, vec![(0, 1440, false), (1440, 96, true)]),
            (2048, Color::Rgbw, vec![(0, 1440, false), (1440, 608, true)]),
            (2880, Color::Rgb, vec![(0, 1440, false), (1440, 1440, true)]),
            (2, Color::Rgb, vec![]),
        ];
        for (channels, color, expected) in cases {
            let found: Vec<_> = packets(data_length(channels, color)).collect();
            assert_eq!(found, expected, "{channels} {color:?}");
        }
    }

    #[test]
    fn the_header_says_what_wled_expects() {
        // (sequence, color, offset, length, last, header)
        let cases = [
            (
                1,
                Color::Rgb,
                0,
                510,
                true,
                [0x41, 1, 0x0B, 1, 0, 0, 0, 0, 0x01, 0xFE],
            ),
            (
                15,
                Color::Rgbw,
                0,
                1440,
                false,
                [0x40, 15, 0x1B, 1, 0, 0, 0, 0, 0x05, 0xA0],
            ),
            (
                7,
                Color::Rgbw,
                1440,
                608,
                true,
                [0x41, 7, 0x1B, 1, 0, 0, 0x05, 0xA0, 0x02, 0x60],
            ),
            (
                2,
                Color::Rgb,
                0x0102_0304,
                3,
                true,
                [0x41, 2, 0x0B, 1, 1, 2, 3, 4, 0, 3],
            ),
        ];
        for (sequence, color, offset, length, last, expected) in cases {
            assert_eq!(header(sequence, color, offset, length, last), expected);
        }
    }

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

    /// A link that fails every send the same way, and says on which thread it was dropped:
    /// 0 not yet, 1 the socket thread, 2 another.
    struct Fails(ErrorKind, Arc<std::sync::atomic::AtomicU8>);
    impl Link for Fails {
        fn send(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(self.0.into())
        }
    }
    impl Drop for Fails {
        fn drop(&mut self) {
            let on_socket_thread = thread::current().name() == Some(SOCKET_THREAD);
            self.1
                .store(if on_socket_thread { 1 } else { 2 }, Ordering::SeqCst);
        }
    }

    /// A link that is refused on every `n`th send, as a host that rate-limits its answers.
    struct Refused(u32, u32);
    impl Link for Refused {
        fn send(&mut self, packet: &[u8]) -> io::Result<usize> {
            self.1 += 1;
            if self.1.is_multiple_of(self.0) {
                Err(ErrorKind::ConnectionRefused.into())
            } else {
                Ok(packet.len())
            }
        }
    }

    #[test]
    fn refusals_now_and_then_keep_the_output_failed_until_a_clean_stretch() {
        let reopen: Reopen = Box::new(|| Err(io::Error::other("not needed")));
        let mut ddp = Ddp::with(Box::new(Refused(6, 0)), reopen, Color::Rgb).unwrap();
        let frame = [0; engine_core::ADDRESSES];
        let outcomes: Vec<_> = (0..198).map(|_| ddp.take(&frame)).collect(); // ends on a refusal
        // The first five frames went out before the first refusal.
        assert!(
            outcomes.iter().take(5).all(|o| *o == Outcome::Accepted),
            "{outcomes:?}"
        );
        assert!(
            outcomes.iter().skip(5).all(|o| *o == Outcome::Failed),
            "{outcomes:?}"
        );
        // The refusals stop: `CLEAN` frames later the output runs again.
        ddp.link = Box::new(Counts(Arc::default()));
        let after: Vec<_> = (0..=CLEAN).map(|_| ddp.take(&frame)).collect();
        let clean = usize::try_from(CLEAN).unwrap();
        assert!(
            after.iter().take(clean - 1).all(|o| *o == Outcome::Failed),
            "{after:?}"
        );
        assert!(
            after
                .iter()
                .skip(clean - 1)
                .all(|o| *o == Outcome::Accepted),
            "{after:?}"
        );
    }

    /// A link that sends, counting what it was given.
    struct Counts(Arc<std::sync::atomic::AtomicUsize>);
    impl Link for Counts {
        fn send(&mut self, packet: &[u8]) -> io::Result<usize> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(packet.len())
        }
    }

    #[test]
    fn a_broken_socket_is_replaced_once_off_the_output_thread() {
        let sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let opened = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let reopen: Reopen = {
            let (sent, opened) = (Arc::clone(&sent), Arc::clone(&opened));
            Box::new(move || {
                opened.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(Counts(Arc::clone(&sent))) as Box<dyn Link>)
            })
        };
        let dropped = Arc::new(std::sync::atomic::AtomicU8::new(0));
        let mut ddp = Ddp::with(
            Box::new(Fails(ErrorKind::AddrNotAvailable, Arc::clone(&dropped))),
            reopen,
            Color::Rgb,
        )
        .unwrap();
        let frame = [0; engine_core::ADDRESSES];
        assert_eq!(
            ddp.take(&frame),
            Outcome::Failed,
            "the old socket is broken"
        );
        let started = std::time::Instant::now();
        while ddp.take(&frame) != Outcome::Accepted {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "never replaced"
            ); // guard
            thread::yield_now();
        }
        assert_eq!(
            opened.load(Ordering::SeqCst),
            1,
            "opened once, however many frames failed meanwhile"
        );
        assert!(
            sent.load(Ordering::SeqCst) > 0,
            "the new socket carries the frames"
        );
        drop(ddp);
        let started = std::time::Instant::now();
        while dropped.load(Ordering::SeqCst) == 0 {
            assert!(started.elapsed() < Duration::from_secs(10), "never dropped"); // guard
            thread::yield_now();
        }
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            1,
            "the broken socket is closed and freed on the socket thread"
        );
    }
}
