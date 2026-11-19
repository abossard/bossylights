//! The bytes a light makes: a mode compiled by the rig, resolved by the core.

mod common;

use common::{at, light, lights, mode};
use engine_core::{Levels, Light, Rgb, Universes, resolve};
use rig::compile;

const FULL: u16 = u16::MAX;
const HALF: u16 = 0x8000;

const RED: (u16, u16, u16) = (FULL, 0, 0);
const WHITE: (u16, u16, u16) = (FULL, FULL, FULL);
const GREY: (u16, u16, u16) = (HALF, HALF, HALF);
const BLACK: (u16, u16, u16) = (0, 0, 0);

/// The bytes from address 37 on of `mode` at 37, showing one light.
fn bytes(file: &str, name: &str, intensity: u16, (red, green, blue): (u16, u16, u16)) -> Vec<u8> {
    let mode = mode(file, name);
    let rigged = compile(&mode, at(37), light(0), Universes::ONE, lights()).unwrap();
    let shown = [Light {
        intensity: Some(intensity),
        colour: Some(Rgb { red, green, blue }),
        ..Light::DARK
    }];
    let mut out = [Levels::ZERO];
    resolve(&shown, &rigged.writers, &mut out);
    out[0].0[36..36 + mode.channels.len()].to_vec()
}

#[test]
fn the_colour_table() {
    let dimmer_rgb = ("Generic-Generic-RGB.qxf", "Dimmer RGB"); // dimmer, red, green, blue
    let rgb = ("Generic-Generic-RGB.qxf", "RGB"); // red, green, blue
    let white = ("Generic-Generic-RGBW.qxf", "Dimmer RGBW"); // dimmer, red, green, blue, white
    let cmy = ("Griven-Kolorado-2500.qxf", "Mode 1"); // lamp, dimmer, yellow, cyan, magenta
    let dimmer = ("Showtec-Par-64-100W-COB-UV.qxf", "8 Bit"); // dimmer
    // (mode, colour, bytes at intensity 0, a half, 1)
    let cases: [(_, _, [&[u8]; 3]); 20] = [
        (
            dimmer_rgb,
            RED,
            [&[0, 255, 0, 0], &[128, 255, 0, 0], &[255, 255, 0, 0]],
        ),
        (
            dimmer_rgb,
            WHITE,
            [
                &[0, 255, 255, 255],
                &[128, 255, 255, 255],
                &[255, 255, 255, 255],
            ],
        ),
        (
            dimmer_rgb,
            GREY,
            [
                &[0, 128, 128, 128],
                &[128, 128, 128, 128],
                &[255, 128, 128, 128],
            ],
        ),
        (
            dimmer_rgb,
            BLACK,
            [&[0, 0, 0, 0], &[128, 0, 0, 0], &[255, 0, 0, 0]],
        ),
        (rgb, RED, [&[0, 0, 0], &[128, 0, 0], &[255, 0, 0]]),
        (rgb, WHITE, [&[0, 0, 0], &[128, 128, 128], &[255, 255, 255]]),
        (rgb, GREY, [&[0, 0, 0], &[64, 64, 64], &[128, 128, 128]]),
        (rgb, BLACK, [&[0, 0, 0], &[0, 0, 0], &[0, 0, 0]]),
        (
            white,
            RED,
            [
                &[0, 255, 0, 0, 0],
                &[128, 255, 0, 0, 0],
                &[255, 255, 0, 0, 0],
            ],
        ),
        (
            white,
            WHITE,
            [
                &[0, 0, 0, 0, 255],
                &[128, 0, 0, 0, 255],
                &[255, 0, 0, 0, 255],
            ],
        ),
        (
            white,
            GREY,
            [
                &[0, 0, 0, 0, 128],
                &[128, 0, 0, 0, 128],
                &[255, 0, 0, 0, 128],
            ],
        ),
        (
            white,
            BLACK,
            [&[0, 0, 0, 0, 0], &[128, 0, 0, 0, 0], &[255, 0, 0, 0, 0]],
        ),
        // full less a half is 32 767: its high byte is 127
        (
            cmy,
            RED,
            [
                &[0, 0, 255, 0, 255],
                &[0, 128, 255, 0, 255],
                &[0, 255, 255, 0, 255],
            ],
        ),
        (
            cmy,
            WHITE,
            [&[0, 0, 0, 0, 0], &[0, 128, 0, 0, 0], &[0, 255, 0, 0, 0]],
        ),
        (
            cmy,
            GREY,
            [
                &[0, 0, 127, 127, 127],
                &[0, 128, 127, 127, 127],
                &[0, 255, 127, 127, 127],
            ],
        ),
        (
            cmy,
            BLACK,
            [
                &[0, 0, 255, 255, 255],
                &[0, 128, 255, 255, 255],
                &[0, 255, 255, 255, 255],
            ],
        ),
        (dimmer, RED, [&[0], &[128], &[255]]),
        (dimmer, WHITE, [&[0], &[128], &[255]]),
        (dimmer, GREY, [&[0], &[128], &[255]]),
        (dimmer, BLACK, [&[0], &[128], &[255]]),
    ];
    for ((file, name), colour, expected) in cases {
        for (intensity, expected) in [0, HALF, FULL].into_iter().zip(expected) {
            assert_eq!(
                bytes(file, name, intensity, colour),
                expected,
                "{name}, colour {colour:?}, intensity {intensity}"
            );
        }
    }
}

