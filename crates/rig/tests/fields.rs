//! A light's value as words on its fields (#250): a table per mode worked by hand, and
//! today's writers over every mode of the test definitions (and of the QLC+ library when
//! `QLCPLUS_FIXTURES` names its folder): the same bytes wherever cx-4 does not apply.

mod common;

use common::{at, fixture, light, lights, mode, whole};
use engine_core::{
    Channel, ChannelNumbers, Levels, Light, LightNumber, Lights, Part, PatchCommand, Place,
    Position, Rgb, Text, Universes, Writer,
};
use fixture_library::parse;
use rig::channels::{Field, Pair};
use rig::{Patch, Placed, Rigged, Unfit, compile};
use std::collections::BTreeSet;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::{env, fs};

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

/// QXF files under `folder`, at any depth.
fn qxf_files(folder: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            qxf_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "qxf") {
            files.push(path);
        }
    }
}

/// What a frame shows for `lights` over the mode's channels: each at its rest, then
/// today's writers.
fn written(rigged: &Rigged, lights: &[Light]) -> Vec<u8> {
    let mut out = vec![Levels::ZERO; 64];
    common::shown(lights, rigged, &mut out);
    rested(rigged, &out)
}

/// What a frame shows for `value` on light `index` alone: each channel at its rest, then
/// the light's offers.
fn offered_over_rest(rigged: &Rigged, index: usize, value: &Light) -> Vec<u8> {
    let mut out = vec![Levels::ZERO; 64];
    let mut put = |at: u32, byte: u8| out[at as usize / 512].0[at as usize % 512] = byte;
    for configured in &rigged.configured {
        put(configured.at, configured.rest);
    }
    for (at, word) in rigged.fields[index].offers(value) {
        let [high, low] = word.to_be_bytes();
        match at {
            Field::Byte(at) => put(at, high),
            Field::Word(pair) => {
                put(pair.coarse, high);
                put(pair.fine, low);
            }
        }
    }
    rested(rigged, &out)
}

fn rested(rigged: &Rigged, out: &[Levels]) -> Vec<u8> {
    rigged
        .configured
        .iter()
        .map(|c| out[c.at as usize / 512].0[c.at as usize % 512])
        .collect()
}

/// The full library's two colour wheels whose file rests at the white slot's middle
/// byte while their writer shows 0, the same slot: (file, mode, channel from 1).
const SAME_SLOT: [(&str, &str, usize); 2] = [
    ("Coemar-ProWash-250-LX.qxf", "Standard", 10),
    ("Laserworld-PRO-1600RGB.qxf", "18 channel", 4),
];

/// Whether light `index` has a writer that takes its brightness: a dimmer of its own.
fn has_own_dimmer(rigged: &Rigged, index: usize) -> bool {
    rigged.writers.iter().any(|writer| {
        matches!(writer, Writer::Takes { light, take, .. }
            if usize::from(light.index()) == index && take.of == Part::Intensity)
    })
}

