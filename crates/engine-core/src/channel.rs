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

    /// Where this channel is in the one channel space, counted from 0.
    pub(crate) const fn index(self) -> usize {
        self.universe as usize * ADDRESSES + self.address as usize
    }
}

/// The channels of the one space: every universe the engine can have.
pub const CHANNELS: usize = MAX_UNIVERSES as usize * ADDRESSES;

/// Modifier slots: how much shows, one level each. 0 Master (full until `SetMaster`
/// comes), 1 Blackout, 2 to 7 free.
pub const MODIFIERS: usize = 8;
/// The Blackout slot: 0 while Blackout is on, 255 while off.
pub const BLACKOUT: usize = 1;

/// Who set a channel's value this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceTag(u16);

impl SourceTag {
    /// Nobody: the channel shows its rest.
    pub const REST: Self = Self(0);
    /// A raw hold from the DMX screen.
    pub const RAW: Self = Self(1);
    /// Entry `index` of the source table, `None` past it.
    pub(crate) const fn of(index: usize) -> Option<Self> {
        if index < crate::mix::SOURCES {
            Some(Self(index as u16 + 2))
        } else {
            None
        }
    }

    /// The entry of the source table this tag names, `None` for rest and a raw hold.
    pub(crate) fn entry(self) -> Option<usize> {
        usize::from(self.0).checked_sub(2)
    }
}

/// Which modifier slots a channel follows, one bit per slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Follows(pub(crate) u8);

impl Follows {
    /// Every channel follows Blackout, until the patch sets rules per channel.
    pub const BLACKOUT: Self = Self(1 << BLACKOUT);
}

/// How a channel moves: bit 0 fades (else jumps), bit 1 highest wins (else latest),
/// bit 2 is one byte of a 16-bit pair. Bits 3 to 7 are zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flags(pub(crate) u8);

impl Flags {
    /// Jumps, latest wins, no pair.
    pub const NONE: Self = Self(0);
    pub const FADE: Self = Self(1);
    /// Among equals the greater brightness wins, not the later start.
    pub const HIGHEST: Self = Self(1 << 1);
    pub const PAIR: Self = Self(1 << 2);

    pub(crate) const fn fades(self) -> bool {
        self.0 & Self::FADE.0 != 0
    }

    pub(crate) const fn highest(self) -> bool {
        self.0 & Self::HIGHEST.0 != 0
    }

    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Every channel of the space, 8 bytes each, as a struct of arrays: a pass touches only
/// the arrays it needs. Made once, before the tick starts; the frame writes `value` and
/// `source`, the patch the others (owner decision q1, `docs/design/channel-model.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channels {
    /// The byte that goes out. Kept as rows of a universe while outputs take rows.
    pub(crate) value: [Levels; MAX_UNIVERSES as usize],
    /// What shows when nothing plays.
    pub(crate) rest: [u8; CHANNELS],
    /// Where a follower goes when its modifiers are at 0.
    pub(crate) dark: [u8; CHANNELS],
    pub(crate) flags: [Flags; CHANNELS],
    pub(crate) follows: [Follows; CHANNELS],
    pub(crate) source: [SourceTag; CHANNELS],
    /// Zero: room for the next rule.
    reserved: [u8; CHANNELS],
}

impl Channels {
    /// At rest 0, dark 0, jumping, following Blackout, nobody's.
    pub const NEW: Self = Self {
        value: [Levels::ZERO; MAX_UNIVERSES as usize],
        rest: [0; CHANNELS],
        dark: [0; CHANNELS],
        flags: [Flags::NONE; CHANNELS],
        follows: [Follows::BLACKOUT; CHANNELS],
        source: [SourceTag::REST; CHANNELS],
        reserved: [0; CHANNELS],
    };

    /// The bytes that go out, universe by universe.
    pub fn values(&self) -> &[Levels] {
        &self.value
    }

    /// Who set `at` this frame.
    pub fn source(&self, at: Channel) -> SourceTag {
        self.source
            .get(at.index())
            .copied()
            .unwrap_or(SourceTag::REST)
    }
}

impl Channels {
    /// The patch's config of every channel from this frame on. The values and who set
    /// them stay until the frame writes them.
    pub fn configure(&mut self, config: &ChannelConfig) {
        self.rest.copy_from_slice(&config.rest);
        self.dark.copy_from_slice(&config.dark);
        self.flags.copy_from_slice(&config.flags);
        self.follows.copy_from_slice(&config.follows);
    }
}

/// What the patch sets of each channel, built off the tick: its rest, its dark byte,
/// its flags and the modifiers it follows. Copied into [`Channels`] in the frame its
/// command is applied, so the tick never builds one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelConfig {
    pub rest: [u8; CHANNELS],
    pub dark: [u8; CHANNELS],
    pub flags: [Flags; CHANNELS],
    pub follows: [Follows; CHANNELS],
}

impl ChannelConfig {
    /// As [`Channels::NEW`]: at rest 0, dark 0, jumping, following Blackout.
    pub const NEW: Self = Self {
        rest: [0; CHANNELS],
        dark: [0; CHANNELS],
        flags: [Flags::NONE; CHANNELS],
        follows: [Follows::BLACKOUT; CHANNELS],
    };
}

