//! A mode at an address becomes writers: real definitions, at universe 1, address 37.

mod common;

use common::{all, at, light, lights, mode, say};
use engine_core::{Levels, Light, Refusal, Rgb, Universes, Writer, resolve};
use rig::{Unfit, compile};

#[test]
fn a_mode_at_an_address_becomes_writers() {
    // (definition, mode, lights it makes, writers in words)
    let cases: [(&str, &str, u16, &[&str]); 10] = [
        (
            "Showtec-Par-64-100W-COB-UV.qxf",
            "8 Bit",
            1,
            &["37 light 0: intensity"],
        ),
        (
            "Generic-Generic-RGB.qxf",
            "RGB",
            1,
            &[
                "37 light 0: red × intensity",
                "38 light 0: green × intensity",
                "39 light 0: blue × intensity",
            ],
        ),
        (
            "Generic-Generic-RGB.qxf",
            "Dimmer RGB",
            1,
            &[
                "37 light 0: intensity",
                "38 light 0: red",
                "39 light 0: green",
                "40 light 0: blue",
            ],
        ),
        (
            "Generic-Generic-RGBW.qxf",
            "Dimmer RGBW",
            1,
            &[
                "37 light 0: intensity",
                "38 light 0: red - shared",
                "39 light 0: green - shared",
                "40 light 0: blue - shared",
                "41 light 0: shared",
            ],
        ),
        (
            "Generic-Generic-RGBW.qxf",
            "WRGB",
            1,
            &[
                "37 light 0: shared × intensity",
                "38 light 0: red - shared × intensity",
                "39 light 0: green - shared × intensity",
                "40 light 0: blue - shared × intensity",
            ],
        ),
        (
            // a moving head with a 16-bit dimmer and a shutter. Its colour presets show no
            // static colour, so they are no wheel: at their default. Zoom runs big to small
            "Martin-Rush-MH6-Wash.qxf",
            "Standard",
            1,
            &[
                "37 = 8",
                "38+39 light 0: intensity",
                "40 light 0: red - shared",
                "41 light 0: green - shared",
                "42 light 0: blue - shared",
                "43 light 0: shared",
                "44 = 0",
                "45 light 0: full - zoom",
                "46+47 light 0: pan",
                "48+49 light 0: tilt",
                "50 = 0",
            ],
        ),
        (
            // white LEDs, 16-bit pan and tilt, zoom small to big
            "Elation-EVC-MH.qxf",
            "12-CH",
            1,
            &[
                "37+38 light 0: pan",
                "39+40 light 0: tilt",
                "41 = 0",
                "42 = 0",
                "43 light 0: shared × intensity",
                "44 light 0: shared × intensity",
                "45 light 0: zoom",
                "46 = 0",
                "47 = 0",
                "48 = 0",
            ],
        ),
        (
            // a colour wheel and nothing that mixes: open is white. Two gobo wheels, open
            // the first slot of each; the shaking gobos and the rotation are no slots.
            // Pan and tilt fine come last in the mode
            "Shehds-LED-Spot-100W.qxf",
            "14 Channel",
            1,
            &[
                "37 light 0: intensity",
                "38 = 252",
                "39+48 light 0: pan",
                "40+49 light 0: tilt",
                "41 = 0",
                "42 light 0: 8 slots, else 0",
                "  #ffffff = 2",
                "  #ff0000 = 7",
                "  #00ff00 = 12",
                "  #0000ff = 17",
                "  #ffff00 = 22",
                "  #ff00ff = 27",
                "  #00ffff = 32",
                "  #55ff00 = 37",
                "43 light 0: 8 slots, else 0",
                "  gobo 1/1 = 4",
                "  gobo 1/2 = 14",
                "  gobo 1/3 = 24",
                "  gobo 1/4 = 34",
                "  gobo 1/5 = 44",
                "  gobo 1/6 = 54",
                "  gobo 1/7 = 64",
                "  gobo 1/8 = 74",
                "44 light 0: 7 slots, else 0",
                "  gobo 2/1 = 4",
                "  gobo 2/2 = 14",
                "  gobo 2/3 = 24",
                "  gobo 2/4 = 34",
                "  gobo 2/5 = 44",
                "  gobo 2/6 = 54",
                "  gobo 2/7 = 64",
                "45 = 0",
                "46 = 0",
                "47 = 0",
                "50 = 0",
            ],
        ),
        (
            "Griven-Kolorado-2500.qxf",
            "Mode 1",
            1,
            &[
                "37 = 0",
                "38 light 0: intensity",
                "39 light 0: full - blue",
                "40 light 0: full - red",
                "41 light 0: full - green",
            ],
        ),
        (
            // 8 pixels: the bar is light 0 with the master dimmer, each pixel a light of
            // its own. The third and the eighth list their channels out of order
            "XStatic-X-240Bar-RGB.qxf",
            "26 Channel",
            9,
            &[
                "37 light 0: intensity",
                "38 light 1: red × intensity",
                "39 light 1: green × intensity",
                "40 light 1: blue × intensity",
                "41 light 2: red × intensity",
                "42 light 2: green × intensity",
                "43 light 2: blue × intensity",
                "44 light 3: red × intensity",
                "45 light 3: green × intensity",
                "46 light 3: blue × intensity",
                "47 light 4: red × intensity",
                "48 light 4: green × intensity",
                "49 light 4: blue × intensity",
                "50 light 5: red × intensity",
                "51 light 5: green × intensity",
                "52 light 5: blue × intensity",
                "53 light 6: red × intensity",
                "54 light 6: green × intensity",
                "55 light 6: blue × intensity",
                "56 light 7: red × intensity",
                "57 light 7: green × intensity",
                "58 light 7: blue × intensity",
                "59 light 8: red × intensity",
                "60 light 8: green × intensity",
                "61 light 8: blue × intensity",
                "62 = 0",
            ],
        ),
    ];
    for (file, name, count, expected) in cases {
        let rigged = compile(
            &mode(file, name),
            at(37),
            light(0),
            Universes::ONE,
            lights(),
        )
        .unwrap_or_else(|e| panic!("{file} {name}: {e:?}"));
        let said: Vec<String> = rigged.writers.iter().map(say).collect();
        assert_eq!(said, expected, "{file} {name}");
        assert_eq!(rigged.lights, count, "{file} {name}");
    }
}

