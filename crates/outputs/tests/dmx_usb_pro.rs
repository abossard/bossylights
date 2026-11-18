//! A DMX USB Pro against a fake serial port: a pseudo terminal. The test reads the other
//! end and checks every byte; "unplugging" closes that end, and the port thread opens a
//! new port by itself.

#![expect(
    clippy::disallowed_methods,
    reason = "the test waits between frames; it is not the output thread"
)]

use outputs::dmx_usb_pro::{DmxUsbPro, Find, LINK_TIME, MESSAGE, never_wait};
use outputs::{Device, Levels, Outcome};
use serialport::{SerialPort, TTYPort};
use std::io::Read;
use std::sync::mpsc;
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
        thread::sleep(LINK_TIME); // the link is free again
        assert_eq!(device.take(&frame), Outcome::Accepted);
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
    thread::sleep(LINK_TIME);
    assert_eq!(device.take(&frame), Outcome::Accepted);

    drop(ends.pop().unwrap()); // unplugged
    let mut seen = vec![];
    let started = Instant::now();
    while seen.last() != Some(&Outcome::Accepted) || !seen.contains(&Outcome::Failed) {
        assert!(started.elapsed() < Duration::from_secs(10), "{seen:?}");
        thread::sleep(Duration::from_millis(25));
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
fn a_port_stuck_in_the_driver_never_holds_up_take() {
    // A port whose writes wait, and nobody reads the other end: the driver queue fills and
    // the port thread waits in `write` for good. A `take` that called the driver would too.
    let (end, port) = TTYPort::pair().unwrap();
    let mut port = Some(port);
    let device = DmxUsbPro::open(Box::new(move || {
        port.take()
            .ok_or_else(|| std::io::Error::other("one port only"))
    }))
    .unwrap();
    let (done, seen) = mpsc::channel();
    thread::spawn(move || {
        let mut device = device;
        let frame = [Levels([1; 512])];
        let mut outcomes = vec![];
        while !outcomes.ends_with(&[Outcome::Dropped; 20]) {
            thread::sleep(Duration::from_millis(5));
            outcomes.push(device.take(&frame));
        }
        let _ = done.send(outcomes);
    });
    let outcomes = seen
        .recv_timeout(Duration::from_secs(10))
        .expect("take came back each time; a hang means it waited on the driver");
    assert_eq!(outcomes.first(), Some(&Outcome::Accepted));
    assert!(!outcomes.contains(&Outcome::Failed), "{outcomes:?}");
    drop(end);
}
