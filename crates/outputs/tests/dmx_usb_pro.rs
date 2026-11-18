//! A DMX USB Pro against a fake serial port: a pseudo terminal. The test reads the other
//! end and checks every byte; "unplugging" closes that end, and the port thread opens a
//! new port by itself.

#![expect(
    clippy::disallowed_methods,
    reason = "the test waits between frames; it is not the output thread"
)]

use outputs::dmx_usb_pro::{DmxUsbPro, Find, MESSAGE, never_wait};
use outputs::{Device, Levels, Outcome};
use serialport::{SerialPort, TTYPort};
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

/// A finder that makes a new fake port each time it is asked, and hands the test the
/// other end.
fn fake() -> (Find, rtrb::Consumer<TTYPort>) {
    let (mut ends, test_ends) = rtrb::RingBuffer::new(8);
    let find: Find = Box::new(move || {
        let (mut ours, port) = TTYPort::pair()?;
        never_wait(&port)?;
        ours.set_timeout(Duration::from_secs(2))?;
        ends.push(ours)
            .map_err(|_| std::io::Error::other("too many ports"))?;
        Ok(port)
    });
    (find, test_ends)
}

/// The next whole message on the test's end.
fn read_message(end: &mut TTYPort) -> [u8; MESSAGE] {
    let mut message = [0; MESSAGE];
    end.read_exact(&mut message).expect("a whole message");
    message
}

/// Takes the frame once the link is free: a busy link drops it, so try again until it is accepted.
fn take_when_free(device: &mut DmxUsbPro, frame: &[Levels]) {
    let started = Instant::now();
    while device.take(frame) != Outcome::Accepted {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "never accepted"
        );
        thread::sleep(Duration::from_millis(1)); // poll
    }
}

#[test]
fn every_frame_is_one_send_dmx_message() {
    let (find, mut ends) = fake();
    let mut device = DmxUsbPro::open(find).unwrap();
    let mut end = ends.pop().unwrap();
    // (frame, channel 1, channel 512)
    let cases = [
        (vec![Levels([0; 512])], 0, 0),
        (
            vec![Levels(std::array::from_fn(|n| (n % 256) as u8))],
            0,
            255,
        ),
        (vec![Levels([200; 512]), Levels([1; 512])], 200, 200),
    ];
    for (frame, first, last) in cases {
        take_when_free(&mut device, &frame);
        let message = read_message(&mut end);
        assert_eq!(
            message[..5],
            [0x7E, 6, 0x01, 0x02, 0],
            "header, start code 0"
        );
        assert_eq!((message[5], message[5 + 511]), (first, last));
        assert_eq!(message[MESSAGE - 1], 0xE7, "end of message");
    }
}

#[test]
fn an_unplugged_port_fails_until_the_port_thread_opens_a_new_one() {
    let (find, mut ends) = fake();
    let mut device = DmxUsbPro::open(find).unwrap();
    let frame = [Levels([42; 512])];
    take_when_free(&mut device, &frame);

    drop(ends.pop().unwrap()); // unplugged
    let mut seen = vec![];
    let started = Instant::now();
    while seen.last() != Some(&Outcome::Accepted) || !seen.contains(&Outcome::Failed) {
        assert!(started.elapsed() < Duration::from_secs(10), "{seen:?}");
        thread::sleep(Duration::from_millis(25)); // poll
        let outcome = device.take(&frame);
        if seen.last() != Some(&outcome) {
            seen.push(outcome);
        }
    }
    // A full driver queue may drop a frame on the way; never a wait.
    seen.retain(|outcome| *outcome != Outcome::Dropped);
    seen.dedup();
    // A frame may still land just after the unplug, before the port says so.
    if seen.first() == Some(&Outcome::Accepted) {
        seen.remove(0);
    }
    assert_eq!(seen, [Outcome::Failed, Outcome::Accepted]);
    let mut end = ends.pop().expect("the finder opened a new port");
    assert_eq!(read_message(&mut end)[5], 42);
}

