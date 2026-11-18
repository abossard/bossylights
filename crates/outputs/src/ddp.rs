//! DDP over UDP, as WLED reads it. A frame's run of universes is one stream of channels;
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

use crate::{Color, Device, Levels, Outcome};
use engine_core::ADDRESSES;
use std::io::{self, ErrorKind};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const HEADER: usize = 10;
/// Data bytes per packet: 480 RGB or 360 RGBW pixels, as WLED expects.
pub const MAX_DATA: usize = 1440;
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
#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
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

/// The table of send errors. A calculation.
pub fn fault(error: &io::Error) -> Fault {
    if error.raw_os_error() == Some(NO_BUFFERS) {
        return Fault::Busy;
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
/// socket is replaced by one the socket thread opens, off the output thread.
pub struct Ddp {
    link: Box<dyn Link>,
    color: Color,
    sequence: u8,
    packet: [u8; HEADER + MAX_DATA],
    /// A refusal was seen and fewer than `CLEAN` frames went out clean since.
    down: bool,
    clean: u32,
    /// A new link was asked for and has not come yet.
    reopening: bool,
    renewed: rtrb::Consumer<Box<dyn Link>>,
    shared: Arc<Shared>,
    thread: JoinHandle<()>,
}

impl Ddp {
    /// Opens a socket to `address`. Off the output thread.
    pub fn open(address: SocketAddrV4, color: Color) -> io::Result<Self> {
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
        let thread = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("ddp-socket".into())
                .spawn(move || {
                    thread::park_timeout(Duration::ZERO); // sets up the park state
                    shared.ready.store(true, Ordering::Release);
                    while !shared.stop.load(Ordering::Acquire) {
                        if !shared.want.swap(false, Ordering::AcqRel) {
                            thread::park();
                            continue;
                        }
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
            down: false,
            clean: 0,
            reopening: false,
            renewed,
            shared,
            thread,
        })
    }

    fn refused(&mut self) -> Outcome {
        self.down = true;
        self.clean = 0;
        Outcome::Failed
    }

    fn ask_for_a_new_link(&mut self) {
        if !self.reopening {
            self.reopening = true;
            self.shared.want.store(true, Ordering::Release);
            self.thread.thread().unpark();
        }
    }
}

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
    fn take(&mut self, frame: &[Levels]) -> Outcome {
        if let Ok(link) = self.renewed.pop() {
            self.link = link;
            self.reopening = false;
        }
        self.sequence = next_sequence(self.sequence);
        let total = data_length(frame.len() * ADDRESSES, self.color);
        for (offset, length, last) in packets(total) {
            let (header, data) = self.packet.split_at_mut(HEADER);
            header.copy_from_slice(&self::header(
                self.sequence,
                self.color,
                offset,
                length,
                last,
            ));
            let Some(data) = data.get_mut(..length) else {
                return Outcome::Failed; // cannot happen: length is at most MAX_DATA
            };
            channels(frame, offset, data);
            let Some(packet) = self.packet.get(..HEADER + length) else {
                return Outcome::Failed;
            };
            match self.link.send(packet) {
                Ok(sent) if sent == packet.len() => {}
                Ok(_) => return self.refused(),
                Err(error) => match fault(&error) {
                    Fault::Busy => return Outcome::Dropped,
                    Fault::DeviceNotThere => return self.refused(),
                    Fault::SocketBroken => {
                        self.ask_for_a_new_link();
                        return self.refused();
                    }
                },
            }
        }
        if self.down {
            self.clean += 1;
            if self.clean < CLEAN {
                return Outcome::Failed;
            }
            self.down = false;
        }
        Outcome::Accepted
    }
}

/// The next sequence number: 1 to 15, then 1 again. 0 means "no sequence" to WLED.
pub fn next_sequence(sequence: u8) -> u8 {
    sequence % 15 + 1
}

/// The bytes of whole pixels in `channels` channels.
pub fn data_length(channels: usize, color: Color) -> usize {
    let size = match color {
        Color::Rgb => 3,
        Color::Rgbw => 4,
    };
    channels / size * size
}

/// The packets for `total` bytes: offset, length, and whether it is the last.
pub fn packets(total: usize) -> impl Iterator<Item = (usize, usize, bool)> {
    (0..total).step_by(MAX_DATA).map(move |offset| {
        let length = MAX_DATA.min(total - offset);
        (offset, length, offset + length == total)
    })
}

/// The header of one packet.
pub fn header(
    sequence: u8,
    color: Color,
    offset: usize,
    length: usize,
    last: bool,
) -> [u8; HEADER] {
    let kind = match color {
        Color::Rgb => 0x0B,
        Color::Rgbw => 0x1B,
    };
    let [o0, o1, o2, o3] = u32::try_from(offset).unwrap_or(u32::MAX).to_be_bytes();
    let [l0, l1] = u16::try_from(length).unwrap_or(u16::MAX).to_be_bytes();
    let flags = if last { VERSION | PUSH } else { VERSION };
    [flags, sequence, kind, DESTINATION, o0, o1, o2, o3, l0, l1]
}

/// Copies the channels from `offset` of the frame's stream into `out`.
pub fn channels(frame: &[Levels], offset: usize, out: &mut [u8]) {
    let stream = frame.iter().flat_map(|universe| universe.0.iter());
    for (byte, level) in out.iter_mut().zip(stream.skip(offset)) {
        *byte = *level;
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test that cannot set itself up fails"
)]
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

    #[test]
    fn channels_run_across_universes() {
        let mut frame = [Levels::ZERO; 2];
        frame[0].0[510] = 1;
        frame[0].0[511] = 2;
        frame[1].0[0] = 3;
        // (offset, length, bytes)
        let cases = [
            (510, 3, vec![1, 2, 3]),
            (0, 2, vec![0, 0]),
            (1023, 2, vec![0, 0]),
        ];
        for (offset, length, expected) in cases {
            let mut out = vec![0; length];
            channels(&frame, offset, &mut out);
            assert_eq!(out, expected, "{offset}");
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

    fn free_port() -> SocketAddrV4 {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let std::net::SocketAddr::V4(address) = socket.local_addr().unwrap() else {
            panic!("IPv4");
        };
        address
    }

    #[test]
    fn a_device_nobody_listens_on_is_steadily_failed_and_runs_again_when_one_appears() {
        let address = free_port();
        let mut ddp = Ddp::open(address, Color::Rgb).unwrap();
        let frame = [Levels::ZERO];
        // The host reports the first refusal on a later send.
        let started = std::time::Instant::now();
        while ddp.take(&frame) != Outcome::Failed {
            assert!(started.elapsed() < Duration::from_secs(10), "no refusal"); // guard
            thread::yield_now();
        }
        // The host reports a refusal now and then: the output stays failed in between.
        let steady: Vec<_> = (1..CLEAN).map(|_| ddp.take(&frame)).collect();
        assert!(steady.iter().all(|o| *o == Outcome::Failed), "{steady:?}");

        let listener = UdpSocket::bind(address).unwrap();
        listener
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let started = std::time::Instant::now();
        while ddp.take(&frame) != Outcome::Accepted {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "never runs again"
            ); // guard
            thread::yield_now();
        }
        let mut packet = [0; 2000];
        assert_eq!(listener.recv(&mut packet).unwrap(), HEADER + 510);
        assert_eq!(packet.first(), Some(&0x41), "version 1 and push");
    }

    /// A link that fails every send the same way.
    struct Fails(ErrorKind);
    impl Link for Fails {
        fn send(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(self.0.into())
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
        let frame = [Levels::ZERO];
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
        let mut ddp = Ddp::with(
            Box::new(Fails(ErrorKind::AddrNotAvailable)),
            reopen,
            Color::Rgb,
        )
        .unwrap();
        let frame = [Levels::ZERO];
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
    }
}
