mod mock;

use engine::{Device, Engine, Levels, Outcome, OutputNumber, Rate, RunNumbers, Universes};
use std::thread;
use std::time::Duration;

#[test]
fn with_no_output_the_counter_advances() {
    let (mut engine, _sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    let before = engine.fresh().state.frame;
    mock::wait_until("the counter does not advance", || {
        engine.latest().state.frame >= before + 3
    });
}

#[test]
fn dropping_the_engine_stops_the_tick_thread() {
    let (mut engine, _sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    mock::wait_until("the counter does not advance", || {
        engine.latest().state.frame > 0
    });
    // `drop` joins the tick thread: a thread that never stops hangs the test.
    drop(engine);
}

#[test]
fn every_frame_counts_how_late_its_output_was_served() {
    let (mut engine, mut sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    let output = OutputNumber::try_from(1).unwrap();
    assert_eq!(engine.fresh().lateness(output), None, "no output yet");
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

/// A device that takes at least 3 ms per frame.
struct Slow;

const SLOW: Duration = Duration::from_millis(3);

impl Device for Slow {
    fn take(&mut self, _: &[Levels]) -> Outcome {
        thread::sleep(SLOW); // poll: the device's work. A noisy runner only makes it longer
        Outcome::Accepted
    }
}

#[test]
fn an_output_served_after_a_slow_one_in_its_wake_counts_that_wait_as_late() {
    let (mut engine, mut sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    let run = RunNumbers { first: 1, count: 1 };
    let slow = OutputNumber::try_from(1).unwrap();
    sender
        .attach(slow, Box::new(Slow), run.check(Universes::ONE).unwrap(), Rate::Sixty)
        .unwrap();
    // Same rate: due with output 1, so handed in its wake, after its 3 ms.
    mock::attach(&mut sender, Universes::ONE, 2, run, Rate::Sixty);
    let two = OutputNumber::try_from(2).unwrap();
    mock::wait_until("output 2 counted no frame", || {
        engine.latest().lateness(two).is_some()
    });
    let lateness = engine.latest().lateness(two).expect("frames were counted");
    assert!(lateness.worst >= SLOW, "{lateness:?}");
}
