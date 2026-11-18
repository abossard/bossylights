use engine::Engine;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn counter_advances_44_times_a_second() {
    // (how long we watch, frames we expect)
    let cases = [(250, 11), (500, 22), (1000, 44)];
    for (ms, expected) in cases {
        let engine = Engine::start();
        let before = engine.query().frame;
        thread::sleep(Duration::from_millis(ms));
        let ticks = engine.query().frame - before;
        // Generous slack: CI machines oversleep.
        let slack = 2 + expected / 5;
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
