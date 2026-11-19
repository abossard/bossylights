//! What a light can be told: one value per property, or nothing. Who wins a property
//! is decided in `mix.rs`; how a value reaches a channel, by the Rig's writers in
//! `resolve.rs`. Nothing here knows a fixture or a channel.
//!
//! | Property | Value | 16 bits are 0 to 1 |
//! |---|---|---|
//! | intensity | how bright | yes |
//! | colour | red, green and blue: one colour, whether a fixture mixes it or picks it on a wheel | yes |
//! | position | pan and tilt, always together | yes |
//! | zoom | how wide | yes |
//! | gobo | a wheel and a slot on it, counted from 1 | no: a choice |
//!
//! Absent (`None`) lets another source through. Zero and black are values: they claim
//! the property.

use serde::{Deserialize, Serialize};

/// One property of a light: one winner each, never half of one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Property {
    Intensity,
    Colour,
    Position,
    Zoom,
    Gobo,
}

impl Property {
    /// Every property, in the order of [`Light`]'s fields.
    pub const ALL: [Self; 5] = [
        Self::Intensity,
        Self::Colour,
        Self::Position,
        Self::Zoom,
        Self::Gobo,
    ];

    /// Its place in [`Property::ALL`].
    pub const fn index(self) -> usize {
        match self {
            Self::Intensity => 0,
            Self::Colour => 1,
            Self::Position => 2,
            Self::Zoom => 3,
            Self::Gobo => 4,
        }
    }
}

/// A light's value of each property, `None` where nothing gives one. What a source
/// supplies (sparse), what a light holds, and what it shows once the Mixer decided.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Light {
    #[serde(default)]
    pub intensity: Option<u16>,
    #[serde(default)]
    pub colour: Option<Rgb>,
    #[serde(default)]
    pub position: Option<Position>,
    #[serde(default)]
    pub zoom: Option<u16>,
    #[serde(default)]
    pub gobo: Option<Gobo>,
}

impl Light {
    /// Nothing held: each property shows the definition's default, which is dark unless
    /// the definition says otherwise.
    pub const DARK: Self = Self {
        intensity: None,
        colour: None,
        position: None,
        zoom: None,
        gobo: None,
    };

    /// Whether it has a value of `property`.
    pub const fn has(&self, property: Property) -> bool {
        match property {
            Property::Intensity => self.intensity.is_some(),
            Property::Colour => self.colour.is_some(),
            Property::Position => self.position.is_some(),
            Property::Zoom => self.zoom.is_some(),
            Property::Gobo => self.gobo.is_some(),
        }
    }

    /// `self` with the value of `property` taken from `from`, absent where `from` has
    /// none.
    pub const fn with(self, property: Property, from: &Self) -> Self {
        let mut light = self;
        match property {
            Property::Intensity => light.intensity = from.intensity,
            Property::Colour => light.colour = from.colour,
            Property::Position => light.position = from.position,
            Property::Zoom => light.zoom = from.zoom,
            Property::Gobo => light.gobo = from.gobo,
        }
        light
    }
}

/// One value of one property: what a direct grab holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Value {
    Intensity(u16),
    Colour(Rgb),
    Position(Position),
    Zoom(u16),
    Gobo(Gobo),
}

impl Value {
    /// The property it is a value of.
    pub const fn property(self) -> Property {
        match self {
            Self::Intensity(_) => Property::Intensity,
            Self::Colour(_) => Property::Colour,
            Self::Position(_) => Property::Position,
            Self::Zoom(_) => Property::Zoom,
            Self::Gobo(_) => Property::Gobo,
        }
    }

    /// A light with this value and nothing else.
    pub const fn light(self) -> Light {
        let mut light = Light::DARK;
        match self {
            Self::Intensity(level) => light.intensity = Some(level),
            Self::Colour(rgb) => light.colour = Some(rgb),
            Self::Position(position) => light.position = Some(position),
            Self::Zoom(level) => light.zoom = Some(level),
            Self::Gobo(gobo) => light.gobo = Some(gobo),
        }
        light
    }
}

/// A colour, one property of a light.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Rgb {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
}

impl Rgb {
    pub const BLACK: Self = Self {
        red: 0,
        green: 0,
        blue: 0,
    };

    /// The part red, green and blue all have: the smallest.
    pub(crate) const fn shared(self) -> u16 {
        let least = if self.red < self.green {
            self.red
        } else {
            self.green
        };
        if least < self.blue { least } else { self.blue }
    }
}

/// Where a moving head points: pan and tilt travel together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Position {
    pub pan: u16,
    pub tilt: u16,
}

/// A gobo: the wheel and the slot on it, both counted from 1 in the definition's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Gobo {
    pub wheel: u8,
    pub slot: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_light_has_only_what_it_was_given() {
        let red = Rgb {
            red: u16::MAX,
            ..Rgb::BLACK
        };
        // (light, property, has)
        let cases = [
            (Light::DARK, Property::Colour, false),
            (
                Light {
                    colour: Some(Rgb::BLACK),
                    ..Light::DARK
                },
                Property::Colour,
                true,
            ),
            (
                Light {
                    intensity: Some(0),
                    ..Light::DARK
                },
                Property::Intensity,
                true,
            ),
            (
                Light {
                    colour: Some(red),
                    ..Light::DARK
                },
                Property::Intensity,
                false,
            ),
            (
                Light {
                    position: Some(Position { pan: 1, tilt: 2 }),
                    ..Light::DARK
                },
                Property::Position,
                true,
            ),
            (
                Light {
                    zoom: Some(0),
                    ..Light::DARK
                },
                Property::Zoom,
                true,
            ),
            (
                Light {
                    gobo: Some(Gobo { wheel: 1, slot: 2 }),
                    ..Light::DARK
                },
                Property::Gobo,
                true,
            ),
        ];
        for (light, property, has) in cases {
            assert_eq!(light.has(property), has, "{light:?} {property:?}");
        }
    }

    #[test]
    fn a_value_is_one_property_and_nothing_else() {
        let values = [
            Value::Intensity(0),
            Value::Colour(Rgb::BLACK),
            Value::Position(Position { pan: 1, tilt: 2 }),
            Value::Zoom(3),
            Value::Gobo(Gobo { wheel: 1, slot: 2 }),
        ];
        for (value, property) in values.into_iter().zip(Property::ALL) {
            assert_eq!(value.property(), property, "{value:?}");
            for other in Property::ALL {
                assert_eq!(
                    value.light().has(other),
                    other == property,
                    "{value:?} {other:?}"
                );
            }
        }
    }

    #[test]
    fn with_takes_one_property_and_its_absence() {
        let from = Light {
            intensity: Some(7),
            colour: Some(Rgb::BLACK),
            ..Light::DARK
        };
        let base = Light {
            intensity: Some(1),
            zoom: Some(3),
            ..Light::DARK
        };
        for property in Property::ALL {
            let got = base.with(property, &from);
            for other in Property::ALL {
                let source = if other == property { &from } else { &base };
                assert_eq!(got.has(other), source.has(other), "{property:?} {other:?}");
            }
        }
        assert_eq!(base.with(Property::Intensity, &from).intensity, Some(7));
        assert_eq!(base.with(Property::Zoom, &from).zoom, None);
    }
}
