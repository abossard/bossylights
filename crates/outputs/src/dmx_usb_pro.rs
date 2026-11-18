//! ENTTEC DMX USB Pro over the serial driver the system already has. The adapter makes
//! the DMX timing itself; the host sends one "send DMX" message per frame.
//!
//! | Byte | Message |
//! |---|---|
//! | 0 | start of message `0x7E` |
//! | 1 | label 6: send DMX |
//! | 2 to 3 | data length, little endian: 513 |
//! | 4 | DMX start code 0 |
//! | 5 to 516 | 512 channels: the first 512 of the run |
//! | 517 | end of message `0xE7` |
//!
//! Who does what:
//!
//! | Thread | Does |
//! |---|---|
//! | output thread | [`DmxUsbPro::take`]: one non-blocking write, never more often than the link drains. A lost port goes to the finder; `take` fails until the finder hands in a new one |
//! | finder, one per adapter | asleep. While the port is lost: looks for the adapter every [`LOOK_EVERY`], opens it and hands it in through a wait-free queue. Closes lost ports |

use crate::{Device, Levels, Outcome};
use engine_core::ADDRESSES;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use serialport::{DataBits, FlowControl, Parity, SerialPortType, StopBits, TTYPort};
use std::io::{self, ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// The adapter's USB identity: an FTDI chip that calls itself "DMX USB PRO".
pub const VENDOR: u16 = 0x0403;
pub const PRODUCT: u16 = 0x6001;
pub const NAME: &str = "DMX USB PRO";
/// The link from the USB chip to the widget is a real UART. 44 frames need 22.8 kB/s;
/// 57 600 baud queued frames minutes behind, out of sight. 500 000 is about 2x headroom.
pub const BAUD: u32 = 500_000;
/// One "send DMX" message.
pub const MESSAGE: usize = 4 + 1 + ADDRESSES + 1;
/// The time the link takes for one message, 11 bits a byte, plus a quarter. A frame
/// that comes sooner is dropped instead of queued.
pub const LINK_TIME: Duration =
    Duration::from_nanos(MESSAGE as u64 * 11 * 1_000_000_000 * 5 / (BAUD as u64 * 4));
/// How often the finder looks for a lost adapter.
pub const LOOK_EVERY: Duration = Duration::from_millis(500);
/// How often the finder checks whether the port was lost.
const WATCH_EVERY: Duration = Duration::from_millis(100);

/// Writes the message for a frame into `out`: the first 512 channels of the run.
pub fn message(frame: &[Levels], out: &mut [u8; MESSAGE]) {
    let [lsb, msb] = ((ADDRESSES + 1) as u16).to_le_bytes();
    let (head, rest) = out.split_at_mut(5);
    head.copy_from_slice(&[0x7E, 6, lsb, msb, 0]);
    let (channels, end) = rest.split_at_mut(ADDRESSES);
    channels.copy_from_slice(&frame.first().map_or([0; ADDRESSES], |universe| universe.0));
    end.copy_from_slice(&[0xE7]);
}

/// Finds a port and opens it. Runs on the finder thread, and once in [`DmxUsbPro::open`].
pub type Find = Box<dyn FnMut() -> io::Result<TTYPort> + Send>;

/// A DMX USB Pro, as the output thread holds it.
pub struct DmxUsbPro {
    port: Option<TTYPort>,
    message: [u8; MESSAGE],
    /// Bytes of the last message the driver did not take yet.
    unsent: usize,
    link_free_at: Option<Instant>,
    /// Ports the finder opened.
    found: rtrb::Consumer<TTYPort>,
    /// Lost ports, for the finder to close.
    lost: rtrb::Producer<TTYPort>,
    /// Set while there is no port: the finder looks.
    looking: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl DmxUsbPro {
    /// Finds and opens the port once, here, then starts the finder for later. Call it off
    /// the output thread.
    pub fn open(mut find: Find) -> io::Result<Self> {
        let port = find()?;
        let (found_in, found) = rtrb::RingBuffer::new(2);
        let (lost, lost_out) = rtrb::RingBuffer::new(2);
        let looking = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let finder = Finder {
            find,
            found: found_in,
            lost: lost_out,
            looking: Arc::clone(&looking),
            stop: Arc::clone(&stop),
        };
        thread::Builder::new()
            .name("dmx usb pro finder".into())
            .spawn(move || finder.run())?;
        Ok(Self {
            port: Some(port),
            message: [0; MESSAGE],
            unsent: 0,
            link_free_at: None,
            found,
            lost,
            looking,
            stop,
        })
    }

    /// The port is gone: hand it to the finder to close, and let it look.
    fn lose(&mut self) -> Outcome {
        if let Some(port) = self.port.take() {
            let _ = self.lost.push(port); // the finder empties it every WATCH_EVERY
        }
        self.unsent = 0;
        self.link_free_at = None;
        self.looking.store(true, Ordering::Release);
        Outcome::Failed
    }
}

impl Device for DmxUsbPro {
    /// One non-blocking write. A busy link or a full driver queue drops the frame.
    fn take(&mut self, frame: &[Levels]) -> Outcome {
        if let Ok(port) = self.found.pop() {
            if let Some(old) = self.port.replace(port) {
                let _ = self.lost.push(old);
            }
            self.unsent = 0;
            self.link_free_at = None;
        }
        let Some(port) = &mut self.port else {
            return Outcome::Failed;
        };
        let now = Instant::now();
        // Half a message would garble the link: finish it first, drop this frame.
        if self.unsent > 0 {
            let from = MESSAGE - self.unsent;
            return match write(port, self.message.get(from..).unwrap_or_default()) {
                Ok(written) => {
                    self.unsent -= written;
                    Outcome::Dropped
                }
                Err(_) => self.lose(),
            };
        }
        if self.link_free_at.is_some_and(|free| free > now) {
            return Outcome::Dropped;
        }
        message(frame, &mut self.message);
        match write(port, &self.message) {
            Ok(0) => Outcome::Dropped,
            Ok(written) => {
                self.unsent = MESSAGE - written;
                self.link_free_at = Some(now + LINK_TIME);
                Outcome::Accepted
            }
            Err(_) => self.lose(),
        }
    }
}

impl Drop for DmxUsbPro {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed); // the finder ends within WATCH_EVERY
    }
}

