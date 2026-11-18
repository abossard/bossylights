use alloc::string::String;
use alloc::vec::Vec;

/// One fixture definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    pub maker: String,
    pub model: String,
    pub kind: Kind,
    pub modes: Vec<Mode>,
}

/// The type of fixture, as QLC+ names them. A name it does not list is kept as `Unlisted`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
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
    Unlisted(String),
}

/// A way to run the fixture: how many channels it takes and what each does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mode {
    pub name: String,
    /// In DMX order: the first is address 1 of the fixture.
    pub channels: Vec<Channel>,
    /// One entry per pixel of a bar or a panel, each the channel numbers (counted from 0)
    /// of that pixel. Empty for a fixture without pixels.
    pub pixels: Vec<Vec<u16>>,
    pub layout: Option<Layout>,
}

/// The pixels of a bar or a panel in a grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    pub name: String,
    pub function: Function,
    /// The high byte, or the second byte of a 16-bit value.
    pub byte: Byte,
    pub ranges: Vec<Range>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Byte {
    Coarse,
    Fine,
}

/// What a range of values on a channel does, in the words of the definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range {
    pub min: u8,
    pub max: u8,
    pub label: String,
}

/// What a channel does. `Other` keeps the name of what is not in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Function {
    Dimmer,
    Red,
    Green,
    Blue,
    White,
    Amber,
    Uv,
    Lime,
    Cyan,
    Magenta,
    Yellow,
    Pan,
    Tilt,
    Zoom,
    Focus,
    Gobo,
    ColorWheel,
    Strobe,
    Shutter,
    Prism,
    Speed,
    Rotation,
    Effect,
    Control,
    Other(String),
}