/// The example of the spec: an RGBW PAR with a dimmer at 37, red, intensity 80%.
#[test]
fn red_at_80_percent() {
    let eighty = (u32::from(FULL) * 8 / 10) as u16;
    assert_eq!(
        bytes("Generic-Generic-RGBW.qxf", "Dimmer RGBW", eighty, RED),
        [204, 255, 0, 0, 0]
    );
}

/// A 16-bit dimmer: coarse and fine come from one value. The shutter at 37 stays open.
#[test]
fn coarse_and_fine_come_from_one_value() {
    // (intensity, strobe at 37, dimmer at 38, fine at 39)
    let cases = [
        (0x0000, [8, 0x00, 0x00]),
        (0x00FF, [8, 0x00, 0xFF]),
        (0x0100, [8, 0x01, 0x00]),
        (0x1234, [8, 0x12, 0x34]),
        (HALF, [8, 0x80, 0x00]),
        (FULL, [8, 0xFF, 0xFF]),
    ];
    for (intensity, expected) in cases {
        let bytes = bytes("Martin-Rush-MH6-Wash.qxf", "Standard", intensity, BLACK);
        assert_eq!(bytes[..3], expected, "{intensity:#06x}");
    }
}

/// A dimmer and RGB mode whose red defaults to 200.
const DEFAULT_RED: &str = r#"<?xml version="1.0"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Test</Manufacturer><Model>Default red</Model><Type>Color Changer</Type>
 <Channel Name="Dimmer" Preset="IntensityDimmer"/>
 <Channel Name="Red" Preset="IntensityRed" Default="200"/>
 <Channel Name="Green" Preset="IntensityGreen"/>
 <Channel Name="Blue" Preset="IntensityBlue"/>
 <Mode Name="Dimmer RGB">
  <Channel Number="0">Dimmer</Channel>
  <Channel Number="1">Red</Channel>
  <Channel Number="2">Green</Channel>
  <Channel Number="3">Blue</Channel>
 </Mode>
</FixtureDefinition>"#;

/// A part nothing holds shows the definition's default, also after a release. A part held
/// at 0 is 0 (#160).
#[test]
fn a_part_not_held_is_the_definition_default() {
    use engine_core::{ChannelSet, EngineCommand, LightRun, Priority, State, update};
    let fixture = fixture_library::parse("default.qxf", DEFAULT_RED.as_bytes()).unwrap();
    let rigged = compile(
        &fixture.modes[0],
        at(37),
        light(0),
        Universes::ONE,
        lights(),
    )
    .unwrap();
    let one = LightRun::new(light(0), 1, lights()).unwrap();
    let full = EngineCommand::SetIntensity {
        lights: one,
        intensity: FULL,
    };
    let colour = |(red, green, blue)| EngineCommand::SetColour {
        lights: one,
        red,
        green,
        blue,
    };
    let release_colour = EngineCommand::ReleaseColour { lights: one };
    let release_intensity = EngineCommand::ReleaseIntensity { lights: one };
    // (name, commands, bytes 37 to 40)
    let cases = [
        ("nothing held", vec![], [0, 200, 0, 0]),
        ("only intensity set", vec![full], [255, 200, 0, 0]),
        (
            "colour set, then released",
            vec![full, colour((0, FULL, 0)), release_colour],
            [255, 200, 0, 0],
        ),
        (
            "colour set",
            vec![full, colour((0, FULL, 0))],
            [255, 0, 255, 0],
        ),
        (
            "colour held black",
            vec![full, colour(BLACK)],
            [255, 0, 0, 0],
        ),
        (
            "intensity set, then released",
            vec![full, release_intensity],
            [0, 200, 0, 0],
        ),
    ];
    for (name, commands, expected) in cases {
        let mut shown = [Light::DARK];
        let (mut universes, mut holds) = ([Levels::ZERO], [ChannelSet::NONE]);
        let mut state = State::new(Priority::RealTime);
        for command in commands {
            state = update(state, &mut universes, &mut holds, &mut shown, command);
        }
        let mut out = [Levels::ZERO];
        resolve(&shown, &rigged.writers, &mut out);
        assert_eq!(out[0].0[36..40], expected, "{name}");
    }
}
