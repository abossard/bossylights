use engine::{BEAT, Command, Engine, OutputKind, OutputNumber, RunNumbers, Universes};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn with_no_output_the_counter_advances_on_the_beat() {
    // how long we watch, in ms
    for ms in [250, 500, 1000] {
        let (engine, _sender) =
            Engine::start(Universes::ONE).expect("the tick thread starts");
        let before = engine.query().frame;
        let started = Instant::now();
        thread::sleep(Duration::from_millis(ms));
        let ticks = engine.query().frame - before;
        // Count against the time that really passed: CI machines oversleep.
        let expected = (started.elapsed().as_secs_f64() * (1.0 / BEAT.as_secs_f64())) as u64;
        let slack = 2 + expected / 10;
        assert!(
            ticks.abs_diff(expected) <= slack,
            "{ms} ms: expected {expected} frames (±{slack}), got {ticks}"
        );
    }
}

#[test]
fn dropping_the_engine_stops_the_tick_thread_quickly() {
    let (engine, _sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    drop(engine);
    assert!(started.elapsed() < Duration::from_millis(100));
}

#[test]
fn every_frame_counts_how_late_its_output_was_served() {
    let (engine, mut sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    let output = OutputNumber::try_from(1).unwrap();
    assert_eq!(engine.lateness(output), None, "no output yet");
    let mock = Command::SetOutput {
        output,
        kind: OutputKind::Mock,
        universes: RunNumbers { first: 1, count: 1 },
        rate: None,
    };
    assert_eq!(sender.send(mock), Ok(()));
    thread::sleep(Duration::from_millis(250));
    let lateness = engine.lateness(output).expect("frames were counted");
    assert!(
        lateness.p99 > Duration::ZERO,
        "the p99 is a bucket's upper edge: {lateness:?}"
    );
    assert!(lateness.worst >= lateness.p99, "{lateness:?}");
}
