#![cfg(feature = "test-support")]

mod mock;

use engine::Unsent;
use engine::{
    ChannelNumbers, Color, Command, Engine, Light, Rank, Rate, Rgb, RunNumbers, Target, Text,
    Universes, When,
};
use engine_core::{Palette, Pattern, Pixels, Sequence, SequenceItem, Step, Swatches, WaveTarget};
use std::sync::Arc;
use std::time::Duration;

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

fn strip(pixels: u16, universes: u16) -> (Engine, engine::Sender, engine::Wakes, Arc<mock::Heard>) {
    let universes = Universes::new(universes).unwrap();
    let (engine, mut sender, wakes) =
        Engine::start_stepped(universes, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let output = RunNumbers {
        first: 1,
        count: universes.count(),
    };
    let heard = mock::attach(&mut sender, universes, 1, output, Rate::Sixty);
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
    (engine, sender, wakes, heard)
}

#[test]
fn a_saved_rgb_wave_leaves_room_to_light_its_first_pixel() {
    let name = Text::new("wall").unwrap();
    let colour = Rgb {
        red: u16::MAX,
        ..Rgb::BLACK
    };
    let wave = Command::PutPattern {
        look: 1,
        name,
        pattern: Pattern::Wave {
            target: WaveTarget::Matrix { fixture: 1 },
            period: 1,
            palette: Palette::Inline(Swatches::new(&[colour, Rgb::BLACK]).unwrap()),
        },
    };
    let start = Command::StartLook {
        look: 1,
        rank: Rank::Show,
        when: When::Now,
    };
    let intensity = Command::SetIntensity {
        target: Target::Light { light: 1 },
        intensity: u16::MAX,
    };
    let mut replies = Vec::new();
    for (pixels, first, second) in [
        (4095, intensity, start),
        (4095, start, intensity),
        (4096, intensity, start),
        (4096, start, intensity),
    ] {
        let (_engine, mut sender, wakes, heard) = strip(pixels, 24);
        sender.send(wave).unwrap();
        sender
            .send(Command::SetInternalTempo {
                bpm: engine::Bpm::new(60).unwrap(),
            })
            .unwrap();
        let beat = Duration::from_secs(1);
        let first_at = wakes.wake().unwrap() + beat;
        mock::wait_until("the patch frame did not arrive", || heard.frames() == 1);
        wakes.set_time(first_at);
        sender.send(first).unwrap();
        wakes.wake_at(first_at).unwrap();
        mock::wait_until("the first command's frame did not arrive", || {
            heard.frames() == 2
        });
        // A full beat later the first pixel is red for either start order.
        let second_at = first_at + beat;
        wakes.set_time(second_at);
        // A strip without a dimmer emits no Wave colour without a separate hold.
        replies.push(sender.send(second));
        if pixels == 4096 {
            // A full Wave has room for one light's write, end and release.
            let another = Command::SetIntensity {
                target: Target::Light { light: 2 },
                intensity: u16::MAX,
            };
            assert_eq!(sender.send(another), Err(Unsent::Refused(engine::Refusal::WorkLimit)));
        }
        wakes.wake_at(second_at).unwrap();
        mock::wait_until("the second command's frame did not arrive", || {
            heard.frames() == 3
        });
        assert_eq!(
            [1, 2, 3].map(|address| heard.level(1, address)),
            [255, 0, 0]
        );
        assert_eq!(heard.lit_of(3), Some(1));
        wakes.run();
    }
    assert_eq!(replies, [Ok(()); 4]);
}

fn playing_strip() -> (Engine, engine::Sender, engine::Wakes) {
    let (mut engine, mut sender, wakes, _) = strip(PIXELS, 18);
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
    assert_eq!(sender.send(press(13)), Err(Unsent::Refused(engine::Refusal::WorkLimit)));
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
    let (mut engine, mut sender, wakes, _) = strip(pixels, 19);
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
