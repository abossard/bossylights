//! Who wins: the one place that decides between two values for one channel
//! (`docs/design/who-wins.md`, `docs/design/channel-model.md`). Nothing else compares or
//! overrides a level. Each channel is won on its own.
//!
//! | Step | Rule |
//! |---|---|
//! | 1 Offers | a live source offers a byte, or a 16-bit pair as one word. A source may offer nothing at a moment (a gap): a lower source shows through. A channel nobody offers keeps its rest |
//! | 2 Rank | Manual over Show over Autopilot. A direct grab is Manual |
//! | 3 Among equals | the latest start. A channel with `highest`: the greater brightness (the distance from `dark`, 16 bits on a pair), the latest on a tie |
//! | 4 Raw hold | replaces the byte, also with 0 |
//! | 6 Modifiers | each follower moves toward `dark` by the levels of the modifiers it follows: Blackout |
//!
//! [`offer`] is steps 1 to 3, [`channels`] steps 4 and 6.
//!
//! | A source ends | [`MixRules`] | What is left |
//! |---|---|---|
//! | Manual, `StopLook` | [`ManualEnd::UntilRelease`] | kept: each channel's last offer at its rank and start, until released or started again |
//! | Manual, `StopLook` | [`ManualEnd::UntilStop`] | nothing: what is underneath shows |
//! | Show or Autopilot, `StopLook` | [`OtherEnd::Reveal`] | nothing |
//! | Show or Autopilot, `StopLook` | [`OtherEnd::Stay`] | kept; a channel in a gap keeps none |
//! | Manual, `EndFlash`; any, `ReleaseHold` | any | nothing |
//! | a grab, `EndGrab` | [`ManualEnd::UntilRelease`] | kept: its offers at its start, until released |
//! | a grab, `EndGrab` | [`ManualEnd::UntilStop`] | nothing |
//! | a grab, `ReleaseProperty`, `ReleaseAll` | any | nothing; `ReleaseProperty` leaves the plays |
//! | kept, rules change to remove it | `UntilStop` for Manual, `Reveal` for the others | nothing |

use crate::channel::{
    ADDRESSES, CHANNELS, ChannelSet, Channels, Flags, Follows, Levels, MODIFIERS, SourceTag,
};
use crate::playback::{PLAYS, PlayState, Rank};
use serde::{Deserialize, Serialize};

/// The Mixer's three settings, saved with the show. An edit takes effect in the frame
/// that applies it. `brightness` is the `highest` flag of every dimmer channel: the front
/// sets the flags from it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MixRules {
    pub brightness: Brightness,
    pub manual_end: ManualEnd,
    pub other_end: OtherEnd,
}

/// Between two brightness values of one rank: the latest start, or the highest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Brightness {
    #[default]
    Latest,
    Highest,
}

/// What a stopped Manual play leaves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum ManualEnd {
    /// Its values, until released.
    #[default]
    UntilRelease,
    /// Nothing: the stop ends it.
    UntilStop,
}

/// What a stopped Show or Autopilot play leaves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum OtherEnd {
    /// Nothing: what is underneath shows.
    #[default]
    Reveal,
    /// Its values, at its rank and start, until released or started again.
    Stay,
}

/// What a stop leaves of a play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// The play is kept: its values stay.
    Keep,
    /// The play ends: what is underneath shows.
    Remove,
}

/// What `StopLook` leaves of a play at `rank` under `rules`.
pub const fn stops(rank: Rank, rules: MixRules) -> End {
    match (rank, rules.manual_end, rules.other_end) {
        (Rank::Manual, ManualEnd::UntilRelease, _)
        | (Rank::Show | Rank::Autopilot, _, OtherEnd::Stay) => End::Keep,
        (Rank::Manual, ManualEnd::UntilStop, _)
        | (Rank::Show | Rank::Autopilot, _, OtherEnd::Reveal) => End::Remove,
    }
}

/// Whether a play at `rank` in `state` is kept and `rules` no longer keep it: switching
/// to `UntilStop` or `Reveal` clears the stopped holds of those ranks.
pub const fn unkept(rank: Rank, state: PlayState, rules: MixRules) -> bool {
    matches!(state, PlayState::Kept) && matches!(stops(rank, rules), End::Remove)
}

