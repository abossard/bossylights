//! Where a level goes: universes, addresses and the buffers that hold their levels.
//!
//! A client counts universes and addresses from 1. [`ChannelNumbers::check`] is the
//! only way to a [`Channel`], so inside the engine every channel exists.

use crate::Refusal;
use serde::{Deserialize, Serialize, Serializer};

/// Addresses in one universe.
pub const ADDRESSES: usize = 512;
/// The most universes an engine can have: 64 × 170 RGB pixels fit.
pub const MAX_UNIVERSES: u16 = 64;

/// How many universes this engine has, 1 to [`MAX_UNIVERSES`]. A setting, fixed while
/// the engine runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Universes(u16);

impl Universes {
    pub const ONE: Self = Self(1);

    /// `None` outside 1 to [`MAX_UNIVERSES`].
    pub const fn new(count: u16) -> Option<Self> {
        if count >= 1 && count <= MAX_UNIVERSES {
            Some(Self(count))
        } else {
            None
        }
    }

    pub const fn count(self) -> u16 {
        self.0
    }
}

/// One channel of this engine: a universe and an address in it, both in range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Channel {
    /// Counted from 0.
    universe: u16,
    /// Counted from 0.
    address: u16,
}

impl Channel {
    /// The level this channel holds in `universes`.
    pub(crate) fn level(self, universes: &mut [Levels]) -> Option<&mut u8> {
        universes
            .get_mut(usize::from(self.universe))?
            .0
            .get_mut(usize::from(self.address))
    }
}

/// A channel as a client sends it, counted from 1. Not checked yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChannelNumbers {
    pub universe: u16,
    pub address: u16,
}

impl ChannelNumbers {
    /// The channel, if this engine has it.
    pub fn check(self, universes: Universes) -> Result<Channel, Refusal> {
        if self.universe < 1 || self.universe > universes.0 {
            return Err(Refusal::NoSuchUniverse {
                universes: universes.0,
            });
        }
        if self.address < 1 || usize::from(self.address) > ADDRESSES {
            return Err(Refusal::NoSuchAddress);
        }
        Ok(Channel {
            universe: self.universe - 1,
            address: self.address - 1,
        })
    }
}

/// The levels of one universe. Buffers of these belong to the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Levels(#[cfg_attr(test, ts(type = "number[]"))] pub [u8; ADDRESSES]);

impl Levels {
    pub const ZERO: Self = Self([0; ADDRESSES]);
}

impl Serialize for Levels {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter())
    }
}

/// A channel's new level, counted from 1, as the state stream sends it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChannelLevel {
    pub universe: u16,
    pub address: u16,
    pub value: u8,
}

/// Every channel whose level differs between `before` and `after`.
pub fn changes<'a>(
    before: &'a [Levels],
    after: &'a [Levels],
) -> impl Iterator<Item = ChannelLevel> + 'a {
    (1..)
        .zip(before.iter().zip(after))
        .flat_map(|(universe, (old, new))| {
            (1..)
                .zip(old.0.iter().zip(new.0.iter()))
                .filter(|(_, (old, new))| old != new)
                .map(move |(address, (_, value))| ChannelLevel {
                    universe,
                    address,
                    value: *value,
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_exists_only_inside_this_engine() {
        let two = Universes::new(2).unwrap();
        // (universe, address, result)
        let cases = [
            (1, 1, Ok((0, 0))),
            (2, 512, Ok((1, 511))),
            (0, 1, Err(Refusal::NoSuchUniverse { universes: 2 })),
            (3, 1, Err(Refusal::NoSuchUniverse { universes: 2 })),
            (1, 0, Err(Refusal::NoSuchAddress)),
            (1, 513, Err(Refusal::NoSuchAddress)),
            (
                u16::MAX,
                u16::MAX,
                Err(Refusal::NoSuchUniverse { universes: 2 }),
            ),
        ];
        for (universe, address, expected) in cases {
            let checked = ChannelNumbers { universe, address }.check(two);
            let expected = expected.map(|(universe, address)| Channel { universe, address });
            assert_eq!(checked, expected, "universe {universe}, address {address}");
        }
    }

    #[test]
    fn universes_are_1_to_64() {
        // (count, valid)
        let cases = [
            (0, false),
            (1, true),
            (64, true),
            (65, false),
            (u16::MAX, false),
        ];
        for (count, valid) in cases {
            assert_eq!(Universes::new(count).is_some(), valid, "{count}");
        }
    }

    #[test]
    fn changes_lists_each_channel_that_differs() {
        let with = |levels: &[(usize, u8)]| {
            let mut universe = Levels::ZERO;
            for (address, value) in levels {
                universe.0[*address] = *value;
            }
            universe
        };
        let level = |universe, address, value| ChannelLevel {
            universe,
            address,
            value,
        };
        // (before, after, changes)
        let cases = [
            (vec![Levels::ZERO], vec![Levels::ZERO], vec![]),
            (
                vec![Levels::ZERO, Levels::ZERO],
                vec![with(&[(0, 9)]), with(&[(511, 1)])],
                vec![level(1, 1, 9), level(2, 512, 1)],
            ),
            (
                vec![with(&[(3, 200), (4, 7)])],
                vec![with(&[(4, 7)])],
                vec![level(1, 4, 0)],
            ),
        ];
        for (before, after, expected) in cases {
            let found: Vec<_> = changes(&before, &after).collect();
            assert_eq!(found, expected);
        }
    }
}
