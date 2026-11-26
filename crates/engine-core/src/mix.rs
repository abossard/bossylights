//! Who wins: the one place that decides between two values for one channel
//! (`docs/design/who-wins.md`, `docs/design/channel-model.md`). Nothing else compares or
//! overrides a level. Each channel is won on its own.
//!
//! | Step | Rule |
//! |---|---|
//! | 1 Offers | a live source offers a byte, or a 16-bit pair as one word. A source may offer nothing at a moment (a gap): a lower source shows through. A channel nobody offers keeps its rest |
//! | 2 Rank | Manual over Show over Autopilot. A direct grab is Manual |
//! | 3 Among equals | the latest start. A channel with `highest`: the greater brightness (the distance from `dark`, 16 bits on a pair), the latest on a tie |
//! | 4 Raw hold | replaces the byte, also with 0 (`Channels`) |
//! | 6 Modifiers | each follower moves toward `dark` by the levels it follows: Blackout (`Channels`) |
//!
//! [`Offers`] is steps 1 to 3. The source table has [`SOURCES`] entries: the plays by
//! slot, then the grabs by [`GrabEntry`]. A channel's source tag names its entry.
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

use crate::channel::{ADDRESSES, CHANNELS, Channels, Field, Levels, Pair, SourceTag};
use crate::playback::{PLAYS, PlayState, Plays, Rank};
use crate::{ManualEnd, MixRules, OtherEnd};

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

/// Whether a source started at `order` wins over one started at `over`: the latest start.
pub const fn wins(order: u64, over: u64) -> bool {
    order > over
}

/// Entries of the source table: plays and grabs (later Identify). Entry `i` sets the
/// source tag `2 + i` of each channel it wins. The plays come first, by slot.
pub const SOURCES: usize = 1024;

/// Direct grabs at once: the entries of the source table after the plays.
pub const GRABS: usize = SOURCES - PLAYS;

/// What the Mixer compares between two offers for one channel: rank, then start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Source {
    rank: Rank,
    order: u64,
}

/// A grab's place in the source table, below [`GRABS`]. The front picks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GrabEntry(u16);

impl GrabEntry {
    /// `None` at or past [`GRABS`].
    pub const fn new(index: usize) -> Option<Self> {
        if index < GRABS {
            Some(Self(index as u16))
        } else {
            None
        }
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// How a direct grab holds its offers.
///
/// ```text
/// (free) --Start--> Live --Keep--> Kept --Rules with UntilStop--> (free)
///                     |  --Offers--> Live, same start
/// any --Free, ReleaseAll--> (free)
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holding {
    /// Its finger is down: updates reach it.
    Live,
    /// It ended under [`ManualEnd::UntilRelease`]: its offers stay until released.
    Kept,
}

/// A direct grab as the tick keeps it: Manual, its start, how it holds. Its offers are
/// the tick's, by the grab's entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grab {
    pub order: u64,
    pub holding: Holding,
}

/// What a command does to one grab entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrabChange {
    /// A new grab takes the entry, live, as the latest start. Its offers come with it.
    Start,
    /// Its offers are replaced (an update, or lights a newer grab took); its start and
    /// holding stay. Nothing on a free entry.
    Offers,
    /// Its end under [`ManualEnd::UntilRelease`]: kept. Nothing on a free entry.
    Keep,
    /// The entry is free: its offers go.
    Free,
}

/// The direct grabs, by entry. A buffer the caller owns beside the state: a command
/// changes it in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grabs([Option<Grab>; GRABS]);

impl Grabs {
    pub const NONE: Self = Self([None; GRABS]);

    /// The grab at `entry`, if any.
    pub fn get(&self, entry: GrabEntry) -> Option<Grab> {
        self.0.get(entry.index()).copied().flatten()
    }

    /// Whether `change` takes the start order `order`: it does exactly when it starts a grab.
    pub(crate) fn change(&mut self, entry: GrabEntry, change: GrabChange, order: u64) -> bool {
        let Some(slot) = self.0.get_mut(entry.index()) else {
            return false;
        };
        match (change, *slot) {
            (GrabChange::Start, _) => {
                *slot = Some(Grab {
                    order,
                    holding: Holding::Live,
                });
                return true;
            }
            (GrabChange::Keep, Some(grab)) => {
                *slot = Some(Grab {
                    holding: Holding::Kept,
                    ..grab
                });
            }
            (GrabChange::Free, _) => *slot = None,
            (GrabChange::Offers | GrabChange::Keep, _) => {}
        }
        false
    }

    /// Every grab let go.
    pub(crate) fn release_all(&mut self) {
        self.0.fill(None);
    }