/// One direct grab: the generation the front gave it. Its gestures reach only the light
/// properties it still holds: a later grab takes them away, and an old finger cannot
/// update or end its replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GrabId {
    pub generation: u32,
}

/// Whether a source started at `order` wins over one started at `over`: the latest start.
pub const fn wins(order: u64, over: u64) -> bool {
    order > over
}

/// Live sources of offers: plays and grabs (later Identify). Entry `i` sets the source
/// tag `2 + i` of each channel it wins. The plays come first, by slot.
pub const SOURCES: usize = 1024;

/// Direct grabs at once: the entries of the source table after the plays.
pub const GRABS: usize = SOURCES - PLAYS;

/// An entry of the source table: what the Mixer compares between two offers for one
/// channel, rank then start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub rank: Rank,
    /// Start order: a later start is greater. Unique per start.
    pub order: u64,
}

/// A direct grab as the tick keeps it: Manual, its start, and whether its end kept it.
/// Its offers stay with the engine, by the grab's entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grab {
    pub order: u64,
    pub kept: bool,
}

/// Where an offer goes: one channel, or a 16-bit pair as one word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Byte(u32),
    Word { coarse: u32, fine: u32 },
}

/// A value for a field this frame, 16 bits: a byte takes its high byte. The pair is
/// offered, compared and written as one value, so it is never torn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    pub at: Field,
    pub value: u16,
}

/// What one source offers this frame.
#[derive(Clone, Copy, Debug)]
pub enum Offering<'a> {
    /// Fixed offers: a step, a grab.
    List(&'a [Offer]),
    /// A sampled play: field `i` offers `values[i]`, nothing where it is `None` (a gap).
    Sampled {
        fields: &'a [Field],
        values: &'a [Option<u16>],
    },
}

impl Offering<'_> {
    pub const NOTHING: Self = Self::List(&[]);
}

/// The source table of this frame, written into `sources`: each sounding play by its
/// slot, then each grab (`grabs`, by entry) at Manual.
pub(crate) fn sources<'a>(
    plays: impl Iterator<Item = &'a crate::playback::Play>,
    grabs: &[Option<Grab>],
    sources: &mut [Option<Entry>],
) {
    sources.fill(None);
    for play in plays {
        if let Some(entry) = sources.get_mut(play.play.slot()) {
            *entry = Some(Entry {
                rank: play.rank,
                order: play.order,
            });
        }
    }
    let after = sources.get_mut(PLAYS..).unwrap_or_default();
    for (entry, grab) in after.iter_mut().zip(grabs) {
        *entry = grab.map(|grab| Entry {
            rank: Rank::Manual,
            order: grab.order,
        });
    }
}

/// Steps 1 to 3 for every source of `sources` with an entry: what each offers
/// (`offering` by entry) takes its channels by [`offer`]. The order of the sources does
/// not matter.
pub(crate) fn offers<'a>(
    channels: &mut Channels,
    sources: &[Option<Entry>],
    offering: impl Fn(usize) -> Offering<'a>,
) {
    for (by, _) in sources.iter().enumerate().filter(|(_, s)| s.is_some()) {
        match offering(by) {
            Offering::List(list) => {
                for sent in list {
                    offer(channels, sources, by, *sent);
                }
            }
            Offering::Sampled { fields, values } => {
                for (at, value) in fields.iter().zip(values) {
                    if let Some(value) = value {
                        offer(
                            channels,
                            sources,
                            by,
                            Offer {
                                at: *at,
                                value: *value,
                            },
                        );
                    }
                }
            }
        }
    }
}

/// Every channel back to its rest, nobody's: where a frame starts.
pub(crate) fn rest(channels: &mut Channels) {
    let Channels {
        value,
        rest,
        source,
        ..
    } = channels;
    for (row, rest) in value.iter_mut().zip(rest.chunks_exact(ADDRESSES)) {
        row.0.copy_from_slice(rest);
    }
    source.fill(SourceTag::REST);
}