/// One write that never waits: how many bytes the driver took, 0 if none.
fn write(port: &mut TTYPort, bytes: &[u8]) -> io::Result<usize> {
    match port.write(bytes) {
        Ok(written) => Ok(written),
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => Ok(0),
        Err(e) => Err(e),
    }
}

/// Looks for the adapter while the port is lost, off the output thread.
struct Finder {
    find: Find,
    found: rtrb::Producer<TTYPort>,
    lost: rtrb::Consumer<TTYPort>,
    looking: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Finder {
    #[expect(
        clippy::disallowed_methods,
        reason = "the finder thread sleeps; the output thread never does"
    )]
    fn run(mut self) {
        while !self.stop.load(Ordering::Relaxed) {
            while let Ok(port) = self.lost.pop() {
                drop(port); // closed here, never on the output thread
            }
            if !self.looking.load(Ordering::Acquire) {
                thread::sleep(WATCH_EVERY);
                continue;
            }
            if let Ok(port) = (self.find)() {
                // Before the hand-over: if the new port is lost at once, the output
                // thread's `true` comes after this and is not overwritten.
                self.looking.store(false, Ordering::Release);
                if let Err(rtrb::PushError::Full(port)) = self.found.push(port) {
                    drop(port);
                    self.looking.store(true, Ordering::Release);
                }
            }
            thread::sleep(LOOK_EVERY);
        }
    }
}

/// The finder for real adapters: the `adapter`th DMX USB Pro by USB identity, opened at
/// [`BAUD`], 8 data bits, 2 stop bits, writes that never wait.
pub fn find(adapter: u8) -> Find {
    Box::new(move || {
        let ports = adapters()?;
        let name = ports
            .get(usize::from(adapter).wrapping_sub(1))
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "no such DMX USB Pro"))?;
        open_port(name)
    })
}

/// The adapter number of the port with this name, as [`find`] counts.
pub fn adapter_of(port: &str) -> io::Result<u8> {
    let at = adapters()?
        .iter()
        .position(|name| name == port)
        .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "that port is no DMX USB Pro"))?;
    u8::try_from(at + 1).map_err(|_| io::Error::other("too many adapters"))
}

/// The port names of every DMX USB Pro, in the order of their USB serial numbers.
fn adapters() -> io::Result<Vec<String>> {
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
    Ok(ports.into_iter().map(|(_, name)| name).collect())
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

/// Makes writes on `port` return at once, also for a port a test made.
pub fn never_wait(port: &TTYPort) -> io::Result<()> {
    fcntl(port.as_raw_fd(), FcntlArg::F_SETFL(OFlag::O_NONBLOCK))
        .map(|_| ())
        .map_err(io::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_becomes_one_send_dmx_message() {
        let counting = Levels(std::array::from_fn(|n| (n % 251) as u8));
        // (frame, channel 1, channel 512)
        let cases = [
            (vec![], 0, 0),
            (vec![counting], 0, (511 % 251) as u8),
            (vec![Levels([9; ADDRESSES]), Levels([3; ADDRESSES])], 9, 9),
        ];
        for (frame, first, last) in cases {
            let mut out = [0xAA; MESSAGE];
            message(&frame, &mut out);
            assert_eq!(
                out[..5],
                [0x7E, 6, 0x01, 0x02, 0],
                "{} universes",
                frame.len()
            );
            assert_eq!((out[5], out[516]), (first, last));
            assert_eq!(out[517], 0xE7);
        }
    }
}
