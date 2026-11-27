//! A DMX USB Pro against a fake serial port: a pseudo terminal. The test reads the other
//! end and checks every byte; "unplugging" closes that end, and the port thread opens a
//! new port by itself.

#![expect(
    clippy::disallowed_methods,
    reason = "the test waits between frames; it is not the output thread"
)]

mod common;

use common::{ASKED, Fake, NEVER, read_message};
use outputs::dmx_usb_pro::{DmxUsbPro, FINISH_WITHIN, Find, MESSAGE, Settings, never_wait};
use outputs::{Device, Outcome};
use serialport::{SerialPort, TTYPort};
use std::io::{self, Read, Write};
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
    common::answer_ask(&mut end);
    settings_read(&device);
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
        // The port found again asks label 3 behind what the stall left: answer it.
        let mut last = Vec::new();
        while let Ok(read) = end.read(&mut bytes) {
            if read == 0 {
                break;
            }
            last.extend_from_slice(&bytes[..read]);
            if last.windows(ASKED.len()).any(|window| window == ASKED) {
                end.write_all(&[0x7E, 3, 5, 0, 44, 1, 9, 1, 40, 0xE7])
                    .unwrap();
                last.clear();
            }
            last.drain(..last.len().saturating_sub(ASKED.len()));
        }
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

/// What [`take_until_stuck`] found.
struct Stuck {
    /// The levels accepted, in order.
    accepted: Vec<u8>,
    /// The port thread's pace, in polls: the longest run of drops between two accepted
    /// frames on the current port while it was healthy (it takes the queued frame once a
    /// message is out).
    pace: u32,
    /// The port was lost on the way: `lost_with` already holds a time out.
    lost: bool,
}

/// Takes a frame with a new level on channel 1 each time until the port thread waits in
/// the middle of a message, one more queued (nobody reads the port). No clock: while the
/// port is healthy, `take` drops only until the port thread took the queued frame, and the
/// longest such run of drops is its pace, counted in polls (0 on a runner that polls
/// slower than a link time). Once a healthy cycle was seen (3 frames accepted), a run of
/// drops more than three times the pace, at least 6 polls, is a write that waits. A slow runner polls less often
/// and so counts a shorter pace: the wait stays a few link times, far inside the 500 ms a
/// stall may take. A runner that pauses past that loses the port on the way; the port
/// thread opens a new one, which fills the same way, and `end` becomes its end. The new
/// port counts its pace again from its first accepted frame: one long run (a late port
/// thread, a runner that paused through a stall) raises the pace of one port only, never
/// above what every later stall can reach before it is lost.
fn take_until_stuck(device: &mut DmxUsbPro, fake: &mut Fake, end: &mut TTYPort) -> Stuck {
    let (mut accepted, mut pace, mut run, started) = (vec![], 0, 0, Instant::now());
    // (the port was lost on the way, `pace` was counted on the current port)
    let (mut lost, mut paced_here) = (false, true);
    loop {
        assert!(started.elapsed() < NEVER, "never stuck"); // guard
        while let Some(mut newer) = fake.new_end() {
            common::answer_ask(&mut newer); // the widget of the port found again
            *end = newer;
            (lost, paced_here, run) = (true, false, 0);
        }
        let level = accepted.last().map_or(1, |last: &u8| last.wrapping_add(1));
        match device.take(&[level; 512]) {
            Outcome::Accepted => {
                accepted.push(level);
                pace = if paced_here { pace.max(run) } else { run };
                (paced_here, run) = (true, 0);
            }
            Outcome::Dropped => run += 1,
            Outcome::Failed => (lost, paced_here, run) = (true, false, 0),
        }
        if accepted.len() >= 3 && run > 3 * pace.max(2) {
            return Stuck {
                accepted,
                pace,
                lost,
            };
        }
        thread::sleep(Duration::from_millis(1)); // poll
    }
}

