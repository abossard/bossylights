//! Where frames go. A lost adapter is a state: `send` fails, the sink closes the port
//! and tries to open it again about twice a second, without allocating. The DDP sink
//! never waits: its UDP socket is non-blocking, and a send that would block drops the frame.
//! Nor does the Pro sink: non-blocking writes, and a frame is dropped while the serial link
//! is still busy with the last one, so no backlog can build up in the driver.

use crate::CHANNELS;
use crate::sys;
use std::ffi::CString;
use std::io::ErrorKind;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

/// DMX512: break of at least 88 µs, mark after break of at least 8 µs. With margin:
const BREAK: Duration = Duration::from_micros(110);
const MARK_AFTER_BREAK: Duration = Duration::from_micros(16);
const DMX_BAUD: u32 = 250_000;
/// The DMX USB Pro's serial link from its USB chip to the widget is a real UART: the rate
/// sets how fast messages get there. 57 600 carries about 5 kB/s, a quarter of what 44 full
/// frames need (22.8 kB/s). 500 000 gives about 2x headroom; 3 000 000 dropped the widget off USB.
const PRO_BAUD: u64 = 500_000;
/// How long one Pro message keeps the link busy: 11 bits a byte (start, 8 data, 2 stop),
/// counted at 80 % of the nominal rate to stay on the safe side.
const PRO_LINK_TIME: Duration =
    Duration::from_nanos(PRO_PACKET as u64 * 11 * 1_000_000_000 * 5 / (PRO_BAUD * 4));
/// While disconnected, try to open the port again every this many frames.
const RETRY_FRAMES: u32 = 22;
/// DDP (as WLED reads it): flags (version 1, push), sequence, data type, device id,
/// offset (u32, big endian), length (u16, big endian).
const DDP_HEADER: usize = 10;
const DDP_FLAGS: u8 = 0x41;
/// type RGBW (011), 8 bits per element (011)
const DDP_RGBW8: u8 = 0x1B;
const DDP_DEVICE: u8 = 1;
/// The WLED simulator's matrix: 4 rows of 80, row-major.
pub const DDP_COLUMNS: usize = 80;
pub const DDP_PIXELS: usize = 320;
pub const DDP_PACKET: usize = DDP_HEADER + DDP_PIXELS * 4;
const PRO_PACKET: usize = CHANNELS + 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The same loop with no device: the frame is only read.
    Mock,
    /// Enttec Open DMX: an FTDI chip and nothing else. The host makes break and
    /// mark-after-break with the serial break signal, then sends 513 bytes at 250 kbaud.
    OpenDmx,
    /// Enttec DMX USB Pro: one "send DMX" message (label 6), the device makes the timing.
    Pro,
    /// One DDP packet over UDP per frame: 320 RGBW pixels, as WLED takes them.
    Ddp,
}

pub struct Sink {
    kind: Kind,
    path: CString,
    fd: Option<i32>,
    socket: Option<UdpSocket>,
    /// start code and channels for Open DMX; the whole message for the Pro or DDP
    packet: [u8; DDP_PACKET],
    checksum: u64,
    retry_in: u32,
    reconnects: u32,
    frame: u32,
    dropped: u32,
    /// when the serial link has sent the last Pro message, as far as the host can tell
    link_free_at: Option<Instant>,
    /// bytes of the last Pro message the driver did not take yet
    unsent: usize,
}

#[derive(Debug)]
pub enum SendError {
    /// The port is closed; the sink is trying to open it again.
    Disconnected,
    /// The link is busy (the DDP socket is full, or the Pro's serial link still sends the
    /// last frame): this frame is dropped instead of waiting.
    Busy,
    /// The receiver refused the packet (no one listens on that port).
    Refused,
}

impl Sink {
    pub fn mock() -> Self {
        Self::new(Kind::Mock, CString::default())
    }

    /// A serial port such as `/dev/cu.usbserial-XXXX`, served by the FTDI driver built into macOS.
    pub fn serial(kind: Kind, path: &str) -> Result<Self, String> {
        let path = CString::new(path).map_err(|_| format!("not a path: {path}"))?;
        let mut sink = Self::new(kind, path);
        sink.fd = Some(
            sink.open()
                .map_err(|e| format!("cannot open {:?}: {e}", sink.path))?,
        );
        Ok(sink)
    }

    /// A DDP receiver such as WLED at `address` (host:port). Sends from a non-blocking socket.
    pub fn ddp(address: &str) -> Result<Self, String> {
        let socket = UdpSocket::bind("0.0.0.0:0")
            .and_then(|socket| socket.connect(address).map(|()| socket))
            .and_then(|socket| socket.set_nonblocking(true).map(|()| socket))
            .map_err(|e| format!("cannot reach {address}: {e}"))?;
        let mut sink = Self::new(Kind::Ddp, CString::default());
        sink.socket = Some(socket);
        Ok(sink)
    }

    fn new(kind: Kind, path: CString) -> Self {
        Self {
            kind,
            path,
            fd: None,
            socket: None,
            packet: [0; DDP_PACKET],
            checksum: 0,
            retry_in: 0,
            reconnects: 0,
            frame: 0,
            dropped: 0,
            link_free_at: None,
            unsent: 0,
        }
    }

    pub fn reconnects(&self) -> u32 {
        self.reconnects
    }

