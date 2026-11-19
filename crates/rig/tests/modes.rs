//! A mode at an address becomes writers: real definitions, at universe 1, address 37.

mod common;

use common::{all, at, light, lights, mode, say};
use engine_core::{Refusal, Universes, Writer};
use rig::{Unfit, compile};

#[test]
fn a_mode_at_an_address_becomes_writers() {
    // (definition, mode, lights it makes, writers in words)
    let cases: [(&str, &str, u16, &[&str]); 8] = [
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
            // a moving head with a 16-bit dimmer and a shutter: pan, tilt, zoom and the
            // colour wheel are the next slice, at their default
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
                "45 = 0",
                "46 = 0",
                "47 = 0",
                "48 = 0",
                "49 = 0",
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
    let martin = mode("Martin-Rush-MH6-Wash.qxf", "Standard"); // 14 channels
    let bar = mode("XStatic-X-240Bar-RGB.qxf", "26 Channel"); // 9 lights
    let two = Universes::new(2).unwrap();
    // (mode, universe, address, first light, result)
    let cases = [
        (&martin, 1, 499, 0, Ok(13)),
        (&martin, 2, 499, 0, Ok(13)),
        // past 512 it runs on at the next universe's address 1, but there is no third
        (&martin, 1, 500, 0, Ok(13)),
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
#[test]
fn every_channel_is_written_once() {
    for (file, fixture) in all() {
        for mode in &fixture.modes {
            let rigged = compile(mode, at(1), light(0), Universes::ONE, lights()).unwrap();
            let mut written: Vec<u16> = rigged
                .writers
                .iter()
                .flat_map(|writer| match *writer {
                    Writer::Fixed { at, .. } => vec![common::address(at)],
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
