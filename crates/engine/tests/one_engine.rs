//! At most one engine runs on the machine at a time (#319): every start, wall or stepped
//! clock, first waits for the machine's one engine slot (`engine-slot`), and the engine
//! holds it until its drop has ended.

use engine::{Device, Engine, Outcome, OutputNumber, Rate, RunNumbers, Sender, Universes};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Starts an engine.
type Start = fn() -> (Engine, Sender);

fn wall() -> (Engine, Sender) {
    Engine::start(Universes::ONE).unwrap()
}

fn stepped() -> (Engine, Sender) {
    let (engine, sender, _wakes) =
        Engine::start_stepped(Universes::ONE, Box::new(outputs::open)).unwrap();
    (engine, sender)
}

fn measured() -> (Engine, Sender) {
    let (engine, sender, _timings) = Engine::start_measured(Universes::ONE).unwrap();
    (engine, sender)
}

/// Starts an engine on a thread of its own: the flag is set once it runs. The thread
/// ends with the engine dropped.
fn beside(start: Start) -> (JoinHandle<()>, Arc<AtomicBool>) {
    let started = Arc::new(AtomicBool::new(false));
    let thread = thread::spawn({
        let started = Arc::clone(&started);
        move || {
            let _engine = start();
            started.store(true, Ordering::Release);
        }
    });
    (thread, started)
}

/// A device whose drop waits until the test lets it go.
#[expect(
    clippy::disallowed_types,
    reason = "only the test device waits to be freed"
)]
struct SlowToFree(mpsc::Receiver<()>);

impl Device for SlowToFree {
    fn take(&mut self, _frame: &[u8]) -> Outcome {
        Outcome::Accepted
    }
}

impl Drop for SlowToFree {
    fn drop(&mut self) {
        let _ = self.0.recv_timeout(Duration::from_secs(10));
    }
}

#[test]
fn a_second_engine_starts_only_once_the_first_is_gone() {
    // (case, first engine, second engine): each start holds the slot once and waits once
    let cases: [(&str, Start, Start); 3] = [
        ("wall, then stepped", wall, stepped),
        ("stepped, then measured", stepped, measured),
        ("measured, then wall", measured, wall),
    ];
    for (case, first, second) in cases {
        let first = first();
        let (second, started) = beside(second);
        thread::sleep(Duration::from_secs(1));
        assert!(
            !started.load(Ordering::Acquire),
            "{case}: the second started beside the first"
        );
        drop(first);
        second.join().unwrap();
        assert!(started.load(Ordering::Acquire), "{case}: never started");
    }
}

#[test]
fn the_slot_is_free_only_once_the_first_engines_drop_has_ended() {
    let (engine, mut sender) = wall();
    let (release, hold) = mpsc::channel();
    let run = RunNumbers { first: 1, count: 1 }
        .check(Universes::ONE)
        .unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    sender
        .attach(output, Box::new(SlowToFree(hold)), run, Rate::Sixty)
        .unwrap();
    // the engine's drop frees the device: it waits for `release`
    let dropping = thread::spawn(move || drop((sender, engine)));
    let (second, started) = beside(wall);
    thread::sleep(Duration::from_secs(1));
    assert!(
        !started.load(Ordering::Acquire),
        "the second started while the first was still being dropped"
    );
    drop(release);
    dropping.join().unwrap();
    second.join().unwrap();
    assert!(started.load(Ordering::Acquire), "never started");
}
