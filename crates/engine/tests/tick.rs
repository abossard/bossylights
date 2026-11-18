mod mock;

use engine::{Engine, OutputNumber, Rate, RunNumbers, Universes};

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
