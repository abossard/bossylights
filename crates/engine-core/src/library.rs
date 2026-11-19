//! The fixture library as the API shows it: where reading the folder is, and the answers
//! to `Search`, `Fixture` and `Failures`. Plain data that borrows what the engine read;
//! `crates/engine-server` reads the folder and fills these in.

use core::fmt;
use serde::de::{Deserializer, Error, Visitor};
use serde::{Deserialize, Serialize};

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

/// A text a client sends: a search, a maker, a model. At most [`Text::MOST`] bytes of
/// UTF-8, kept in place so a request needs no heap.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Text {
    len: u8,
    bytes: [u8; Text::MOST],
}

impl Text {
    /// Longer than any maker, model or mode name of the QLC+ library (66 at most).
    pub const MOST: usize = 128;

    /// `None` when `text` is longer than [`Text::MOST`] bytes.
    pub fn new(text: &str) -> Option<Self> {
        let mut bytes = [0; Self::MOST];
        bytes
            .get_mut(..text.len())?
            .copy_from_slice(text.as_bytes());
        Some(Self {
            len: u8::try_from(text.len()).ok()?,
            bytes,
        })
    }

    pub fn as_str(&self) -> &str {
        let bytes = self.bytes.get(..usize::from(self.len)).unwrap_or_default();
        core::str::from_utf8(bytes).unwrap_or_default() // made from a `&str`: always UTF-8
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl Serialize for Text {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Short;
        impl Visitor<'_> for Short {
            type Value = Text;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a string of at most {} bytes", Text::MOST)
            }
            fn visit_str<E: Error>(self, text: &str) -> Result<Text, E> {
                Text::new(text).ok_or_else(|| E::invalid_length(text.len(), &self))
            }
        }
        deserializer.deserialize_str(Short)
    }
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
    pub problem: &'a str,
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

#[cfg(test)]
mod tests {
    use super::Text;

    #[test]
    fn a_text_keeps_up_to_most_bytes() {
        let most = "a".repeat(Text::MOST);
        let over = "a".repeat(Text::MOST + 1);
        let cases: [(&str, Option<&str>); 5] = [
            ("", Some("")),
            ("rgbw", Some("rgbw")),
            ("BL63 10\" LED Bar", Some("BL63 10\" LED Bar")),
            (&most, Some(&most)),
            (&over, None),
        ];
        for (text, expected) in cases {
            assert_eq!(
                Text::new(text).as_ref().map(Text::as_str),
                expected,
                "{text}"
            );
        }
    }
}