#[test]
fn a_port_that_takes_no_bytes_is_lost_then_found_again() {
    // Nobody reads the other end: the driver queue fills and stays full. `take` never
    // waits on it. After the deadline the port counts as lost; once the other end is read
    // again, the finder gives the port back and frames are taken again.
    let (mut end, port) = TTYPort::pair().unwrap();
    never_wait(&port).unwrap();
    end.set_timeout(Duration::from_secs(1)).unwrap();
    let reading = Arc::new(AtomicBool::new(false));
    let find: Find = {
        let reading = Arc::clone(&reading);
        let mut first = true;
        Box::new(move || {
            if !std::mem::take(&mut first) && !reading.load(Ordering::Relaxed) {
                return Err(std::io::Error::other("still stuck"));
            }
            port.try_clone_native().map_err(std::io::Error::from)
        })
    };
    let device = DmxUsbPro::open(find).unwrap();
    let (done, seen) = mpsc::channel();
    let taking = {
        let reading = Arc::clone(&reading);
        thread::spawn(move || {
            let mut device = device;
            let frame = [Levels([1; 512])];
            let mut seen: Vec<Outcome> = vec![];
            while !(seen.contains(&Outcome::Failed) && seen.last() == Some(&Outcome::Accepted)) {
                let outcome = device.take(&frame);
                if seen.last() != Some(&outcome) {
                    seen.push(outcome);
                }
                if outcome == Outcome::Failed {
                    reading.store(true, Ordering::Relaxed);
                }
                thread::yield_now();
            }
            let _ = done.send(seen);
        })
    };
    let drain = thread::spawn(move || {
        let mut bytes = [0; MESSAGE];
        while !reading.load(Ordering::Relaxed) {
            thread::yield_now();
        }
        while end.read(&mut bytes).is_ok_and(|read| read > 0) {}
    });
    let seen = seen
        .recv_timeout(Duration::from_secs(10))
        .expect("the stalled port was never lost and found again, or take waited on it");
    assert_eq!(seen.first(), Some(&Outcome::Accepted), "{seen:?}");
    assert!(
        seen.ends_with(&[Outcome::Dropped, Outcome::Failed, Outcome::Accepted]),
        "{seen:?}"
    );
    let failed = seen.iter().filter(|o| **o == Outcome::Failed).count();
    assert_eq!(failed, 1, "{seen:?}");
    taking.join().unwrap();
    drop(drain); // ends by itself once the port is closed and nothing more comes
}

#[test]
fn a_loss_throws_away_no_accepted_frame_the_last_goes_out_first_on_the_new_port() {
    let (find, mut ends) = fake();
    let mut device = DmxUsbPro::open(find).unwrap();
    take_when_free(&mut device, &[Levels([1; 512])]);

    drop(ends.pop().unwrap()); // unplugged
    let (mut level, mut last_accepted, mut failed) = (1_u8, 1, false);
    let started = Instant::now();
    loop {
        assert!(started.elapsed() < Duration::from_secs(10), "never back");
        thread::sleep(Duration::from_millis(25)); // poll
        level = level.wrapping_add(1).max(2);
        match device.take(&[Levels([level; 512])]) {
            Outcome::Accepted if failed => break,
            Outcome::Accepted => last_accepted = level,
            Outcome::Failed => failed = true,
            Outcome::Dropped => {}
        }
    }
    let mut end = ends.pop().expect("the finder opened a new port");
    // (message on the new port, level on channel 1)
    let cases = [(1, last_accepted), (2, level)];
    for (n, expected) in cases {
        assert_eq!(read_message(&mut end)[5], expected, "message {n}");
    }
}

#[test]
fn a_stop_in_the_middle_of_a_message_finishes_it() {
    let (find, mut ends) = fake();
    let mut device = DmxUsbPro::open(find).unwrap();
    let mut end = ends.pop().unwrap();
    // Nobody reads the other end: the driver queue fills, and the port thread waits in
    // the middle of a message. Then `take` drops every frame.
    let (mut dropped, started) = (0, Instant::now());
    while dropped < 20 {
        assert!(started.elapsed() < Duration::from_secs(10), "never full");
        dropped = match device.take(&[Levels([7; 512])]) {
            Outcome::Dropped => dropped + 1,
            Outcome::Accepted | Outcome::Failed => 0,
        };
        thread::sleep(Duration::from_millis(5)); // poll
    }
    let stopping = thread::spawn(move || drop(device));
    // Once the port closes, what it had queued is gone: a stop that did not wait leaves
    // nothing to read.
    let mut bytes = 0;
    let mut chunk = [0; 4096];
    while let Ok(read @ 1..) = end.read(&mut chunk) {
        bytes += read;
    }
    stopping.join().unwrap();
    assert!(
        bytes > 0 && bytes % MESSAGE == 0,
        "{bytes} bytes: not whole messages"
    );
}
