//! Mover properties to bytes: position, zoom, a colour on a wheel and a gobo by wheel and
//! slot, each at address 1. A light refuses a choice its wheel cannot show.

mod common;

use common::{at, light, lights, mode};
use engine_core::{
    Gobo, Levels, Light, LightRun, Position, Property, Refusal, Rgb, Universes, resolve,
};
use fixture_library::Mode;
use rig::{Rigged, Unfit, compile, supports};

const FULL: u16 = u16::MAX;
const RED: Rgb = Rgb {
    red: FULL,
    green: 0,
    blue: 0,
};
const WHITE: Rgb = Rgb {
    red: FULL,
    green: FULL,
    blue: FULL,
};
const ORANGE: Rgb = Rgb {
    red: FULL,
    green: 0x8000,
    blue: 0,
};

/// A channel, counted from 1, and its byte.
type Bytes = (usize, u8);

fn rigged(mode: &Mode) -> Rigged {
    compile(mode, at(1), light(0), Universes::ONE, lights()).unwrap()
}

fn shehds() -> Mode {
    mode("Shehds-LED-Spot-100W.qxf", "14 Channel")
}

fn gobo(wheel: u8, slot: u8) -> Gobo {
    Gobo { wheel, slot }
}

const fn unsupported(light: u16, property: Property) -> Refusal {
    Refusal::Unsupported { light, property }
}

#[test]
fn each_property_goes_to_its_channels() {
    let elation = mode("Elation-EVC-MH.qxf", "12-CH");
    let martin = mode("Martin-Rush-MH6-Wash.qxf", "Standard");
    let aimed = Light {
        position: Some(Position {
            pan: 0x1234,
            tilt: 0x5678,
        }),
        zoom: Some(0x8000),
        ..Light::DARK
    };
    let shows = |colour, gobo| Light {
        colour,
        gobo,
        ..Light::DARK
    };
    // (mode, the light, channels from 1 and the bytes they show)
    type Case<'a> = (&'a Mode, Light, &'a [Bytes]);
    let zoomed = |zoom| Light {
        zoom: Some(zoom),
        ..Light::DARK
    };
    let x4 = mode("GLP-Impression-X4.qxf", "Normal");
    let platinum = mode("Elation-Platinum-Spot-5R.qxf", "Basic");
    let mac = mode("Martin-MAC-2000-Wash.qxf", "8-bit with Color Wheel 2");
    let cases: [Case; 15] = [
        (
            &elation,
            aimed,
            &[(1, 0x12), (2, 0x34), (3, 0x56), (4, 0x78), (9, 0x80)],
        ),
        // the same zoom on a zoom that runs big to small
        (&martin, aimed, &[(9, 0x7f), (10, 0x12), (11, 0x34)]),
        // nothing held: each at its default
        (&elation, Light::DARK, &[(1, 0), (2, 0), (9, 0)]),
        // a static colour is the lower midpoint of its range: red is 5 to 9
        (&shehds(), shows(Some(RED), None), &[(6, 7), (7, 0), (8, 0)]),
        // open is white
        (&shehds(), shows(Some(WHITE), None), &[(6, 2)]),
        // Static Gobo / Gobo 2 is the third slot of wheel 1, after open and Gobo 1:
        // 20 to 29. The rotating gobo wheel stays at its default
        (
            &shehds(),
            shows(None, Some(gobo(1, 3))),
            &[(6, 0), (7, 24), (8, 0)],
        ),
        (&shehds(), shows(None, Some(gobo(2, 7))), &[(7, 0), (8, 64)]),
        (&shehds(), Light::DARK, &[(6, 0), (7, 0), (8, 0)]),
        // a hybrid holds its wheel where the mix goes through: 0 to 7, not fixed white
        (
            &x4,
            Light {
                intensity: Some(FULL),
                ..shows(Some(RED), None)
            },
            &[(5, 3), (6, 255)],
        ),
        (&x4, Light::DARK, &[(5, 3)]),
        // an open range without a preset is slot 1, before Gobo 1
        (&platinum, shows(None, Some(gobo(2, 1))), &[(6, 3)]),
        (&platinum, shows(None, Some(gobo(2, 2))), &[(6, 11)]),
        (&platinum, shows(None, Some(gobo(1, 1))), &[(4, 4)]),
        // zoom travels 0 to 244, big to small: 245 to 251 is "no changes"
        (&mac, zoomed(0xFA00), &[(9, 5)]),
        (&mac, zoomed(0), &[(9, 244)]),
    ];
    for (mode, light, bytes) in cases {
        let mut out = [Levels::ZERO];
        resolve(&[light], &rigged(mode).writers, &mut out);
        for &(channel, byte) in bytes {
            assert_eq!(
                out[0].0.get(channel - 1),
                Some(&byte),
                "{} channel {channel} of {light:?}",
                mode.name
            );
        }
    }
}

#[test]
fn a_gobo_cuts_to_its_slot() {
    let writers = rigged(&shehds()).writers;
    let mut out = [Levels::ZERO];
    // (gobo, channel 7) one frame after the other: never a byte between two slots
    for (slot, byte) in [(3, 24), (8, 74), (2, 14), (3, 24)] {
        let light = Light {
            gobo: Some(gobo(1, slot)),
            ..Light::DARK
        };
        resolve(&[light], &writers, &mut out);
        assert_eq!(out[0].0[6], byte, "slot {slot}");
    }
}

