#![cfg(feature = "test-support")]

mod mock;

use engine::{
    AuthoredStep, Begun, ChannelNumbers, Color, Command, Engine, Light, Palette, Pattern, Rank,
    Rate, Rgb, RunNumbers, Sender, Swatches, Target, Text, Universes, Wakes, WaveTarget, When,
};
use engine_core::{Pixels, Step};
use std::sync::Arc;

fn wall(pixels: u16, streams: u16) -> (Engine, Sender, Wakes, Arc<mock::Heard>) {
    let universes = Universes::new(streams).unwrap();
    let (engine, mut sender, wakes) =
        Engine::start_stepped(universes, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let heard = mock::attach(
        &mut sender,
        universes,
        1,
        RunNumbers {
            first: 1,
            count: streams,
        },
        Rate::Sixty,
    );
    sender
        .send(Command::PatchStrip {
            name: Text::new("wall").unwrap(),
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

fn start(look: u16, rank: Rank) -> Command {
    Command::StartLook {
        look,
        rank,
        when: When::Now,
        usage: Default::default(),
    }
}

fn wave(look: u16, palette: Palette) -> Command {
    Command::PutPattern {
        name: Text::new("Wave").unwrap(),
        look,
        pattern: Pattern::Wave {
            target: WaveTarget::Matrix { fixture: 1 },
            period: 1,
            palette,
        },
    }
}

#[test]
fn releasing_a_full_budget_loop_leaves_room_for_the_next_manual_press() {
    let (mut engine, mut sender, wakes, heard) = wall(4096, 24);
    let red = Rgb {
        red: u16::MAX,
        ..Rgb::BLACK
    };
    sender
        .send(wave(
            1,
            Palette::Inline(Swatches::new(&[red, Rgb::BLACK]).unwrap()),
        ))
        .unwrap();
    sender
        .send(Command::PutStep {
            name: Text::new("Manual").unwrap(),
            look: 2,
            step: Step {
                target: Target::Light { light: 1 },
                values: Light {
                    intensity: Some(u16::MAX),
                    colour: Some(red),
                    ..Light::DARK
                },
            }
            .into(),
        })
        .unwrap();
    let Begun::Started { play } = sender.begin(start(1, Rank::Show), &[]).unwrap() else {
        panic!("the Wave did not start");
    };
    engine.latest();
    wakes.wake().unwrap();
    assert!(
        engine
            .latest()
            .state
            .plays
            .iter()
            .any(|row| row.play == play)
    );
    mock::wait_until("the first Wave frame did not arrive", || {
        heard.frames() == 1
    });

    // Both commands precede the same cutoff. A released looping Wave has no
    // terminal sample, and only the new one-light Manual step can be mixed.
    sender.send(Command::ReleaseHold { play }).unwrap();
    let press = sender.send(start(2, Rank::Manual));
    wakes.wake().unwrap();
    mock::wait_until("the applying frame did not arrive", || heard.frames() == 2);
    let next = heard.first_channel_of(2);
    wakes.run();

    assert_eq!((press, next), (Ok(()), Some(255)));
}

#[test]
fn a_palette_edit_fits_when_it_replaces_a_loop_with_the_same_work() {
    let (_engine, mut sender, wakes, heard) = wall(3000, 18);
    let target = Target::Light { light: 1 };
    let palette = |colour| Command::PutStep {
        look: 1,
        name: Text::new("palette").unwrap(),
        step: AuthoredStep {
            target,
            values: Light::DARK,
            swatches: Swatches::new(&[colour]).unwrap(),
        },
    };
    let red = Rgb {
        red: u16::MAX,
        ..Rgb::BLACK
    };
    let green = Rgb {
        green: u16::MAX,
        ..Rgb::BLACK
    };
    for command in [
        Command::SetIntensity {
            target,
            intensity: u16::MAX,
        },
        palette(red),
        wave(2, Palette::Step { look: 1 }),
        start(2, Rank::Show),
    ] {
        sender.send(command).unwrap();
    }
    wakes.wake().unwrap();
    mock::wait_until("the red frame did not arrive", || heard.frames() == 1);
    assert_eq!([heard.level(1, 1), heard.level(1, 2)], [255, 0]);

    // The only queued change swaps the plan before sampling this loop. No
    // stop, once end or transition can call the outgoing plan in this wake.
    let edit = sender.send(palette(green));
    // A subsequent command sees the installed plan's work exactly once too.
    let intensity = sender.send(Command::SetIntensity {
        target,
        intensity: u16::MAX,
    });
    wakes.wake().unwrap();
    mock::wait_until("the applying frame did not arrive", || heard.frames() == 2);
    let next = [heard.level(1, 1), heard.level(1, 2)];
    wakes.run();

    assert_eq!((edit, next), (Ok(()), [0, 255]));
    assert_eq!(intensity, Ok(()));
}
