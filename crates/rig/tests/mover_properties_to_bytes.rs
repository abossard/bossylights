//! Mover properties to bytes: position, zoom, a colour on a wheel and a gobo by wheel and
//! slot, each at address 1. A light refuses a choice its wheel cannot show.

mod common;

use common::{all, at, light, lights, mode};
use engine_core::{
    Gobo, Levels, Light, LightRun, Position, Property, Refusal, Rgb, Universes, Value, Wheel,
    resolve,
};
use fixture_library::{Byte, Does, Function, Mode};
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

fn wheel(number: u8) -> Wheel {
    Wheel::new(number).unwrap()
}

fn gobo(number: u8, slot: u8) -> Gobo {
    Gobo {
        wheel: wheel(number),
        slot,
    }
}

/// A light holding each slot of `gobos`, wheel 1 first.
fn gobos(gobos: [Option<u8>; Wheel::COUNT]) -> Light {
    Light {
        gobos,
        ..Light::DARK
    }
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
    let shows = |colour, gobo: Option<Gobo>| Light {
        colour,
        ..gobo.map_or(Light::DARK, |gobo| Value::Gobo(gobo).light())
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
    let stairville = mode("Stairville-BS-280-R10-BeamSpot.qxf", "15 Channel");
    let alpha = mode("Clay-Paky-Alpha-Spot-575-HPE.qxf", "Option 16b");
    let rush = mode("Martin-Rush-MH5-Profile.qxf", "Mode 1");
    let cases: [Case; 22] = [
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
        // each wheel shows its own slot
        (
            &shehds(),
            gobos([Some(3), Some(7), None]),
            &[(7, 24), (8, 64)],
        ),
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
        // CMY beside a wheel: open white at 0, not "Random color CMY, fast" at 244
        (
            &mac,
            Light {
                intensity: Some(FULL),
                ..shows(Some(RED), None)
            },
            &[(3, 0), (4, 255), (5, 255), (7, 0)],
        ),
        // an open range without a preset is slot 1, before Gobo 1
        (&platinum, shows(None, Some(gobo(2, 1))), &[(6, 3)]),
        (&platinum, shows(None, Some(gobo(2, 2))), &[(6, 11)]),
        (&platinum, shows(None, Some(gobo(1, 1))), &[(4, 4)]),
        // a wheel whose gobos are named "Gobo 2" without a preset is wheel 1: 5 to 8,
        // and the next wheel is wheel 2: open, Gobo 1, then Gobo 2 at 13 to 19
        (
            &stairville,
            gobos([Some(2), Some(3), None]),
            &[(6, 6), (7, 16)],
        ),
        // "No gobo" is open, slot 1: 0 to 7. Gobo 1 is slot 2: 8 to 15
        (&alpha, shows(None, Some(gobo(1, 1))), &[(9, 3)]),
        (&alpha, shows(None, Some(gobo(1, 2))), &[(9, 11)]),
        // "Open - Gobo selection" is open, slot 1: 0 to 4. Gobo 1 is slot 2: 5 to 9
        (&rush, shows(None, Some(gobo(1, 1))), &[(6, 2)]),
        (&rush, shows(None, Some(gobo(1, 2))), &[(6, 7)]),
        // zoom travels 0 to 244, big to small: 245 to 251 is "no changes"
        (&mac, zoomed(0xFA00), &[(9, 5)]),
        (&mac, zoomed(0), &[(9, 244)]),
    ];
    for (mode, light, bytes) in cases {
        let mut out = [Levels::ZERO];
        common::shown(&[light], &rigged(mode), &mut out);
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
        let light = gobos([Some(slot), None, None]);
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
    let gobo_of = |number, slot| Value::Gobo(gobo(number, slot)).light();
    // (lights, values, result)
    let cases: [(&[LightRun], Light, Result<(), Refusal>); 16] = [
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
        (
            &both,
            gobo_of(2, 8),
            Err(unsupported(1, Property::Gobo(wheel(2)))),
        ),
        // each wheel its own property: one slot it has, one it has not
        (&both, gobos([Some(3), Some(7), None]), Ok(())),
        (
            &both,
            gobos([Some(3), Some(8), None]),
            Err(unsupported(1, Property::Gobo(wheel(2)))),
        ),
        (
            &both,
            gobos([Some(40), Some(7), None]),
            Err(unsupported(1, Property::Gobo(wheel(1)))),
        ),
        // a light without that gobo wheel has nothing to show it with: let through
        (&both, gobo_of(3, 1), Ok(())),
        (&par_only, gobo_of(1, 9), Ok(())),
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
    let spin_white = wheel("Wheel", "Spin", "#ffffff");
    let cyan = r#"<Channel Name="Cyan" Preset="IntensityCyan"/>"#;
    let blue = Rgb {
        red: 0,
        green: 0,
        blue: FULL,
    };
    // (channels, mode, colour, bytes 1 and 2, or the refusal)
    type Case<'a> = (String, &'a [&'a str], Rgb, Result<[u8; 2], Unfit>);
    let cases: [Case; 8] = [
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
        // a static white overrides an added mix: refused
        (
            format!("{red}{spin_white}"),
            &["Red", "Wheel"],
            RED,
            Err(Unfit::Refused(unsupported(1, Property::Colour))),
        ),
        // a CMY mix goes through a white (open) slot
        (
            format!("{cyan}{spin_white}"),
            &["Cyan", "Wheel"],
            RED,
            Ok([0, 14]),
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
            common::shown(&[light], &rigged, &mut out);
            [out[0].0[0], out[0].0[1]]
        });
        assert_eq!(found, expected, "{names:?} {colour:?}");
    }
}

/// A hybrid's colour macro at 0 lets the mix through when its label says the macro is off,
/// whatever its preset, or names the channels the light mixes with. Red held: the red
/// channel and the macro's byte, or the refusal.
#[test]
fn a_hybrid_macro_at_0_is_neutral_by_its_label() {
    let black = r##"Preset="ColorMacro" Res1="#000000""##;
    let white = r##"Preset="ColorMacro" Res1="#ffffff""##;
    let amber = r##"Preset="ColorMacro" Res1="#ffcf8f""##;
    let refused = || Err(Unfit::Refused(unsupported(1, Property::Colour)));
    // (the range at 0: label and preset, then red and the macro's byte, or the refusal)
    type Case<'a> = (&'a str, &'a str, Result<[u8; 2], Unfit>);
    let cases: [Case; 24] = [
        ("Macro Color OFF", "", Ok([255, 4])),
        ("Colour off", "", Ok([255, 4])),
        ("Off (no function)", "", Ok([255, 4])),
        ("None", "", Ok([255, 4])),
        ("Nothing", "", Ok([255, 4])),
        ("Inactive", "", Ok([255, 4])),
        ("No function (overrides Ch. 2\u{2013}4)", "", Ok([255, 4])),
        ("Off", black, Ok([255, 4])),
        ("Not functional", white, Ok([255, 4])),
        ("No function (0=default)", white, Ok([255, 4])),
        // names the channel the light mixes with
        ("Use channel 1-1 to control colour", "", Ok([255, 4])),
        // names the macro itself, or a channel the mode does not have
        ("Channel 2-2", "", refused()),
        ("Channels 1-3", "", refused()),
        // channels backwards, or a colour of its own beside them
        ("Channel 2-1", "", refused()),
        ("Use channel 1-1", black, refused()),
        // dark, or more than the macro off
        ("Black", black, refused()),
        ("Blackout", "", refused()),
        ("LEDs off (blackout)", black, refused()),
        ("All LEDs off", black, refused()),
        ("LED off", "", refused()),
        ("OFF/Color", "", refused()),
        ("Color 1 (off)", "", refused()),
        ("Bastard amber", amber, refused()),
        ("White", white, refused()),
    ];
    for (label, preset, expected) in cases {
        let channels = format!(
            r##"<Channel Name="Red" Preset="IntensityRed"/><Channel Name="Macro"><Group Byte="0">Colour</Group>
  <Capability Min="0" Max="9" {preset}>{label}</Capability>
  <Capability Min="10" Max="19" Preset="ColorMacro" Res1="#ff0000">Red</Capability></Channel>"##
        );
        let mode = made(&channels, &["Red", "Macro"]);
        let found = compile(&mode, at(1), light(0), Universes::ONE, lights()).map(|rigged| {
            let red = Light {
                intensity: Some(FULL),
                colour: Some(RED),
                ..Light::DARK
            };
            let mut out = [Levels::ZERO];
            common::shown(&[red], &rigged, &mut out);
            [out[0].0[0], out[0].0[1]]
        });
        assert_eq!(found, expected, "{label}");
    }
}

/// The range at 0 must be the one that turns the macro off: off at 10 is not.
#[test]
fn a_hybrid_macro_off_past_0_is_no_bypass() {
    let channels = r##"<Channel Name="Red" Preset="IntensityRed"/><Channel Name="Macro"><Group Byte="0">Colour</Group>
  <Capability Min="0" Max="9" Preset="ColorMacro" Res1="#ff0000">Red</Capability>
  <Capability Min="10" Max="19">Macro off</Capability></Channel>"##;
    let mode = made(channels, &["Red", "Macro"]);
    let found = compile(&mode, at(1), light(0), Universes::ONE, lights()).map(|_| ());
    assert_eq!(found, Err(Unfit::Refused(unsupported(1, Property::Colour))));
}

/// Every hybrid of the test definitions, red held: the byte each colour wheel holds and
/// the range it is in. Never a range that moves the wheel or picks a colour of its own.
#[test]
fn each_hybrid_holds_its_wheel_where_the_mix_goes_through() {
    let mixing = [
        Function::Red,
        Function::Green,
        Function::Blue,
        Function::White,
        Function::Cyan,
        Function::Magenta,
        Function::Yellow,
    ];
    let is_wheel = |c: &&fixture_library::Channel| {
        c.byte == Byte::Coarse
            && c.function == Function::ColorWheel
            && c.ranges
                .iter()
                .any(|r| matches!(r.does, Some(Does::Color(_))))
    };
    let red = Light {
        intensity: Some(FULL),
        colour: Some(RED),
        ..Light::DARK
    };
    // (file, mode, channel from 1, its byte and the label of its range, or the refusal)
    type Row<'a> = (&'a str, &'a str, usize, Result<(u8, &'a str), Unfit>);
    let x4 = "GLP-Impression-X4.qxf";
    let mac = "Martin-MAC-2000-Wash.qxf";
    let adj = "American-DJ-Ultra-Bar-6.qxf";
    let alpha = "Clay-Paky-Alpha-Spot-575-HPE.qxf";
    let qa5 = "American-DJ-QA5-IP.qxf";
    let hex5 = "American-DJ-WiFly-EXR-HEX5-IP.qxf";
    let vpar = "American-DJ-VPar.qxf";
    let domino = "Ayrton-Domino-LT.qxf";
    let mistral = "Ayrton-Mistral.qxf";
    let briteq = "Briteq-BT-Theatre-261FC.qxf";
    let hydra = "Cameo-HydraBeam-4000.qxf";
    let ovation = "Chauvet-Ovation-CYC-1-FC.qxf";
    let ek = "EK-R3-Wash.qxf";
    let mac301 = "Martin-MAC-301-Wash.qxf";
    let robin = "Robe-Robin-800-LEDWash.qxf";
    let spikie = "Robe-Robin-Spikie.qxf";
    let spectral = "Showtec-Spectral-850-RGBA-Tour.qxf";
    let mh100 = "Stairville-MH-100-Beam-36x3-LED.qxf";
    let mix = "Colors adjustable via RGBW";
    let no_filter = "No function - no filter, color channels used";
    let default = "No function (0=default)";
    // RGB beside colour macros without a bypass: a static white would override the mix
    let refused = || Err(Unfit::Refused(unsupported(1, Property::Colour)));
    let expected: &[Row] = &[
        // a slot at 0 ("Color 1 (off)"), and a colour temperature at 0
        (qa5, "10 Channel", 8, refused()),
        (qa5, "11 Channel", 8, refused()),
        (qa5, "11 Channel", 10, refused()),
        // bastard amber at 0
        (adj, "6 Channels mode", 4, refused()),
        (adj, "8 Channels mode", 4, refused()),
        // "Off" with a black preset: the macro is off
        (vpar, "7 Channel", 7, Ok((7, "Off"))),
        (vpar, "6 Channel", 6, Ok((7, "Off"))),
        // "Off/Color change/fade 1/Slow to fast": more than off
        (hex5, "11 Channel", 10, refused()),
        (hex5, "12 Channel", 10, refused()),
        // v11 has no recognised colour-wheel bypass for "Open / White".
        (domino, "Standard", 12, refused()),
        (domino, "Standard", 19, refused()),
        (domino, "Basic", 10, refused()),
        (domino, "Basic", 17, refused()),
        (domino, "Extended", 15, refused()),
        (domino, "Extended", 29, refused()),
        (mistral, "Standard", 15, Ok((1, "Open"))),
        (mistral, "Basic", 13, Ok((1, "Open"))),
        (mistral, "Extended", 18, Ok((1, "Open"))),
        ("Ayrton-Rivale-Profile.qxf", "Extended", 17, Ok((1, "Open"))),
        ("Ayrton-Rivale-Profile.qxf", "Standard", 14, Ok((1, "Open"))),
        (briteq, "12 Channel CMY", 7, Ok((2, no_filter))),
        (briteq, "14 Channel Full 8bit", 6, Ok((2, no_filter))),
        (briteq, "21 Channel Full 16bit", 7, Ok((2, no_filter))),
        // heads without a mix of their own: a wheel, unheld at its default
        (hydra, "32 Channel", 8, Ok((0, "Channel 29-32"))),
        (hydra, "32 Channel", 14, Ok((0, "Channel 29-32"))),
        (hydra, "32 Channel", 20, Ok((0, "Channel 29-32"))),
        (hydra, "32 Channel", 26, Ok((0, "Channel 29-32"))),
        // heads that mix: the range at 0 names their own red, green, blue and white
        (hydra, "56 Channel", 8, Ok((15, "Channel 11-14"))),
        (hydra, "56 Channel", 22, Ok((15, "Channel 25-28"))),
        (hydra, "56 Channel", 36, Ok((15, "Channel 39-42"))),
        (hydra, "56 Channel", 50, Ok((15, "Channel 53-56"))),
        (
            "Cameo-Q-Spot-15-RGBW.qxf",
            "9 Channel",
            8,
            Ok((2, "Colour off")),
        ),
        (ovation, "10 Channel", 9, Ok((2, "None"))),
        (ovation, "10 Channel", 10, Ok((2, "None"))),
        (ovation, "12 Channel", 8, Ok((2, "None"))),
        (ovation, "12 Channel", 9, Ok((2, "None"))),
        (ovation, "16 Channel", 14, Ok((2, "None"))),
        (ovation, "16 Channel", 15, Ok((2, "None"))),
        // CMY: the wheel's white
        (alpha, "Option 16b", 5, Ok((17, "white"))),
        (alpha, "Option Stnd", 5, Ok((17, "white"))),
        (alpha, "Option Extn", 5, Ok((17, "white"))),
        (
            "Clay-Paky-Arolla-Profile-HP.qxf",
            "Standard",
            6,
            Ok((4, "White")),
        ),
        // "LED off": dark
        ("Contest-SFX-HO150QC.qxf", "8 Channel", 7, refused()),
        (ek, "16 Channel", 6, Ok((4, "Macro Color OFF"))),
        (ek, "20 Channel", 10, Ok((4, "Macro Color OFF"))),
        (
            "Eliminator-Lighting-Stealth-Beam.qxf",
            "17 Channel",
            9,
            Ok((3, "Nothing")),
        ),
        (
            "Equinox-Party-Par-LED-PAR-56.qxf",
            "6 Channel",
            4,
            refused(),
        ),
        (x4, "Normal", 5, Ok((3, mix))),
        (x4, "Compress", 5, Ok((3, mix))),
        (
            "MARQ-Colormax-Par64.qxf",
            "8-Channel",
            6,
            Ok((7, "No function (overrides Ch. 2\u{2013}4)")),
        ),
        // CMY: wheel 1 open white at 0, not "Random color CMY, fast" at 244 to 247
        (mac, "8-bit with Color Wheel 2", 7, Ok((0, "White"))),
        (mac, "8-bit with Color Wheel 2", 8, Ok((249, "No function"))),
        (mac, "8-bit with Dimmer Wheel", 7, Ok((0, "White"))),
        (mac, "16-bit with Color Wheel 2", 7, Ok((0, "White"))),
        (
            mac,
            "16-bit with Color Wheel 2",
            8,
            Ok((249, "No function")),
        ),
        (mac, "16-bit with Dimmer Wheel", 7, Ok((0, "White"))),
        (mac301, "Standard 8bit", 13, Ok((0, "Inactive"))),
        (mac301, "Standard 16bit", 16, Ok((0, "Inactive"))),
        (robin, "38 Channel", 32, Ok((175, "Red"))),
        (robin, "22 Channel", 16, Ok((0, "No function"))),
        (robin, "16 Channel", 12, Ok((0, "No function"))),
        (robin, "38 Channel #2", 14, Ok((0, "No function"))),
        (robin, "37 Channel", 32, Ok((175, "Red"))),
        // "No function" with a white preset: the macro is off
        (spikie, "Standard 16-bit (RGB)", 9, Ok((0, default))),
        (spikie, "Reduced 8-bit (RGB)", 9, Ok((0, default))),
        (spikie, "Standard 16-bit (CMY)", 9, Ok((0, default))),
        (spikie, "Reduced 8-bit (CMY)", 9, Ok((0, default))),
        (spectral, "8 channel", 6, Ok((6, "Not functional"))),
        (spectral, "15 channel", 11, Ok((6, "Not functional"))),
        // "All LEDs off": dark
        (
            "Stairville-AF-180-LED-Fogger-Co2-FX.qxf",
            "7 Channel",
            3,
            refused(),
        ),
        (
            "Stairville-CLB4-RGB-Compact-LED-Bar-4.qxf",
            "7 Channel",
            4,
            Ok((4, "All LEDs Set via Channels 1-3")),
        ),
        (mh100, "14-Ch Mode", 11, refused()),
        (mh100, "14-Ch Mode", 12, refused()),
        // 0 to 7 is "OFF/Color": more than off
        ("XStatic-X-240Bar-RGB.qxf", "7 Channel", 5, refused()),
    ];
    let mut found = Vec::new();
    for (file, fixture) in all() {
        for mode in &fixture.modes {
            let channels = &mode.channels;
            if !channels.iter().any(|c| mixing.contains(&c.function))
                || !channels.iter().any(|c| is_wheel(&c))
            {
                continue;
            }
            let held = compile(mode, at(1), light(0), Universes::ONE, lights()).map(|rigged| {
                let mut out = [Levels::ZERO];
                common::shown(&[red], &rigged, &mut out);
                out[0]
            });
            for (index, channel) in channels.iter().enumerate().filter(|(_, c)| is_wheel(c)) {
                let held = held.map(|out| {
                    let byte = out.0[index];
                    let range = channel
                        .ranges
                        .iter()
                        .find(|r| (r.min..=r.max).contains(&byte));
                    (byte, range.map_or(String::new(), |r| r.label.clone()))
                });
                found.push((file.clone(), mode.name.clone(), index + 1, held));
            }
        }
    }
    let expected: Vec<_> = expected
        .iter()
        .map(|(f, m, c, held)| {
            let held = (*held).map(|(b, l)| (b, l.to_string()));
            (f.to_string(), m.to_string(), *c, held)
        })
        .collect();
    assert_eq!(found, expected);
}
