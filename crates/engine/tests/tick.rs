use engine::{Engine, Universes};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn counter_advances_44_times_a_second() {
    // how long we watch, in ms
    for ms in [250, 500, 1000] {
        let (engine, _sender) =
            Engine::start(Universes::ONE, |_| {}).expect("the tick thread starts");
        let before = engine.query().frame;
        let started = Instant::now();
        thread::sleep(Duration::from_millis(ms));
        let ticks = engine.query().frame - before;
        // Count against the time that really passed: CI machines oversleep.
        let expected = (started.elapsed().as_secs_f64() * f64::from(Engine::RATE_HZ)) as u64;
        let slack = 2 + expected / 10;
        assert!(
            ticks.abs_diff(expected) <= slack,
            "{ms} ms: expected {expected} frames (±{slack}), got {ticks}"
        );
    }
}

#[test]
fn dropping_the_engine_stops_the_tick_thread_quickly() {
    let (engine, _sender) = Engine::start(Universes::ONE, |_| {}).expect("the tick thread starts");
    thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    drop(engine);
    assert!(started.elapsed() < Duration::from_millis(100));
}

#[test]
fn every_tick_counts_how_late_it_woke() {
    let (engine, _sender) = Engine::start(Universes::ONE, |_| {}).expect("the tick thread starts");
    assert_eq!(engine.lateness_p99(), None, "no tick yet");
    thread::sleep(Duration::from_millis(250));
    let p99 = engine.lateness_p99().expect("ticks were counted");
    assert!(
        p99 > Duration::ZERO,
        "the p99 is a bucket's upper edge: {p99:?}"
    );
    let worst = engine.lateness_worst().expect("ticks were counted");
    assert!(worst >= p99, "worst {worst:?} under p99 {p99:?}");
}
