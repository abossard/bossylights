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
//! | output thread | [`DmxUsbPro::take`]: puts the message in a one-slot wait-free queue and wakes the port thread. Never calls the serial driver: a vanishing device holds the driver up for many milliseconds |
//! | port thread, one per adapter | owns the port: writes each message, never more often than the link drains. When a write fails it closes the port and looks for the adapter every [`LOOK_EVERY`] |

use crate::{Device, Levels, Outcome};
use engine_core::ADDRESSES;
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use serialport::{DataBits, FlowControl, Parity, SerialPortType, StopBits, TTYPort};
use std::io::{self, ErrorKind};
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::thread::{self, JoinHandle};
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
/// How often the port thread looks for a lost adapter.
pub const LOOK_EVERY: Duration = Duration::from_millis(500);
/// How often an idle port thread checks whether it should stop.
const WATCH_EVERY: Duration = Duration::from_millis(100);
/// How long the port thread waits for a full driver queue before it tries again.
const QUEUE_FULL: Duration = Duration::from_millis(1);

/// Writes the message for a frame into `out`: the first 512 channels of the run.
pub fn message(frame: &[Levels], out: &mut [u8; MESSAGE]) {
    let [lsb, msb] = ((ADDRESSES + 1) as u16).to_le_bytes();
    let (head, rest) = out.split_at_mut(5);
    head.copy_from_slice(&[0x7E, 6, lsb, msb, 0]);
    let (channels, end) = rest.split_at_mut(ADDRESSES);
    channels.copy_from_slice(&frame.first().map_or([0; ADDRESSES], |universe| universe.0));
    end.copy_from_slice(&[0xE7]);
}

/// Finds a port and opens it. Runs on the port thread, and once in [`DmxUsbPro::open`].
pub type Find = Box<dyn FnMut() -> io::Result<TTYPort> + Send>;

/// A DMX USB Pro, as the output thread holds it: no port, only the queue to its thread.
pub struct DmxUsbPro {
    message: [u8; MESSAGE],
    to_port: rtrb::Producer<[u8; MESSAGE]>,
    port_thread: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
}

/// What the output thread and the port thread share.
#[derive(Default)]
struct Shared {
    /// Set while there is no port.
    lost: AtomicBool,
    stop: AtomicBool,
    /// The system error that lost the port last, 0 if none.
    lost_with: AtomicI32,
}

/// Why the port was lost last: the system's error, as the port thread saw it.
pub struct LostWith(Arc<Shared>);

impl LostWith {
    /// The last error that lost the port, if one did.
    pub fn get(&self) -> Option<io::Error> {
        match self.0.lost_with.load(Ordering::Relaxed) {
            0 => None,
            code => Some(io::Error::from_raw_os_error(code)),
        }
    }
}

impl DmxUsbPro {
    /// Finds and opens the port once, here, then hands it to its own thread. Call it off
    /// the output thread.
    pub fn open(mut find: Find) -> io::Result<Self> {
        let port = find()?;
        let (to_port, messages) = rtrb::RingBuffer::new(1);
        let shared = Arc::new(Shared::default());
        let port_thread = PortThread {
            find,
            port: Some(port),
            messages,
            shared: Arc::clone(&shared),
        };
        let handle = thread::Builder::new()
            .name("dmx usb pro port".into())
            .spawn(move || port_thread.run())?;
        Ok(Self {
            message: [0; MESSAGE],
            to_port,
            port_thread: Some(handle),
            shared,
        })
    }

    /// Why the port was lost last; readable from any thread.
    pub fn lost_with(&self) -> LostWith {
        LostWith(Arc::clone(&self.shared))
    }
}

impl Device for DmxUsbPro {
    /// Hands the message to the port thread. While it has not taken the last one yet,
    /// this frame is dropped; while the port is lost, it fails.
    fn take(&mut self, frame: &[Levels]) -> Outcome {
        if self.shared.lost.load(Ordering::Acquire) {
            return Outcome::Failed;
        }
        if self.to_port.is_full() {
            return Outcome::Dropped;
        }
        message(frame, &mut self.message);
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

impl Drop for DmxUsbPro {
    /// Stops the port thread and waits until it closed the port, so the port is free
    /// for the next opener. Never on the output thread: the engine frees devices off it.
    /// Each wait of the port thread watches `stop`, so this takes at most a link time
    /// or a look for the adapter.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(port_thread) = self.port_thread.take() {
            port_thread.thread().unpark();
            drop(port_thread.join());
        }
    }
}

/// The one thread that calls the serial driver for this adapter.
struct PortThread {
    find: Find,
    port: Option<TTYPort>,
    messages: rtrb::Consumer<[u8; MESSAGE]>,
    shared: Arc<Shared>,
}

impl PortThread {
    #[expect(
        clippy::disallowed_methods,
        reason = "the port thread sleeps; the output thread never does"
    )]
    fn run(mut self) {
        while !self.shared.stop.load(Ordering::Relaxed) {
            let Some(port) = &self.port else {
                // A lost adapter is gone for a while; then look again. `drop` wakes it.
                thread::park_timeout(LOOK_EVERY);
                if self.shared.stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Ok(port) = (self.find)() {
                    while self.messages.pop().is_ok() {} // from before the loss
                    self.port = Some(port);
                    self.shared.lost.store(false, Ordering::Release);
                }
                continue;
            };
            let Ok(message) = self.messages.pop() else {
                thread::park_timeout(WATCH_EVERY);
                continue;
            };
            let started = Instant::now();
            match write_all(port, &message, &self.shared.stop) {
                Ok(()) => thread::sleep(LINK_TIME.saturating_sub(started.elapsed())),
                Err(error) => {
                    self.shared.lost_with.store(error as i32, Ordering::Relaxed);
                    self.shared.lost.store(true, Ordering::Release);
                    self.port = None; // closed here, never on the output thread
                }
            }
        }
    }
}

/// Writes the whole message, straight to the system: a full driver queue is waited out
/// here, on the port thread. Half a message would garble the link.
#[expect(
    clippy::disallowed_methods,
    reason = "the port thread sleeps; the output thread never does"
)]
fn write_all(port: &TTYPort, message: &[u8], stop: &AtomicBool) -> Result<(), Errno> {
    let mut rest = message;
    while !rest.is_empty() && !stop.load(Ordering::Relaxed) {
        match nix::unistd::write(port.as_raw_fd(), rest) {
            Ok(0) | Err(Errno::EAGAIN | Errno::EINTR) => thread::sleep(QUEUE_FULL),
            Ok(written) => rest = rest.get(written..).unwrap_or_default(),
            Err(error) => return Err(error),
        }
    }
    Ok(())
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
