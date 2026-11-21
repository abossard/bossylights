//! Reading the test definitions, and saying a compiled mode in words and bytes, for the
//! tables.

#![allow(dead_code, reason = "each test file uses its own part")]

use engine_core::{
    ChannelNumbers, Color, Field, Light, LightNumber, Lights, MAX_LIGHTS, MixRules, Offer,
    OutputKind,
};
use fixture_library::{Fixture, Mode, parse};
use rig::{
    Choice, Feed, Less, LightFields, Part, Patch, Rigged, Scale, Sense, Take, Travel, Whole,
};
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

/// The address of a flat channel of universe 1, as a client counts it.
pub fn address(channel: u32) -> u32 {
    channel + 1
}

/// A compiled mode in words, one line per channel in channel order: `37 = 8` at its rest,
/// `38 light 0: intensity` or `38+39 light 0: red - shared` for a field (the fine of a
/// pair is said with its coarse), a wheel `42 light 0: 8 slots, else 0` and each slot
/// after it, `  #ff0000 = 7`, `  gobo 1/3 = 24`. A light is counted from the mode's first.
pub fn say(rigged: &Rigged) -> Vec<String> {
    let channels = rigged.lowered.channels.iter().map(|c| (c.at, c.rest));
    say_channels(channels, &rigged.targets)
}

/// Every channel of every fixture of `patch` in words, as [`say`]: a light is its number,
/// counted from 0.
pub fn say_patched(patch: &Patch) -> Vec<String> {
    let rest = patch.config(MixRules::default()).rest;
    let channels = patch.fixtures().into_iter().flat_map(|row| {
        let first = u32::from(row.universe - 1) * 512 + u32::from(row.address - 1);
        (first..first + u32::from(row.channels)).map(|at| (at, rest[at as usize]))
    });
    say_channels(channels, patch.targets())
}

/// `channels` (each with its rest) in words, the fields of `targets` on them.
fn say_channels(channels: impl Iterator<Item = (u32, u8)>, targets: &[LightFields]) -> Vec<String> {
    let fields: Vec<(usize, Field, &Feed)> = targets
        .iter()
        .enumerate()
        .flat_map(|(light, target)| {
            target
                .fields
                .iter()
                .zip(target.feeds())
                .map(move |(&at, feed)| (light, at, feed))
        })
        .collect();
    let mut said = Vec::new();
    for (channel, rest) in channels {
        let fine = fields
            .iter()
            .any(|&(_, at, _)| matches!(at, Field::Word { fine, .. } if fine == channel));
        if fine {
            continue;
        }
        let field = fields.iter().find(|&&(_, at, _)| match at {
            Field::Byte(at) | Field::Word { coarse: at, .. } => at == channel,
        });
        let Some(&(light, at, feed)) = field else {
            said.push(format!("{} = {rest}", address(channel)));
            continue;
        };
        let at = match at {
            Field::Byte(at) => address(at).to_string(),
            Field::Word { coarse, fine } => format!("{}+{}", address(coarse), address(fine)),
        };
        match feed {
            Feed::Takes(take) => said.push(format!("{at} light {light}: {}", say_take(*take))),
            Feed::Picks(slots) => {
                said.push(format!(
                    "{at} light {light}: {} slots, else {rest}",
                    slots.len(),
                ));
                said.extend(slots.iter().map(|&(choice, value)| match choice {
                    Choice::Colour(colour) => format!(
                        "  #{:02x}{:02x}{:02x} = {value}",
                        colour.red >> 8,
                        colour.green >> 8,
                        colour.blue >> 8
                    ),
                    Choice::Gobo(gobo) => {
                        format!("  gobo {}/{} = {value}", u8::from(gobo.wheel), gobo.slot)
                    }
                }));
            }
        }
    }
    said
}

pub fn say_take(take: Take) -> String {
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

/// The bytes of a compiled mode's channels, in channel order, with light `n` of the mode
/// showing `values[n]`: each channel at its rest, then each offer laid on it.
pub fn shown(rigged: &Rigged, values: &[Light]) -> Vec<u8> {
    let mut bytes: Vec<u8> = rigged.lowered.channels.iter().map(|c| c.rest).collect();
    let first = rigged.lowered.channels.first().map_or(0, |c| c.at);
    let offers = rigged
        .targets
        .iter()
        .zip(values)
        .flat_map(|(target, value)| target.offers(value));
    lay(&mut bytes, first, offers);
    bytes
}

/// Every channel of the engine as `patch` shows it with each light of `values` holding
/// its value: each channel at its rest ([`Patch::config`]), then each offer laid on it.
pub fn shown_by(patch: &Patch, values: &[(LightNumber, Light)]) -> Vec<u8> {
    let mut bytes = patch.config(MixRules::default()).rest.to_vec();
    let offers = values.iter().flat_map(|(light, value)| {
        patch
            .target(*light)
            .into_iter()
            .flat_map(move |target| target.offers(value))
    });
    lay(&mut bytes, 0, offers);
    bytes
}

/// Lays `offers` on `bytes`, which start at channel `first`: a byte field takes the high
/// byte of the word, a pair both, the coarse high.
pub fn lay(bytes: &mut [u8], first: u32, offers: impl Iterator<Item = Offer>) {
    let mut set = |at: u32, byte: u8| {
        let slot = usize::try_from(at - first).unwrap();
        bytes[slot] = byte;
    };
    for offer in offers {
        let [high, low] = offer.value.to_be_bytes();
        match offer.at {
            Field::Byte(at) => set(at, high),
            Field::Word { coarse, fine } => {
                set(coarse, high);
                set(fine, low);
            }
        }
    }
}
