//! The fixture library as the API shows it: where reading the folder is, and the answers
//! to `Search`, `Fixture` and `Failures`. Plain data that borrows what the engine read;
//! `crates/engine-server` reads the folder and fills these in.

use fixture_library::Problem;
use serde::Serialize;

/// The fixture library in the state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Library {
    /// The engine was started without `--fixtures`: the library is empty.
    NoFolder,
    /// The folder is being read.
    Reading,
    /// `read` definitions are in the library, `failed` files could not be read
    /// (`Failures` names them).
    Read { read: u32, failed: u32 },
    /// The folder could not be listed: the library is empty.
    CannotOpen,
}

/// A fixture `Search` found, with the size of each of its modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Match<'a> {
    pub maker: &'a str,
    pub model: &'a str,
    pub kind: FixtureKind<'a>,
    pub modes: &'a [ModeSize<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ModeSize<'a> {
    pub name: &'a str,
    pub channels: u16,
}

/// A mode of a fixture and its channels in DMX order: the first is address 1 of the fixture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ModeChannels<'a> {
    pub name: &'a str,
    pub channels: &'a [NamedChannel<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NamedChannel<'a> {
    pub name: &'a str,
    pub function: ChannelFunction<'a>,
}

/// A definition file that could not be read: which file (under the folder), the line
/// when the problem has one, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Failure<'a> {
    pub file: &'a str,
    pub line: Option<u32>,
    pub why: Why<'a>,
}

/// Why a definition file, or a mode of it, is not in the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Why<'a> {
    /// The file or folder could not be read, `with` the system's error number when it
    /// gave one.
    CannotRead { with: Option<i32> },
    /// It is no file: a folder, a pipe or a device with a `.qxf` name.
    NotAFile,
    /// The definition is wrong: no mode of it is read.
    Definition { problem: &'a Problem },
    /// The mode is wrong; the file's other modes are read without it.
    ModeRefused { mode: &'a str, problem: &'a Problem },
    /// The maker or the model is longer than a client may send (`engine_core::Text::MOST`
    /// bytes): `Fixture` could not ask for it.
    TooLong { what: Asked },
    /// A fixture of the same maker and model came first, from `file`.
    SameModelAs { file: &'a str },
}

/// What `Fixture` asks for a fixture by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Asked {
    Maker,
    Model,
}

/// The type of fixture, as QLC+ names them. A name it does not list is `Unlisted`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum FixtureKind<'a> {
    ColorChanger,
    Dimmer,
    Effect,
    Fan,
    Flower,
    Hazer,
    Laser,
    LedBarBeams,
    LedBarPixels,
    MovingHead,
    Other,
    Scanner,
    Smoke,
    Strobe,
    Unlisted { name: &'a str },
}

/// What a channel does. `Other` keeps the name of what is not in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum ChannelFunction<'a> {
    Dimmer,
    Red,
    Green,
    Blue,
    White,
    Amber,
    Uv,
    Indigo,
    Lime,
    Cyan,
    Magenta,
    Yellow,
    Hue,
    Saturation,
    Lightness,
    /// The V of HSV, apart from the L of HSL.
    Value,
    ColorTemperature,
    Pan,
    Tilt,
    Zoom,
    Focus,
    Iris,
    Frost,
    Gobo,
    ColorWheel,
    Strobe,
    /// A shutter, or what QLC+ groups with it: blades, some irises.
    Shutter,
    Prism,
    /// Of pan, tilt or both.
    Speed,
    Effect,
    /// Maintenance: reset, lamp, settings.
    Control,
    /// The definition says the channel does nothing.
    NoFunction,
    Other {
        name: &'a str,
    },
}
