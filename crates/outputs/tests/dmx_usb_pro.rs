//! A DMX USB Pro against a fake serial port: a pseudo terminal. The test reads the other
//! end and checks every byte; "unplugging" closes that end, and the port thread opens a
//! new port by itself.

#![expect(
    clippy::disallowed_methods,
    reason = "the test waits between frames; it is not the output thread"
)]

mod common;

use common::{Fake, NEVER, read_message};
use outputs::dmx_usb_pro::{DmxUsbPro, FINISH_WITHIN, Find, MESSAGE, never_wait};
use outputs::{Device, Outcome};
use serialport::TTYPort;
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

/// Takes the frame once the link is free: a busy link drops it, so try again until it is accepted.
fn take_when_free(device: &mut DmxUsbPro, frame: &[u8]) {
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
    let (find, mut fake) = Fake::new(true);
    let mut device = DmxUsbPro::open(find).unwrap();
    let mut end = fake.next_end();
    // (frame, channel 1, channel 512)
    let cases = [
        (vec![0; 512], 0, 0),
        ((0..512).map(|n| (n % 256) as u8).collect(), 0, 255),
        ([[200; 512], [1; 512]].concat(), 200, 200),
        (vec![7, 8, 9], 7, 0),
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
        let (used, padding) = message[5..5 + 512].split_at(frame.len().min(512));
        assert_eq!(used, &frame[..used.len()]);
        assert!(padding.iter().all(|&slot| slot == 0));
        assert_eq!(message[MESSAGE - 1], 0xE7, "end of message");
    }
}

#[test]
fn a_healthy_route_finishes_its_last_accepted_message_before_the_next_route() {
    let (find, mut fake) = Fake::new(true);
    let mut device = DmxUsbPro::open(find).unwrap();
    let mut end = fake.next_end();
    let received = thread::spawn(move || {
        let mut markers = Vec::new();
        while markers.last() != Some(&91) {
            markers.push(read_message(&mut end)[5]);
        }
        markers
    });

    // The serial worker has taken the first message. The last one can still be
    // in its one-slot queue while the driver sends the first.
    for marker in [11, 73] {
        take_when_free(&mut device, &[marker]);
    }
    device.finish_route();
    take_when_free(&mut device, &[91]);
    assert_eq!(received.join().unwrap(), [11, 73, 91]);
}

#[test]
fn an_unplugged_port_fails_until_the_port_thread_opens_a_new_one() {
    let (find, mut fake) = Fake::new(true);
    let mut device = DmxUsbPro::open(find).unwrap();
    let frame = [42; 512];
    take_when_free(&mut device, &frame);

    drop(fake.next_end()); // unplugged
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
    let mut end = fake.next_end();
    assert_eq!(read_message(&mut end)[5], 42);
}

#[test]
fn a_port_that_takes_no_bytes_is_lost_then_found_again() {
    // Nobody reads the other end: the driver queue fills and stays full. `take` never
    // waits on it. After the deadline the port counts as lost; once the other end is read
    // again, the finder gives the port back and frames are taken again.
    let (end, port) = TTYPort::pair().unwrap();
    let mut end = common::end(end).unwrap();
    never_wait(&port).unwrap();
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
            let frame = [1; 512];
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
        .recv_timeout(NEVER)
        .expect("the stalled port was never lost and found again, or take waited on it");
    assert_eq!(seen.first(), Some(&Outcome::Accepted), "{seen:?}");
    let (before, after) = seen.split_at(seen.iter().position(|o| *o == Outcome::Failed).unwrap());
    assert_eq!(before.last(), Some(&Outcome::Dropped), "stalled: {seen:?}");
    // Found again, the frame queued before the stall may still be on its way: busy.
    let after: Vec<_> = after.iter().filter(|o| **o != Outcome::Dropped).collect();
    assert_eq!(after, [&Outcome::Failed, &Outcome::Accepted], "{seen:?}");
    taking.join().unwrap();
    drop(drain); // ends by itself once the port is closed and nothing more comes
}

