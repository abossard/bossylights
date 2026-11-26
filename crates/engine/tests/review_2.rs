#![cfg(feature = "test-support")]

mod mock;

use engine::{
    Begun, ChannelNumbers, Command, Device, Engine, Event, Outcome, OutputNumber, Rate, RunNumbers,
    Sender, Universes,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

struct Gate {
    held_started: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    last_sent: Arc<AtomicU8>,
}

impl Device for Gate {
    fn take(&mut self, frame: &[u8]) -> Outcome {
        if frame[0] == 17 {
            self.held_started.store(true, Ordering::Release);
            mock::wait_until("release the sealed frame", || {
                self.release.load(Ordering::Acquire)
            });
        }
        self.last_sent.store(frame[0], Ordering::Release);
        Outcome::Accepted
    }
}

// Claim: A second cutover loses the newest result when the old lane returns a sealed frame and three queued frames.
// Source: docs/design/output-cutover.md, rules R1 and R2, transition T9.
// Severity: medium. Health and press timing can miss the newest frame that made it to the lights.
#[test]
fn finding_2_02_cutover_loses_fourth_frame_outcome() {
    let held_started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let last_sent = Arc::new(AtomicU8::new(0));
    let open = {
        let held_started = Arc::clone(&held_started);
        let release = Arc::clone(&release);
        let last_sent = Arc::clone(&last_sent);
        Box::new(move |_| {
            Ok(Box::new(Gate {
                held_started: Arc::clone(&held_started),
                release: Arc::clone(&release),
                last_sent: Arc::clone(&last_sent),
            }) as Box<dyn Device>)
        })
    };
    let (engine, mut sender, mut wakes) =
        Engine::start_stepped(Universes::new(2).unwrap(), open).unwrap();
    wakes.hold().unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let set = |first| Command::SetOutput {
        output,
        kind: engine_core::TEST_STREAM_KIND,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    let handoff = |sender: &mut Sender, first| {
        let Begun::Opening(opening) = sender.begin(set(first), &[]).unwrap() else {
            panic!("a changed run needs a new lane");
        };
        sender.publish_output(opening.open()).unwrap().unwrap()
    };
    let press = |value| Command::SetChannel {
        channel: ChannelNumbers {
            universe: 2,
            address: 1,
        },
        value,
    };

    sender.send(set(1)).unwrap();
    let at = wakes.wake().unwrap();
    wakes.events();
    let first = handoff(&mut sender, 2);
    sender.send(press(17)).unwrap();
    let step = Duration::from_millis(100);
    wakes.wake_at(at + step).unwrap();
    assert!(first.wait(), "the first reroute is accepted");
    mock::wait_until("the sealed frame is being sent", || {
        held_started.load(Ordering::Acquire)
    });

    // Install the lane and fill its three ordinary boxes while its sealed frame is in flight.
    for (frame, value) in [(2, 18), (3, 19), (4, 20)] {
        sender.send(press(value)).unwrap();
        wakes.wake_at(at + step * frame).unwrap();
    }
    let due: Vec<_> = wakes
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Handed { due, .. } => Some(due),
            Event::Came { .. } | Event::Counted { .. } => None,
        })
        .collect();
    assert_eq!(due.len(), 4, "one sealed frame and three ordinary frames");

    let second = handoff(&mut sender, 1);
    release.store(true, Ordering::Release);
    // The second handoff cannot finish until this lane has returned all its receipts.
    assert!(second.wait(), "the second reroute is accepted");
    assert_eq!(
        last_sent.load(Ordering::Acquire),
        20,
        "the old lane sent the newest queued frame before returning the device"
    );
    wakes.wake_at(at + step * 5).unwrap();
    let outcomes: Vec<_> = wakes
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Came {
                due: at, outcome, ..
            } if due.contains(&at) => Some(outcome),
            Event::Handed { .. } | Event::Came { .. } | Event::Counted { .. } => None,
        })
        .collect();
    drop(engine);
    assert_eq!(
        outcomes,
        [
            Outcome::Accepted,
            Outcome::Dropped,
            Outcome::Dropped,
            Outcome::Accepted,
        ],
        "the tick must count the sealed frame, the two passed-over frames and the newest frame"
    );
}
