//! A fake DMX USB Pro on pseudo terminals, for the tests of `outputs` and of the engine
//! (`crates/engine-server/tests` take this file by its path). Each time the port thread
//! looks for the adapter while it is plugged in, the finder makes a new pseudo terminal;
//! the test gets the other ends, in order. "Unplugging" drops an end.

#![allow(dead_code, reason = "each test file uses its own part")]
#![allow(
    clippy::disallowed_methods,
    reason = "the test polls for a port; it is not the output thread (only outputs bans sleep)"
)]

use outputs::dmx_usb_pro::{Find, MESSAGE, never_wait};
use serialport::{SerialPort, TTYPort};
use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// The longest a test waits for something that should come: a read of the test's end,
/// a port the finder should open. A hang guard: no test waits it out, then asserts.
pub const NEVER: Duration = Duration::from_secs(10);

/// The test's side of a fake adapter.
pub struct Fake {
    plugged: Arc<AtomicBool>,
    ends: rtrb::Consumer<TTYPort>,
}

impl Fake {
    /// The finder for the device, and the test's side. A fake that is not `plugged` is not
    /// found until it is.
    pub fn new(plugged: bool) -> (Find, Self) {
        let plugged = Arc::new(AtomicBool::new(plugged));
        let (mut give, ends) = rtrb::RingBuffer::new(8);
        let find: Find = {
            let plugged = Arc::clone(&plugged);
            Box::new(move || {
                if !plugged.load(Ordering::Relaxed) {
                    return Err(io::Error::from(io::ErrorKind::NotFound));
                }
                let (ours, port) = TTYPort::pair()?;
                never_wait(&port)?;
                give.push(end(ours)?)
                    .map_err(|_| io::Error::other("too many ports"))?;
                Ok(port)
            })
        };
        (find, Self { plugged, ends })
    }

    /// Whether the adapter is plugged in, for the test to change.
    pub fn plugged(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.plugged)
    }

    /// The test's end of the next port the finder opens, once it did.
    pub fn next_end(&mut self) -> TTYPort {
        let started = Instant::now();
        loop {
            if let Ok(end) = self.ends.pop() {
                return end;
            }
            assert!(started.elapsed() < NEVER, "the finder opened no port"); // guard
            thread::sleep(Duration::from_millis(5)); // poll
        }
    }

    /// The test's end of a port the finder opened, if it opened one not handed out yet.
    pub fn new_end(&mut self) -> Option<TTYPort> {
        self.ends.pop().ok()
    }
}

/// The test's end of a pseudo terminal: a read waits at most [`NEVER`].
pub fn end(mut ours: TTYPort) -> io::Result<TTYPort> {
    ours.set_timeout(NEVER)?;
    Ok(ours)
}

/// The next whole message on the test's end.
pub fn read_message(end: &mut TTYPort) -> [u8; MESSAGE] {
    let mut message = [0; MESSAGE];
    end.read_exact(&mut message).expect("a whole message");
    message
}
