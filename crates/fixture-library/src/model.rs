use crate::error::Warning;
use alloc::string::String;
use alloc::vec::Vec;

/// One fixture definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    pub maker: String,
    pub model: String,
    pub kind: Kind,
    pub modes: Vec<Mode>,
    /// What was left out so the rest could be read. Empty for a definition without faults.
    pub warnings: Vec<Warning>,
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
    /// of that pixel. Empty for a fixture without pixels. A head that names a channel the
    /// mode does not have is left out, with a warning.
    pub pixels: Vec<Vec<u16>>,
    /// The grid of the pixels, from the mode or else the fixture. Left out, with a
    /// warning, when the mode has heads and the grid does not have exactly one cell for each.
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
    /// The value the definition gives when nothing else is said. `None` when it gives none.
    pub default: Option<u8>,
    pub ranges: Vec<Range>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Byte {
    Coarse,
    Fine,
}

/// What a range of values on a channel does, in the words of the definition, and in
/// the model's words when the definition says it by preset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range {
    pub min: u8,
    pub max: u8,
    pub label: String,
    pub does: Option<Does>,
}

/// What a range does, from its QLC+ capability preset. Only what the Rig needs (open,
/// closed, strobe, lamp on, reset) and what the colour wheel slice needs (a colour).
/// Every other capability preset says nothing here: its label still does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Does {
    /// `ShutterOpen`: light comes out.
    Open,
    /// `ShutterClose`: no light comes out.
    Closed,
    /// `Strobe*`, `Pulse*`, `Ramp*`: the light flashes.
    Strobe,
    /// `ColorMacro` with its colour.
    Color(Rgb),
    /// `ColorDoubleMacro`: a split colour, both halves.
    Colors(Rgb, Rgb),
    /// `LampOn`.
    LampOn,
    /// `Reset*`: all, pan and tilt, gobo, and so on.
    Reset,
}

/// A colour as a definition gives it: `#rrggbb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

/// What a channel does. Every QLC+ channel preset has an entry, or is `Other` on purpose
/// (see `function.rs`). `Other` keeps the name of what is not in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Function {
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
    /// A shutter, or what QLC+ groups with it without a preset: blades, some irises.
    Shutter,
    Prism,
    /// Of pan, tilt or both, either direction.
    Speed,
    Effect,
    /// Maintenance: reset, lamp, settings.
    Control,
    /// The definition says the channel does nothing.
    NoFunction,
    Other(String),
}