    /// The kept grabs `rules` no longer keep let go: switching to `UntilStop`.
    pub(crate) fn unkeep(&mut self, rules: MixRules) {
        let gone = |grab: &Grab| {
            grab.holding == Holding::Kept && unkept(Rank::Manual, PlayState::Kept, rules)
        };
        for slot in &mut self.0 {
            if slot.as_ref().is_some_and(gone) {
                *slot = None;
            }
        }
    }
}

/// One frame's steps 1 to 3: each source offers its words, and each offer takes its
/// channel, or its pair, from what holds it when it wins. The order of the offers does not
/// matter. An offer past the space, or of a source that is not sounding, does nothing.
pub struct Offers<'a> {
    pub(crate) channels: &'a mut Channels,
    pub(crate) plays: &'a Plays,
    pub(crate) grabs: &'a Grabs,
}

impl Offers<'_> {
    /// `word` on `at` from the play in `slot`: a byte takes its high byte.
    pub fn play(&mut self, slot: usize, at: Field, word: u16) {
        if slot < PLAYS {
            self.offer(slot, at, word);
        }
    }

    /// `word` on `at` from the grab at `entry`.
    pub fn grab(&mut self, entry: GrabEntry, at: Field, word: u16) {
        self.offer(PLAYS + entry.index(), at, word);
    }

    /// Entry `by` of the source table, while it sounds.
    fn source(&self, by: usize) -> Option<Source> {
        match by.checked_sub(PLAYS) {
            None => self
                .plays
                .at(by)
                .filter(|play| matches!(play.state, PlayState::Playing | PlayState::Kept))
                .map(|play| Source {
                    rank: play.rank,
                    order: play.order,
                }),
            Some(entry) => self
                .grabs
                .0
                .get(entry)
                .copied()
                .flatten()
                .map(|grab| Source {
                    rank: Rank::Manual,
                    order: grab.order,
                }),
        }
    }

    fn offer(&mut self, by: usize, at: Field, word: u16) {
        let (Some(new), Some(tag)) = (self.source(by), SourceTag::of(by)) else {
            return;
        };
        let place = |at: u32| usize::try_from(at).ok().filter(|at| *at < CHANNELS);
        let (Some(coarse), fine) = (match at {
            Field::Byte(at) => (place(at), None),
            Field::Word(Pair { coarse, fine }) => (place(coarse), Some(place(fine))),
        }) else {
            return;
        };
        let fine = match fine {
            None => None,
            Some(Some(fine)) => Some(fine),
            Some(None) => return,
        };
        let held = self
            .channels
            .source
            .get(coarse)
            .and_then(|tag| tag.entry())
            .and_then(|entry| self.source(entry));
        let channels = &mut *self.channels;
        let Some(flags) = channels.flags.get(coarse) else {
            return;
        };
        let dark = word_at(coarse, fine, |at| channels.dark.get(at).copied());
        let now = word_at(coarse, fine, |at| byte(&channels.value, at));
        let [high, low] = word.to_be_bytes();
        let offered = if fine.is_some() {
            word
        } else {
            u16::from(high)
        };
        if !takes(
            new,
            held,
            flags.highest(),
            (offered.abs_diff(dark), now.abs_diff(dark)),
        ) {
            return;
        }
        for (at, byte) in core::iter::once((coarse, high)).chain(fine.map(|fine| (fine, low))) {
            if let (Some(row), Some(source)) = (
                channels.value.get_mut(at / ADDRESSES),
                channels.source.get_mut(at),
            ) && let Some(value) = row.0.get_mut(at % ADDRESSES)
            {
                (*value, *source) = (byte, tag);
            }
        }
    }
}

/// Whether `new` takes a channel `held` has (if anyone): steps 2 and 3. `bright` is the
/// offer's distance from the channel's dark value and the held one's.
fn takes(new: Source, held: Option<Source>, highest: bool, (bright, over): (u16, u16)) -> bool {
    let Some(held) = held else {
        return true;
    };
    if new.rank != held.rank {
        return new.rank > held.rank;
    }
    if highest && bright != over {
        return bright > over;
    }
    wins(new.order, held.order)
}

/// A channel's byte, or its pair's word, as `bytes` reads them.
fn word_at(at: usize, fine: Option<usize>, bytes: impl Fn(usize) -> Option<u8>) -> u16 {
    let byte = |at: usize| bytes(at).map_or(0, u16::from);
    match fine {
        Some(fine) => (byte(at) << 8) | byte(fine),
        None => byte(at),
    }
}

/// The byte of channel `at` (counted from 0 over the space) in rows of a universe.
fn byte(rows: &[Levels], at: usize) -> Option<u8> {
    rows.get(at / ADDRESSES)?.0.get(at % ADDRESSES).copied()
}