#[test]
fn a_light_refuses_what_its_wheel_cannot_show() {
    // light 0: a Shehds spot (wheels), light 1: a Generic RGB (mixes, no wheel)
    let spot = rigged(&shehds());
    let par = compile(
        &mode("Generic-Generic-RGB.qxf", "RGB"),
        at(100),
        light(1),
        Universes::ONE,
        lights(),
    )
    .unwrap();
    let writers: Vec<_> = spot.writers.into_iter().chain(par.writers).collect();
    let run = |first, count| LightRun::new(light(first), count, lights()).unwrap();
    let both = [run(0, 2)];
    let par_only = [run(1, 1)];
    let colour = |colour| Light {
        colour: Some(colour),
        ..Light::DARK
    };
    let gobo_of = |wheel, slot| Light {
        gobo: Some(gobo(wheel, slot)),
        ..Light::DARK
    };
    // (lights, values, result)
    let cases: [(&[LightRun], Light, Result<(), Refusal>); 13] = [
        (&both, colour(RED), Ok(())),
        (&both, colour(WHITE), Ok(())),
        // no black and no orange on the wheel
        (
            &both,
            colour(Rgb::BLACK),
            Err(unsupported(1, Property::Colour)),
        ),
        (&both, colour(ORANGE), Err(unsupported(1, Property::Colour))),
        // a light that mixes shows any colour
        (&par_only, colour(Rgb::BLACK), Ok(())),
        (&par_only, colour(ORANGE), Ok(())),
        (&both, gobo_of(1, 3), Ok(())),
        (&both, gobo_of(2, 7), Ok(())),
        (&both, gobo_of(2, 8), Err(unsupported(1, Property::Gobo))),
        (&both, gobo_of(3, 1), Err(unsupported(1, Property::Gobo))),
        // a light without a gobo wheel has nothing to show it with: let through
        (&par_only, gobo_of(9, 9), Ok(())),
        // position and zoom are amounts: any value
        (
            &both,
            Light {
                position: Some(Position { pan: 1, tilt: 2 }),
                zoom: Some(3),
                ..Light::DARK
            },
            Ok(()),
        ),
        (&both, Light::DARK, Ok(())),
    ];
    for (runs, values, expected) in cases {
        assert_eq!(
            supports(&writers, runs, &values),
            expected,
            "{values:?} on {runs:?}"
        );
    }
}

/// A test fixture: `channels` as QLC+ writes them, one mode with all of them in order.
fn made(channels: &str, names: &[&str]) -> Mode {
    let numbers: String = names
        .iter()
        .enumerate()
        .map(|(n, name)| format!(r#"<Channel Number="{n}">{name}</Channel>"#))
        .collect();
    let definition = format!(
        r#"<?xml version="1.0"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Test</Manufacturer><Model>Wheels</Model><Type>Moving Head</Type>
 {channels}
 <Mode Name="Test">{numbers}</Mode>
</FixtureDefinition>"#
    );
    fixture_library::parse("wheels.qxf", definition.as_bytes())
        .unwrap()
        .modes
        .remove(0)
}

#[test]
fn a_hybrid_binds_its_companions_to_neutral_or_is_refused() {
    let red = r#"<Channel Name="Red" Preset="IntensityRed"/>"#;
    let wheel = |name: &str, open: &str, colour: &str| {
        format!(
            r#"<Channel Name="{name}"><Group Byte="0">Colour</Group>
  <Capability Min="0" Max="9">{open}</Capability>
  <Capability Min="10" Max="19" Preset="ColorMacro" Res1="{colour}">Slot</Capability></Channel>"#
        )
    };
    let open_red = wheel("Wheel", "Open", "#ff0000");
    let spin_red = wheel("Wheel", "Spin", "#ff0000");
    let open_blue = wheel("Second", "Open", "#0000ff");
    let spin_blue = wheel("Second", "Spin", "#0000ff");
    let blue = Rgb {
        red: 0,
        green: 0,
        blue: FULL,
    };
    // (channels, mode, colour, bytes 1 and 2, or the refusal)
    type Case<'a> = (String, &'a [&'a str], Rgb, Result<[u8; 2], Unfit>);
    let cases: [Case; 6] = [
        // mixes and has a wheel: the wheel holds its neutral slot, the colour is mixed
        (
            format!("{red}{open_red}"),
            &["Red", "Wheel"],
            RED,
            Ok([255, 4]),
        ),
        (
            format!("{red}{open_red}"),
            &["Red", "Wheel"],
            blue,
            Ok([0, 4]),
        ),
        // no neutral slot: refused
        (
            format!("{red}{spin_red}"),
            &["Red", "Wheel"],
            RED,
            Err(Unfit::Refused(unsupported(1, Property::Colour))),
        ),
        // two wheels: the one without the colour shows its neutral slot
        (
            format!("{open_red}{open_blue}"),
            &["Wheel", "Second"],
            RED,
            Ok([14, 4]),
        ),
        (
            format!("{open_red}{open_blue}"),
            &["Wheel", "Second"],
            blue,
            Ok([4, 14]),
        ),
        (
            format!("{open_red}{spin_blue}"),
            &["Wheel", "Second"],
            RED,
            Err(Unfit::Refused(unsupported(1, Property::Colour))),
        ),
    ];
    for (channels, names, colour, expected) in cases {
        let mode = made(&channels, names);
        let found = compile(&mode, at(1), light(0), Universes::ONE, lights()).map(|rigged| {
            let light = Light {
                intensity: Some(FULL),
                colour: Some(colour),
                ..Light::DARK
            };
            let mut out = [Levels::ZERO];
            resolve(&[light], &rigged.writers, &mut out);
            [out[0].0[0], out[0].0[1]]
        });
        assert_eq!(found, expected, "{names:?} {colour:?}");
    }
}