    /// Frames dropped because the link was busy.
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    pub fn send(&mut self, channels: &[u8; CHANNELS]) -> Result<(), SendError> {
        self.frame = self.frame.wrapping_add(1);
        match self.kind {
            Kind::Mock => {
                self.checksum = channels.iter().fold(self.checksum, |sum, v| {
                    sum.wrapping_mul(31).wrapping_add(u64::from(*v))
                });
                std::hint::black_box(self.checksum);
                return Ok(());
            }
            Kind::Ddp => return self.send_ddp(),
            Kind::OpenDmx | Kind::Pro => {}
        }
        let fd = self.connected()?;
        let sent = match self.kind {
            Kind::OpenDmx => self.send_open_dmx(fd, channels).map(|()| true),
            Kind::Pro => self.send_pro(fd, channels),
            Kind::Mock | Kind::Ddp => Ok(true),
        };
        match sent {
            Ok(true) => Ok(()),
            Ok(false) => {
                self.dropped += 1;
                Err(SendError::Busy)
            }
            Err(_) => {
                sys::close(fd);
                self.fd = None;
                self.link_free_at = None;
                self.unsent = 0;
                Err(SendError::Disconnected)
            }
        }
    }

    fn connected(&mut self) -> Result<i32, SendError> {
        if let Some(fd) = self.fd {
            return Ok(fd);
        }
        if self.retry_in > 0 {
            self.retry_in -= 1;
            return Err(SendError::Disconnected);
        }
        self.retry_in = RETRY_FRAMES;
        let fd = self.open().map_err(|_| SendError::Disconnected)?;
        self.fd = Some(fd);
        self.reconnects += 1;
        Ok(fd)
    }

    fn open(&self) -> std::io::Result<i32> {
        match self.kind {
            Kind::Pro => sys::open_serial(&self.path, PRO_BAUD as u32, false),
            _ => sys::open_serial(&self.path, DMX_BAUD, true),
        }
    }

    fn send_open_dmx(&mut self, fd: i32, channels: &[u8; CHANNELS]) -> std::io::Result<()> {
        self.packet[0] = 0; // start code
        self.packet[1..=CHANNELS].copy_from_slice(channels);
        sys::set_break(fd, true)?;
        spin(BREAK);
        sys::set_break(fd, false)?;
        spin(MARK_AFTER_BREAK);
        sys::write_all(fd, &self.packet[..=CHANNELS])?;
        // Wait until the last byte is on the wire, or the next break would cut it off.
        // 513 bytes of 11 bits at 250 kbaud take 22.6 ms: almost the whole 22.7 ms frame.
        // This blocks the output thread by design. Not run: the Pro run showed that the
        // driver buffers below the tty, so whether tcdrain waits for the wire is open.
        sys::drain(fd)
    }

    /// `Ok(true)` when this frame went out, `Ok(false)` when it was dropped (link busy).
    fn send_pro(&mut self, fd: i32, channels: &[u8; CHANNELS]) -> std::io::Result<bool> {
        let now = Instant::now();
        // A message cut short by a full driver queue is finished first: half a message
        // would garble the link. The new frame is dropped.
        if self.unsent > 0 {
            let from = PRO_PACKET - self.unsent;
            self.unsent -= sys::write_some(fd, &self.packet[from..PRO_PACKET])?;
            return Ok(false);
        }
        if self.link_free_at.is_some_and(|free| free > now) {
            return Ok(false);
        }
        let length = (CHANNELS + 1) as u16;
        let [lsb, msb] = length.to_le_bytes();
        self.packet[..5].copy_from_slice(&[0x7E, 6, lsb, msb, 0]);
        self.packet[5..5 + CHANNELS].copy_from_slice(channels);
        self.packet[5 + CHANNELS] = 0xE7;
        let written = sys::write_some(fd, &self.packet[..PRO_PACKET])?;
        if written == 0 {
            return Ok(false);
        }
        self.unsent = PRO_PACKET - written;
        self.link_free_at = Some(now + PRO_LINK_TIME);
        Ok(true)
    }

    /// A full-colour rainbow that moves one column per frame, the same on all 4 rows:
    /// a dropped or repeated frame shows as a jump or a stop. White stays 0.
    fn send_ddp(&mut self) -> Result<(), SendError> {
        let length = (DDP_PIXELS * 4) as u16;
        let [l0, l1] = length.to_be_bytes();
        let sequence = (self.frame % 15) as u8 + 1; // 1 to 15; 0 means "no sequence"
        self.packet[..DDP_HEADER].copy_from_slice(&[
            DDP_FLAGS, sequence, DDP_RGBW8, DDP_DEVICE, 0, 0, 0, 0, l0, l1,
        ]);
        let shift = self.frame as usize % DDP_COLUMNS;
        for (i, pixel) in self.packet[DDP_HEADER..].chunks_exact_mut(4).enumerate() {
            let column = i % DDP_COLUMNS;
            let [r, g, b] = rainbow((column + DDP_COLUMNS - shift) % DDP_COLUMNS);
            pixel.copy_from_slice(&[r, g, b, 0]);
        }
        let Some(socket) = &self.socket else {
            return Err(SendError::Disconnected);
        };
        match socket.send(&self.packet) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                self.dropped += 1;
                Err(SendError::Busy)
            }
            Err(_) => Err(SendError::Refused),
        }
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        if let Some(fd) = self.fd.take() {
            sys::close(fd);
        }
    }
}

/// Step `step` of `DDP_COLUMNS` around the colour wheel, full saturation and brightness.
pub fn rainbow(step: usize) -> [u8; 3] {
    let hue = step * 6 * 256 / DDP_COLUMNS;
    let x = (hue % 256) as u8;
    match hue / 256 {
        0 => [255, x, 0],
        1 => [255 - x, 255, 0],
        2 => [0, 255, x],
        3 => [0, 255 - x, 255],
        4 => [x, 0, 255],
        _ => [255, 0, 255 - x],
    }
}

fn spin(how_long: Duration) {
    let until = Instant::now() + how_long;
    while Instant::now() < until {
        std::hint::spin_loop();
    }
}
