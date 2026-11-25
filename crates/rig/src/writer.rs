//! How a light's value lands on its channels inside the Rig: what each channel takes from
//! the value and how. `compile` makes these per mode at an address; [`crate::LightFields`]
//! turns them into the words a light offers per field. Nothing here knows a fixture, a
//! mode, a maker or a colour model: only the values of a light and arithmetic on them.
//!
//! | A light | 16 bits, 0 to 65 535 is 0 to 1, or not held |
//! |---|---|
//! | intensity | how bright |
//! | red, green, blue | its colour, one part |
//! | pan, tilt | its position, one part |
//! | zoom | how wide |
//!
//! A colour on a wheel and a gobo are choices, not amounts: a [`Writer::Picks`] shows the
//! byte of the slot the light holds, never one between two slots.

use engine_core::{Channel, Gobo, Light, LightNumber, Rgb};

/// One channel, or a coarse and fine pair, and what goes there each frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    /// The same byte every frame: a shutter held open, the default of a channel no
    /// light writes.
    Fixed { at: Channel, value: u8 },
    /// A value taken from a light, or `unheld` when nothing holds the part it takes: the
    /// default of its channel from the definition, the coarse byte high.
    Takes {
        light: LightNumber,
        take: Take,
        at: Place,
        unheld: u16,
    },
    /// A channel of slots, a wheel: the value of the first of the `slots` slot writers
    /// right after it whose choice its light holds, else `unheld`, the default of its
    /// channel. A change of choice cuts to the new slot.
    Picks {
        light: LightNumber,
        at: Channel,
        slots: u8,
        unheld: u8,
    },
    /// One slot of the [`Writer::Picks`] before it: its channel shows `value` while the
    /// light holds `colour`, exactly. Writes nothing of its own.
    ColourSlot { colour: Rgb, value: u8 },
    /// One slot of the [`Writer::Picks`] before it: its channel shows `value` while the
    /// light holds `gobo` on its wheel. Writes nothing of its own.
    GoboSlot { gobo: Gobo, value: u8 },
}

impl Writer {
    /// The value of a slot writer while `light` holds its choice. `None` for any other.
    pub fn slot(&self, light: &Light) -> Option<u8> {
        match *self {
            Self::ColourSlot { colour, value } => (light.colour == Some(colour)).then_some(value),
            Self::GoboSlot { gobo, value } => {
                (light.gobo(gobo.wheel) == Some(gobo.slot)).then_some(value)
            }
            Self::Fixed { .. } | Self::Takes { .. } | Self::Picks { .. } => None,
        }
    }
}

/// Where a value of 16 bits goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Its high byte only.
    Byte(Channel),
    /// The high byte to `coarse`, the low byte to `fine`.
    Pair { coarse: Channel, fine: Channel },
}

/// What a writer takes from its light, and how: the part `of` the light, less `less`,
/// then scaled, then turned, then laid over the bytes it travels.
///
/// | Channel | Take |
/// |---|---|
/// | a dimmer | intensity |
/// | red beside a dimmer | red |
/// | red without a dimmer | red, times intensity |
/// | red beside white | red, less shared |
/// | white | shared: the part red, green and blue all have |
/// | cyan | red, falling: full less red |
/// | pan, tilt, zoom | pan, tilt, zoom, falling when the channel runs the other way, over the range it travels |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Take {
    pub of: Part,
    pub less: Less,
    pub scale: Scale,
    pub sense: Sense,
    pub travel: Travel,
}

impl Take {
    /// Intensity as it is: a dimmer.
    pub const INTENSITY: Self = Self {
        of: Part::Intensity,
        less: Less::Nothing,
        scale: Scale::Whole,
        sense: Sense::Rising,
        travel: Travel::FULL,
    };

    /// The value of this take for `light`, 16 bits. `None` when nothing holds the part it
    /// takes. Intensity not held scales as 0.
    pub fn value(self, light: &Light) -> Option<u16> {
        let part = match self.of {
            Part::Intensity => light.intensity,
            Part::Red => light.colour.map(|c| c.red),
            Part::Green => light.colour.map(|c| c.green),
            Part::Blue => light.colour.map(|c| c.blue),
            Part::Shared => light.colour.map(Rgb::shared),
            Part::Pan => light.position.map(|p| p.pan),
            Part::Tilt => light.position.map(|p| p.tilt),
            Part::Zoom => light.zoom,
        }?;
        let shared = light.colour.map_or(0, Rgb::shared);
        let less = match self.less {
            Less::Nothing => part,
            Less::Shared => part.saturating_sub(shared),
        };
        let scaled = match self.scale {
            Scale::Whole => less,
            Scale::ByIntensity => times(less, light.intensity.unwrap_or(0)),
        };
        let turned = match self.sense {
            Sense::Rising => scaled,
            Sense::Falling => u16::MAX - scaled,
        };
        Some(self.travel.over(turned))
    }
}

/// The high bytes a part travels over, both included: a zoom of 0 to 244 leaves 245 to
/// 255 to what is not zoom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Travel {
    pub low: u8,
    pub high: u8,
}

impl Travel {
    /// Every byte.
    pub const FULL: Self = Self {
        low: 0,
        high: u8::MAX,
    };

    /// `value`, 0 to full, laid over the travel: 0 is `low` 00, full is `high` ff.
    fn over(self, value: u16) -> u16 {
        let low = u32::from(self.low) << 8;
        let span = (u32::from(self.high) << 8 | 0xff).saturating_sub(low);
        let laid = low + (u32::from(value) * span + u32::from(u16::MAX / 2)) / u32::from(u16::MAX);
        u16::try_from(laid).unwrap_or(u16::MAX)
    }
}