/// Steps 4 and 6, per channel, once the offers are in: a raw hold (`raw` at `holds`)
/// replaces the byte, also with 0; then each follower moves toward its dark byte by the
/// `levels` of the modifiers it follows ([`follow`]). Blackout is the modifier every
/// channel follows.
pub(crate) fn channels(
    channels: &mut Channels,
    (raw, holds): (&[Levels], &[ChannelSet]),
    levels: [u8; MODIFIERS],
) {
    let Channels {
        value,
        dark,
        flags,
        follows,
        source,
        ..
    } = channels;
    let rows = value.iter_mut().zip(source.chunks_exact_mut(ADDRESSES));
    for (universe, (value, source)) in rows.enumerate() {
        let (Some(raw), Some(holds)) = (raw.get(universe), holds.get(universe)) else {
            continue;
        };
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
    }
    // every level full: no follower moves
    if levels == [u8::MAX; MODIFIERS] {
        return;
    }
    let by = scales(levels);
    let bytes = value.iter_mut().flat_map(|row| row.0.iter_mut());
    for (value, ((dark, flags), follows)) in
        bytes.zip(dark.iter().zip(flags.iter()).zip(follows.iter()))
    {
        if let Some(by) = by.get(usize::from(follows.0)) {
            *value = scale(*value, *dark, *flags, *by);
        }
    }
}

/// Steps 1 to 3 of who wins, for one offer of source table entry `by` (`sources`):
/// it takes its channel (or pair) from what is there when it ranks higher, or ranks
/// the same and is brighter under `highest`, or the same and started later. A channel
/// nobody offered yet takes any offer. An offer past the space or of an empty entry
/// does nothing.
pub(crate) fn offer(channels: &mut Channels, sources: &[Option<Entry>], by: usize, offer: Offer) {
    let (Some(Some(new)), Some(tag)) = (sources.get(by), SourceTag::of(by)) else {
        return;
    };
    let (coarse, fine) = match offer.at {
        Field::Byte(at) => (at, None),
        Field::Word { coarse, fine } => (coarse, Some(fine)),
    };
    let Some(at) = usize::try_from(coarse).ok().filter(|at| *at < CHANNELS) else {
        return;
    };
    let fine = match fine.map(|fine| usize::try_from(fine).ok().filter(|f| *f < CHANNELS)) {
        Some(None) => return,
        Some(Some(fine)) => Some(fine),
        None => None,
    };
    let (Some(held_by), Some(flags)) = (channels.source.get(at), channels.flags.get(at)) else {
        return;
    };
    let held = held_by
        .entry()
        .and_then(|entry| sources.get(entry).copied().flatten());
    let dark = word(at, fine, |at| channels.dark.get(at).copied());
    let now = word(at, fine, |at| {
        let row = channels.value.get(at / ADDRESSES)?;
        row.0.get(at % ADDRESSES).copied()
    });
    let [high, low] = offer.value.to_be_bytes();
    let value = match fine {
        Some(_) => offer.value,
        None => u16::from(high),
    };
    if !takes(
        *new,
        held,
        flags.highest(),
        value.abs_diff(dark),
        now.abs_diff(dark),
    ) {
        return;
    }
    let second = fine.map(|fine| (fine, low));
    for (at, byte) in core::iter::once((at, high)).chain(second) {
        if let (Some(row), Some(source)) = (
            channels.value.get_mut(at / ADDRESSES),
            channels.source.get_mut(at),
        ) && let Some(value) = row.0.get_mut(at % ADDRESSES)
        {
            *value = byte;
            *source = tag;
        }
    }
}

/// Whether `new` takes a channel `held` (if anyone) has: steps 2 and 3 of the table.
/// `bright` and `held_bright`: distances from the channel's dark value.
const fn takes(
    new: Entry,
    held: Option<Entry>,
    highest: bool,
    bright: u16,
    held_bright: u16,
) -> bool {
    let Some(held) = held else {
        return true;
    };
    if new.rank as u8 != held.rank as u8 {
        return new.rank as u8 > held.rank as u8;
    }
    if highest && bright != held_bright {
        return bright > held_bright;
    }
    wins(new.order, held.order)
}