/// Takes a frame with a new level on channel 1 each time until the port thread waits in
/// the middle of a message, one more queued (nobody reads the port): `take` drops 20
/// frames in a row. A runner that pauses past the 500 ms a stall may take loses the port
/// on the way; the port thread opens a new one, which fills the same way, and `end`
/// becomes its end. Returns the levels accepted, in order.
fn take_until_stuck(device: &mut DmxUsbPro, fake: &mut Fake, end: &mut TTYPort) -> Vec<u8> {
    let (mut accepted, mut dropped, started) = (vec![], 0, Instant::now());
    while dropped < 20 {
        assert!(started.elapsed() < NEVER, "never stuck"); // guard
        let level = accepted.last().map_or(1, |last: &u8| last.wrapping_add(1));
        dropped = match device.take(&[level; 512]) {
            Outcome::Accepted => {
                accepted.push(level);
                0
            }
            Outcome::Dropped => dropped + 1,
            Outcome::Failed => 0,
        };
        while let Some(newer) = fake.new_end() {
            *end = newer;
        }
        thread::sleep(Duration::from_millis(5)); // poll
    }
    accepted
}

#[test]
fn a_loss_throws_away_no_accepted_frame_they_go_out_first_on_the_new_port_in_order() {
    let (find, mut fake) = Fake::new(true);
    let mut device = DmxUsbPro::open(find).unwrap();
    let mut end = fake.next_end();
    let accepted = take_until_stuck(&mut device, &mut fake, &mut end);
    // The one being written, and the one queued behind it.
    let unsent = accepted.get(accepted.len() - 2..).unwrap().to_vec();

    drop(end); // unplugged
    let (mut failed, started) = (false, Instant::now());
    loop {
        assert!(started.elapsed() < NEVER, "never back"); // guard
        match device.take(&[0; 512]) {
            Outcome::Accepted if failed => break,
            Outcome::Failed => failed = true,
            Outcome::Accepted | Outcome::Dropped => {}
        }
        thread::sleep(Duration::from_millis(5)); // poll
    }
    let mut end = fake.next_end();
    let sent: Vec<u8> = (0..2).map(|_| read_message(&mut end)[5]).collect();
    assert_eq!(sent, unsent, "accepted {accepted:?}");
}

#[test]
fn a_stop_in_the_middle_of_a_message_finishes_it() {
    // The case needs the test to read within the 500 ms a stall may take. A runner that
    // pauses past it loses the port first (a stall's loss is a time out): the case was
    // not reached, so it is set up again. Any other loss fails.
    let started = Instant::now();
    let bytes = loop {
        assert!(started.elapsed() < NEVER, "every stop came after a loss"); // guard
        let (find, mut fake) = Fake::new(true);
        let mut device = DmxUsbPro::open(find).unwrap();
        let lost_with = device.lost_with();
        let mut end = fake.next_end();
        take_until_stuck(&mut device, &mut fake, &mut end);
        let stopping = thread::spawn(move || drop(device));
        // Once the port closes, what it had queued is gone: a stop that did not wait
        // leaves nothing to read.
        let mut bytes = 0;
        let mut chunk = [0; 4096];
        while let Ok(read @ 1..) = end.read(&mut chunk) {
            bytes += read;
        }
        stopping.join().unwrap();
        match lost_with.get() {
            None => break bytes,
            Some(lost) => assert_eq!(lost.kind(), io::ErrorKind::TimedOut, "{lost}"),
        }
    };
    assert!(
        bytes > 0 && bytes % MESSAGE == 0,
        "{bytes} bytes: not whole messages"
    );
}

/// Expected kinds: open(2) (ENOENT: no such file, the adapter is absent; EACCES: no
/// permission) and TIOCEXCL in tty_ioctl(4) and termios(4) (EBUSY: another program holds
/// the port). An absent adapter keeps no cause; a refusal is kept, also on a later look.
#[test]
fn a_find_error_shows_why_an_adapter_that_is_there_cannot_open() {
    use nix::errno::Errno;
    use std::io::ErrorKind;
    // (find answers at open, then on every look, the cause kept)
    let cases = [
        (Errno::ENOENT, Errno::ENOENT, None),
        (
            Errno::EACCES,
            Errno::EACCES,
            Some(ErrorKind::PermissionDenied),
        ),
        (Errno::EBUSY, Errno::EBUSY, Some(ErrorKind::ResourceBusy)),
        (
            Errno::ENOENT,
            Errno::EACCES,
            Some(ErrorKind::PermissionDenied),
        ),
    ];
    for (at_open, on_look, expected) in cases {
        let looks = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let find: Find = {
            let looks = Arc::clone(&looks);
            Box::new(move || {
                let error = if looks.fetch_add(1, Ordering::Relaxed) == 0 {
                    at_open
                } else {
                    on_look
                };
                Err(error.into())
            })
        };
        let mut device = DmxUsbPro::open(find).unwrap();
        assert_eq!(device.take(&[0; 512]), Outcome::Failed, "{at_open:?}");
        let started = Instant::now();
        while looks.load(Ordering::Relaxed) < 2 {
            assert!(started.elapsed() < NEVER, "never looked again"); // guard
            thread::sleep(Duration::from_millis(5)); // poll
        }
        let kept = device.lost_with().get().map(|cause| cause.kind());
        assert_eq!(kept, expected, "{at_open:?} then {on_look:?}");
    }
}