#[test]
fn offers_show_what_todays_writers_show_wherever_cx_4_does_not_apply() {
    let mut paths = Vec::new();
    for folder in common::folders() {
        qxf_files(&folder, &mut paths);
    }
    let library = env::var_os("QLCPLUS_FIXTURES");
    if let Some(root) = &library {
        qxf_files(Path::new(root), &mut paths);
    }
    paths.sort();
    let tint = Rgb {
        red: FULL,
        green: HALF,
        blue: 0x4000,
    };
    let universes = Universes::new(64).unwrap();
    let (mut modes, mut undimmed) = (0, BTreeSet::new());
    let mut problems = Vec::new();
    for path in paths {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let Ok(fixture) = parse(&file, &fs::read(&path).unwrap()) else {
            continue;
        };
        for mode in &fixture.modes {
            let rigged = match compile(mode, at(1), light(0), universes, lights()) {
                Err(Unfit::Refused(_)) => continue,
                rigged => rigged.unwrap(),
            };
            modes += 1;
            // every choice of every wheel, at full brightness
            let picks = rigged.writers.iter().filter_map(|writer| {
                let mut light = Light {
                    intensity: Some(FULL),
                    ..Light::DARK
                };
                match *writer {
                    Writer::ColourSlot { colour, .. } => light.colour = Some(colour),
                    Writer::GoboSlot { gobo, .. } => {
                        light.gobos[usize::from(u8::from(gobo.wheel)) - 1] = Some(gobo.slot);
                    }
                    Writer::Fixed { .. } | Writer::Takes { .. } | Writer::Picks { .. } => {
                        return None;
                    }
                }
                Some(light)
            });
            let full = Light {
                intensity: Some(FULL),
                colour: Some(tint),
                position: Some(Position {
                    pan: 0x1234,
                    tilt: 0xabcd,
                }),
                zoom: Some(0x5678),
                ..Light::DARK
            };
            let same: Vec<Light> = [
                Light::DARK,
                Light {
                    intensity: Some(HALF),
                    ..Light::DARK
                },
                full,
                Light {
                    intensity: Some(HALF),
                    colour: Some(tint),
                    ..Light::DARK
                },
            ]
            .into_iter()
            .chain(picks)
            .collect();
            let tinted = |intensity| Light {
                intensity,
                colour: Some(tint),
                ..Light::DARK
            };
            // each channel of the mode, counted from 0 over the universes in a row
            let places: Vec<Channel> = (0..mode.channels.len())
                .map(|channel| {
                    ChannelNumbers {
                        universe: u16::try_from(channel / 512 + 1).unwrap(),
                        address: u16::try_from(channel % 512 + 1).unwrap(),
                    }
                    .check(universes)
                    .unwrap()
                })
                .collect();
            let flat = |at: Channel| {
                u32::try_from(places.iter().position(|&place| place == at).unwrap()).unwrap()
            };
            for index in 0..usize::from(rigged.lights) {
                let shown = |value: Light| {
                    let mut lights = vec![Light::DARK; usize::from(rigged.lights)];
                    lights[index] = value;
                    written(&rigged, &lights)
                };
                // the channels, counted from 0, where the offers show other bytes
                let differ = |value: &Light| -> Vec<usize> {
                    let (want, got) = (shown(*value), offered_over_rest(&rigged, index, value));
                    (0..want.len())
                        .filter(|&c| want[c] != got[c])
                        .filter(|&c| {
                            !SAME_SLOT.contains(&(file.as_str(), mode.name.as_str(), c + 1))
                        })
                        .collect()
                };
                let named = format!("{file} / {} / light {index}", mode.name);
                // the fields are where the light's writers write, in channel order
                let own = |light: engine_core::LightNumber| usize::from(light.index()) == index;
                let wrote: Vec<Field> = rigged
                    .writers
                    .iter()
                    .filter_map(|writer| match *writer {
                        Writer::Takes { light, at, .. } if own(light) => Some(match at {
                            Place::Byte(at) => Field::Byte(flat(at)),
                            Place::Pair { coarse, fine } => Field::Word(Pair {
                                coarse: flat(coarse),
                                fine: flat(fine),
                            }),
                        }),
                        Writer::Picks { light, at, .. } if own(light) => {
                            Some(Field::Byte(flat(at)))
                        }
                        Writer::Takes { .. }
                        | Writer::Picks { .. }
                        | Writer::Fixed { .. }
                        | Writer::ColourSlot { .. }
                        | Writer::GoboSlot { .. } => None,
                    })
                    .collect();
                let fields: Vec<Field> = rigged.fields[index].fields().collect();
                if fields != wrote {
                    problems.push(format!("{named}: fields {fields:?}, writers at {wrote:?}"));
                }
                for value in &same {
                    let moved = differ(value);
                    if !moved.is_empty() {
                        problems.push(format!("{named}: {value:?} differs at {moved:?}"));
                    }
                }
                // a colour alone on a light without a dimmer: the colour at full brightness,
                // where today's writers show it dark (cx-4)
                let alone = tinted(None);
                let moved = differ(&alone);
                if moved.is_empty() {
                    continue;
                }
                let (got, at_full) = (
                    offered_over_rest(&rigged, index, &alone),
                    shown(tinted(Some(FULL))),
                );
                if has_own_dimmer(&rigged, index) || moved.iter().any(|&c| got[c] != at_full[c]) {
                    problems.push(format!("{named}: {alone:?} differs at {moved:?}"));
                }
                undimmed.insert((file.clone(), mode.name.clone()));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
    println!(
        "{modes} modes; a colour alone shows at full instead of dark in {}",
        undimmed.len()
    );
    if library.is_none() {
        assert_eq!((modes, undimmed.len()), (175, 40));
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
    // (patch, the fields of lights 0, 1, 2 in words; - for a light no fixture has)
    let cases: [(Patch, [&str; 3]); 6] = [
        (by(vec![par(1, 37)]), ["37 38 39", "-", "-"]),
        (by(vec![par(1, 37), mac]), ["37 38 39", mac_fields, "-"]),
        (
            by(vec![par(1, 37), mac, moved]),
            ["2.1 2.2 2.3", mac_fields, "-"],
        ),
        (by(vec![par(1, 37), mac, gone]), ["-", mac_fields, "-"]),
        // the bar's own light is its shared dimmer, then a light per head
        (by(vec![bar]), ["14", "1 2 3", "4 5 6"]),
        (opened, ["-", "37 38 39", "-"]),
    ];
    for (patch, want) in cases {
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
    }
}
