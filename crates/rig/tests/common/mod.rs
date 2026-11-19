//! Reading the test definitions and saying a writer in words, for the tables.

#![allow(dead_code, reason = "each test file uses its own part")]

use engine_core::{
    Channel, ChannelNumbers, Color, Less, LightNumber, Lights, MAX_LIGHTS, OutputKind, Part, Place,
    Scale, Sense, Take, Universes, Writer,
};
use fixture_library::{Fixture, Mode, parse};
use rig::Whole;
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

/// A writer in words: `37 = 8`, `38 light 0: intensity`, `38+39 light 0: red - shared`.
pub fn say(writer: &Writer) -> String {
    match *writer {
        Writer::Fixed { at, value } => format!("{} = {value}", address(at)),
        Writer::Takes { light, take, at } => {
            let at = match at {
                Place::Byte(at) => address(at).to_string(),
                Place::Pair { coarse, fine } => format!("{}+{}", address(coarse), address(fine)),
            };
            format!("{at} light {}: {}", light.index(), say_take(take))
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
    words
}
