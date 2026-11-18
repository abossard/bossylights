use engine::Engine;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn counter_advances_44_times_a_second() {
    // how long we watch, in ms
    for ms in [250, 500, 1000] {
        let engine = Engine::start();
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
    let engine = Engine::start();
    thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    drop(engine);
    assert!(started.elapsed() < Duration::from_millis(100));
}
