mod mock;

use engine::{Device, Engine, Outcome, OutputNumber, Rate, RunNumbers, Status, Universes};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread;
use std::time::Duration;

#[test]
fn with_no_output_the_counter_advances() {
    let (mut engine, _sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    let before = engine.fresh().unwrap().frame();
    mock::wait_until("the counter does not advance", || {
        engine.latest().frame() >= before + 3
    });
}

#[test]
fn a_measured_engine_counts_its_output_wakes_and_reads_its_threads_cpu_by_name() {
    let (_engine, mut sender, timings) =
        Engine::start_measured(Universes::ONE).expect("the tick thread starts");
    let run = RunNumbers { first: 1, count: 1 };
    let heard = mock::attach(&mut sender, Universes::ONE, 1, run, Rate::Sixty);
    heard.two_more_frames();
    // Two, so a count stuck at one is caught too.
    mock::wait_until("no output wakes counted", || timings.output_wakes() >= 2);
    // The names the scorecard reads: read where the system is measured, else None.
    let named = |name| {
        timings
            .cpu()
            .map(|threads| threads.iter().any(|(named, _)| named == name))
    };
    for name in ["tick", "output"] {
        assert_eq!(
            named(name),
            cfg!(target_os = "macos").then_some(true),
            "{name}"
        );
    }
}

#[test]
fn dropping_the_engine_stops_the_tick_thread_and_releases_the_device() {
    let (engine, mut sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    let run = RunNumbers { first: 1, count: 1 };
    let heard = mock::attach(&mut sender, Universes::ONE, 1, run, Rate::Sixty);
    heard.two_more_frames();
    let device = Arc::downgrade(&heard);
    drop(heard);

    drop(engine);
    assert!(
        device.upgrade().is_none(),
        "the stopped output still owns the device"
    );
}

#[test]
fn every_frame_counts_how_late_its_output_was_served() {
    let (mut engine, mut sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    let output = OutputNumber::try_from(1).unwrap();
    assert_eq!(
        engine.fresh().unwrap().lateness(output),
        None,
        "no output yet"
    );
    let run = RunNumbers { first: 1, count: 1 };
    mock::attach(&mut sender, Universes::ONE, 1, run, Rate::FortyFour);
    mock::wait_until("no frame was counted", || {
        engine.latest().lateness(output).is_some()
    });
    let lateness = engine
        .latest()
        .lateness(output)
        .expect("frames were counted");
    assert!(lateness.worst >= lateness.p99, "{lateness:?}");
}

/// What the device of the test below does with each frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Does {
    Takes,
    /// Busy: drops the frame itself.
    Busy,
    /// Unplugged.
    Gone,
    /// Holds the frame until it does something else: a device far slower than a frame.
    Stuck,
}

/// A device that does what the test says, frame by frame.
struct Scripted(Arc<AtomicU8>);

impl Scripted {
    fn does(&self) -> Does {
        match self.0.load(Ordering::Acquire) {
            0 => Does::Takes,
            1 => Does::Busy,
            2 => Does::Gone,
            _ => Does::Stuck,
        }
    }
}

impl Device for Scripted {
    fn take(&mut self, _: &[u8]) -> Outcome {
        loop {
            match self.does() {
                Does::Takes => return Outcome::Accepted,
                Does::Busy => return Outcome::Dropped,
                Does::Gone => return Outcome::Failed,
                Does::Stuck => thread::sleep(Duration::from_millis(1)), // poll
            }
        }
    }
}

/// The status without its count, to compare.
fn kind(status: Status) -> Status {
    match status {
        Status::Reconnecting { .. } => Status::Reconnecting { failures: 1 },
        other @ (Status::NotFound | Status::Connecting | Status::Running) => other,
    }
}

#[test]
fn an_output_says_what_its_device_did_and_a_stuck_one_holds_up_no_other() {
    let (mut engine, mut sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    let run = RunNumbers { first: 1, count: 1 };
    let does = Arc::new(AtomicU8::new(Does::Gone as u8));
    let one = OutputNumber::try_from(1).unwrap();
    let device = Box::new(Scripted(Arc::clone(&does)));
    sender
        .attach(one, device, run.check(Universes::ONE).unwrap(), Rate::Sixty)
        .unwrap();
    // Same rate: due with output 1, so handed over in the same wakes.
    let two = mock::attach(&mut sender, Universes::ONE, 2, run, Rate::Sixty);
    engine.fresh().unwrap();
    let reconnecting = Status::Reconnecting { failures: 1 };
    // (the device does, output 1's status, the counter that grows: accepted, dropped, failed)
    let table = [
        (Does::Gone, Status::NotFound, 2), // never seen
        (Does::Takes, Status::Running, 0),
        (Does::Gone, reconnecting, 2), // unplugged
        (Does::Takes, Status::Running, 0),
        (Does::Busy, Status::Running, 1),  // dropped by the device
        (Does::Stuck, Status::Running, 1), // dropped at the hand-over: its thread is full
        (Does::Takes, Status::Running, 0),
    ];
    for (now, status, grows) in table {
        let counted = |engine: &mut Engine| {
            let output = engine.latest().output(one).expect("output 1 is set");
            let c = output.counters;
            (
                output.connection(),
                [c.accepted, c.dropped, c.failed][grows],
            )
        };
        let (_, before) = counted(&mut engine);
        let heard = two.frames();
        does.store(now as u8, Ordering::Release);
        mock::wait_until(&format!("{now:?}: output 1 is not {status:?}"), || {
            let (found, count) = counted(&mut engine);
            kind(found) == status && count >= before + 3
        });
        mock::wait_until(&format!("{now:?}: output 2 is held up"), || {
            two.frames() >= heard + 3
        });
    }
}
