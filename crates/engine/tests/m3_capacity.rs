//! A stalled free thread bounds both retained plan bytes and occupied return entries
//! while the tick drains its command FIFO and the reader stops polling.
#![cfg(feature = "test-support")]

mod mock;

use engine::{
    Begun, ChannelNumbers, Color, Command, Engine, Light, PLAN_BYTES, Rank, Rate, Refusal, Rgb,
    RunNumbers, Sender, Target, Text, Universes, When,
};
use engine_core::{Pixels, Sequence, SequenceItem, Step};
use std::sync::Arc;

fn start(sender: &mut Sender, look: u16) -> engine::PlayId {
    let command = Command::StartLook {
        look,
        rank: Rank::Show,
        when: When::Now,
    };
    let Begun::Started { play } = sender.begin(command, &[]).unwrap() else {
        panic!("the sequence starts")
    };
    play
}

#[test]
fn retired_plan_bytes_refuse_the_next_live_edit_until_reclaimed() {
    // 64 different children give each revision its own large binding table.
    // The byte bound is reached before the command or return rings fill.
    const CHILDREN: usize = 64;
    const ROOT: u16 = CHILDREN as u16 + 1;
    let name = Text::new("capacity").unwrap();
    let universes = Universes::new(3).unwrap();
    let (mut engine, mut sender, wakes) =
        Engine::start_stepped(universes, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let output = mock::attach(
        &mut sender,
        universes,
        1,
        RunNumbers { first: 1, count: 3 },
        Rate::Sixty,
    );
    sender
        .send(Command::PatchStrip {
            name,
            pixels: Pixels::try_from(350).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    let step = |look, red| Command::PutStep {
        look,
        name,
        step: Step {
            target: Target::Fixture { fixture: 1 },
            values: Light {
                intensity: Some(u16::MAX),
                colour: Some(Rgb { red, ..Rgb::BLACK }),
                ..Light::DARK
            },
        },
    };
    let items = std::array::from_fn::<_, CHILDREN, _>(|n| SequenceItem {
        look: n as u16 + 1,
        ..Default::default()
    });
    for item in items {
        sender.send(step(item.look, 0)).unwrap();
    }
    sender
        .send(Command::PutSequence {
            look: ROOT,
            name,
            sequence: Sequence::new(&items).unwrap(),
        })
        .unwrap();
    let play = start(&mut sender, ROOT);
    let one = Target::Light { light: 1 };
    sender
        .send(Command::SetIntensity {
            target: one,
            intensity: 0,
        })
        .unwrap();
    engine.latest();
    let at = wakes.wake().unwrap();
    mock::wait_until("the first frame never arrived", || output.frames() == 1);
    let compiled = sender.looks().plan(ROOT).unwrap();
    let versions = PLAN_BYTES / compiled.bytes();
    assert!(
        (2..engine::QUEUE).contains(&versions),
        "{versions} versions"
    );
    let first = Arc::downgrade(&compiled);
    drop(compiled);

    engine.stall_reclamation(true);
    // Never advance the clock beyond the first item. Drain the FIFO between
    // bursts but leave its old plans with the reclaimer and the reader idle.
    for version in 1..versions {
        sender.send(step(1, (version as u16) * 257)).unwrap();
        if version % 32 == 0 {
            wakes.wake_at(at).unwrap();
        }
    }
    wakes.wake_at(at).unwrap();
    assert!(
        first.upgrade().is_some(),
        "the oldest retired plan is pinned"
    );
    let last = sender.looks();
    let sent = sender.sent();
    let refused = step(1, u16::MAX);
    assert_eq!(sender.send(refused), Err(Refusal::WorkLimit));
    assert_eq!(sent, sender.sent());
    assert!(Arc::ptr_eq(&last, &sender.looks()));

    // The last two FIFO places take a release then a press. The first pixel
    // reveals the last admitted red and the second takes Manual green.
    for _ in 2..engine::QUEUE {
        sender.send(Command::Blackout { on: false }).unwrap();
    }
    sender
        .send(Command::ReleaseIntensity { target: one })
        .unwrap();
    sender
        .send(Command::SetColour {
            target: Target::Light { light: 2 },
            red: 0,
            green: u16::MAX,
            blue: 0,
        })
        .unwrap();
    let sent = sender.sent();
    assert_eq!(
        sender.send(Command::Blackout { on: true }),
        Err(Refusal::QueueFull)
    );
    assert_eq!(sender.sent(), sent);
    engine.latest();
    wakes.wake().unwrap();
    mock::wait_until("release never reached the output", || output.frames() == 2);
    assert_eq!(
        [1, 2, 3, 4, 5, 6].map(|address| output.level(1, address)),
        [(versions - 1) as u8, 0, 0, 0, 255, 0]
    );
    let picture = engine.latest();
    assert_eq!(picture.applied(), sent);
    assert!(picture.state.plays.iter().any(|p| p.play == play));
    assert_eq!(sender.send(refused), Err(Refusal::WorkLimit));

    engine.stall_reclamation(false);
    mock::wait_until("the oldest plan never returned", || {
        first.strong_count() == 0
    });
    mock::wait_until("the refused edit never fits again", || {
        match sender.send(refused) {
            Ok(()) => true,
            Err(Refusal::WorkLimit) => false,
            Err(other) => panic!("retry: {other:?}"),
        }
    });
    wakes.wake().unwrap();
    mock::wait_until("the retry never reached the output", || {
        output.frames() == 3
    });
    assert_eq!(output.level(1, 1), 255);
}

#[test]
fn each_live_plan_swap_reserves_a_return_and_release_keeps_its_place() {
    let (mut engine, mut sender, wakes) =
        Engine::start_stepped(Universes::ONE, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    engine.stall_reclamation(true);
    let name = Text::new("one pixel").unwrap();
    sender
        .send(Command::PatchStrip {
            name,
            color: Color::Rgb,
            pixels: Pixels::try_from(1).unwrap(),
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    // the pixel has no dimmer: a step shows its number as its red and green bytes
    let step = |number: u16| Command::PutStep {
        look: 1,
        name,
        step: Step {
            target: Target::Light { light: 1 },
            values: Light {
                colour: Some(Rgb {
                    red: number & 0xff00,
                    green: number << 8,
                    blue: 0,
                }),
                ..Light::DARK
            },
        },
    };
    let shown = |engine: &mut Engine| {
        let levels = engine.latest().shown()[0].0;
        u16::from_be_bytes([levels[0], levels[1]])
    };
    sender.send(step(0)).unwrap();
    let play = start(&mut sender, 1);
    let at = wakes.wake().unwrap();

    // The patch retired the empty writer table and the playing step owns one
    // return. Every subsequent live replacement must book one additional entry.
    let returns = engine::QUEUE + engine::MAX_OUTPUTS;
    assert!(sender.looks().plan(1).unwrap().bytes() * returns < PLAN_BYTES);
    let swaps = u16::try_from(returns - 2).unwrap();
    for intensity in 1..=swaps {
        sender.send(step(intensity)).unwrap();
        if intensity % 32 == 0 {
            wakes.wake_at(at).unwrap();
        }
    }
    engine.latest();
    wakes.wake_at(at).unwrap();
    assert_eq!(shown(&mut engine), swaps);
    let sent = sender.sent();
    let project = sender.looks();
    let restart = Command::StartLook {
        look: 1,
        rank: Rank::Show,
        when: When::Now,
    };
    for refused in [step(u16::MAX), restart] {
        assert_eq!(sender.send(refused), Err(Refusal::QueueFull));
        assert_eq!(sender.sent(), sent);
        assert!(Arc::ptr_eq(&project, &sender.looks()));
    }

    sender.send(Command::ReleaseHold { play }).unwrap();
    engine.latest();
    wakes.wake_at(at).unwrap();
    assert_eq!(engine.latest().state.plays.iter().count(), 0);
    assert_eq!(shown(&mut engine), 0);
    // The released plan still occupies its return entry until the free thread runs.
    assert_eq!(sender.send(restart), Err(Refusal::QueueFull));
    engine.stall_reclamation(false);
    mock::wait_until(
        "no return entry reclaimed for the next start",
        || match sender.send(restart) {
            Ok(()) => true,
            Err(Refusal::QueueFull) => false,
            Err(other) => panic!("restart: {other:?}"),
        },
    );
    engine.latest();
    wakes.wake_at(at).unwrap();
    assert_eq!(shown(&mut engine), swaps);
}
