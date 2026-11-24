//! A light's value as words on its fields: what a play or a grab will offer on each
//! channel once one mixer decides per channel (#250). A calculation off the tick.
//!
//! ```text
//! light value --LightFields::offers--> a word on each field the value has one for
//! ```
//!
//! A field is one channel, or a coarse and fine pair as one 16-bit word
//! ([`Field`], from the lowering). On one channel the word's high byte goes out.
//!
//! Each field shows what the light's writer there shows (`crate::compile`), but for a
//! colour alone (cx-4, `docs/design/channel-model.md`): on a light without a dimmer of
//! its own the brightness is in its colour bytes, and a colour without one is not dimmed.
//! Where a v11 brightness goes is an interim rule until light values leave (#253).
//!
//! | Value | On a colour channel of a light without a dimmer | Of a light with one |
//! |---|---|---|
//! | colour and brightness | the colour scaled by the brightness | the colour |
//! | colour alone | the colour as it is | the colour |
//! | brightness alone | nothing | nothing |
//!
//! A part the value does not have offers nothing: the channel shows what is underneath,
//! else its rest.

use crate::channels::Field;
use alloc::vec::Vec;
use engine_core::{Light, Scale, Take, Writer};

/// One light's fields in channel order, and what a value of the light offers on each.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LightFields(Vec<(Field, Feed)>);

/// What a light shows on one field.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Feed {
    /// A part of the value, taken as `Take` says, scaled only when the value has a
    /// brightness.
    Takes(Take),
    /// A wheel: the byte of the first slot whose choice the value holds, exactly.
    Picks(Vec<Writer>),
}

impl LightFields {
    /// Its fields, in channel order.
    pub fn fields(&self) -> impl Iterator<Item = Field> + '_ {
        self.0.iter().map(|&(at, _)| at)
    }

    /// What `value` offers: a word on each field it has one for, in channel order.
    pub fn offers<'a>(&'a self, value: &'a Light) -> impl Iterator<Item = (Field, u16)> + 'a {
        self.0
            .iter()
            .filter_map(move |(at, feed)| feed.word(value).map(|word| (*at, word)))
    }

    /// The fields of a light, in channel order, from each of its coarse channels: the
    /// lowering's field there and the writers `compile` binds to it. A wheel picks on its
    /// coarse byte alone, as its writer does.
    pub(crate) fn of<'a>(bound: impl Iterator<Item = (Field, &'a [Writer])>) -> Self {
        Self(
            bound
                .filter_map(|(at, writers)| {
                    let (at, feed) = match writers.split_first()? {
                        (Writer::Picks { .. }, slots) => {
                            let coarse = match at {
                                Field::Byte(at) => at,
                                Field::Word(pair) => pair.coarse,
                            };
                            (Field::Byte(coarse), Feed::Picks(slots.to_vec()))
                        }
                        (Writer::Takes { take, .. }, _) => (at, Feed::Takes(*take)),
                        (
                            Writer::Fixed { .. }
                            | Writer::ColourSlot { .. }
                            | Writer::GoboSlot { .. },
                            _,
                        ) => return None,
                    };
                    Some((at, feed))
                })
                .collect(),
        )
    }
}

impl Feed {
    /// The word `value` offers here. `None` when it has nothing for it.
    fn word(&self, value: &Light) -> Option<u16> {
        match self {
            Self::Takes(take) => {
                let take = match value.intensity {
                    Some(_) => *take,
                    None => Take {
                        scale: Scale::Whole,
                        ..*take
                    },
                };
                take.value(value)
            }
            Self::Picks(slots) => slots
                .iter()
                .find_map(|slot| slot.slot(value))
                .map(|byte| u16::from(byte) << 8),
        }
    }
}
