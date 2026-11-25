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
    /// Nobody: the channel shows its rest, or a default the writers give it.
    pub const REST: Self = Self(0);
    /// A raw hold from the DMX screen.
    pub const RAW: Self = Self(1);

    /// Entry `entry` of the source table (`crate::mix`). `None` past it.
    pub const fn of(entry: usize) -> Option<Self> {
        if entry < crate::mix::SOURCES {
            Some(Self(entry as u16 + 2))
        } else {
            None
        }
    }

    /// The entry of the source table this tag names; `None` for rest and a raw hold.
    pub const fn entry(self) -> Option<usize> {
        match self.0.checked_sub(2) {
            Some(entry) => Some(entry as usize),
            None => None,
        }
    }
}

/// A coarse byte and its fine byte: one 16-bit value, counted from 0 over the space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub coarse: u32,
    pub fine: u32,
}

/// Where an offer goes: one channel, or a coarse and fine pair as one 16-bit word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Byte(u32),
    Word(Pair),
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
    pub const HIGHEST: Self = Self(1 << 1);
    pub const PAIR: Self = Self(1 << 2);

    /// These flags and `other`'s.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether it fades between values, else jumps.
    pub const fn fades(self) -> bool {
        self.0 & Self::FADE.0 != 0
    }

    pub(crate) const fn highest(self) -> bool {
        self.0 & Self::HIGHEST.0 != 0
    }
}

/// One patched channel as its fixture's file says (the Rig's lowering): what shows when
/// nothing plays, where a modifier at 0 takes it, how it moves. Every channel nobody
/// patched rests at 0, dark 0, jumping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Configured {
    /// The channel, counted from 0 over the whole space.
    pub at: u32,
    pub rest: u8,
    pub dark: u8,
    pub flags: Flags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scale {
    Full,
    Dark,
    Part { product: i128, whole: i128 },
}

pub(crate) fn scales(levels: [u8; MODIFIERS]) -> [Scale; 1 << MODIFIERS] {
    let mut scales = [Scale::Full; 1 << MODIFIERS];
    for (follows, scale) in (0..=u8::MAX).zip(scales.iter_mut()) {
        *scale = scale_of(Follows(follows), levels);
    }
    scales
}

pub(crate) fn scale_of(follows: Follows, levels: [u8; MODIFIERS]) -> Scale {
    let (mut product, mut whole) = (1_i128, 1_i128);
    for (slot, level) in levels.iter().enumerate() {
        if follows.0 & (1 << slot) != 0 {
            product *= i128::from(*level);
            whole *= 255;
        }
    }
    match product {
        0 => Scale::Dark,
        _ if product == whole => Scale::Full,
        _ => Scale::Part { product, whole },
    }
}

fn scale(value: u8, dark: u8, flags: Flags, scale: Scale) -> u8 {
    match scale {
        Scale::Full => value,
        Scale::Dark => dark,
        Scale::Part { .. } if !flags.fades() => value,
        Scale::Part { product, whole } => {
            let distance = (i128::from(value) - i128::from(dark)) * product;
            let moved = (2 * distance.abs() + whole) / (2 * whole);
            u8::try_from(i128::from(dark) + distance.signum() * moved).unwrap_or(dark)
        }
    }
}

/// Every channel of the space, 8 bytes each, as a struct of arrays: a pass touches only
/// the arrays it needs. Made once, before the tick starts; the frame writes `value` and
/// `source`, the patch the others (owner decision q1, `docs/design/channel-model.md`).
///
/// A frame visits only the patched channels and the raw holds, so its work follows the
/// rig, not the size of the space: a channel nobody patched or holds stays at its rest, 0,
/// which is also its dark.
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
    /// The channels a raw hold set in the last frame: the next frame starts them at rest.
    raw: Raw,
}