/// A channel's byte, or its pair's word, as `bytes` reads them.
fn word(at: usize, fine: Option<usize>, bytes: impl Fn(usize) -> Option<u8>) -> u16 {
    let byte = |at: usize| bytes(at).map_or(0, u16::from);
    match fine {
        Some(fine) => (byte(at) << 8) | byte(fine),
        None => byte(at),
    }
}

/// A follower's byte: `value` moved toward `dark` by the `levels` of the modifiers it
/// `follows`, `dark + (value - dark) x m1 x ... x mn / 255^n` with one rounding at the end,
/// half away from `dark`. A channel that jumps goes all the way or not at all: `dark`
/// when a level it follows is 0, else `value`.
#[cfg(test)]
fn follow(value: u8, dark: u8, flags: Flags, follows: Follows, levels: [u8; MODIFIERS]) -> u8 {
    scale(value, dark, flags, scale_of(follows, levels))
}

/// What the levels of a set of modifier slots multiply to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scale {
    /// Every level full: the byte stays.
    Full,
    /// A level at 0: the byte goes to dark.
    Dark,
    /// `product / whole`, both above 0, `product` below `whole`.
    Part { product: i128, whole: i128 },
}

/// The scale of every set of slots, by its bits: worked out once a frame, so a channel
/// only looks its own up.
fn scales(levels: [u8; MODIFIERS]) -> [Scale; 1 << MODIFIERS] {
    let mut scales = [Scale::Full; 1 << MODIFIERS];
    for (follows, scale) in (0..=u8::MAX).zip(scales.iter_mut()) {
        *scale = scale_of(Follows(follows), levels);
    }
    scales
}

