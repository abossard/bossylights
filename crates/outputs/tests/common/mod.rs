//! A fake DMX USB Pro on pseudo terminals, for the tests of `outputs` and of the engine
//! (`crates/engine-server/tests` take this file by its path). Each time the port thread
//! looks for the adapter while it is plugged in, the finder makes a new pseudo terminal;
//! the test gets the other ends, in order. "Unplugging" drops an end.

#![allow(dead_code, reason = "each test file uses its own part")]
#![allow(
    clippy::disallowed_methods,
    reason = "the test polls for a port; it is not the output thread (only outputs bans sleep)"
)]
#![allow(
    clippy::disallowed_types,
    reason = "the widget's side hands each port's end over a channel, off the output thread"
)]

use outputs::Device;
use outputs::dmx_usb_pro::{DmxUsbPro, Find, MESSAGE, never_wait};
use serialport::{SerialPort, TTYPort};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

/// The longest a test waits for something that should come: a read of the test's end,
/// a port the finder should open. A hang guard: no test waits it out, then asserts.
pub const NEVER: Duration = Duration::from_secs(10);

/// The test's side of a fake adapter.
pub struct Fake {
    plugged: Arc<AtomicBool>,
    ends: mpsc::Receiver<TTYPort>,
}

impl Fake {
    /// The finder for a DMX USB Pro, and the test's side. Its widget answers the label 3
    /// request at each new port ([`answer_ask`]) before the test gets the port's end. A
    /// fake that is not `plugged` is not found until it is.
    pub fn new(plugged: bool) -> (Find, Self) {
        Self::with(plugged, true)
    }

    /// [`Fake::new`] with a widget that answers nothing: the test plays the widget.
    pub fn silent(plugged: bool) -> (Find, Self) {
        Self::with(plugged, false)
    }

    fn with(plugged: bool, answers: bool) -> (Find, Self) {
        let plugged = Arc::new(AtomicBool::new(plugged));
        let (give, ends) = mpsc::channel();
        let find: Find = {
            let plugged = Arc::clone(&plugged);
            Box::new(move || {
                if !plugged.load(Ordering::Relaxed) {
                    return Err(io::Error::from(io::ErrorKind::NotFound));
                }
                let (ours, port) = TTYPort::pair()?;
                never_wait(&port)?;
                let mut ours = end(ours)?;
                let give = give.clone();
                if answers {
                    // The port thread asks right after this find: answer it on the side.
                    thread::spawn(move || {
                        answer_ask(&mut ours);
                        let _ = give.send(ours);
                    });
                } else {
                    let _ = give.send(ours);
                }
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
        self.ends
            .recv_timeout(NEVER)
            .expect("the finder opened no port") // guard
    }

    /// The test's end of a port the finder opened, if it opened one not handed out yet.
    pub fn new_end(&mut self) -> Option<TTYPort> {
        self.ends.try_recv().ok()
    }
}

/// Returns once the widget on `device`'s port answered label 3: until then `take` drops
/// every frame (#293), and a test that counts frames hands the device over after it.
pub fn until_answered(device: &DmxUsbPro) {
    let started = Instant::now();
    while device.reading().settings().is_none() {
        assert!(started.elapsed() < NEVER, "the widget never answered"); // guard
        thread::yield_now();
    }
}

/// The widget's side of a new port: reads the label 3 request and answers it with the
/// API's defaults, break 9, mark after break 1, rate 40 (API p.3). A port closed before
/// it asked is left as it is.
pub fn answer_ask(end: &mut TTYPort) {
    let mut asked = [0; ASKED.len()];
    if end.read_exact(&mut asked).is_ok() && asked == ASKED {
        // firmware 1.44, then break, mark after break, rate
        let _ = end.write_all(&[0x7E, 3, 5, 0, 44, 1, 9, 1, 40, 0xE7]);
    }
}

/// The test's end of a pseudo terminal: a read waits at most [`NEVER`].
pub fn end(mut ours: TTYPort) -> io::Result<TTYPort> {
    ours.set_timeout(NEVER)?;
    Ok(ours)
}

/// Label 3, "get widget parameters", no user memory (API p.3): the port thread asks it
/// at each new port, before its first message.
pub const ASKED: [u8; 7] = [0x7E, 3, 2, 0, 0, 0, 0xE7];

/// The next whole message on the test's end, past any label 3 request.
pub fn read_message(end: &mut TTYPort) -> [u8; MESSAGE] {
    let mut message = [0; MESSAGE];
    let (head, rest) = message.split_at_mut(ASKED.len());
    loop {
        end.read_exact(head).expect("a whole message");
        if *head != ASKED {
            break;
        }
    }
    end.read_exact(rest).expect("a whole message");
    message
}