/// The raw holds of the last frame, and whether there was one at all: most frames have
/// none, and then the reset and the modifiers walk no universe for them. Applying the
/// holds still looks at each universe's set once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Raw {
    sets: [ChannelSet; MAX_UNIVERSES as usize],
    any: bool,
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
        raw: Raw {
            sets: [ChannelSet::NONE; MAX_UNIVERSES as usize],
            any: false,
        },
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

    /// The patch's channels: those of `old` go back to rest 0, dark 0, jumping, and show 0;
    /// then each of `new` takes its rest, dark and flags. Every channel still follows
    /// Blackout (rules per channel: #251). Called when the patch changes, not each frame.
    pub fn configure(&mut self, old: &[Configured], new: &[Configured]) {
        for at in old.iter().filter_map(|c| usize::try_from(c.at).ok()) {
            self.set(at, 0, 0, Flags::NONE);
            if let Some(value) = byte(&mut self.value, at) {
                *value = 0;
            }
            if let Some(source) = self.source.get_mut(at) {
                *source = SourceTag::REST;
            }
        }
        for c in new {
            if let Ok(at) = usize::try_from(c.at) {
                self.set(at, c.rest, c.dark, c.flags);
            }
        }
    }

    fn set(&mut self, at: usize, rest: u8, dark: u8, flags: Flags) {
        if let (Some(r), Some(d), Some(f)) = (
            self.rest.get_mut(at),
            self.dark.get_mut(at),
            self.flags.get_mut(at),
        ) {
            (*r, *d, *f) = (rest, dark, flags);
        }
    }

    /// `on`: the highest offer wins on each of `dimmers` among equals, else the latest
    /// (v11 `SetMixRules.brightness`). Called when the rules or the patch change.
    pub fn highest(&mut self, dimmers: &[u32], on: bool) {
        for at in dimmers.iter().filter_map(|at| usize::try_from(*at).ok()) {
            if let Some(flags) = self.flags.get_mut(at) {
                *flags = if on {
                    flags.with(Flags::HIGHEST)
                } else {
                    Flags(flags.0 & !Flags::HIGHEST.0)
                };
            }
        }
    }

    /// Every patched channel, and every channel a raw hold set last frame, back to its
    /// rest and nobody's. Every other channel is at rest already.
    pub(crate) fn reset(&mut self, patched: &[Configured]) {
        let held = held(&self.raw);
        let ats = patched.iter().filter_map(|c| usize::try_from(c.at).ok());
        for at in ats.chain(held) {
            if let (Some(value), Some(rest), Some(source)) = (
                byte(&mut self.value, at),
                self.rest.get(at),
                self.source.get_mut(at),
            ) {
                (*value, *source) = (*rest, SourceTag::REST);
            }
        }
    }

    pub(crate) fn apply_winners(
        &mut self,
        patched: &[Configured],
        raw: &[Levels],
        holds: &[ChannelSet],
        levels: [u8; MODIFIERS],
    ) {
        // the raw holds, a universe that has one at a time; `reset` gave last frame's back
        if self.raw.any {
            self.raw.sets.fill(ChannelSet::NONE);
            self.raw.any = false;
        }
        let rows = self
            .value
            .iter_mut()
            .zip(self.source.chunks_exact_mut(ADDRESSES));
        let held_rows = raw.iter().zip(holds).zip(self.raw.sets.iter_mut());
        for ((value, source), ((raw, holds), last)) in rows.zip(held_rows) {
            if *holds == ChannelSet::NONE {
                continue;
            }
            for address in holds.addresses() {
                if let (Some(value), Some(source), Some(raw)) = (
                    value.0.get_mut(address),
                    source.get_mut(address),
                    raw.0.get(address),
                ) {
                    *value = *raw;
                    *source = SourceTag::RAW;
                }
            }
            *last = *holds;
            self.raw.any = true;
        }
        if levels == [u8::MAX; MODIFIERS] {
            return;
        }
        // each channel once: a held one in the second pass, the other patched in the first
        let by = scales(levels);
        let patched = patched
            .iter()
            .filter_map(|c| usize::try_from(c.at).ok())
            .filter(|at| self.source.get(*at) != Some(&SourceTag::RAW));
        for at in patched.chain(held(&self.raw)) {
            if let (Some(value), Some(dark), Some(flags), Some(follows)) = (
                byte(&mut self.value, at),
                self.dark.get(at),
                self.flags.get(at),
                self.follows.get(at),
            ) && let Some(by) = by.get(usize::from(follows.0))
            {
                *value = scale(*value, *dark, *flags, *by);
            }
        }
    }
}