/// A channel as a client sends it, counted from 1. Not checked yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// A set of the addresses of one universe: which raw channels are held, which a light
/// wrote this frame. Buffers of these belong to the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelSet([u64; ADDRESSES / 64]);

impl ChannelSet {
    pub const NONE: Self = Self([0; ADDRESSES / 64]);

    /// Whether `address` (counted from 0) is in the set. Past the universe it is not.
    pub fn has(&self, address: usize) -> bool {
        self.0
            .get(address / 64)
            .is_some_and(|word| word & (1 << (address % 64)) != 0)
    }

    /// The addresses in the set, in order.
    pub(crate) fn addresses(&self) -> impl Iterator<Item = usize> + '_ {
        (0..).zip(&self.0).flat_map(|(word, bits)| {
            let mut bits = *bits;
            core::iter::from_fn(move || {
                let bit = bits.trailing_zeros() as usize;
                bits &= bits.wrapping_sub(1);
                (bit < 64).then_some(word * 64 + bit)
            })
        })
    }

    fn put(&mut self, address: usize, on: bool) {
        if let Some(word) = self.0.get_mut(address / 64) {
            let bit = 1 << (address % 64);
            *word = if on { *word | bit } else { *word & !bit };
        }
    }
}

impl Channel {
    /// Puts this channel in `sets`, or takes it out.
    pub(crate) fn mark(self, sets: &mut [ChannelSet], on: bool) {
        if let Some(set) = sets.get_mut(usize::from(self.universe)) {
            set.put(usize::from(self.address), on);
        }
    }
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
    fn a_channel_set_holds_each_address_on_its_own() {
        let two = Universes::new(2).unwrap();
        let channel = |universe, address| ChannelNumbers { universe, address }.check(two).unwrap();
        // (marked on, then off, the addresses in the set after, counted from 1)
        type Addresses<'a> = &'a [(u16, u16)];
        let cases: [(Addresses, Addresses, Addresses); 5] = [
            (&[], &[], &[]),
            (
                &[(1, 1), (1, 64), (1, 65), (2, 512)],
                &[],
                &[(1, 1), (1, 64), (1, 65), (2, 512)],
            ),
            (&[(1, 64), (1, 65)], &[(1, 65)], &[(1, 64)]),
            (&[(2, 1)], &[(2, 1), (2, 1)], &[]),
            (&[(1, 3)], &[(2, 3)], &[(1, 3)]), // another universe stays
        ];
        for (on, off, after) in cases {
            let mut sets = [ChannelSet::NONE; 2];
            for (u, a) in on {
                channel(*u, *a).mark(&mut sets, true);
            }
            for (u, a) in off {
                channel(*u, *a).mark(&mut sets, false);
            }
            for u in 1..=2 {
                for a in 1..=512 {
                    let has = after.contains(&(u, a));
                    let set = sets[usize::from(u - 1)];
                    assert_eq!(set.has(usize::from(a - 1)), has, "{on:?} {off:?}: {u} {a}");
                }
                let listed: Vec<_> = sets[usize::from(u - 1)].addresses().collect();
                let expected: Vec<_> = (1..=512)
                    .filter(|a| after.contains(&(u, *a)))
                    .map(|a| usize::from(a - 1))
                    .collect();
                assert_eq!(listed, expected, "{on:?} {off:?}: universe {u}");
            }
        }
        // past the universe, or on a shorter buffer: nothing
        assert!(!ChannelSet([u64::MAX; 8]).has(512));
        let mut one = [ChannelSet::NONE];
        channel(2, 1).mark(&mut one, true);
        assert_eq!(one, [ChannelSet::NONE]);
    }

    #[test]
    fn a_channel_takes_8_bytes() {
        assert_eq!(CHANNELS, 32_768);
        assert_eq!(size_of::<Channels>(), 8 * CHANNELS);
        // (array, bytes per channel)
        let one = Channels::NEW;
        let cases = [
            ("value", size_of_val(&one.value)),
            ("rest", size_of_val(&one.rest)),
            ("dark", size_of_val(&one.dark)),
            ("flags", size_of_val(&one.flags)),
            ("follows", size_of_val(&one.follows)),
            ("source", size_of_val(&one.source) / 2),
            ("reserved", size_of_val(&one.reserved)),
        ];
        for (array, bytes) in cases {
            assert_eq!(bytes, CHANNELS, "{array}");
        }
    }

    #[test]
    fn a_channel_has_one_place_in_the_space() {
        let all = Universes::new(MAX_UNIVERSES).unwrap();
        // (universe, address, index)
        let cases = [
            (1, 1, 0),
            (1, 512, 511),
            (2, 1, 512),
            (64, 512, CHANNELS - 1),
        ];
        for (universe, address, index) in cases {
            let channel = ChannelNumbers { universe, address }.check(all).unwrap();
            assert_eq!(channel.index(), index, "{universe}/{address}");
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