/// A part of a light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Intensity,
    Red,
    Green,
    Blue,
    /// The smallest of red, green and blue: the part all three have.
    Shared,
    Pan,
    Tilt,
    Zoom,
}

/// What is taken off the part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Less {
    Nothing,
    /// The part red, green and blue all have, which another channel shows.
    Shared,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    Whole,
    /// Times the light's intensity: the mode has nothing else that dims it.
    ByIntensity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sense {
    /// More of the part, a higher value.
    Rising,
    /// More of the part, a lower value: full less the value.
    Falling,
}

/// `a` times `b`, both 0 to 1 in 16 bits, rounded to the nearest.
fn times(a: u16, b: u16) -> u16 {
    let product = u32::from(a) * u32::from(b) + u32::from(u16::MAX / 2);
    u16::try_from(product / u32::from(u16::MAX)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::Position;

    const FULL: u16 = u16::MAX;
    const HALF: u16 = 0x8000;

    fn light(intensity: u16, (red, green, blue): (u16, u16, u16)) -> Light {
        Light {
            intensity: Some(intensity),
            colour: Some(Rgb { red, green, blue }),
            ..Light::DARK
        }
    }

    /// Pan 0x1234, tilt 0x5678, zoom half, no intensity.
    fn aimed() -> Light {
        Light {
            position: Some(Position {
                pan: 0x1234,
                tilt: 0x5678,
            }),
            zoom: Some(HALF),
            ..Light::DARK
        }
    }

    fn take(of: Part, less: Less, scale: Scale, sense: Sense) -> Take {
        Take {
            of,
            less,
            scale,
            sense,
            travel: Travel::FULL,
        }
    }

    #[test]
    fn a_take_is_a_part_less_scaled_turned() {
        use Less::{Nothing, Shared as LessShared};
        use Part::{Blue, Green, Intensity, Pan, Red, Shared, Tilt, Zoom};
        use Scale::{ByIntensity, Whole};
        use Sense::{Falling, Rising};
        let orange = (FULL, HALF, 0x1000);
        let grey = (HALF, HALF, HALF);
        // (take, light, value)
        let cases = [
            (
                take(Intensity, Nothing, Whole, Rising),
                light(HALF, orange),
                Some(HALF),
            ),
            (
                take(Red, Nothing, Whole, Rising),
                light(0, orange),
                Some(FULL),
            ),
            (
                take(Green, Nothing, Whole, Rising),
                light(0, orange),
                Some(HALF),
            ),
            (
                take(Blue, Nothing, Whole, Rising),
                light(0, orange),
                Some(0x1000),
            ),
            (
                take(Shared, Nothing, Whole, Rising),
                light(0, orange),
                Some(0x1000),
            ),
            (
                take(Red, LessShared, Whole, Rising),
                light(0, orange),
                Some(FULL - 0x1000),
            ),
            (
                take(Shared, LessShared, Whole, Rising),
                light(0, orange),
                Some(0),
            ),
            (
                take(Red, Nothing, ByIntensity, Rising),
                light(HALF, orange),
                Some(HALF),
            ),
            (
                take(Red, Nothing, ByIntensity, Rising),
                light(0, orange),
                Some(0),
            ),
            (
                take(Red, Nothing, ByIntensity, Rising),
                light(FULL, orange),
                Some(FULL),
            ),
            // 0.5 × 0.5 is 0.25, rounded to the nearest
            (
                take(Green, Nothing, ByIntensity, Rising),
                light(HALF, grey),
                Some(0x4000),
            ),
            (
                take(Red, Nothing, Whole, Falling),
                light(0, orange),
                Some(0),
            ),
            (
                take(Blue, Nothing, Whole, Falling),
                light(0, orange),
                Some(FULL - 0x1000),
            ),
            (
                take(Red, Nothing, ByIntensity, Falling),
                light(0, orange),
                Some(FULL),
            ),
            (Take::INTENSITY, light(0x1234, grey), Some(0x1234)),
            // a part nothing holds: no value, its writer writes the default
            (
                Take::INTENSITY,
                Light {
                    intensity: None,
                    ..light(FULL, grey)
                },
                None,
            ),
            (
                take(Red, Nothing, Whole, Rising),
                Light {
                    colour: None,
                    ..light(FULL, grey)
                },
                None,
            ),
            (take(Shared, Nothing, Whole, Rising), Light::DARK, None),
            // pan and tilt travel together, zoom on its own; neither dims
            (take(Pan, Nothing, Whole, Rising), aimed(), Some(0x1234)),
            (take(Tilt, Nothing, Whole, Rising), aimed(), Some(0x5678)),
            (take(Zoom, Nothing, Whole, Rising), aimed(), Some(HALF)),
            (take(Zoom, Nothing, Whole, Falling), aimed(), Some(0x7fff)),
            (take(Pan, Nothing, Whole, Rising), light(FULL, grey), None),
            (take(Zoom, Nothing, Whole, Rising), light(FULL, grey), None),
            // intensity not held scales as 0
            (
                take(Red, Nothing, ByIntensity, Rising),
                Light {
                    intensity: None,
                    ..light(FULL, orange)
                },
                Some(0),
            ),
        ];
        for (take, light, value) in cases {
            assert_eq!(take.value(&light), value, "{take:?} of {light:?}");
        }
    }
}