fn scale_of(follows: Follows, levels: [u8; MODIFIERS]) -> Scale {
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

/// [`follow`], with the levels multiplied already.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{BLACKOUT, ChannelNumbers, Universes};

    /// What reached channel 7 before the channel pass: its rest, or a Show offer.
    #[derive(Clone, Copy)]
    enum Wrote {
        Rest(u8),
        Offered(u8),
    }
    use Wrote::{Offered, Rest};

    const OFFERED: SourceTag = SourceTag::of(0).unwrap();

    /// The modifier levels with Blackout on or off, the others full.
    fn blackout(on: bool) -> [u8; MODIFIERS] {
        let mut levels = [u8::MAX; MODIFIERS];
        levels[BLACKOUT] = if on { 0 } else { u8::MAX };
        levels
    }

    #[test]
    fn rows_1_and_2_decide_each_channel() {
        // (row, what an offer or the rest wrote, raw hold, blackout, byte out, source)
        let cases = [
            ("nothing", None, None, false, 0, SourceTag::REST),
            (
                "a default",
                Some(Rest(200)),
                None,
                false,
                200,
                SourceTag::REST,
            ),
            ("a light", Some(Offered(255)), None, false, 255, OFFERED),
            (
                "2 raw over a light",
                Some(Offered(255)),
                Some(9),
                false,
                9,
                SourceTag::RAW,
            ),
            (
                "2 raw at 0 over a light",
                Some(Offered(255)),
                Some(0),
                false,
                0,
                SourceTag::RAW,
            ),
            (
                "2 raw over a default",
                Some(Rest(255)),
                Some(9),
                false,
                9,
                SourceTag::RAW,
            ),
            (
                "2 raw over nothing",
                None,
                Some(9),
                false,
                9,
                SourceTag::RAW,
            ),
            (
                "1 blackout over a raw hold",
                None,
                Some(9),
                true,
                0,
                SourceTag::RAW,
            ),
            (
                "1 blackout over a light",
                Some(Offered(255)),
                None,
                true,
                0,
                OFFERED,
            ),
            (
                "1 blackout over a default",
                Some(Rest(255)),
                None,
                true,
                0,
                SourceTag::REST,
            ),
            (
                "1 blackout over nothing",
                None,
                None,
                true,
                0,
                SourceTag::REST,
            ),
        ];
        let at = ChannelNumbers {
            universe: 1,
            address: 7,
        }
        .check(Universes::ONE)
        .unwrap();
        let beside = ChannelNumbers {
            universe: 1,
            address: 8,
        }
        .check(Universes::ONE)
        .unwrap();
        for (row, wrote, hold, dark, out, source) in cases {
            let mut all = Box::new(Channels::NEW);
            all.value[0].0[8] = 3; // whatever the frame before left
            rest(&mut all);
            match wrote {
                Some(Rest(level)) => {
                    all.rest[6] = level;
                    rest(&mut all);
                }
                Some(Offered(level)) => offer(
                    &mut all,
                    &[Some(Entry {
                        rank: Rank::Show,
                        order: 1,
                    })],
                    0,
                    Offer {
                        at: Field::Byte(6),
                        value: u16::from(level) << 8,
                    },
                ),
                None => {}
            }
            let (mut raw, mut holds) = ([Levels::ZERO], [ChannelSet::NONE]);
            if let Some(level) = hold {
                raw[0].0[6] = level;
                at.mark(&mut holds, true);
            }
            channels(&mut all, (&raw, &holds), blackout(dark));
            assert_eq!(
                (all.values()[0].0[6], all.source(at)),
                (out, source),
                "{row}"
            );
            assert_eq!(
                (all.values()[0].0[7], all.source(beside)),
                (0, SourceTag::REST)
            );
        }
    }

    #[test]
    fn blackout_reaches_every_channel_of_the_space() {
        let space = Universes::new(crate::MAX_UNIVERSES).unwrap();
        let (raw, mut holds) = (
            [Levels([255; ADDRESSES]); crate::MAX_UNIVERSES as usize],
            [ChannelSet::NONE; crate::MAX_UNIVERSES as usize],
        );
        for universe in 1..=crate::MAX_UNIVERSES {
            for address in 1..=ADDRESSES as u16 {
                let channel = ChannelNumbers { universe, address }.check(space).unwrap();
                channel.mark(&mut holds, true);
            }
        }
        // (blackout, the byte of every channel)
        let cases = [(false, 255), (true, 0)];
        for (dark, out) in cases {
            let mut all = Box::new(Channels::NEW);
            rest(&mut all);
            channels(&mut all, (&raw, &holds), blackout(dark));
            let missed = all
                .values()
                .iter()
                .flat_map(|row| row.0.iter())
                .position(|byte| *byte != out);
            assert_eq!(
                missed, None,
                "blackout {dark}: the first channel not at {out}"
            );
        }
    }

    #[test]
    fn a_released_hold_gives_the_channel_back_to_rest_in_the_next_frame() {
        let at = ChannelNumbers {
            universe: 1,
            address: 7,
        }
        .check(Universes::ONE)
        .unwrap();
        let mut all = Box::new(Channels::NEW);
        all.rest[6] = 40;
        // (frame, raw hold, byte out, source)
        let frames = [
            ("held", Some(9), 9, SourceTag::RAW),
            ("released", None, 40, SourceTag::REST),
        ];
        for (frame, hold, out, source) in frames {
            rest(&mut all);
            let (mut raw, mut holds) = ([Levels::ZERO], [ChannelSet::NONE]);
            if let Some(level) = hold {
                raw[0].0[6] = level;
                at.mark(&mut holds, true);
            }
            channels(&mut all, (&raw, &holds), blackout(false));
            assert_eq!(
                (all.values()[0].0[6], all.source(at)),
                (out, source),
                "{frame}"
            );
        }
    }

    #[test]
    fn a_follower_moves_toward_dark_by_its_levels() {
        let (fade, jump) = (Flags::FADE, Flags::NONE);
        let blackout_bit = Follows::BLACKOUT;
        let (master, group) = (Follows(1), Follows(1 << 2));
        let master_and_group = Follows(1 | 1 << 2);
        let with = |slots: &[(usize, u8)]| {
            let mut levels = [u8::MAX; MODIFIERS];
            for (slot, level) in slots {
                levels[*slot] = *level;
            }
            levels
        };
        // (case, value, dark, flags, follows, levels, out)
        let cases = [
            (
                "jump, blackout on",
                200,
                0,
                jump,
                blackout_bit,
                blackout(true),
                0,
            ),
            (
                "jump, blackout off",
                200,
                0,
                jump,
                blackout_bit,
                blackout(false),
                200,
            ),
            (
                "fade, blackout on",
                200,
                0,
                fade,
                blackout_bit,
                blackout(true),
                0,
            ),
            (
                "fade, blackout off",
                200,
                0,
                fade,
                blackout_bit,
                blackout(false),
                200,
            ),
            (
                "reverse dimmer dark",
                10,
                255,
                fade,
                blackout_bit,
                blackout(true),
                255,
            ),
            (
                "master at half",
                200,
                0,
                fade,
                master,
                with(&[(0, 128)]),
                100,
            ),
            (
                "half rounds away from dark",
                1,
                0,
                fade,
                master,
                with(&[(0, 128)]),
                1,
            ),
            (
                "under half rounds to dark",
                1,
                0,
                fade,
                master,
                with(&[(0, 127)]),
                0,
            ),
            (
                "toward a dark of 255",
                0,
                255,
                fade,
                master,
                with(&[(0, 128)]),
                127,
            ),
            (
                "jump at 1 stays",
                200,
                0,
                jump,
                master,
                with(&[(0, 1)]),
                200,
            ),
            (
                "one rounding for two",
                255,
                0,
                fade,
                master_and_group,
                with(&[(0, 128), (2, 128)]),
                64,
            ),
            (
                "a slot not followed",
                200,
                0,
                fade,
                group,
                with(&[(0, 0), (1, 0)]),
                200,
            ),
            (
                "follows nothing",
                200,
                0,
                fade,
                Follows(0),
                [0; MODIFIERS],
                200,
            ),
            (
                "all eight full",
                255,
                0,
                fade,
                Follows(0xff),
                [255; MODIFIERS],
                255,
            ),
            (
                "all eight at 254",
                255,
                0,
                fade,
                Follows(0xff),
                [254; MODIFIERS],
                247,
            ),
        ];
        for (case, value, dark, flags, follows, levels, out) in cases {
            assert_eq!(follow(value, dark, flags, follows, levels), out, "{case}");
        }
    }

    #[test]
    fn followed_levels_say_full_dark_or_part() {
        let (master, both) = (Follows(1), Follows(1 | 1 << 2));
        // (case, follows, levels, scale)
        let cases = [
            ("follows nothing", Follows(0), [0; MODIFIERS], Scale::Full),
            ("master full", master, [255; MODIFIERS], Scale::Full),
            ("two slots full", both, [255; MODIFIERS], Scale::Full),
            ("master at 0", master, [0; MODIFIERS], Scale::Dark),
            (
                "master at 128",
                master,
                [128; MODIFIERS],
                Scale::Part {
                    product: 128,
                    whole: 255,
                },
            ),
            (
                "two slots at 128",
                both,
                [128; MODIFIERS],
                Scale::Part {
                    product: 128 * 128,
                    whole: 255 * 255,
                },
            ),
        ];
        for (case, follows, levels, scale) in cases {
            assert_eq!(scale_of(follows, levels), scale, "{case}");
        }
    }
}