#[test]
fn the_lights_are_numbered_from_the_first() {
    let rigged = compile(
        &mode("XStatic-X-240Bar-RGB.qxf", "4 Channel"),
        at(1),
        light(100),
        Universes::ONE,
        lights(),
    )
    .unwrap();
    let said: Vec<String> = rigged.writers.iter().map(say).collect();
    assert_eq!(
        said,
        [
            "1 light 100: intensity",
            "2 light 100: red",
            "3 light 100: green",
            "4 light 100: blue",
        ]
    );
}

#[test]
fn a_mode_that_does_not_fit_is_refused() {
    let martin = mode("Martin-Rush-MH6-Wash.qxf", "Standard"); // 14 channels, 11 writers
    let bar = mode("XStatic-X-240Bar-RGB.qxf", "26 Channel"); // 9 lights
    let two = Universes::new(2).unwrap();
    // (mode, universe, address, first light, result)
    let cases = [
        (&martin, 1, 499, 0, Ok(11)),
        (&martin, 2, 499, 0, Ok(11)),
        // past 512 it runs on at the next universe's address 1, but there is no third
        (&martin, 1, 500, 0, Ok(11)),
        (&martin, 2, 500, 0, Err(Unfit::PastTheEnd { channels: 14 })),
        (
            &martin,
            1,
            0,
            0,
            Err(Unfit::Refused(Refusal::NoSuchAddress)),
        ),
        (
            &martin,
            3,
            1,
            0,
            Err(Unfit::Refused(Refusal::NoSuchUniverse { universes: 2 })),
        ),
        (&bar, 1, 1, 16_375, Ok(26)),
        (&bar, 1, 1, 16_376, Err(Unfit::TooManyLights { needs: 9 })),
    ];
    for (mode, universe, address, first, expected) in cases {
        let at = engine_core::ChannelNumbers { universe, address };
        let found = compile(mode, at, light(first), two, lights()).map(|r| r.writers.len());
        assert_eq!(
            found, expected,
            "{} at {universe}/{address}, light {first}",
            mode.name
        );
    }
}

