mod mock;

use engine::{Engine, OutputNumber, Rate, RunNumbers, Universes};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn with_no_output_the_counter_advances() {
    let (engine, _sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    let before = engine.query().frame;
    let started = Instant::now();
    // A hang guard of seconds, not a rate: the speed is measured on a real machine.
    while engine.query().frame < before + 3 {
        assert!(started.elapsed() < Duration::from_secs(10), "the counter does not advance");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn dropping_the_engine_stops_the_tick_thread() {
    let (engine, _sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    thread::sleep(Duration::from_millis(50));
    // `drop` joins the tick thread: a thread that never stops hangs the test.
    drop(engine);
}

#[test]
fn every_frame_counts_how_late_its_output_was_served() {
    let (engine, mut sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    let output = OutputNumber::try_from(1).unwrap();
    assert_eq!(engine.lateness(output), None, "no output yet");
    let run = RunNumbers { first: 1, count: 1 };
    mock::attach(&mut sender, Universes::ONE, 1, run, Rate::FortyFour);
    thread::sleep(Duration::from_millis(250));
    let lateness = engine.lateness(output).expect("frames were counted");
    assert!(
        lateness.p99 > Duration::ZERO,
        "the p99 is a bucket's upper edge: {lateness:?}"
    );
    assert!(lateness.worst >= lateness.p99, "{lateness:?}");
}