#[cfg(test)]
mod offers {
    use super::{Entry, Field, Offer, offer};
    use crate::channel::{ADDRESSES, CHANNELS, Channels, Flags, SourceTag};
    use crate::playback::Rank::{self, Autopilot, Manual, Show};

    /// One offer of a row: its rank, start order and value (a word on a pair).
    type Side = (Rank, u64, u16);

    /// The channel of a row, address 8 of universe 2, and the fine of its pair.
    const COARSE: usize = ADDRESSES + 7;
    const FINE: usize = COARSE + 1;

    /// Who wins one channel, or one 16-bit pair, in both candidate orders: (row, dark
    /// word, highest, pair, the winner, the other).
    #[test]
    fn the_offer_table_row_by_row_in_both_candidate_orders() {
        let cases: [(&str, u16, bool, bool, Side, Side); 11] = [
            (
                "rank: Show over Autopilot",
                0,
                false,
                false,
                (Show, 1, 10),
                (Autopilot, 2, 200),
            ),
            (
                "rank: Manual over Show",
                0,
                false,
                false,
                (Manual, 1, 0),
                (Show, 2, 255),
            ),
            (
                "rank beats highest",
                0,
                true,
                false,
                (Manual, 1, 10),
                (Show, 2, 200),
            ),
            (
                "latest start among equals",
                0,
                false,
                false,
                (Show, 2, 3),
                (Show, 1, 9),
            ),
            (
                "highest: greater brightness",
                0,
                true,
                false,
                (Show, 1, 200),
                (Show, 2, 100),
            ),
            (
                "highest, equal brightness: latest",
                0,
                true,
                false,
                (Show, 2, 100),
                (Show, 1, 100),
            ),
            (
                "highest on a reverse dimmer: 0 is brightest",
                255,
                true,
                false,
                (Show, 1, 0),
                (Show, 2, 255),
            ),
            (
                "highest on a reverse dimmer at equal distance: latest",
                255,
                true,
                false,
                (Show, 2, 155),
                (Show, 1, 155),
            ),
            (
                "highest on a 16-bit dimmer: the word decides",
                0,
                true,
                true,
                (Show, 1, 0x8100),
                (Show, 2, 0x80FF),
            ),
            (
                "highest on a reverse 16-bit dimmer",
                0xFFFF,
                true,
                true,
                (Show, 1, 0x00FF),
                (Show, 2, 0x0100),
            ),
            (
                "latest on a pair: both bytes from the later",
                0,
                false,
                true,
                (Show, 2, 0x0001),
                (Show, 1, 0xFF00),
            ),
        ];
        for (row, dark, highest, pair, a, b) in cases {
            let sources = [a, b].map(|(rank, order, _)| Some(Entry { rank, order }));
            let offered = |(_, _, value): Side| {
                if pair {
                    Offer {
                        at: Field::Word {
                            coarse: COARSE as u32,
                            fine: FINE as u32,
                        },
                        value,
                    }
                } else {
                    Offer {
                        at: Field::Byte(COARSE as u32),
                        value: u16::from(u8::try_from(value).unwrap()) << 8,
                    }
                }
            };
            for order in [[0, 1], [1, 0]] {
                let mut channels = Box::new(Channels::NEW);
                let [high, low] = dark.to_be_bytes();
                if pair {
                    channels.dark[COARSE] = high;
                    channels.dark[FINE] = low;
                } else {
                    channels.dark[COARSE] = low;
                }
                if highest {
                    channels.flags[COARSE] = Flags::HIGHEST;
                }
                for by in order {
                    offer(&mut channels, &sources, by, offered([a, b][by]));
                }
                let bytes = &channels.values()[1].0;
                let (coarse, fine) = (COARSE - ADDRESSES, FINE - ADDRESSES);
                let seen = if pair {
                    (
                        u16::from(bytes[coarse]) << 8 | u16::from(bytes[fine]),
                        channels.source[FINE],
                    )
                } else {
                    (u16::from(bytes[coarse]), channels.source[COARSE])
                };
                assert_eq!(
                    (seen.0, channels.source[COARSE], seen.1),
                    (a.2, SourceTag::of(0).unwrap(), SourceTag::of(0).unwrap()),
                    "{row}, offered in order {order:?}"
                );
            }
        }
    }

    /// Nothing changes for an offer of an empty entry, past the table or past the space.
    #[test]
    fn an_offer_that_names_nothing_changes_nothing() {
        let sources = [
            Some(Entry {
                rank: Show,
                order: 1,
            }),
            None,
        ];
        let cases = [
            (
                1,
                Offer {
                    at: Field::Byte(0),
                    value: 9,
                },
            ),
            (
                2,
                Offer {
                    at: Field::Byte(0),
                    value: 9,
                },
            ),
            (
                0,
                Offer {
                    at: Field::Byte(u32::MAX),
                    value: 9,
                },
            ),
            (
                0,
                Offer {
                    at: Field::Word {
                        coarse: 0,
                        fine: u32::MAX,
                    },
                    value: 9,
                },
            ),
            (
                0,
                Offer {
                    at: Field::Word {
                        coarse: 0,
                        fine: CHANNELS as u32,
                    },
                    value: 9,
                },
            ),
        ];
        for (by, sent) in cases {
            let mut channels = Box::new(Channels::NEW);
            offer(&mut channels, &sources, by, sent);
            assert_eq!(*channels, Channels::NEW, "{by} {sent:?}");
        }
    }
}
