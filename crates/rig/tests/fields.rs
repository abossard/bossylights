//! A light's value as words on its fields (#250): a table per mode worked by hand, and
//! the fields the patch gives each light.

mod common;

use common::{at, fixture, light, lights, mode, whole};
use engine_core::{
    ChannelNumbers, Light, LightNumber, Lights, PatchCommand, Position, Rgb, Text, Universes,
};
use rig::channels::Field;
use rig::{Patch, Placed, compile};
use std::num::NonZeroU16;

const FULL: u16 = u16::MAX;
const HALF: u16 = 0x8000;

/// A channel counted from 0 over the universes in a row, as a client counts it: `37`
/// in universe 1, `2.1` past it.
fn said_at(channel: u32) -> String {
    match (channel / 512 + 1, channel % 512 + 1) {
        (1, address) => address.to_string(),
        (universe, address) => format!("{universe}.{address}"),
    }
}

/// Offers in words, in channel order: `37=255` a channel and its byte, `52+53=8000` a
/// pair and its word.
fn said(offers: impl Iterator<Item = (Field, u16)>) -> String {
    offers
        .map(|(at, word)| match at {
            Field::Byte(at) => format!("{}={}", said_at(at), word >> 8),
            Field::Word(pair) => {
                format!("{}+{}={word:04x}", said_at(pair.coarse), said_at(pair.fine))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn colour(red: u16, green: u16, blue: u16) -> Light {
    Light {
        colour: Some(Rgb { red, green, blue }),
        ..Light::DARK
    }
}

/// The light of `index` of `mode` at 37 offering `value`.
fn offered(file: &str, name: &str, index: usize, value: Light) -> String {
    let rigged = compile(
        &mode(file, name),
        at(37),
        light(0),
        Universes::ONE,
        lights(),
    )
    .unwrap();
    said(rigged.fields[index].offers(&value))
}

#[test]
fn a_lights_value_becomes_words_on_its_fields() {
    let red = colour(FULL, 0, 0);
    let mac = Light {
        position: Some(Position {
            pan: HALF,
            tilt: FULL,
        }),
        ..colour(FULL, HALF, 0)
    };
    let evc = Light {
        position: Some(Position {
            pan: 0x4000,
            tilt: 0xc000,
        }),
        ..colour(FULL, FULL, HALF)
    };
    let mut spot = Light {
        position: Some(Position {
            pan: 0x1234,
            tilt: 0xabcd,
        }),
        ..red
    };
    spot.gobos[0] = Some(3);
    let (none, mac_moves, evc_moves, spot_picks) = (
        "",
        "39=0 40=127 41=255 52+53=8000 54+55=ffff",
        "37+38=4000 39+40=c000",
        "39+48=1234 40+49=abcd 42=7 43=24",
    );
    let both = |brightness: &str, rest: &str| format!("{brightness} {rest}");
    // (mode, light, the value without brightness,
    //  [brightness 0, a half, full alone], [with the value], [the value alone])
    let cases: [(_, _, Light, [&str; 3], [String; 3], String); 10] = [
        // fog and haze never take a brightness (#295)
        (
            ("Stairville-Hz-200-DMX.qxf", "Default"),
            0,
            red,
            [none; 3],
            [none.into(), none.into(), none.into()],
            none.into(),
        ),
        (
            ("Generic-Generic-Smoke.qxf", "Normal"),
            0,
            red,
            [none; 3],
            [none.into(), none.into(), none.into()],
            none.into(),
        ),
        // a dimmer carries the colour
        (
            ("Generic-Generic-RGB.qxf", "Dimmer RGB"),
            0,
            red,
            ["37=0", "37=128", "37=255"],
            [
                "37=0 38=255 39=0 40=0".into(),
                "37=128 38=255 39=0 40=0".into(),
                "37=255 38=255 39=0 40=0".into(),
            ],
            "38=255 39=0 40=0".into(),
        ),
        // no dimmer: brightness is in the colour, a colour alone as it is (cx-4)
        (
            ("Generic-Generic-RGB.qxf", "RGB"),
            0,
            red,
            [none; 3],
            [
                "37=0 38=0 39=0".into(),
                "37=128 38=0 39=0".into(),
                "37=255 38=0 39=0".into(),
            ],
            "37=255 38=0 39=0".into(),
        ),
        // white shows the shared quarter: red - shared is 0xbfff, a half of it 0x6000
        (
            ("Generic-Generic-RGBW.qxf", "RGBW"),
            0,
            colour(FULL, HALF, 0x4000),
            [none; 3],
            [
                "37=0 38=0 39=0 40=0".into(),
                "37=96 38=32 39=0 40=32".into(),
                "37=191 38=64 39=0 40=64".into(),
            ],
            "37=191 38=64 39=0 40=64".into(),
        ),
        // a bar's shared dimmer at 50 is the fixture's own light, light 0
        (
            ("Stairville-CLB4-RGB-Compact-LED-Bar-4.qxf", "14 Channel"),
            0,
            red,
            ["50=0", "50=128", "50=255"],
            ["50=0".into(), "50=128".into(), "50=255".into()],
            none.into(),
        ),
        // head 1 has no dimmer of its own: its brightness is in its colour, as on a
        // light without any (an interim rule until light values leave, #253)
        (
            ("Stairville-CLB4-RGB-Compact-LED-Bar-4.qxf", "14 Channel"),
            1,
            red,
            [none; 3],
            [
                "37=0 38=0 39=0".into(),
                "37=128 38=0 39=0".into(),
                "37=255 38=0 39=0".into(),
            ],
            "37=255 38=0 39=0".into(),
        ),
        // cyan, magenta, yellow beside a dimmer; pan and tilt as words
        (
            ("Martin-MAC-2000-Wash.qxf", "16-bit with Color Wheel 2"),
            0,
            mac,
            ["38=0", "38=128", "38=255"],
            [
                both("38=0", mac_moves),
                both("38=128", mac_moves),
                both("38=255", mac_moves),
            ],
            mac_moves.into(),
        ),
        // a mover without a dimmer: its two whites show the shared half, scaled
        (
            ("Elation-EVC-MH.qxf", "12-CH"),
            0,
            evc,
            [none; 3],
            [
                both(evc_moves, "43=0 44=0"),
                both(evc_moves, "43=64 44=64"),
                both(evc_moves, "43=128 44=128"),
            ],
            both(evc_moves, "43=128 44=128"),
        ),
        // wheels pick their slot's byte: red is 7, gobo 1/3 is 24. Pan and tilt pair
        // channels 3 and 12, 4 and 13
        (
            ("Shehds-LED-Spot-100W.qxf", "14 Channel"),
            0,
            spot,
            ["37=0", "37=128", "37=255"],
            [
                both("37=0", spot_picks),
                both("37=128", spot_picks),
                both("37=255", spot_picks),
            ],
            spot_picks.into(),
        ),
    ];
    for ((file, name), index, value, alone, with, without) in cases {
        let at = |brightness: Option<u16>, value: Light| {
            offered(
                file,
                name,
                index,
                Light {
                    intensity: brightness,
                    ..value
                },
            )
        };
        let levels = [0, HALF, FULL];
        let got_alone = levels.map(|level| at(Some(level), Light::DARK));
        let got_with = levels.map(|level| at(Some(level), value));
        assert_eq!(
            got_alone, alone,
            "{file} {name} light {index} brightness alone"
        );
        assert_eq!(
            got_with, with,
            "{file} {name} light {index} with brightness"
        );
        assert_eq!(
            at(None, value),
            without,
            "{file} {name} light {index} alone"
        );
    }
}

fn text(text: &str) -> Text {
    Text::new(text).unwrap()
}

fn patch(maker: &str, model: &str, mode: &str, universe: u16, address: u16) -> PatchCommand {
    PatchCommand::Patch {
        name: text(model),
        maker: text(maker),
        model: text(model),
        mode: text(mode),
        at: ChannelNumbers { universe, address },
    }
}

#[test]
fn the_patch_gives_each_light_its_fields() {
    let library = [
        "Generic-Generic-RGB.qxf",
        "Martin-MAC-2000-Wash.qxf",
        "Stairville-CLB4-RGB-Compact-LED-Bar-4.qxf",
    ]
    .map(fixture);
    let lights = Lights::new(8).unwrap();
    let empty = Patch::new(Universes::new(2).unwrap(), lights, whole());
    let par = |universe, address| patch("Generic", "Generic RGB", "RGB", universe, address);
    let mac = patch(
        "Martin",
        "MAC 2000 Wash",
        "16-bit with Color Wheel 2",
        1,
        101,
    );
    let bar = patch(
        "Stairville",
        "CLB4 RGB Compact LED Bar 4",
        "14 Channel",
        1,
        1,
    );
    let moved = PatchCommand::Move {
        fixture: 1,
        at: ChannelNumbers {
            universe: 2,
            address: 1,
        },
    };
    let gone = PatchCommand::Unpatch { fixture: 1 };
    let by = |commands: Vec<PatchCommand>| {
        commands.into_iter().fold(empty.clone(), |patch, command| {
            patch.update(command, &library).unwrap().0.unwrap_or(patch)
        })
    };
    // a show file's PAR at 1.37, its first light 2
    let opened = empty
        .restore(vec![Placed {
            fixture: NonZeroU16::new(1).unwrap(),
            name: "PAR".into(),
            maker: "Generic".into(),
            model: "Generic RGB".into(),
            mode: mode("Generic-Generic-RGB.qxf", "RGB"),
            at: ChannelNumbers {
                universe: 1,
                address: 37,
            },
            first: 2,
        }])
        .unwrap();
    let mac_fields = "102 103 104 105 109 116+117 118+119";
    // (patch, the fields of lights 0, 1, 2 in words; - for a light no fixture has,
    //  the dimmer channels: the MAC's "Dimmer blades", the bar's "Dimmer")
    let cases: [(Patch, [&str; 3], &str); 6] = [
        (by(vec![par(1, 37)]), ["37 38 39", "-", "-"], ""),
        (
            by(vec![par(1, 37), mac]),
            ["37 38 39", mac_fields, "-"],
            "102",
        ),
        (
            by(vec![par(1, 37), mac, moved]),
            ["2.1 2.2 2.3", mac_fields, "-"],
            "102",
        ),
        (
            by(vec![par(1, 37), mac, gone]),
            ["-", mac_fields, "-"],
            "102",
        ),
        // the bar's own light is its shared dimmer, then a light per head
        (by(vec![bar]), ["14", "1 2 3", "4 5 6"], "14"),
        (opened, ["-", "37 38 39", "-"], ""),
    ];
    for (patch, want, dimmers) in cases {
        let got = [0, 1, 2].map(|index| {
            patch
                .fields(LightNumber::new(index, lights).unwrap())
                .map_or("-".to_string(), |fields| {
                    fields
                        .fields()
                        .map(|at| match at {
                            Field::Byte(at) => said_at(at),
                            Field::Word(pair) => {
                                format!("{}+{}", said_at(pair.coarse), said_at(pair.fine))
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
        });
        assert_eq!(got, want);
        let table = patch.prepare();
        let got: Vec<_> = table.dimmers().iter().map(|at| said_at(*at)).collect();
        assert_eq!(got.join(" "), dimmers, "{want:?}");
    }
}