#[test]
fn a_loss_throws_away_no_accepted_frame_they_go_out_first_on_the_new_port_in_order() {
    let (find, mut fake) = Fake::new(true);
    let mut device = DmxUsbPro::open(find).unwrap();
    let mut end = fake.next_end();
    let accepted = take_until_stuck(&mut device, &mut fake, &mut end).accepted;
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
fn a_stop_in_the_middle_of_a_message_finishes_it_and_sends_what_the_driver_holds() {
    // The port thread waits in the middle of a message. The stop begins; the test reads
    // one message, which lets the rest of the stalled one in, and reads on. Every message
    // arrives whole and in order. On macOS the test first pauses three times the port
    // thread's pace, well inside the 500 ms a drain may wait: the drain waits for the
    // read, so every accepted message but the queued one arrives (the queued one may). A
    // stop that closed without waiting for the driver to empty loses some. A Linux pseudo
    // terminal reports no bytes held, so there the drain cannot wait and is not checked.
    // A runner that pauses past the 500 ms a stall may take loses the port first (a time
    // out): the case was not reached, so it is set up again. Any other loss fails.
    let checks_drain = cfg!(target_os = "macos");
    let started = Instant::now();
    let (accepted, levels) = loop {
        assert!(started.elapsed() < NEVER, "every stop came after a loss"); // guard
        let (find, mut fake) = Fake::silent(true);
        let mut device = DmxUsbPro::open(find).unwrap();
        let lost_with = device.lost_with();
        let mut end = fake.next_end();
        // The widget answers at once: no ask holds the port thread while its pace is counted.
        read_ask(&mut end);
        end.write_all(&answer(9, 1, 40)).unwrap();
        settings_read(&device);
        let Stuck {
            accepted,
            pace,
            lost,
        } = take_until_stuck(&mut device, &mut fake, &mut end);
        if lost {
            continue; // the case was not reached
        }
        let stopping = thread::spawn(move || drop(device));
        let mut levels = vec![];
        let mut message = [0; MESSAGE];
        if end.read_exact(&mut message).is_ok() {
            levels.push(message[5]);
            // The port thread has finished the message and reached the drain, which
            // waits for this read. A stop that closed early is done.
            for _ in 0..if checks_drain { 3 * pace.max(2) } else { 0 } {
                if stopping.is_finished() {
                    break;
                }
                thread::sleep(Duration::from_millis(1)); // poll
            }
            while end.read_exact(&mut message).is_ok() {
                levels.push(message[5]);
            }
        }
        stopping.join().unwrap();
        match lost_with.get() {
            None => break (accepted, levels),
            Some(lost) => assert_eq!(lost.kind(), io::ErrorKind::TimedOut, "{lost}"),
        }
    };
    let in_order = !levels.is_empty() && accepted.starts_with(&levels);
    assert!(in_order, "sent {levels:?}, accepted {accepted:?}");
    if checks_drain {
        // The queued message goes out too when the port thread takes it before it sees
        // the stop; nothing before it may be missing.
        let all_but_queued = accepted.get(..accepted.len() - 1).unwrap();
        assert!(
            levels == all_but_queued || levels == accepted,
            "sent {levels:?}, accepted {accepted:?}"
        );
    }
}

/// Expected kinds: open(2) (ENOENT: no such file, the adapter is absent; EACCES: no
/// permission) and TIOCEXCL in tty_ioctl(4) and termios(4) (EBUSY: another program holds
/// the port). An absent adapter keeps no cause; a refusal is kept, also on a later look.
#[test]
fn a_find_error_shows_why_an_adapter_that_is_there_cannot_open() {
    use nix::errno::Errno;
    use std::io::ErrorKind;
    let (denied, busy) = (
        Some(ErrorKind::PermissionDenied),
        Some(ErrorKind::ResourceBusy),
    );
    // (find answers at open, then on every look, the cause kept at open, then after a look)
    let cases = [
        (Errno::ENOENT, Errno::ENOENT, None, None),
        (Errno::EACCES, Errno::EACCES, denied, denied),
        (Errno::EBUSY, Errno::EBUSY, busy, busy),
        (Errno::ENOENT, Errno::EACCES, None, denied),
    ];
    for (at_open, on_look, kept_at_open, expected) in cases {
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
        // The port thread's first look is a look time away: this is the open's own cause.
        let kept = device.lost_with().get().map(|cause| cause.kind());
        assert_eq!(kept, kept_at_open, "{at_open:?}, at open");
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

/// A widget's answer to label 3 (API p.3): firmware 1.44, break, mark after break, rate.
fn answer(break_steps: u8, mark_steps: u8, rate: u8) -> [u8; 10] {
    [0x7E, 3, 5, 0, 44, 1, break_steps, mark_steps, rate, 0xE7]
}

/// Reads the next label 3 request on the test's end.
fn read_ask(end: &mut TTYPort) {
    let mut asked = [0; ASKED.len()];
    end.read_exact(&mut asked).expect("a request");
    assert_eq!(asked, ASKED, "label 3, no user memory");
}

/// The settings the device read, once it read some.
fn settings_read(device: &DmxUsbPro) -> Settings {
    let reading = device.reading();
    let started = Instant::now();
    loop {
        if let Some(settings) = reading.settings() {
            return settings;
        }
        assert!(started.elapsed() < NEVER, "never read"); // guard
        thread::sleep(Duration::from_millis(1)); // poll
    }
}

/// The Ask table of `docs/design/outputs.md`, on the wire: the widget's answer to each try
/// the port thread makes, what the device keeps, and that a frame's message comes next:
/// never a third try. Read asks no more on this port, also after a route finish.
#[test]
fn the_port_thread_asks_the_widget_its_settings_at_most_twice_before_its_first_message() {
    let in_range = Some(answer(20, 4, 30));
    let garbled = Some([0x7E, 6, 5, 0, 44, 1, 20, 4, 30, 0xE7]);
    let twenty = Settings::new(20, 4, 30).unwrap();
    // (case, the answer to each try)
    let cases: [(&str, &[Option<[u8; 10]>]); 4] = [
        ("answers try 1", &[in_range]),
        ("ignores try 1, answers try 2", &[None, in_range]),
        (
            "out of range, then in range",
            &[Some(answer(8, 4, 30)), in_range],
        ),
        ("garbled, then in range", &[garbled, in_range]),
    ];
    for (case, answers) in cases {
        let (find, mut fake) = Fake::silent(true);
        let mut device = DmxUsbPro::open(find).unwrap();
        let mut end = fake.next_end();
        for answered in answers {
            read_ask(&mut end);
            if let Some(answered) = answered {
                end.write_all(answered).unwrap();
            }
        }
        assert_eq!(settings_read(&device), twenty, "{case}");
        take_when_free(&mut device, &[7; 512]);
        let mut message = [0; MESSAGE];
        end.read_exact(&mut message).unwrap();
        assert_eq!(
            message[..6],
            [0x7E, 6, 0x01, 0x02, 0, 7],
            "{case}: label 6 next"
        );
        device.finish_route();
        take_when_free(&mut device, &[8; 512]);
        end.read_exact(&mut message).unwrap();
        let next = [0x7E, 6, 0x01, 0x02, 0, 8];
        assert_eq!(message[..6], next, "{case}: no ask after a finish");
        assert_eq!(
            device.reading().settings(),
            Some(twenty),
            "{case}, after a finish"
        );
    }
}

/// #293, the Ask table's Refused cell: two tries that hear no answer in range close the
/// port. Not a DMX USB Pro: no label 6 ever reaches it, `take` fails, the cause says why,
/// and the adapter is never looked for again (the finder is called no more).
#[test]
fn a_widget_that_never_answers_in_range_is_refused_as_not_a_dmx_usb_pro() {
    // (case, the answer to each try)
    let cases: [(&str, &[Option<[u8; 10]>]); 2] = [
        ("never answers", &[None, None]),
        (
            "out of range twice",
            &[Some(answer(20, 0, 30)), Some(answer(20, 4, 41))],
        ),
    ];
    for (case, answers) in cases {
        let (find, mut fake) = Fake::silent(true);
        let mut device = DmxUsbPro::open(find).unwrap();
        let mut end = fake.next_end();
        assert_eq!(device.take(&[7; 512]), Outcome::Dropped, "{case}: asking");
        for answered in answers {
            read_ask(&mut end);
            if let Some(answered) = answered {
                end.write_all(answered).unwrap();
            }
        }
        let started = Instant::now();
        while device.take(&[7; 512]) != Outcome::Failed {
            assert!(started.elapsed() < NEVER, "{case}: never refused"); // guard
            thread::sleep(Duration::from_millis(1)); // poll
        }
        let why = device.lost_with().get().expect("a cause");
        assert_eq!(why.to_string(), "not a DMX USB Pro", "{case}");
        assert_eq!(device.reading().settings(), None, "{case}");
        // The port is closed: its end reads the end of the file (or EIO), never waits out a
        // time out, and no byte more came.
        let mut rest = Vec::new();
        let read = end.read_to_end(&mut rest);
        let closed = read
            .as_ref()
            .map_or_else(|e| e.kind() != io::ErrorKind::TimedOut, |_| true);
        assert!(closed, "{case}: the port is still open: {read:?}");
        assert!(rest.is_empty(), "{case}: {rest:?}");
        // Looked for every 500 ms if it were: three looks' time, no new port.
        thread::sleep(Duration::from_millis(1_600));
        assert!(fake.new_end().is_none(), "{case}: looked for again");
        assert_eq!(device.take(&[7; 512]), Outcome::Failed, "{case}");
    }
}

/// A widget that sends DMX it received (label 5, input mode) while it is asked is still
/// heard: its answer before or after a message longer than the answer buffer, in one write.
#[test]
fn an_answer_beside_a_long_label_5_message_is_read() {
    // label 5: status 0, start code 0, 512 slots (API p.4)
    let [lsb, msb] = 514_u16.to_le_bytes();
    let received = [&[0x7E, 5, lsb, msb, 0, 0][..], &[0x55; 512], &[0xE7]].concat();
    let answered = answer(20, 4, 30).to_vec();
    // (case, what the widget writes)
    let cases = [
        (
            "label 5, then the answer",
            [received.clone(), answered.clone()].concat(),
        ),
        ("the answer, then label 5", [answered, received].concat()),
    ];
    for (case, written) in cases {
        let (find, mut fake) = Fake::silent(true);
        let device = DmxUsbPro::open(find).unwrap();
        let mut end = fake.next_end();
        read_ask(&mut end);
        end.write_all(&written).unwrap();
        assert_eq!(
            Some(settings_read(&device)),
            Settings::new(20, 4, 30),
            "{case}"
        );
    }
}

/// A lost port clears what its widget answered; the port found again is asked again.
#[test]
fn a_port_found_again_is_asked_again_and_keeps_nothing_until_it_answers() {
    let (find, mut fake) = Fake::silent(true);
    let mut device = DmxUsbPro::open(find).unwrap();
    let mut end = fake.next_end();
    read_ask(&mut end);
    end.write_all(&answer(20, 4, 30)).unwrap();
    assert_eq!(Some(settings_read(&device)), Settings::new(20, 4, 30));

    drop(end); // unplugged
    let frame = [42; 512];
    let started = Instant::now();
    while device.take(&frame) != Outcome::Failed {
        assert!(started.elapsed() < NEVER, "never lost"); // guard
        thread::sleep(Duration::from_millis(5)); // poll
    }
    let mut end = fake.next_end();
    read_ask(&mut end);
    assert_eq!(
        device.reading().settings(),
        None,
        "nothing kept from the lost port"
    );
    end.write_all(&answer(127, 127, 0)).unwrap();
    assert_eq!(Some(settings_read(&device)), Settings::new(127, 127, 0));
}

/// A dropped device reads nothing.
#[test]
fn a_dropped_device_reads_nothing() {
    let (find, mut fake) = Fake::silent(true);
    let device = DmxUsbPro::open(find).unwrap();
    let mut end = fake.next_end();
    read_ask(&mut end);
    end.write_all(&answer(20, 4, 30)).unwrap();
    let reading = device.reading();
    assert_eq!(Some(settings_read(&device)), Settings::new(20, 4, 30));
    assert!(reading.is_open());
    drop(device);
    assert!(!reading.is_open());
    assert_eq!(reading.settings(), None);
}

/// DMX the widget received (label 5), then its answer, framed by length (#353, finding
/// OUT-02 of #349): a `0x7E` in the answer's settings is data, not a start, also where
/// the answer crosses the end of a full answer buffer.
#[test]
fn a_0x7e_in_an_answer_after_label_5_is_data_not_a_start() {
    // (case, label 5 data length, the answer, its settings)
    let cases = [
        (
            "firmware 1.126 with factory default settings (9, 1, 40)",
            54_usize,
            [0x7E, 3, 5, 0, 126, 1, 9, 1, 40, 0xE7],
            Settings::new(9, 1, 40).unwrap(),
        ),
        (
            "firmware 1.44 with valid break_steps 126 (126, 1, 40)",
            52_usize,
            [0x7E, 3, 5, 0, 44, 1, 126, 1, 40, 0xE7],
            Settings::new(126, 1, 40).unwrap(),
        ),
        (
            "firmware 1.126 where the answer crosses the end of a full answer buffer",
            595_usize,
            [0x7E, 3, 5, 0, 126, 1, 9, 1, 40, 0xE7],
            Settings::new(9, 1, 40).unwrap(),
        ),
    ];

    for (case, dmx_in_len, reply, expected) in cases {
        let (find, mut fake) = Fake::silent(true);
        let mut device = DmxUsbPro::open(find).unwrap();
        let lost_with = device.lost_with();
        let reading = device.reading();
        let mut end = fake.next_end();

        let [lsb, msb] = u16::try_from(dmx_in_len).unwrap().to_le_bytes();
        let mut wire = vec![0x7E, 5, lsb, msb];
        wire.resize(4 + dmx_in_len, 0);
        wire.push(0xE7);
        wire.extend_from_slice(&reply);

        read_ask(&mut end);
        end.write_all(&wire).unwrap();

        // A second try is answered the same; a try not asked is waited for 5 ms.
        end.set_timeout(Duration::from_millis(5)).unwrap();
        let started = Instant::now();
        while reading.settings().is_none() && lost_with.get().is_none() {
            let mut extra = [0; ASKED.len()];
            if end.read_exact(&mut extra).is_ok() && extra == ASKED {
                let _ = end.write_all(&wire);
            }
            assert!(started.elapsed() < NEVER, "{case}: ask never settled"); // guard
            thread::sleep(Duration::from_millis(5)); // poll
        }

        assert_eq!(
            lost_with.get().map(|err| err.to_string()),
            None,
            "{case}: valid DMX USB Pro was permanently refused"
        );
        assert_eq!(reading.settings(), Some(expected), "{case}");
        assert_ne!(
            device.take(&[255; 512]),
            Outcome::Failed,
            "{case}: output failed after valid label-3 answers"
        );
    }
}