/// Every channel of every mode of the test definitions is written by exactly one writer.
/// A mode refused for a colour it cannot show (`m3_properties`) has no writers.
#[test]
fn every_channel_is_written_once() {
    for (file, fixture) in all() {
        for mode in &fixture.modes {
            let rigged = match compile(mode, at(1), light(0), Universes::ONE, lights()) {
                Err(Unfit::Refused(Refusal::Unsupported { .. })) => continue,
                rigged => rigged.unwrap(),
            };
            let mut written: Vec<u16> = rigged
                .writers
                .iter()
                .flat_map(|writer| match *writer {
                    Writer::Fixed { at, .. } | Writer::Picks { at, .. } => {
                        vec![common::address(at)]
                    }
                    Writer::ColourSlot { .. } | Writer::GoboSlot { .. } => vec![],
                    Writer::Takes { at, .. } => match at {
                        engine_core::Place::Byte(at) => vec![common::address(at)],
                        engine_core::Place::Pair { coarse, fine } => {
                            vec![common::address(coarse), common::address(fine)]
                        }
                    },
                })
                .collect();
            written.sort_unstable();
            let all: Vec<u16> = (1..).take(mode.channels.len()).collect();
            assert_eq!(written, all, "{file} {}", mode.name);
        }
    }
}

/// A channel two heads name (14 modes of the QLC+ library, GLP JDC1 among them) is the
/// fixture's own: the fixture is a light, its one writer writes the channel, and no two
/// writers ever reach it.
#[test]
fn a_channel_two_heads_name_is_the_fixtures_own() {
    const RED: Light = Light {
        intensity: Some(u16::MAX),
        colour: Some(Rgb {
            red: u16::MAX,
            green: 0,
            blue: 0,
        }),
        ..Light::DARK
    };
    const FULL: Light = Light {
        intensity: Some(u16::MAX),
        ..Light::DARK
    };
    let dimmer = r#"<Channel Name="Shared" Preset="IntensityDimmer"/>"#;
    let strobe = r#"<Channel Name="Shared"><Group Byte="0">Shutter</Group>
  <Capability Min="0" Max="7" Preset="ShutterClose">Closed</Capability>
  <Capability Min="8" Max="255" Preset="ShutterOpen">Open</Capability></Channel>"#;
    // (kind, the shared channel, writers in words, bytes 37 to 39 with the fixture full,
    // head 1 red, head 2 dark)
    let cases: [(&str, &str, [&str; 3], [u8; 3]); 2] = [
        (
            "shared dimmer",
            dimmer,
            [
                "37 light 0: intensity",
                "38 light 1: red × intensity",
                "39 light 2: red × intensity",
            ],
            [255, 255, 0],
        ),
        (
            "shared strobe",
            strobe,
            [
                "37 = 8",
                "38 light 1: red × intensity",
                "39 light 2: red × intensity",
            ],
            [8, 255, 0],
        ),
    ];
    for (kind, shared, writers, bytes) in cases {
        let definition = format!(
            r#"<?xml version="1.0"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Test</Manufacturer><Model>Shared</Model><Type>LED Bar (Pixels)</Type>
 {shared}
 <Channel Name="Red 1" Preset="IntensityRed"/>
 <Channel Name="Red 2" Preset="IntensityRed"/>
 <Mode Name="Shared">
  <Channel Number="0">Shared</Channel>
  <Channel Number="1">Red 1</Channel>
  <Channel Number="2">Red 2</Channel>
  <Head><Channel>0</Channel><Channel>1</Channel></Head>
  <Head><Channel>0</Channel><Channel>2</Channel></Head>
 </Mode>
</FixtureDefinition>"#
        );
        let fixture = fixture_library::parse("shared.qxf", definition.as_bytes()).unwrap();
        let rigged = compile(
            &fixture.modes[0],
            at(37),
            light(0),
            Universes::ONE,
            lights(),
        )
        .unwrap();
        assert_eq!(rigged.lights, 3, "{kind}: the fixture, then each head");
        assert_eq!(
            rigged.writers.iter().map(say).collect::<Vec<_>>(),
            writers,
            "{kind}"
        );
        let mut out = [Levels::ZERO];
        resolve(&[FULL, RED, Light::DARK], &rigged.writers, &mut out);
        assert_eq!(out[0].0[36..39], bytes, "{kind}");
    }
}
