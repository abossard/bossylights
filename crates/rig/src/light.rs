//! v11 light values to channel words, off the tick, until the light commands leave at T7
//! (`docs/design/channel-model.md`). The Rig's inference of what each channel of a light
//! does stays here as data: a step, a grab or a swatch becomes words on the light's
//! fields, and the tick sees only the words.
//!
//! | A light | 16 bits, 0 to 65 535 is 0 to 1, or no value |
//! |---|---|
//! | intensity | how bright |
//! | red, green, blue | its colour, one part |
//! | pan, tilt | its position, one part |
//! | zoom | how wide |
//!
//! A colour on a wheel and a gobo are choices, not amounts: a [`Feed::Picks`] shows the
//! byte of the slot the light holds, never one between two slots.
//!
//! A part the value does not have offers nothing: the channel shows what is underneath,
//! else its rest.
//!
//! A word is 16 bits: on one channel its high byte goes out, on a pair both bytes. So an
//! 8-bit channel always shows the coarse byte of the same value.

use alloc::vec::Vec;
use engine_core::{Field, Gobo, Light, Offer, Rgb};

/// What a light shows on its fields. One per field, beside it in [`LightFields`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Feed {
    /// A value taken from the light.
    Takes(Take),
    /// A channel of slots, a wheel: the byte of the first slot whose choice the light
    /// holds, exactly. A change of choice cuts to the new slot.
    Picks(Vec<(Choice, u8)>),
}

/// A choice on a wheel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Colour(Rgb),
    Gobo(Gobo),
}

impl Choice {
    /// Whether `light` holds this choice.
    fn held(self, light: &Light) -> bool {
        match self {
            Self::Colour(colour) => light.colour == Some(colour),
            Self::Gobo(gobo) => light.gobo(gobo.wheel) == Some(gobo.slot),
        }
    }
}

/// One light's fields, in channel order, and what it shows on each. A light value
/// becomes a word per field: what the screens of T7 will send as typed values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LightFields {
    pub fields: Vec<Field>,
    feeds: Vec<Feed>,
}

impl LightFields {
    pub(crate) fn push(&mut self, at: Field, feed: Feed) {
        self.fields.push(at);
        self.feeds.push(feed);
    }

    pub fn feeds(&self) -> &[Feed] {
        &self.feeds
    }

    /// Which fields fade between two values: the takes. A wheel cuts.
    pub fn fades(&self) -> impl Iterator<Item = bool> + '_ {
        self.feeds.iter().map(|feed| matches!(feed, Feed::Takes(_)))
    }

    /// `value` on each field, in order: `None` where it has nothing for the field.
    pub fn words<'a>(&'a self, value: &'a Light) -> impl Iterator<Item = Option<u16>> + 'a {
        self.feeds.iter().map(move |feed| match feed {
            Feed::Takes(take) => take.value(value),
            Feed::Picks(slots) => slots
                .iter()
                .find(|(choice, _)| choice.held(value))
                .map(|&(_, byte)| u16::from(byte) << 8),
        })
    }

    /// What `value` offers on this light: a word on each field it has one for.
    pub fn offers<'a>(&'a self, value: &'a Light) -> impl Iterator<Item = Offer> + 'a {
        self.fields
            .iter()
            .zip(self.words(value))
            .filter_map(|(&at, word)| word.map(|value| Offer { at, value }))
    }
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

    /// The value of this take for `light`, 16 bits. `None` when `light` has no value of
    /// the part it takes. A colour without brightness is not scaled (cx-4).
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
        // cx-4: brightness on a light without a dimmer is in its colour bytes, when the
        // value has both
        let scaled = match (self.scale, light.intensity) {
            (Scale::ByIntensity, Some(intensity)) => times(less, intensity),
            (Scale::Whole, _) | (Scale::ByIntensity, None) => less,
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
    pub(crate) fn over(self, value: u16) -> u16 {
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