#[cfg(test)]
mod tests {
    use crate::channel::{
        ADDRESSES, BLACKOUT, ChannelNumbers, ChannelSet, Channels, Configured, Field, Flags,
        Follows, Levels, MODIFIERS, Scale, SourceTag, Universes, scale_of,
    };

    /// The modifier levels with Blackout on or off, the others full.
    fn blackout(on: bool) -> [u8; MODIFIERS] {
        let mut levels = [u8::MAX; MODIFIERS];
        levels[BLACKOUT] = if on { 0 } else { u8::MAX };
        levels
    }

    #[test]
    fn rows_1_and_2_decide_each_channel() {
        let play = SourceTag::of(0).unwrap();
        // (row, what a play offered, raw hold, blackout, byte out, source)
        let cases = [
            ("nothing", None, None, false, 0, SourceTag::REST),
            ("a light", Some(255), None, false, 255, play),
            (
                "2 raw over a light",
                Some(255),
                Some(9),
                false,
                9,
                SourceTag::RAW,
            ),
            (
                "2 raw at 0 over a light",
                Some(255),
                Some(0),
                false,
                0,
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
            ("1 blackout over a light", Some(255), None, true, 0, play),
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
        // channels 7 and 8 of universe 1 are patched, at rest 0, following Blackout
        let patched = [6, 7].map(|at| Configured {
            at,
            rest: 0,
            dark: 0,
            flags: Flags::NONE,
            follows: Follows::BLACKOUT,
        });
        let followers = [6, 7].map(Field::Byte);
        for (row, wrote, hold, dark, out, source) in cases {
            let mut all = Box::new(Channels::NEW);
            all.configure(&[], &patched);
            all.value[0].0[7] = 3; // whatever the frame before left
            all.reset(&patched);
            if let Some(byte) = wrote {
                all.value[0].0[6] = byte;
                all.source[6] = play;
            }
            let (mut raw, mut holds) = ([Levels::ZERO], [ChannelSet::NONE]);
            if let Some(level) = hold {
                raw[0].0[6] = level;
                at.mark(&mut holds, true);
            }
            all.apply_winners(&followers, &raw, &holds, blackout(dark));
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
    fn blackout_reaches_every_follower_of_the_space() {
        use crate::channel::CHANNELS;
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
        // every channel patched; a channel nobody patched follows nothing (cx-3)
        let followers: Vec<Field> = (0..CHANNELS as u32).map(Field::Byte).collect();
        let patched = |follows| -> Vec<Configured> {
            (0..CHANNELS as u32)
                .map(|at| Configured {
                    at,
                    rest: 0,
                    dark: 0,
                    flags: Flags::NONE,
                    follows,
                })
                .collect()
        };
        // (blackout, what every channel follows, the byte of every channel)
        let cases = [
            (false, Follows::BLACKOUT, 255),
            (true, Follows::BLACKOUT, 0),
            (true, Follows::NONE, 255),
        ];
        for (dark, follows, out) in cases {
            let mut all = Box::new(Channels::NEW);
            all.configure(&[], &patched(follows));
            all.reset(&[]);
            all.apply_winners(&followers, &raw, &holds, blackout(dark));
            let missed = all
                .values()
                .iter()
                .flat_map(|row| row.0.iter())
                .position(|byte| *byte != out);
            assert_eq!(
                missed, None,
                "blackout {dark}, {follows:?}: the first channel not at {out}"
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
            all.reset(&[]);
            let (mut raw, mut holds) = ([Levels::ZERO], [ChannelSet::NONE]);
            if let Some(level) = hold {
                raw[0].0[6] = level;
                at.mark(&mut holds, true);
            }
            all.apply_winners(&[], &raw, &holds, blackout(false));
            assert_eq!(
                (all.values()[0].0[6], all.source(at)),
                (out, source),
                "{frame}"
            );
        }
    }

    #[test]
    fn a_patched_channel_a_raw_hold_sets_moves_by_its_levels_once() {
        // channel 7 of universe 1, patched, fades, follows Master at half: the held 200
        // shows 200 x 128 / 255 = 100.4, so 100; moved twice it would be 50
        let patched = [Configured {
            at: 6,
            rest: 0,
            dark: 0,
            flags: Flags::FADE,
            follows: Follows::MASTER,
        }];
        let at = ChannelNumbers {
            universe: 1,
            address: 7,
        }
        .check(Universes::ONE)
        .unwrap();
        let mut all = Box::new(Channels::NEW);
        all.configure(&[], &patched);
        all.reset(&patched);
        let (mut raw, mut holds) = ([Levels::ZERO], [ChannelSet::NONE]);
        raw[0].0[6] = 200;
        at.mark(&mut holds, true);
        all.apply_winners(&[Field::Byte(6)], &raw, &holds, levels(&[(0, 128)]));
        assert_eq!(
            (all.values()[0].0[6], all.source(at)),
            (100, SourceTag::RAW)
        );
    }

    /// The levels with `slots` set, the others full.
    fn levels(slots: &[(usize, u8)]) -> [u8; MODIFIERS] {
        let mut levels = [u8::MAX; MODIFIERS];
        for (slot, level) in slots {
            levels[*slot] = *level;
        }
        levels
    }

    #[test]
    fn a_pair_follower_moves_as_one_word() {
        use crate::channel::Pair;
        // Master at 128 (half) is slot 0. Worked by hand (docs/design/channel-model.md,
        // Rules): 0xB333 = 45,875; x 128 / 255 = 23,027.45, so 0x59F3: 89, 243. Byte by
        // byte it would be 90, 26. A reverse dimmer, dark 0xFFFF, at 0x4C00 = 19,456:
        // 65,535 - round(46,079 x 128 / 255) = 42,405 = 0xA5A5.
        let both = Follows::MASTER.with(Follows::BLACKOUT);
        let (fade, jump) = (Flags::FADE.with(Flags::PAIR), Flags::PAIR);
        let master = levels(&[(0, 128)]);
        // (case, flags, dark bytes, value bytes, levels, out)
        let cases = [
            ("master half", fade, [0, 0], [0xB3, 0x33], master, [89, 243]),
            (
                "blackout",
                fade,
                [0, 0],
                [0xB3, 0x33],
                blackout(true),
                [0, 0],
            ),
            (
                "master and blackout full",
                fade,
                [0, 0],
                [0xB3, 0x33],
                blackout(false),
                [0xB3, 0x33],
            ),
            (
                "reverse, master half",
                fade,
                [255, 255],
                [0x4C, 0],
                master,
                [165, 165],
            ),
            (
                "reverse, blackout",
                fade,
                [255, 255],
                [0x4C, 0],
                blackout(true),
                [255, 255],
            ),
            (
                "jump, master half",
                jump,
                [0, 0],
                [0xB3, 0x33],
                master,
                [0xB3, 0x33],
            ),
            (
                "jump, master 0",
                jump,
                [3, 0],
                [0xB3, 0x33],
                levels(&[(0, 0)]),
                [3, 0],
            ),
        ];
        for (case, flags, dark, value, levels, out) in cases {
            let pair = [0, 1].map(|at| Configured {
                at,
                rest: 0,
                dark: dark[at as usize],
                flags,
                follows: both,
            });
            let followers = [Field::Word(Pair { coarse: 0, fine: 1 })];
            let mut all = Box::new(Channels::NEW);
            all.configure(&[], &pair);
            all.reset(&pair);
            all.value[0].0[..2].copy_from_slice(&value);
            all.apply_winners(&followers, &[Levels::ZERO], &[ChannelSet::NONE], levels);
            assert_eq!(all.values()[0].0[..2], out, "{case}");
        }
    }

    #[test]
    fn a_channel_follows_only_the_slots_it_names() {
        // channels 1 to 4 patched at 200, fading, following nothing, Master, Blackout,
        // both; channel 5 unpatched under a raw hold of 200
        let follows = [
            Follows::NONE,
            Follows::MASTER,
            Follows::BLACKOUT,
            Follows::MASTER.with(Follows::BLACKOUT),
        ];
        let patched: Vec<Configured> = (0..4)
            .map(|at| Configured {
                at,
                rest: 200,
                dark: 0,
                flags: Flags::FADE,
                follows: follows[at as usize],
            })
            .collect();
        let followers = [1, 2, 3].map(Field::Byte);
        let held = ChannelNumbers {
            universe: 1,
            address: 5,
        }
        .check(Universes::ONE)
        .unwrap();
        // (case, levels, bytes of channels 1 to 5): 200 x 128 / 255 = 100.4
        let cases = [
            ("all full", levels(&[]), [200, 200, 200, 200, 200]),
            (
                "master half",
                levels(&[(0, 128)]),
                [200, 100, 200, 100, 200],
            ),
            ("blackout", blackout(true), [200, 200, 0, 0, 200]),
        ];
        for (case, levels, out) in cases {
            let mut all = Box::new(Channels::NEW);
            all.configure(&[], &patched);
            all.reset(&patched);
            let (mut raw, mut holds) = ([Levels::ZERO], [ChannelSet::NONE]);
            raw[0].0[4] = 200;
            held.mark(&mut holds, true);
            all.apply_winners(&followers, &raw, &holds, levels);
            assert_eq!(all.values()[0].0[..5], out, "{case}");
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
            // 255 x 128 / 255 = 128
            (
                "full at master 128",
                255,
                0,
                fade,
                master,
                with(&[(0, 128)]),
                128,
            ),
            // 200 x 64 / 255 = 50.20
            (
                "master at a quarter",
                200,
                0,
                fade,
                master,
                with(&[(0, 64)]),
                50,
            ),
            // a level at 0: dark, wherever dark is
            (
                "jump at master 0",
                200,
                10,
                jump,
                master,
                with(&[(0, 0)]),
                10,
            ),
            // a slot it does not follow moves nothing
            (
                "blackout on, follows master",
                200,
                0,
                fade,
                master,
                blackout(true),
                200,
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
                // Blackout is on only at 0: 255 x (254 / 255)^7 = 248.08
                "all eight at 254",
                255,
                0,
                fade,
                Follows(0xff),
                [254; MODIFIERS],
                248,
            ),
            (
                // one rounding: 1 x 128 x 128 / 255^2 = 0.25; one per level gives 1
                "one rounding for all levels",
                1,
                0,
                fade,
                master_and_group,
                with(&[(0, 128), (2, 128)]),
                0,
            ),
            (
                "blackout after master and group",
                200,
                9,
                fade,
                Follows(0xff),
                with(&[(0, 128), (1, 0), (2, 128)]),
                9,
            ),
        ];
        for (case, value, dark, flags, follows, levels, out) in cases {
            // one channel as the mixer left it, through the frame's own pass
            let mut all = Box::new(Channels::NEW);
            all.value[0].0[6] = value;
            (all.dark[6], all.flags[6], all.follows[6]) = (dark, flags, follows);
            let (raw, holds) = ([Levels::ZERO], [ChannelSet::NONE]);
            all.apply_winners(&[Field::Byte(6)], &raw, &holds, levels);
            assert_eq!(all.values()[0].0[6], out, "{case}");
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
    use super::*;
    use crate::Brightness;
    use crate::channel::{Configured, Flags, Follows};
    use crate::clock::{Clock, When};
    use crate::playback::{Launches, PlayId};
    use Rank::{Autopilot, Show};
    use core::time::Duration;

    /// Who offers: a play in a slot at a rank, kept or not, a slot where no play
    /// started, or a grab at an entry.
    #[derive(Clone, Copy, Debug)]
    enum By {
        Play(usize, Rank),
        Kept(usize, Rank),
        Waiting(usize),
        NoPlay(usize),
        Grab(usize),
    }
    use By::{Kept, Play, Waiting};
    const fn grab(entry: usize) -> By {
        By::Grab(entry)
    }

    /// One offer: by whom, its start order, where, the word.
    type Sent = (By, u64, Field, u16);

    const DIMMER: u32 = 0;
    const PAIR: Field = Field::Word(Pair { coarse: 1, fine: 2 });

    /// Channel 0 a dimmer, 1 and 2 a pair, as `dark` and `flags` say.
    fn config(dark: u8, flags: Flags) -> [Configured; 3] {
        [0, 1, 2].map(|at| Configured {
            at,
            rest: 7,
            dark,
            flags,
            follows: Follows::NONE,
        })
    }

    /// The bytes and source tags of channels 0 to 2 once `sent` is offered in its order.
    fn mixed(config: &[Configured], sent: &[Sent]) -> [(u8, SourceTag); 3] {
        let channels = offered(config, sent);
        [0, 1, 2].map(|at| (channels.value[0].0[at], channels.source[at]))
    }

    /// The channels once `sent` is offered in its order, before raw holds and modifiers.
    fn offered(config: &[Configured], sent: &[Sent]) -> Box<Channels> {
        let mut channels = Box::new(Channels::NEW);
        channels.configure(&[], config);
        channels.reset(config);
        let (mut plays, mut grabs) = (Plays::NONE, Grabs::NONE);
        for (by, order, _, _) in sent {
            let (slot, rank, when) = match *by {
                Play(slot, rank) | Kept(slot, rank) => (slot, rank, When::Now),
                Waiting(slot) => (slot, Show, When::Beat),
                By::NoPlay(_) => continue,
                By::Grab(entry) => {
                    grabs.change(GrabEntry::new(entry).unwrap(), GrabChange::Start, *order);
                    continue;
                }
            };
            let id = PlayId::new(slot, 1).unwrap();
            let clock = Clock::START.settle(Duration::from_millis(1));
            let mut launches = Launches::from(0).0;
            plays.start(
                (id, 7, rank),
                when,
                &clock,
                Duration::ZERO,
                (*order, &mut launches),
            );
            if matches!(by, Kept(..)) {
                plays.keep(id);
            }
        }
        let mut offers = Offers {
            channels: &mut channels,
            plays: &plays,
            grabs: &grabs,
        };
        for (by, _, at, word) in sent {
            match *by {
                Play(slot, _) | Kept(slot, _) | Waiting(slot) | By::NoPlay(slot) => {
                    offers.play(slot, *at, *word);
                }
                By::Grab(entry) => offers.grab(GrabEntry::new(entry).unwrap(), *at, *word),
            }
        }
        channels
    }

    fn tag(by: By) -> SourceTag {
        SourceTag::of(match by {
            Play(slot, _) | Kept(slot, _) | Waiting(slot) | By::NoPlay(slot) => slot,
            By::Grab(entry) => PLAYS + entry,
        })
        .unwrap()
    }

    #[test]
    fn the_offer_table_row_by_row_in_both_orders() {
        let (latest, highest) = (Flags::FADE, Flags::FADE.with(Flags::HIGHEST));
        let byte = Field::Byte(DIMMER);
        let rest = (7, SourceTag::REST);
        // (row, dark, flags, offers, channels 0 to 2: byte and who)
        type Row = (&'static str, u8, Flags, Vec<Sent>, [(u8, By); 3]);
        let rows: [Row; 15] = [
            ("1 none", 0, latest, vec![], [(7, Waiting(31)); 3]),
            (
                "2 one",
                0,
                latest,
                vec![(Play(0, Show), 1, byte, 0x80ff)],
                [(0x80, Play(0, Show)), (7, Waiting(31)), (7, Waiting(31))],
            ),
            (
                "3 Manual over Show",
                0,
                latest,
                vec![(grab(5), 1, byte, 0x0a00), (Play(3, Show), 2, byte, 0xc800)],
                [(10, grab(5)), (7, Waiting(31)), (7, Waiting(31))],
            ),
            (
                "3 Show over Autopilot",
                0,
                latest,
                vec![
                    (Play(1, Show), 1, byte, 0x0a00),
                    (Play(2, Autopilot), 2, byte, 0xc800),
                ],
                [(10, Play(1, Show)), (7, Waiting(31)), (7, Waiting(31))],
            ),
            (
                "4 two Show: the later start",
                0,
                latest,
                vec![
                    (Play(1, Show), 1, byte, 0x3200),
                    (Play(2, Show), 2, byte, 0x3c00),
                ],
                [(60, Play(2, Show)), (7, Waiting(31)), (7, Waiting(31))],
            ),
            (
                "4 a kept play is a source at its rank and start",
                0,
                latest,
                vec![
                    (Kept(1, Show), 2, byte, 0x3200),
                    (Play(2, Show), 1, byte, 0x3c00),
                ],
                [(50, Kept(1, Show)), (7, Waiting(31)), (7, Waiting(31))],
            ),
            (
                "6 highest: 200 over a later 40",
                0,
                highest,
                vec![
                    (Play(1, Show), 1, byte, 0xc800),
                    (Play(2, Show), 2, byte, 0x2800),
                ],
                [(200, Play(1, Show)), (7, Waiting(31)), (7, Waiting(31))],
            ),
            (
                "6 highest still ranks first",
                0,
                highest,
                vec![(Play(1, Show), 1, byte, 0xc800), (grab(0), 2, byte, 0x2800)],
                [(40, grab(0)), (7, Waiting(31)), (7, Waiting(31))],
            ),
            (
                "7 highest, equal: the later start",
                0,
                highest,
                vec![
                    (Play(1, Show), 1, byte, 0x6400),
                    (Play(2, Show), 2, byte, 0x6400),
                ],
                [(100, Play(2, Show)), (7, Waiting(31)), (7, Waiting(31))],
            ),
            (
                "8 reverse dimmer: 0 is brightest",
                255,
                highest,
                vec![
                    (Play(1, Show), 1, byte, 0x0000),
                    (Play(2, Show), 2, byte, 0xff00),
                ],
                [(0, Play(1, Show)), (7, Waiting(31)), (7, Waiting(31))],
            ),
            (
                "9 16-bit dimmer: the word decides, not the coarse",
                0,
                highest,
                vec![
                    (Play(1, Show), 1, PAIR, 0x0100),
                    (Play(2, Show), 2, PAIR, 0x00ff),
                ],
                [
                    (7, Waiting(31)),
                    (0x01, Play(1, Show)),
                    (0x00, Play(1, Show)),
                ],
            ),
            (
                "9 16-bit dimmer, equal coarse: the fine decides",
                0,
                highest,
                vec![
                    (Play(1, Show), 1, PAIR, 0x12ff),
                    (Play(2, Show), 2, PAIR, 0x1200),
                ],
                [
                    (7, Waiting(31)),
                    (0x12, Play(1, Show)),
                    (0xff, Play(1, Show)),
                ],
            ),
            (
                "10 a pair is never torn",
                0,
                latest,
                vec![
                    (Play(1, Show), 1, PAIR, 0x1234),
                    (Play(2, Show), 2, PAIR, 0x5678),
                ],
                [
                    (7, Waiting(31)),
                    (0x56, Play(2, Show)),
                    (0x78, Play(2, Show)),
                ],
            ),
            (
                "11 Autopilot under Manual",
                0,
                latest,
                vec![
                    (Play(1, Autopilot), 1, byte, 0x1100),
                    (grab(2), 2, byte, 0x5b00),
                ],
                [(91, grab(2)), (7, Waiting(31)), (7, Waiting(31))],
            ),
            (
                "11 the Manual offer ends: Autopilot again",
                0,
                latest,
                vec![(Play(1, Autopilot), 1, byte, 0x1100)],
                [(17, Play(1, Autopilot)), (7, Waiting(31)), (7, Waiting(31))],
            ),
        ];
        for (row, dark, flags, sent, out) in rows {
            let want = out.map(|(byte, by)| match by {
                Waiting(_) | By::NoPlay(_) => (byte, rest.1),
                by @ (Play(..) | Kept(..) | By::Grab(_)) => (byte, tag(by)),
            });
            let config = config(dark, flags);
            let mut reversed = sent.clone();
            reversed.reverse();
            assert_eq!(mixed(&config, &sent), want, "{row}");
            assert_eq!(mixed(&config, &reversed), want, "{row}, the other order");
        }
    }

    /// Row 13: a pair won by a play, a raw hold on its fine alone: the coarse stays the
    /// play's, the fine is the hold's.
    #[test]
    fn a_raw_hold_on_a_pairs_fine_takes_the_fine_alone() {
        use crate::channel::{ChannelNumbers, ChannelSet, MODIFIERS, Universes};
        let config = config(0, Flags::FADE);
        let mut channels = offered(&config, &[(Play(1, Show), 1, PAIR, 0x1234)]);
        let fine = ChannelNumbers {
            universe: 1,
            address: 3,
        }
        .check(Universes::ONE)
        .unwrap();
        let (mut raw, mut holds) = ([Levels::ZERO], [ChannelSet::NONE]);
        raw[0].0[2] = 0x99;
        fine.mark(&mut holds, true);
        channels.apply_winners(&[], &raw, &holds, [u8::MAX; MODIFIERS]);
        assert_eq!(
            [1, 2].map(|at| (channels.value[0].0[at], channels.source[at])),
            [(0x12, tag(Play(1, Show))), (0x99, SourceTag::RAW)]
        );
    }

    #[test]
    fn a_source_that_does_not_sound_offers_nothing() {
        let config = config(0, Flags::FADE);
        let byte = Field::Byte(DIMMER);
        let last = CHANNELS as u32 - 1;
        let past = Field::Word(Pair {
            coarse: last,
            fine: last + 1,
        });
        let grab_0 = (grab(0), 1, PAIR, 0x5000);
        // (case, offers, the ones of them that sound: the space is as if only they offered)
        let cases: [(&str, Vec<Sent>, Vec<Sent>); 4] = [
            (
                "a waiting play",
                vec![(Waiting(4), 1, byte, 0x5000)],
                vec![],
            ),
            (
                "past the space",
                vec![(Play(1, Show), 1, Field::Byte(u32::MAX), 0x5000)],
                vec![],
            ),
            (
                "a pair whose fine byte is past the space",
                vec![(Play(1, Show), 1, past, 0x5000)],
                vec![],
            ),
            (
                "a slot past the play table, while grab 0 sounds",
                vec![grab_0, (By::NoPlay(PLAYS), 2, byte, 0x5000)],
                vec![grab_0],
            ),
        ];
        for (case, sent, sounding) in cases {
            let (seen, only) = (offered(&config, &sent), offered(&config, &sounding));
            assert!(seen.value == only.value, "{case}: bytes");
            assert!(seen.source == only.source, "{case}: sources");
        }
    }

    #[test]
    fn a_source_wins_over_an_earlier_start_only() {
        // docs/engine.md, "Who wins": among equal ranks, the latest start. (order, over, wins)
        let cases = [(2, 1, true), (1, 2, false), (1, 1, false)];
        for (order, over, won) in cases {
            assert_eq!(wins(order, over), won, "{order} over {over}");
        }
    }

    #[test]
    fn a_grab_entry_follows_its_changes() {
        let entry = GrabEntry::new(3).unwrap();
        let live = |order| {
            Some(Grab {
                order,
                holding: Holding::Live,
            })
        };
        let kept = |order| {
            Some(Grab {
                order,
                holding: Holding::Kept,
            })
        };
        // (case, before, change, after, takes the order)
        let cases = [
            ("start on free", None, GrabChange::Start, live(9), true),
            (
                "start on a kept one",
                kept(2),
                GrabChange::Start,
                live(9),
                true,
            ),
            (
                "offers keep the start",
                live(2),
                GrabChange::Offers,
                live(2),
                false,
            ),
            ("offers on free", None, GrabChange::Offers, None, false),
            ("keep", live(2), GrabChange::Keep, kept(2), false),
            ("keep on free", None, GrabChange::Keep, None, false),
            ("free", kept(2), GrabChange::Free, None, false),
        ];
        for (case, before, change, after, takes) in cases {
            let mut grabs = Grabs::NONE;
            grabs.0[3] = before;
            assert_eq!(grabs.change(entry, change, 9), takes, "{case}");
            assert_eq!(grabs.get(entry), after, "{case}");
        }
        assert_eq!(GrabEntry::new(GRABS), None);
        assert_eq!(GRABS + PLAYS, SOURCES);
    }

    #[test]
    fn new_rules_let_go_of_the_kept_grabs_they_no_longer_keep() {
        let rules = |manual_end| MixRules {
            brightness: Brightness::Latest,
            manual_end,
            other_end: OtherEnd::Reveal,
        };
        // (case, holding, rules, left)
        let cases = [
            (
                "kept, until release",
                Holding::Kept,
                ManualEnd::UntilRelease,
                true,
            ),
            (
                "kept, until stop",
                Holding::Kept,
                ManualEnd::UntilStop,
                false,
            ),
            (
                "live, until stop",
                Holding::Live,
                ManualEnd::UntilStop,
                true,
            ),
        ];
        for (case, holding, manual_end, left) in cases {
            let mut grabs = Grabs::NONE;
            grabs.0[0] = Some(Grab { order: 1, holding });
            grabs.unkeep(rules(manual_end));
            assert_eq!(grabs.0[0].is_some(), left, "{case}");
        }
        let mut grabs = Grabs::NONE;
        grabs.0 = [Some(Grab {
            order: 1,
            holding: Holding::Live,
        }); GRABS];
        grabs.release_all();
        assert_eq!(grabs, Grabs::NONE);
    }
}

#[cfg(test)]
mod ends {
    use super::*;
    use crate::Brightness;

    const LATEST: MixRules = MixRules {
        brightness: Brightness::Latest,
        manual_end: ManualEnd::UntilRelease,
        other_end: OtherEnd::Reveal,
    };

    #[test]
    fn a_stop_keeps_or_removes_by_rank_and_rules() {
        use Rank::{Autopilot, Manual, Show};
        let rules = |manual_end, other_end| MixRules {
            brightness: Brightness::Latest,
            manual_end,
            other_end,
        };
        // (rank, manual end, other end, what is left)
        let cases = [
            (Manual, ManualEnd::UntilRelease, OtherEnd::Reveal, End::Keep),
            (Manual, ManualEnd::UntilRelease, OtherEnd::Stay, End::Keep),
            (Manual, ManualEnd::UntilStop, OtherEnd::Reveal, End::Remove),
            (Manual, ManualEnd::UntilStop, OtherEnd::Stay, End::Remove),
            (Show, ManualEnd::UntilRelease, OtherEnd::Reveal, End::Remove),
            (Show, ManualEnd::UntilStop, OtherEnd::Stay, End::Keep),
            (
                Autopilot,
                ManualEnd::UntilRelease,
                OtherEnd::Reveal,
                End::Remove,
            ),
            (Autopilot, ManualEnd::UntilStop, OtherEnd::Stay, End::Keep),
        ];
        for (rank, manual_end, other_end, end) in cases {
            assert_eq!(
                stops(rank, rules(manual_end, other_end)),
                end,
                "{rank:?} {manual_end:?} {other_end:?}"
            );
        }
    }

    #[test]
    fn new_rules_clear_only_the_kept_plays_they_no_longer_keep() {
        use Rank::{Autopilot, Manual, Show};
        let stay_until_stop = MixRules {
            manual_end: ManualEnd::UntilStop,
            other_end: OtherEnd::Stay,
            ..LATEST
        };
        // (rank, state, rules, cleared)
        let cases = [
            (Manual, PlayState::Kept, LATEST, false),
            (Manual, PlayState::Kept, stay_until_stop, true),
            (Manual, PlayState::Playing, stay_until_stop, false),
            (Show, PlayState::Kept, LATEST, true),
            (Show, PlayState::Kept, stay_until_stop, false),
            (Autopilot, PlayState::Kept, LATEST, true),
            (Autopilot, PlayState::Playing, LATEST, false),
        ];
        for (rank, state, rules, cleared) in cases {
            assert_eq!(
                unkept(rank, state, rules),
                cleared,
                "{rank:?} {state:?} {rules:?}"
            );
        }
    }
}
