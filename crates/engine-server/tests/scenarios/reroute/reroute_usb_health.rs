//! A health reader that samples before the reroute and after the same USB port returns.

use crate::output_common as usb;

use engine::{
    Adapter, Begun, ChannelNumbers, Command, Device, Engine, Event, Outcome, OutputKind,
    OutputNumber, Rate, RunNumbers, Status, Universes, Wakes,
};
use engine_core::health::{HealthEvent, HealthSample, RecoveredReason, status_events};
use outputs::dmx_usb_pro::DmxUsbPro;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const RATE: Rate = Rate::FortyFour;

fn output() -> OutputNumber {
    OutputNumber::try_from(1).unwrap()
}

fn health_until(
    engine: &mut Engine,
    wakes: &Wakes,
    at: Duration,
    ready: impl Fn(Status) -> bool,
) -> HealthSample {
    let started = Instant::now();
    loop {
        let mut sample = HealthSample::new(Duration::ZERO, 0, Status::NotFound);
        if engine.latest().health_sample(output(), &mut sample) && ready(sample.status) {
            return sample;
        }
        assert!(
            started.elapsed() < usb::NEVER,
            "last sample: {:?}",
            sample.status
        ); // guard
        wakes.wake_at(at).unwrap();
    }
}

fn frame_with(wakes: &mut Wakes, mut at: Duration, expected: Outcome) -> Duration {
    let started = Instant::now();
    loop {
        at = wakes.wake_at(at + RATE.period()).unwrap();
        loop {
            assert!(
                started.elapsed() < usb::NEVER,
                "no frame with outcome {expected:?}"
            ); // guard
            if let Some(outcome) = wakes.events().into_iter().find_map(|event| match event {
                Event::Came { outcome, due, .. } if due == at => Some(outcome),
                Event::Came { .. } | Event::Handed { .. } | Event::Counted { .. } => None,
            }) {
                if outcome == expected {
                    return at;
                }
                break;
            }
            wakes.wake_at(at).unwrap();
        }
    }
}

#[test]
fn health_reports_a_usb_return_even_if_the_reader_missed_the_rerouted_outage() {
    let (find, mut fake) = usb::Fake::new(true);
    let mut find = Some(find);
    let (mut engine, mut sender, mut wakes) = Engine::start_stepped(
        Universes::new(2).unwrap(),
        Box::new(move |_| {
            let device = DmxUsbPro::open(find.take().unwrap())?;
            usb::until_answered(&device);
            Ok(Box::new(device) as Box<dyn Device>)
        }),
    )
    .unwrap();
    wakes.hold().unwrap();
    let set = |first| Command::SetOutput {
        output: output(),
        kind: OutputKind::DmxUsbPro {
            adapter: Adapter::FIRST,
        },
        universes: RunNumbers { first, count: 1 },
        rate: Some(RATE),
    };
    for (universe, value) in [(1, 11), (2, 91)] {
        sender
            .send(Command::SetChannel {
                channel: ChannelNumbers {
                    universe,
                    address: 1,
                },
                value,
            })
            .unwrap();
    }
    sender.send(set(1)).unwrap();
    let mut port = fake.next_end();
    let at = wakes.wake().unwrap();
    assert_eq!(usb::read_message(&mut port)[5], 11);
    health_until(&mut engine, &wakes, at, |status| {
        status == Status::Connecting
    });

    fake.plugged().store(false, Ordering::Release);
    drop(port);
    let at = frame_with(&mut wakes, at, Outcome::Failed);
    let before = health_until(&mut engine, &wakes, at, |status| {
        matches!(status, Status::Reconnecting { .. })
    });
    // Finish the outstanding reader request on the old route. All the wakes until
    // the port is writing again happen before the reader can ask for another sample.
    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("USB reroute must prepare a lane");
    };
    wakes.wake_at(at).unwrap();
    sender.finish(opening.open()).unwrap();
    fake.plugged().store(true, Ordering::Release);
    let mut port = fake.next_end();
    let mut at = at;
    for _ in 0..2 {
        at = frame_with(&mut wakes, at, Outcome::Accepted);
        assert_eq!(usb::read_message(&mut port)[5], 91);
    }
    let after = health_until(&mut engine, &wakes, at, |status| status == Status::Running);
    assert_eq!(
        status_events(Some(&before), Some(&after)),
        [
            Some(HealthEvent::Recovered(RecoveredReason::OutputAvailable)),
            None
        ],
        "same adapter returned on the wire across generation {} -> {}",
        before.generation,
        after.generation,
    );
}
