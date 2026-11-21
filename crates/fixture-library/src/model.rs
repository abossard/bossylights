use crate::error::{Refused, Warning};
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// One fixture definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    pub maker: String,
    pub model: String,
    pub kind: Kind,
    /// The modes that can be used. Never empty: a file with none is an error.
    pub modes: Vec<Mode>,
    /// The modes that cannot be used, each with why, in the file's order.
    pub refused: Vec<Refused>,
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// How far the head pans and tilts, from the mode or else the fixture. `None` when
    /// the definition says 0 for both, or nothing.
    pub degrees: Option<Degrees>,
}

/// The full pan and tilt range of a moving head or scanner, in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Degrees {
    pub pan: u16,
    pub tilt: u16,
}

/// The pixels of a bar or a panel in a grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub width: u16,
    pub height: u16,
}

/// One DMX channel. The v11 writers use its `function`. Its [`Self::role`] comes from
/// the declarations in its fixture and mode. Pairing also uses its ranges and heads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub name: String,
    pub function: Function,
    /// QXF declarations live only in memory. A v6 channel has none, starts with
    /// role `Other("")` and stays a single byte.
    #[serde(skip)]
    pub declared: Declared,
    /// The high byte, or the second byte of a 16-bit value.
    pub byte: Byte,
    /// The value the definition gives when nothing else is said. `None` when it gives none.
    pub default: Option<u8>,
    /// Which way the values run, when the channel preset says. `None` for a fine byte: it
    /// runs as its coarse byte.
    pub direction: Option<Direction>,
    pub ranges: Vec<Range>,
}

impl Channel {
    /// A plain byte has no steps. A single range is plain when it names no function
    /// or covers the whole byte.
    #[must_use]
    pub fn is_plain(&self) -> bool {
        match self.ranges.as_slice() {
            [] => true,
            [range] => range.does.is_none() || (range.min == 0 && range.max == u8::MAX),
            _ => false,
        }
    }
}

/// The channel's QLC+ declarations and how its fixture and heads use a dimmer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Declared {
    /// The preset as written, including its `Fine` suffix.
    pub preset: Option<String>,
    /// The text of the `<Group>` child.
    pub group: Option<String>,
    /// The emitter or subtractive colour named by the `<Colour>` child.
    pub colour: Option<Function>,
    /// The mode's `ActsOn`: the channel number (counted from 0) this channel acts on.
    pub acts_on: Option<u16>,
    pub(crate) dimming: Dimming,
}

/// Which declarations identify a light dimmer at this channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Dimming {
    /// A dimmer preset controls light.
    #[default]
    Preset,
    /// On a dimmer fixture, a group-only Intensity channel also controls light.
    DimmerPack,
    /// A Smoke or Hazer channel with steps, or without an emitter in its head
    /// (in its mode when headless). A dimmer preset here controls fog.
    Fog,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Byte {
    Coarse,
    Fine,
}

/// What a range of values on a channel does, in the words of the definition, and in
/// the model's words when the definition says it by preset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub min: u8,
    pub max: u8,
    pub label: String,
    pub does: Option<Does>,
}

/// What a range does, from its QLC+ capability preset (`engine/src/qlccapability.h`).
/// The presets left as the label only, on purpose: `ColorWheelIndex` (an index has no
/// fixed meaning), `GenericPicture` (a picture of an effect, not a gobo), `SilentMode*`
/// (the fan, no light), `Alias` (another channel's meaning, a later slice).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Does {
    /// `ShutterOpen`: light comes out.
    Open,
    /// `ShutterClose`: no light comes out.
    Closed,
    /// `Strobe*`, `Pulse*`, `Ramp*`: the light flashes, at this pace.
    Strobe(Pace),
    /// `ColorMacro` with its colour.
    Color(Rgb),
    /// `ColorDoubleMacro`: a split colour, both halves.
    Colors(Rgb, Rgb),
    /// `Rotation*`: a wheel, a gobo or a prism turns, or stops.
    Rotation(Turn),
    /// `PrismEffectOn`, with its number of faces when the definition gives it.
    Prism { faces: Option<u8> },
    /// `PrismEffectOff`.
    PrismOff,
    /// `GoboMacro`, `GoboShakeMacro`, or a legacy `Res`: the gobo, its picture as QLC+
    /// names the file (`Others/open.svg`) when it gives one, and whether it shakes.
    Gobo {
        picture: Option<String>,
        shakes: bool,
    },
    /// `LampOn`.
    LampOn,
    /// `LampOff`.
    LampOff,
    /// `Reset*`: all, pan and tilt, gobo, and so on.
    Reset,
    /// `SlowToFast`, `NearToFar`, `SmallToBig` (up) and their reverses (down): the range
    /// sweeps what its channel does.
    Runs(Direction),
}

/// Which way the values of a channel or a range run: more of what it does as the value
/// rises (faster, bigger, farther, more open), or less.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
}

/// How fast something flashes or turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pace {
    /// The definition does not say.
    Unsaid,
    /// From slow to fast (up) or fast to slow (down) across the range.
    Goes(Direction),
    /// At random. `StrobeRandomSlowToFast` and its reverse keep only this: 469 ranges of
    /// the library lose which way they run.
    Random,
    /// In hertz, from the start of the range to its end.
    Hertz(Hertz),
}

/// A frequency from the start of a range to its end, in millihertz: 0.5 Hz is 500.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hertz {
    pub from: u32,
    pub to: u32,
}

/// What a rotation does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Turn {
    Stop,
    /// Turned to a position, not spinning.
    Indexed,
    Clockwise(Pace),
    CounterClockwise(Pace),
}

/// A colour as a definition gives it: `#rrggbb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

/// What a channel does. Every QLC+ channel preset has an entry, or is `Other` on purpose
/// (see `function.rs`). `Other` keeps the name of what is not in the list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