#[test]
fn a_route_finished_while_lost_never_replays_the_old_route_on_the_found_port() {
    let (find, mut fake) = Fake::new(true);
    let mut device = DmxUsbPro::open(find).unwrap();
    let mut end = fake.next_end();
    take_until_stuck(&mut device, &mut fake, &mut end);
    fake.plugged().store(false, Ordering::Relaxed);
    drop(end); // unplugged with a message unsent and one queued
    let started = Instant::now();
    while device.take(&[0; 512]) != Outcome::Failed {
        assert!(started.elapsed() < NEVER, "never lost"); // guard
        thread::sleep(Duration::from_millis(5)); // poll
    }
    device.finish_route();
    fake.plugged().store(true, Ordering::Relaxed);
    take_when_free(&mut device, &[77]);
    let mut end = fake.next_end();
    assert_eq!(
        read_message(&mut end)[5],
        77,
        "an old-route message came first"
    );
}

#[test]
fn a_route_finish_on_a_port_that_takes_no_byte_ends_within_its_bound() {
    let (find, mut fake) = Fake::new(true);
    let mut device = DmxUsbPro::open(find).unwrap();
    let lost_with = device.lost_with();
    let mut end = fake.next_end();
    take_until_stuck(&mut device, &mut fake, &mut end);
    fake.plugged().store(false, Ordering::Relaxed);
    let started = Instant::now();
    device.finish_route();
    let took = started.elapsed();
    assert!(took <= FINISH_WITHIN, "{took:?} > {FINISH_WITHIN:?}");
    assert_eq!(device.take(&[0; 512]), Outcome::Failed);
    let cause = lost_with.get().map(|cause| cause.kind());
    assert_eq!(cause, Some(io::ErrorKind::TimedOut));
    drop(end);
}

#[test]
fn a_finish_the_opener_gave_up_on_still_ends_before_the_new_route_is_written() {
    // The port listing hangs past FINISH_WITHIN: the opener gives up. Once the port is
    // found, the port thread still finishes the old route first; the first message on
    // the wire is the new route's.
    let (mut fake_find, mut fake) = Fake::new(true);
    let (entered, in_find) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let mut looks = 0;
    let find: Find = Box::new(move || {
        looks += 1;
        match looks {
            1 => return Err(io::ErrorKind::NotFound.into()),
            2 => {
                let _ = entered.send(());
                let _ = released.recv();
            }
            _ => {}
        }
        fake_find()
    });
    let mut device = DmxUsbPro::open(find).unwrap();
    in_find.recv_timeout(NEVER).expect("never looked again");
    device.finish_route(); // gives up: the look hangs
    release.send(()).unwrap();
    let (mut seen, started) = (vec![], Instant::now());
    while seen.last() != Some(&Outcome::Accepted) {
        assert!(started.elapsed() < NEVER, "{seen:?}"); // guard
        let outcome = device.take(&[5; 512]);
        if seen.last() != Some(&outcome) {
            seen.push(outcome);
        }
    }
    // `take` drops while the finish is open (the gate table walks that); whether a
    // frame came in that short time is up to the scheduler.
    seen.retain(|outcome| *outcome != Outcome::Dropped);
    assert_eq!(seen, [Outcome::Failed, Outcome::Accepted]);
    let mut end = fake.next_end();
    assert_eq!(read_message(&mut end)[5], 5);
}
