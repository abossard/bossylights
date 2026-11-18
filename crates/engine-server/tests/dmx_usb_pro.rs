//! The engine with a DMX USB Pro on a fake serial port (a pseudo terminal): the output
//! runs, goes to reconnecting when its port is "unplugged", and runs again by itself,
//! while another output never fails.

use engine::{Device, Engine, Levels, Outcome, OutputNumber, Rate, RunNumbers, Status, Universes};
use outputs::dmx_usb_pro::{DmxUsbPro, Find, MESSAGE, never_wait};
use serialport::{SerialPort, TTYPort};
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

struct Nowhere;

impl Device for Nowhere {
    fn take(&mut self, _frame: &[Levels]) -> Outcome {
        Outcome::Accepted
    }
}

/// Reads every message on the test's ends of the fake ports; drops the current end when
/// told to unplug.
fn reader(mut ends: rtrb::Consumer<TTYPort>, unplug: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
    let mut end: Option<TTYPort> = None;
    let mut message = [0; MESSAGE];
    while !stop.load(Ordering::Relaxed) {
        if let Ok(new) = ends.pop() {
            end = Some(new);
        }
        if unplug.swap(false, Ordering::Relaxed) {
            end = None;
        }
        match &mut end {
            Some(port) => {
                let _ = port.read(&mut message);
            }
            None => thread::sleep(Duration::from_millis(5)),
        }
    }
}

fn status_until(engine: &Engine, done: impl Fn(Status) -> bool) -> Status {
    let started = Instant::now();
    loop {
        // Output 1 is in the state from the wake after it was attached.
        let status = engine.query().outputs[0].map(|o| o.status);
        if let Some(status) = status.filter(|s| done(*s)) {
            return status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "stuck at {status:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn unplugged_it_reconnects_and_runs_again_by_itself() {
    let (mut give, ends) = rtrb::RingBuffer::new(8);
    let find: Find = Box::new(move || {
        let (mut ours, port) = TTYPort::pair()?;
        never_wait(&port)?;
        ours.set_timeout(Duration::from_millis(20))?;
        give.push(ours)
            .map_err(|_| std::io::Error::other("too many ports"))?;
        Ok(port)
    });
    let (unplug, stop) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let reading = {
        let (unplug, stop) = (Arc::clone(&unplug), Arc::clone(&stop));
        thread::spawn(move || reader(ends, unplug, stop))
    };

    let (engine, mut sender) = Engine::start(Universes::ONE).unwrap();
    let run = RunNumbers { first: 1, count: 1 }
        .check(Universes::ONE)
        .unwrap();
    let pro = Box::new(DmxUsbPro::open(find).unwrap());
    let one = OutputNumber::try_from(1).unwrap();
    let two = OutputNumber::try_from(2).unwrap();
    sender.attach(one, pro, run, Rate::FortyFour).unwrap();
    sender
        .attach(two, Box::new(Nowhere), run, Rate::Sixty)
        .unwrap();

    status_until(&engine, |s| s == Status::Running);
    unplug.store(true, Ordering::Relaxed);
    let lost = status_until(&engine, |s| s != Status::Running);
    assert!(
        matches!(lost, Status::Reconnecting { .. }),
        "lost: {lost:?}"
    );
    status_until(&engine, |s| s == Status::Running);

    let state = engine.query();
    let pro = state.outputs[0].unwrap().counters;
    let other = state.outputs[1].unwrap().counters;
    assert!(pro.failed > 0 && pro.accepted > 0, "{pro:?}");
    assert_eq!(other.failed, 0, "the other output never failed");
    stop.store(true, Ordering::Relaxed);
    reading.join().unwrap();
}
