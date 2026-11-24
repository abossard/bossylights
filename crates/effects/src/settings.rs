//! The kinds, the settings each one publishes, and an effect whose values are all in range.

use crate::Rgb;
use core::num::NonZeroU16;

/// One of the four built-in effects. A closed set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Wave,
    Stripes,
    Plasma,
    Sparkle,
}

impl Kind {
    pub const ALL: [Self; 4] = [Self::Wave, Self::Stripes, Self::Plasma, Self::Sparkle];

    /// The kind's settings, in the order an [`Effect`] takes their values.
    pub const fn settings(self) -> &'static [Setting] {
        match self {
            Self::Wave => &WAVE,
            Self::Stripes => &STRIPES,
            Self::Plasma => &PLASMA,
            Self::Sparkle => &SPARKLE,
        }
    }
}

/// What a setting's number means. The screens pick the control from it: buttons for a
/// `Choice`, a fader otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Beats,
    Percent,
    Cells,
    PerBeat,
    Times,
    Choice,
    Seed,
}

/// A setting as the engine publishes it. Every value is one `u16` from `min` to `max`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setting {
    /// Short and stable: the show file uses it.
    pub id: &'static str,
    /// Words for the screen.
    pub name: &'static str,
    pub unit: Unit,
    pub min: u16,
    pub max: u16,
    pub default: u16,
    /// The names of the values `min` to `max`, only for a `Choice`.
    pub choices: &'static [&'static str],
}

/// Beats per loop, common to every kind.
pub const MEASURE: Setting = Setting {
    id: "measure",
    name: "Measure",
    unit: Unit::Beats,
    min: 1,
    max: 32,
    default: 4,
    choices: &[],
};

const COLOURS: usize = 4;
const DIRECTION: Setting = choice(
    "direction",
    "Direction",
    0,
    &["Right", "Left", "Down", "Up"],
);
const WAVE: [Setting; 2] = [
    number("spread", "Spread", Unit::Percent, 0, 100, 100),
    DIRECTION,
];
const STRIPES: [Setting; 2] = [
    number("size", "Stripe width", Unit::Cells, 1, 64, 4),
    DIRECTION,
];
const PLASMA: [Setting; 1] = [number("scale", "Scale", Unit::Times, 1, 8, 1)];
const SPARKLE: [Setting; 5] = [
    number("density", "Density", Unit::Percent, 1, 100, 10),
    number("rate", "Rate", Unit::PerBeat, 1, 8, 4),
    choice("fade", "Fade", 1, &["Off", "On"]),
    number("seed", "Seed", Unit::Seed, 0, 65535, 1),
    choice("background", "Background", 0, &["Black", "Through"]),
];

const fn number(
    id: &'static str,
    name: &'static str,
    unit: Unit,
    min: u16,
    max: u16,
    default: u16,
) -> Setting {
    Setting {
        id,
        name,
        unit,
        min,
        max,
        default,
        choices: &[],
    }
}

const fn choice(
    id: &'static str,
    name: &'static str,
    default: u16,
    choices: &'static [&'static str],
) -> Setting {
    Setting {
        id,
        name,
        unit: Unit::Choice,
        min: 0,
        max: choices.len() as u16 - 1,
        default,
        choices,
    }
}

/// Why an effect was not built. Nothing changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// No colours.
    EmptyPalette,
    /// More values than the kind has settings.
    NoSuchSetting,
    /// The measure, the number of colours or a setting's value is outside its range.
    OutOfRange {
        setting: &'static str,
        min: u16,
        max: u16,
    },
}

/// An effect look's values: measure, 1 to 4 colours, and every setting in its range, decoded.
/// Only [`Effect::new`] builds one, so a wrong value cannot exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    pub(crate) measure: NonZeroU16,
    pub(crate) palette: Palette,
    pub(crate) settings: Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Palette {
    pub(crate) colours: [Rgb; COLOURS],
    pub(crate) count: NonZeroU16,
}

/// Each kind with exactly its settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Settings {
    Wave {
        spread: u16,
        way: Way,
    },
    Stripes {
        size: NonZeroU16,
        way: Way,
    },
    Plasma {
        scale: u16,
    },
    Sparkle {
        density: u16,
        rate: u16,
        fade: Fade,
        seed: u16,
        background: Background,
    },
}

/// Which way an effect travels: the `direction` setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Way {
    Right,
    Left,
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fade {
    Off,
    On,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Background {
    Black,
    Through,
}

impl Effect {
    /// `values` in the order of [`Kind::settings`]. A value left out takes its default.
    pub fn new(kind: Kind, measure: u16, colours: &[Rgb], values: &[u16]) -> Result<Self, Refused> {
        let measure = in_range(&MEASURE, measure)?;
        let measure = NonZeroU16::new(measure).ok_or(out_of_range(&MEASURE))?;
        let count = NonZeroU16::new(colours.len().try_into().unwrap_or(u16::MAX))
            .ok_or(Refused::EmptyPalette)?;
        if colours.len() > COLOURS {
            return Err(out_of_range(&COLOURS_RANGE));
        }
        let mut palette = Palette {
            colours: [Rgb::default(); COLOURS],
            count,
        };
        palette
            .colours
            .iter_mut()
            .zip(colours)
            .for_each(|(slot, colour)| *slot = *colour);
        let descriptors = kind.settings();
        if values.len() > descriptors.len() {
            return Err(Refused::NoSuchSetting);
        }
        let value = |n: usize| {
            let setting = descriptors.get(n).ok_or(Refused::NoSuchSetting)?;
            in_range(setting, values.get(n).copied().unwrap_or(setting.default))
        };
        let ways = [Way::Right, Way::Left, Way::Down, Way::Up];
        let settings = match kind {
            Kind::Wave => Settings::Wave {
                spread: value(0)?,
                way: pick(&ways, value(1)?)?,
            },
            Kind::Stripes => Settings::Stripes {
                size: NonZeroU16::new(value(0)?).ok_or(Refused::NoSuchSetting)?,
                way: pick(&ways, value(1)?)?,
            },
            Kind::Plasma => Settings::Plasma { scale: value(0)? },
            Kind::Sparkle => Settings::Sparkle {
                density: value(0)?,
                rate: value(1)?,
                fade: pick(&[Fade::Off, Fade::On], value(2)?)?,
                seed: value(3)?,
                background: pick(&[Background::Black, Background::Through], value(4)?)?,
            },
        };
        Ok(Self {
            measure,
            palette,
            settings,
        })
    }
}

/// The choice a value names. Its range was checked against the same list of names.
fn pick<T: Copy>(choices: &[T], value: u16) -> Result<T, Refused> {
    choices
        .get(usize::from(value))
        .copied()
        .ok_or(Refused::NoSuchSetting)
}

/// The range of the number of colours, for its refusal.
const COLOURS_RANGE: Setting = number("colours", "Colours", Unit::Times, 1, COLOURS as u16, 1);

fn in_range(setting: &Setting, value: u16) -> Result<u16, Refused> {
    if (setting.min..=setting.max).contains(&value) {
        Ok(value)
    } else {
        Err(out_of_range(setting))
    }
}

const fn out_of_range(setting: &Setting) -> Refused {
    Refused::OutOfRange {
        setting: setting.id,
        min: setting.min,
        max: setting.max,
    }
}
