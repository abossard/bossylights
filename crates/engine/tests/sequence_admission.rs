#![cfg(feature = "test-support")]

mod mock;

use engine::{
    ChannelNumbers, Color, Command, Engine, Light, Rank, Rate, Rgb, RunNumbers, Target, Text,
    Universes, When,
};
use engine_core::{Pixels, Sequence, SequenceItem, Step};

#[test]
fn an_empty_property_release_does_not_exhaust_a_playing_sequences_work_budget() {
    let mut results = Vec::new();
    for (case, release_for, press) in [
        (
            "release absent intensity, then press colour",
            Command::ReleaseIntensity {
                target: Target::Fixture { fixture: 1 },
            },
            Command::SetColour {
                target: Target::Light { light: 1 },
                red: 0,
                green: 0,
                blue: u16::MAX,
            },
        ),
        (
            "release absent colour, then press intensity",
            Command::ReleaseColour {
                target: Target::Fixture { fixture: 1 },
            },
            Command::SetIntensity {
                target: Target::Light { light: 1 },
                intensity: 0,
            },
        ),
    ] {
        let (_engine, mut sender, wakes) = playing_strip();
        // There is room for a one-light press without the redundant release.
        let work = looks::play_work(sender.looks().plan(2).unwrap().work());
        assert!(work < looks::WORK_LIMIT);
        assert!(work + usize::from(PIXELS) > looks::WORK_LIMIT);
        sender.send(release_for).unwrap();
        results.push((case, sender.send(press)));
        wakes.run();
    }
    assert_eq!(
        results,
        [
            ("release absent intensity, then press colour", Ok(())),
            ("release absent colour, then press intensity", Ok(())),
        ]
    );
}

const PIXELS: u16 = 2978;

fn strip(pixels: u16, universes: u16) -> (Engine, engine::Sender, engine::Wakes) {
    let universes = Universes::new(universes).unwrap();
    let (engine, mut sender, wakes) =
        Engine::start_stepped(universes, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let output = RunNumbers {
        first: 1,
        count: universes.count(),
    };
    let _heard = mock::attach(&mut sender, universes, 1, output, Rate::Sixty);
    let name = Text::new("red pixels").unwrap();
    sender
        .send(Command::PatchStrip {
            name,
            pixels: Pixels::try_from(pixels).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    (engine, sender, wakes)
}

fn playing_strip() -> (Engine, engine::Sender, engine::Wakes) {
    let (mut engine, mut sender, wakes) = strip(PIXELS, 18);
    let name = Text::new("red pixels").unwrap();
    sender
        .send(Command::PutStep {
            look: 1,
            name,
            step: Step {
                target: Target::Fixture { fixture: 1 },
                values: Light {
                    colour: Some(Rgb {
                        red: u16::MAX,
                        ..Rgb::BLACK
                    }),
                    ..Light::DARK
                },
            },
        })
        .unwrap();
    sender
        .send(Command::PutSequence {
            look: 2,
            name,
            sequence: Sequence::new(&[SequenceItem {
                look: 1,
                ..SequenceItem::default()
            }])
            .unwrap(),
        })
        .unwrap();
    sender
        .send(Command::StartLook {
            look: 2,
            rank: Rank::Show,
            when: When::Now,
        })
        .unwrap();
    engine.latest();
    wakes.wake().unwrap();
    assert_eq!(engine.latest().state.plays.iter().count(), 1);
    (engine, sender, wakes)
}

#[test]
fn direct_grabs_reserve_their_end_and_release_before_they_are_accepted() {
    let (mut engine, mut sender, wakes) = playing_strip();
    let press = |light| Command::SetIntensity {
        target: Target::Light { light },
        intensity: 11,
    };
    let mut grabs = Vec::new();
    // The playing root leaves twenty visits free. Six distinct grabs each
    // book their write, their possible EndGrab, and their final release.
    for light in [1, 3, 5, 7, 9, 11] {
        let engine::Begun::Grabbed { grab } = sender.begin(press(light), &[]).unwrap() else {
            panic!("a direct press returns a grab");
        };
        grabs.push(grab);
    }
    let tail = sender.sent();
    assert_eq!(sender.send(press(13)), Err(engine::Refusal::WorkLimit));
    assert_eq!(sender.sent(), tail);

    // EndGrab keeps the even-indexed grabs. Releasing the whole fixture
    // clears the same property on both the kept and the still-live lights.
    for &grab in grabs.iter().step_by(2) {
        sender.send(Command::EndGrab { grab }).unwrap();
    }
    let release = Command::ReleaseIntensity {
        target: Target::Fixture { fixture: 1 },
    };
    sender.send(release).unwrap();
    let tail = sender.sent();
    sender.send(release).unwrap();
    assert_eq!(sender.sent(), tail);
    sender.send(press(13)).unwrap();
    engine.latest();
    wakes.wake().unwrap();
    let picture = engine.latest();
    for (index, light) in picture.lights().iter().take(14).enumerate() {
        assert_eq!(light.intensity, (index == 12).then_some(11));
    }
    wakes.run();
}

#[test]
fn a_fragmented_fixture_release_fits_in_one_wake() {
    let pixels = 3200;
    let (mut engine, mut sender, wakes) = strip(pixels, 19);
    let target = Target::Fixture { fixture: 1 };
    sender
        .send(Command::SetIntensity {
            target,
            intensity: 11,
        })
        .unwrap();

    // Leave 1,600 single-light runs held in one fixture.
    let group = Text::new("evens").unwrap();
    let evens: Vec<_> = (2..=pixels)
        .step_by(2)
        .map(|light| engine::Member::Light { light })
        .collect();
    for members in evens.chunks(engine::Members::MOST) {
        sender
            .send(Command::SetGroup {
                name: group,
                members: engine::MemberList::Fits(engine::Members::new(members).unwrap()),
            })
            .unwrap();
        sender
            .send(Command::ReleaseIntensity {
                target: Target::Group { group },
            })
            .unwrap();
        engine.latest();
        wakes.wake().unwrap();
    }
    let intensities = |engine: &mut Engine| {
        engine
            .latest()
            .lights()
            .iter()
            .take(usize::from(pixels))
            .map(|light| light.intensity)
            .collect::<Vec<_>>()
    };
    let alternating: Vec<_> = (1..=pixels)
        .map(|light| (light % 2 == 1).then_some(11))
        .collect();
    assert_eq!(intensities(&mut engine), alternating);

    sender.send(Command::ReleaseIntensity { target }).unwrap();
    engine.latest();
    wakes.wake().unwrap();
    assert_eq!(intensities(&mut engine), vec![None; usize::from(pixels)]);
    wakes.run();
}
