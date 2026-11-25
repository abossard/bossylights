//! Reading the test definitions and saying a writer in words, for the tables.

#![allow(dead_code, reason = "each test file uses its own part")]

use engine_core::{
    Channel, ChannelNumbers, Color, Field, Levels, Light, LightNumber, Lights, MAX_LIGHTS,
    OutputKind, Universes,
};
use fixture_library::{Fixture, Mode, parse};
use rig::writer::{Less, Part, Place, Scale, Sense, Take, Travel, Writer};
use rig::{Rigged, Whole};
use std::num::NonZeroU16;
use std::path::PathBuf;

/// The folders of real QLC+ definitions: the fixture library's and the rig's own.
pub fn folders() -> [PathBuf; 2] {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    [
        here.join("../fixture-library/tests/definitions"),
        here.join("tests/definitions"),
    ]
}

/// A definition by file name, from either folder.
pub fn fixture(file: &str) -> Fixture {
    let path = folders()
        .into_iter()
        .map(|folder| folder.join(file))
        .find(|path| path.exists())
        .unwrap_or_else(|| panic!("no definition {file}"));
    parse(file, &std::fs::read(path).unwrap()).unwrap()
}

/// Every definition of both folders.
pub fn all() -> Vec<(String, Fixture)> {
    let mut found: Vec<_> = folders()
        .into_iter()
        .flat_map(|folder| std::fs::read_dir(folder).unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "qxf"))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let fixture = parse(&name, &std::fs::read(&path).unwrap()).unwrap();
            (name, fixture)
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

pub fn mode(file: &str, name: &str) -> Mode {
    fixture(file)
        .modes
        .into_iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("{file} has no mode {name}"))
}

/// What outputs send, as `outputs` says: DDP whole pixels, a DMX USB Pro every channel.
pub const fn whole() -> Whole {
    Whole(|kind| {
        let channels = match kind {
            OutputKind::Ddp {
                color: Color::Rgb, ..
            } => 3,
            OutputKind::Ddp {
                color: Color::Rgbw, ..
            } => 4,
            OutputKind::DmxUsbPro { .. } => 1,
        };
        NonZeroU16::new(channels).unwrap()
    })
}

pub fn lights() -> Lights {
    Lights::new(MAX_LIGHTS).unwrap()
}

pub fn light(index: u16) -> LightNumber {
    LightNumber::new(index, lights()).unwrap()
}

/// Universe 1, `address`.
pub fn at(address: u16) -> ChannelNumbers {
    ChannelNumbers {
        universe: 1,
        address,
    }
}

/// The address of a channel of universe 1, as a client counts it.
pub fn address(channel: Channel) -> u16 {
    (1..=512)
        .find(|&a| at(a).check(Universes::ONE) == Ok(channel))
        .expect("a channel of universe 1")
}

/// A compiled mode of universe 1 in words, in channel order: each writer by [`say`], and
/// `37 = 8` for a channel no writer writes, which shows its rest.
pub fn said(rigged: &Rigged) -> Vec<String> {
    let mut lines: Vec<(u16, String)> = Vec::new();
    let mut written = Vec::new();
    let mut last = 0;
    for writer in &rigged.writers {
        let at = match *writer {
            Writer::Takes {
                at: Place::Byte(at),
                ..
            }
            | Writer::Picks { at, .. }
            | Writer::Fixed { at, .. } => {
                written.push(address(at));
                address(at)
            }
            Writer::Takes {
                at: Place::Pair { coarse, fine },
                ..
            } => {
                written.extend([address(coarse), address(fine)]);
                address(coarse)
            }
            Writer::ColourSlot { .. } | Writer::GoboSlot { .. } => last,
        };
        last = at;
        lines.push((at, say(writer)));
    }
    for configured in &rigged.configured {
        let at = u16::try_from(configured.at % 512 + 1).unwrap();
        if !written.contains(&at) {
            lines.push((at, format!("{at} = {}", configured.rest)));
        }
    }
    lines.sort_by_key(|(at, _)| *at);
    lines.into_iter().map(|(_, line)| line).collect()
}

/// What `rigged` sends for `lights` into `out`, as a frame makes it with these values
/// alone: each channel of the mode at its rest, then each light's offers on its fields.
pub fn shown(lights: &[Light], rigged: &Rigged, out: &mut [Levels]) {
    for configured in &rigged.configured {
        put(out, configured.at, configured.rest);
    }
    for (fields, value) in rigged.fields.iter().zip(lights) {
        for (field, word) in fields.offers(value) {
            offer(out, field, word);
        }
    }
}

/// `word` on `field` in `out`: a byte takes its high byte, a pair both.
pub fn offer(out: &mut [Levels], field: Field, word: u16) {
    let [high, low] = word.to_be_bytes();
    match field {
        Field::Byte(at) => put(out, at, high),
        Field::Word(pair) => {
            put(out, pair.coarse, high);
            put(out, pair.fine, low);
        }
    }
}

/// `byte` at channel `at`, counted from 0 over the universes in a row.
fn put(out: &mut [Levels], at: u32, byte: u8) {
    let at = at as usize;
    if let Some(row) = out.get_mut(at / 512) {
        row.0[at % 512] = byte;
    }
}

/// A writer in words: `37 = 8`, `38 light 0: intensity`, `38+39 light 0: red - shared`,
/// a wheel `42 light 0: 8 slots, else 0` and each slot after it, `  #ff0000 = 7`,
/// `  gobo 1/3 = 24`.
pub fn say(writer: &Writer) -> String {
    match *writer {
        Writer::Fixed { at, value } => format!("{} = {value}", address(at)),
        Writer::Takes {
            light, take, at, ..
        } => {
            let at = match at {
                Place::Byte(at) => address(at).to_string(),
                Place::Pair { coarse, fine } => format!("{}+{}", address(coarse), address(fine)),
            };
            format!("{at} light {}: {}", light.index(), say_take(take))
        }
        Writer::Picks {
            light,
            at,
            slots,
            unheld,
        } => format!(
            "{} light {}: {slots} slots, else {unheld}",
            address(at),
            light.index()
        ),
        Writer::ColourSlot { colour, value } => format!(
            "  #{:02x}{:02x}{:02x} = {value}",
            colour.red >> 8,
            colour.green >> 8,
            colour.blue >> 8
        ),
        Writer::GoboSlot { gobo, value } => {
            format!("  gobo {}/{} = {value}", u8::from(gobo.wheel), gobo.slot)
        }
    }
}

fn say_take(take: Take) -> String {
    let mut words = match take.of {
        Part::Intensity => "intensity",
        Part::Red => "red",
        Part::Green => "green",
        Part::Blue => "blue",
        Part::Shared => "shared",
        Part::Pan => "pan",
        Part::Tilt => "tilt",
        Part::Zoom => "zoom",
    }
    .to_string();
    if take.less == Less::Shared {
        words += " - shared";
    }
    if take.scale == Scale::ByIntensity {
        words += " × intensity";
    }
    if take.sense == Sense::Falling {
        words = format!("full - {words}");
    }
    if take.travel != Travel::FULL {
        words += &format!(" over {}-{}", take.travel.low, take.travel.high);
    }
    words
}