/// The byte of channel `at` (counted from 0 over the space) in rows of a universe.
fn byte(rows: &mut [Levels], at: usize) -> Option<&mut u8> {
    rows.get_mut(at / ADDRESSES)?.0.get_mut(at % ADDRESSES)
}

/// The channels raw holds set in the last frame, counted from 0 over the space. None at
/// all when there was none: no universe is walked.
fn held(raw: &Raw) -> impl Iterator<Item = usize> + '_ {
    let sets: &[ChannelSet] = if raw.any { &raw.sets } else { &[] };
    (0..)
        .zip(sets)
        .filter(|(_, set)| **set != ChannelSet::NONE)
        .flat_map(|(universe, set)| {
            set.addresses()
                .map(move |address| universe * ADDRESSES + address)
        })
}

/// A channel as a client sends it, counted from 1. Not checked yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Levels(#[cfg_attr(feature = "ts", ts(type = "number[]"))] pub [u8; ADDRESSES]);

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

    /// Whether `sets` has this channel.
    #[cfg(test)]
    pub(crate) fn marked(self, sets: &[ChannelSet]) -> bool {
        sets.get(usize::from(self.universe))
            .is_some_and(|set| set.has(usize::from(self.address)))
    }
}

impl Serialize for Levels {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter())
    }
}

/// A channel's new level, counted from 1, as the state stream sends it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
                    assert_eq!(channel(u, a).marked(&sets), has, "{on:?} {off:?}: {u} {a}");
                    assert_eq!(sets[usize::from(u - 1)].has(usize::from(a - 1)), has);
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
        assert!(!channel(2, 1).marked(&one));
    }

    #[test]
    fn a_channel_takes_8_bytes() {
        assert_eq!(CHANNELS, 32_768);
        assert_eq!(size_of::<Channels>(), 8 * CHANNELS + size_of::<Raw>());
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
    fn flags_are_the_bits_of_the_design() {
        // docs/design/channel-model.md, "Bit of flags": 0 fade, 1 highest, 2 pair
        // (case, flags, byte)
        let cases = [
            ("fade", Flags::FADE, 0b001),
            ("highest", Flags::HIGHEST, 0b010),
            ("pair", Flags::PAIR, 0b100),
            ("fade and pair", Flags::FADE.with(Flags::PAIR), 0b101),
            ("fade twice", Flags::FADE.with(Flags::FADE), 0b001),
            ("none", Flags::NONE.with(Flags::NONE), 0),
        ];
        for (case, flags, byte) in cases {
            assert_eq!(flags.0, byte, "{case}");
        }
    }

    #[test]
    fn a_patch_configures_its_channels_and_gives_the_old_ones_back() {
        let one = |at, rest, dark, flags| Configured {
            at,
            rest,
            dark,
            flags,
        };
        let old = [one(0, 200, 3, Flags::NONE), one(1, 7, 0, Flags::FADE)];
        let pair = Flags::FADE.with(Flags::PAIR);
        let new = [one(1, 9, 255, pair), one(2, 4, 0, Flags::HIGHEST)];
        let mut all = Box::new(Channels::NEW);
        all.configure(&[], &old);
        all.reset(&old);
        assert_eq!(all.values()[0].0[..3], [200, 7, 0], "the old patch at rest");
        all.configure(&old, &new);
        // (channel, byte out before the next frame, rest, dark, flags)
        let cases = [
            (0, 0, 0, 0, Flags::NONE),
            (1, 0, 9, 255, pair),
            (2, 0, 4, 0, Flags::HIGHEST),
            (3, 0, 0, 0, Flags::NONE),
        ];
        for (at, out, rest, dark, flags) in cases {
            let seen = (
                all.value[0].0[at],
                all.rest[at],
                all.dark[at],
                all.flags[at],
            );
            assert_eq!(seen, (out, rest, dark, flags), "channel {at}");
        }
        all.reset(&new);
        assert_eq!(
            all.values()[0].0[..4],
            [0, 9, 4, 0],
            "the new patch at rest"
        );
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
