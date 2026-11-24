//! Who wins: the one place that decides between two values for one channel
//! (`docs/design/who-wins.md`). Nothing else compares or overrides a level.
//!
//! | Rank | | Wins over |
//! |---|---|---|
//! | 1 | blackout | everything |
//! | 2 | a raw channel held on the DMX screen | the light that writes that channel, until released |
//! | 3 | a held value of a light | its default |
//!
//! Rows 1 and 2 are decided channel by channel in one pass by `Channels`: a raw hold,
//! then Blackout as a modifier every channel follows. Row 3 is [`mix`], where two
//! writers meet.
//!
//! Two lights never reach one channel (decision 48): the patch refuses two fixtures on
//! one channel, and the Rig makes a channel two heads name the fixture's own, one writer.
//! Who wins between lights is decided per light property, before the Rig.
//!
//! | Between sources of one light property | Winner |
//! |---|---|
//! | two ranks | Manual over Show over Autopilot. A direct hold is Manual. Zero and black take part |
//! | one rank | the latest start; [`Brightness::Highest`]: the highest brightness, equal ones by start |
//! | absent | lets the other through |
//! | nothing | absent: the writers write the definition's default |
//!
//! | A play stops | [`MixRules`] | What is left |
//! |---|---|---|
//! | Manual, `StopLook` | [`ManualEnd::UntilRelease`] | kept: its values at its rank and start, until released or started again |
//! | Manual, `StopLook` | [`ManualEnd::UntilStop`] | nothing: what is underneath shows |
//! | Show or Autopilot, `StopLook` | [`OtherEnd::Reveal`] | nothing |
//! | Show or Autopilot, `StopLook` | [`OtherEnd::Stay`] | kept |
//! | Manual, `EndFlash`; any, `ReleaseHold` | any | nothing |
//! | a grab, `EndGrab` | [`ManualEnd::UntilRelease`] | kept: its value at its start, until released |
//! | a grab, `EndGrab` | [`ManualEnd::UntilStop`] | nothing |
//! | a grab, `ReleaseProperty`, `ReleaseAll` | any | nothing; `ReleaseProperty` leaves the plays |
//! | kept, rules change to remove it | `UntilStop` for Manual, `Reveal` for the others | nothing |

use crate::LightChange;
use crate::playback::{Entry, PlayState, Plays, Rank};
use crate::{Brightness, GrabId, Light, ManualEnd, MixRules, OtherEnd, Property};

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

/// Who holds a light's property: a rank and a start order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    rank: Rank,
    order: u64,
}

/// Who holds each property of a light this frame, by [`Property::index`]. A buffer of
/// the caller's, one per light, so [`show`] allocates nothing.
pub type Claims = [Option<Claim>; Property::ALL.len()];

/// Whether `value` of `property` claimed by `claim` wins over `now`'s, claimed by `over`
/// (`None`: nobody). Either order of two sources gives the same winner.
fn beats(
    property: Property,
    rules: MixRules,
    (claim, value): (Claim, &Light),
    (over, now): (Option<Claim>, &Light),
) -> bool {
    let Some(over) = over else {
        return true;
    };
    if claim.rank != over.rank {
        return claim.rank > over.rank;
    }
    if let (Property::Intensity, Brightness::Highest, Some(level), Some(was)) =
        (property, rules.brightness, value.intensity, now.intensity)
        && level != was
    {
        return level > was;
    }
    wins(claim.order, over.order)
}
/// Who holds a property of a light directly.
///
/// ```text
/// off --Grab--> Grab --Update--> Grab, same start
///                 |  --End, Keep--> Kept --Rules with UntilStop--> off
///                 |  --End, Remove--> off
/// any --Release, ReleaseAll, a later Grab--> off, or the later grab
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hand {
    /// A live grab: its updates and its end reach it.
    Grab(GrabId),
    /// Its grab ended under [`ManualEnd::UntilRelease`]: the value stays until released.
    Kept,
}

/// A held property: who holds it, and the start order it was grabbed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Holding {
    hand: Hand,
    since: u64,
}

/// A light's direct holds, all Manual: the values held of it, and per property who holds
/// it since when. A property has a value exactly when it has a holding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    values: Light,
    holds: [Option<Holding>; Property::ALL.len()],
}

impl Held {
    pub const NONE: Self = Self {
        values: Light::DARK,
        holds: [None; Property::ALL.len()],
    };

    /// The values held.
    pub const fn values(&self) -> &Light {
        &self.values
    }

    /// Offers each held property to the Mixer for this frame.
    fn offer(&self, out: &mut Light, claims: &mut Claims) {
        *out = self.values;
        for (claim, property) in claims.iter_mut().zip(Property::ALL) {
            *claim = self.values.has(property).then(|| Claim {
                rank: Rank::Manual,
                order: self.since(property),
            });
        }
    }

    /// `change` made to this light; a grab takes the start order `order`.
    pub fn change(&mut self, change: LightChange, order: u64) {
        match change {
            LightChange::Grab { value, grab } => self.set(
                value.property(),
                &value.light(),
                Some(Holding {
                    hand: Hand::Grab(grab),
                    since: order,
                }),
            ),
            LightChange::Update { value, grab } => {
                if let Some(holding) = self.grabbed(value.property(), grab) {
                    self.set(value.property(), &value.light(), Some(holding));
                }
            }
            LightChange::End {
                property,
                grab,
                end,
            } => {
                if let Some(holding) = self.grabbed(property, grab) {
                    match end {
                        End::Keep => self.set(
                            property,
                            &self.values.clone(),
                            Some(Holding {
                                hand: Hand::Kept,
                                ..holding
                            }),
                        ),
                        End::Remove => self.set(property, &Light::DARK, None),
                    }
                }
            }
            LightChange::Release { property } => self.set(property, &Light::DARK, None),
        }
    }

    /// The kept holds `rules` no longer keep let go: switching to `UntilStop`. A live
    /// grab stays.
    pub fn unkeep(&mut self, rules: MixRules) {
        for property in Property::ALL {
            let kept = self
                .holding(property)
                .is_some_and(|holding| holding.hand == Hand::Kept);
            if kept && unkept(Rank::Manual, PlayState::Kept, rules) {
                self.set(property, &Light::DARK, None);
            }
        }
    }

    /// `property` takes the value `from` has, held by `holding`: both or neither.
    fn set(&mut self, property: Property, from: &Light, holding: Option<Holding>) {
        if let Some(slot) = self.holds.get_mut(property.index()) {
            *slot = holding;
            self.values = self.values.with(
                property,
                if holding.is_some() {
                    from
                } else {
                    &Light::DARK
                },
            );
        }
    }

    fn holding(&self, property: Property) -> Option<Holding> {
        self.holds.get(property.index()).copied().flatten()
    }

    /// The holding of `property`, when `grab` holds it live.
    fn grabbed(&self, property: Property, grab: GrabId) -> Option<Holding> {
        self.holding(property)
            .filter(|holding| holding.hand == Hand::Grab(grab))
    }

    fn since(&self, property: Property) -> u64 {
        self.holding(property).map_or(0, |holding| holding.since)
    }
}

/// Whether a source started at `order` wins over one started at `over`: the latest start.
pub const fn wins(order: u64, over: u64) -> bool {
    order > over
}

/// What each light shows this frame, written into `out`: per property, the winner of its
/// direct holds (Manual) and the plays, playing or kept, under `rules`. `plan` gives a
/// play's entries by slot; `claims` is the caller's buffer beside `out`. The slot order
/// of the plays does not matter. Allocates nothing.
pub fn show<'a>(
    held: &[Held],
    plays: &Plays,
    plan: impl Fn(usize) -> &'a [Entry],
    rules: MixRules,
    out: &mut [Light],
    claims: &mut [Claims],
) {
    for ((out, claims), held) in out.iter_mut().zip(claims.iter_mut()).zip(held) {
        held.offer(out, claims);
    }
    for play in plays.sounding() {
        let claim = Claim {
            rank: play.rank,
            order: play.order,
        };
        for entry in plan(play.play.slot()) {
            let first = usize::from(entry.lights.first().index());
            let run = first..first + usize::from(entry.lights.count());
            let (Some(out), Some(claims)) = (out.get_mut(run.clone()), claims.get_mut(run)) else {
                continue;
            };
            for (light, claims) in out.iter_mut().zip(claims) {
                for (over, property) in claims.iter_mut().zip(Property::ALL) {
                    if entry.values.has(property)
                        && beats(property, rules, (claim, &entry.values), (*over, light))
                    {
                        *light = light.with(property, &entry.values);
                        *over = Some(claim);
                    }
                }
            }
        }
    }
}

/// What the resolver writes on a channel this frame, and how strongly. The base the
/// channel pass ([`channels`]) starts from, until plays offer per channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// What the definition gives a channel no light value writes (a shutter held open),
    /// or 0 where nothing writes.
    Default(u8),
    /// The held values of a light, through its writer. A part not held is `Default`: its
    /// writer writes the channel's default from the definition.
    Light(u8),
}

impl Want {
    /// The byte it writes.
    pub const fn level(self) -> u8 {
        match self {
            Self::Default(level) | Self::Light(level) => level,
        }
    }
}

/// The winner of `first` and `then`, which came after it, where two writers name one
/// channel: row 3 of the table.
pub const fn mix(first: Want, then: Want) -> Want {
    use Want::{Default, Light};
    match (first, then) {
        // 3: a held value of a light wins over its default
        (Default(_), Light(level)) | (Light(level), Default(_)) => Light(level),
        // the same rank: the later wins. Two lights never meet here (decision 48)
        (Default(_), Default(_)) | (Light(_), Light(_)) => then,
    }
}

/// What the levels of a set of modifier slots multiply to.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{
        ADDRESSES, BLACKOUT, ChannelNumbers, ChannelSet, Channels, Flags, Follows, Levels,
        MODIFIERS, Scale, SourceTag, Universes, scale_of,
    };
    use Want::{Default, Light};

    #[test]
    fn who_wins_row_by_row() {
        // (row, first, then, winner, level out)
        let cases = [
            (
                "3 a light over its default",
                Default(255),
                Light(0),
                Light(0),
                0,
            ),
            (
                "3 a light over its default",
                Light(7),
                Default(255),
                Light(7),
                7,
            ),
            ("3 a light over nothing", Default(0), Light(7), Light(7), 7),
            ("the later default", Default(1), Default(2), Default(2), 2),
        ];
        for (row, first, then, winner, level) in cases {
            assert_eq!(mix(first, then), winner, "{row}: {first:?} then {then:?}");
            assert_eq!(mix(first, then).level(), level, "{row}");
        }
    }

    /// The modifier levels with Blackout on or off, the others full.
    fn blackout(on: bool) -> [u8; MODIFIERS] {
        let mut levels = [u8::MAX; MODIFIERS];
        levels[BLACKOUT] = if on { 0 } else { u8::MAX };
        levels
    }

    #[test]
    fn rows_1_and_2_decide_each_channel() {
        // (row, what the resolver wrote, raw hold, blackout, byte out, source)
        let cases = [
            ("nothing", None, None, false, 0, SourceTag::REST),
            (
                "a default",
                Some(Default(200)),
                None,
                false,
                200,
                SourceTag::REST,
            ),
            (
                "a light",
                Some(Light(255)),
                None,
                false,
                255,
                SourceTag::LIGHTS,
            ),
            (
                "2 raw over a light",
                Some(Light(255)),
                Some(9),
                false,
                9,
                SourceTag::RAW,
            ),
            (
                "2 raw at 0 over a light",
                Some(Light(255)),
                Some(0),
                false,
                0,
                SourceTag::RAW,
            ),
            (
                "2 raw over a default",
                Some(Default(255)),
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
                Some(Light(255)),
                None,
                true,
                0,
                SourceTag::LIGHTS,
            ),
            (
                "1 blackout over a default",
                Some(Default(255)),
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
            all.reset_frame();
            let mut lit = [ChannelSet::NONE];
            if let Some(want) = wrote {
                all.value[0].0[6] = want.level();
                at.mark(&mut lit, matches!(want, Light(_)));
            }
            let (mut raw, mut holds) = ([Levels::ZERO], [ChannelSet::NONE]);
            if let Some(level) = hold {
                raw[0].0[6] = level;
                at.mark(&mut holds, true);
            }
            all.apply_winners(&lit, &raw, &holds, blackout(dark));
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
            all.reset_frame();
            let lit = [ChannelSet::NONE; crate::MAX_UNIVERSES as usize];
            all.apply_winners(&lit, &raw, &holds, blackout(dark));
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
            all.reset_frame();
            let (mut raw, mut holds) = ([Levels::ZERO], [ChannelSet::NONE]);
            if let Some(level) = hold {
                raw[0].0[6] = level;
                at.mark(&mut holds, true);
            }
            all.apply_winners(&[ChannelSet::NONE], &raw, &holds, blackout(false));
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
            // one channel as the resolver left it, through the frame's own pass
            let mut all = Box::new(Channels::NEW);
            all.value[0].0[6] = value;
            (all.dark[6], all.flags[6], all.follows[6]) = (dark, flags, follows);
            let (raw, holds) = ([Levels::ZERO], [ChannelSet::NONE]);
            all.apply_winners(&[ChannelSet::NONE], &raw, &holds, levels);
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
mod properties {
    use super::*;
    use crate::clock::{Clock, When};
    use crate::playback::{Launches, PLAYS, PlayId, Plays};
    use crate::{Gobo, LightNumber, LightRun, Lights, Position, Rgb, Value, Wheel};
    use core::time::Duration;

    const RED: Rgb = Rgb {
        red: u16::MAX,
        green: 0,
        blue: 0,
    };
    const GREEN: Rgb = Rgb {
        red: 0,
        green: u16::MAX,
        blue: 0,
    };
    const BLUE: Rgb = Rgb {
        red: 0,
        green: 0,
        blue: u16::MAX,
    };
    const LATEST: MixRules = MixRules {
        brightness: Brightness::Latest,
        manual_end: ManualEnd::UntilRelease,
        other_end: OtherEnd::Reveal,
    };
    const HIGHEST: MixRules = MixRules {
        brightness: Brightness::Highest,
        ..LATEST
    };

    fn colour(rgb: Rgb) -> Light {
        Light {
            colour: Some(rgb),
            ..Light::DARK
        }
    }

    fn intensity(level: u16) -> Light {
        Light {
            intensity: Some(level),
            ..Light::DARK
        }
    }

    /// The slot of each gobo wheel, wheel 1 first.
    fn gobos(gobos: [Option<u8>; Wheel::COUNT]) -> Light {
        Light {
            gobos,
            ..Light::DARK
        }
    }

    fn both(level: u16, rgb: Rgb) -> Light {
        Light {
            intensity: Some(level),
            colour: Some(rgb),
            ..Light::DARK
        }
    }

    /// `values` grabbed at `order`, one grab per property.
    fn held(values: Light, order: u64) -> Held {
        let mut held = Held::NONE;
        let grab = GrabId { generation: 1 };
        let each = [
            values.intensity.map(Value::Intensity),
            values.colour.map(Value::Colour),
            values.position.map(Value::Position),
            values.zoom.map(Value::Zoom),
        ];
        let gobos = (1..=3).filter_map(Wheel::new).map(|wheel| {
            values
                .gobo(wheel)
                .map(|slot| Value::Gobo(Gobo { wheel, slot }))
        });
        for value in each.into_iter().chain(gobos).flatten() {
            held.change(LightChange::Grab { value, grab }, order);
        }
        held
    }

    /// A play of light 1: its rank, its start order, whether it is kept, its values.
    type Source = (Rank, u64, PlayState, Light);

    const fn play(rank: Rank, order: u64, values: Light) -> Source {
        (rank, order, PlayState::Playing, values)
    }

    /// What light 1 shows with `hold` and `sources` played from slot `first` on, one
    /// slot apart: the slot order is the candidate order.
    fn shown(hold: Held, sources: &[Source], rules: MixRules, first: usize) -> Light {
        mixed(hold, sources, rules, first).0
    }

    /// What light 1 shows, as [`shown`], and who won each property.
    fn mixed(hold: Held, sources: &[Source], rules: MixRules, first: usize) -> (Light, Claims) {
        let run =
            LightRun::new(LightNumber::new(0, Lights::MOST).unwrap(), 1, Lights::MOST).unwrap();
        let mut plays = Plays::NONE;
        let mut entries = [None; PLAYS];
        for (n, (rank, order, state, values)) in sources.iter().enumerate() {
            let slot = (first + n * (PLAYS - 1)) % PLAYS;
            let id = PlayId::new(slot, 1).unwrap();
            plays.start(
                (id, 7, *rank),
                When::Now,
                &Clock::START,
                Duration::ZERO,
                (*order, &mut Launches::from(0).0),
            );
            if *state == PlayState::Kept {
                plays.keep(id);
            }
            entries[slot] = Some([Entry {
                lights: run,
                values: *values,
            }]);
        }
        let mut out = [Light::DARK];
        let mut claims = [[None; Property::ALL.len()]];
        show(
            &[hold],
            &plays,
            |slot| entries[slot].as_ref().map_or(&[][..], |e| &e[..]),
            rules,
            &mut out,
            &mut claims,
        );
        (out[0], claims[0])
    }

    #[test]
    fn the_mixer_table_row_by_row_in_both_candidate_orders() {
        use Rank::{Autopilot, Manual, Show};
        let kept = |rank, order, values| (rank, order, PlayState::Kept, values);
        // (row, direct hold, plays, rules, shown)
        let cases: [(&str, Held, Vec<Source>, MixRules, Light); 26] = [
            ("nothing", Held::NONE, vec![], LATEST, Light::DARK),
            (
                "a hold alone",
                held(colour(BLUE), 1),
                vec![],
                LATEST,
                colour(BLUE),
            ),
            (
                "a play alone",
                Held::NONE,
                vec![play(Show, 1, colour(RED))],
                LATEST,
                colour(RED),
            ),
            // rank: Manual > Show > Autopilot, whatever started last
            (
                "Manual over a later Show",
                Held::NONE,
                vec![play(Manual, 1, colour(RED)), play(Show, 2, colour(GREEN))],
                LATEST,
                colour(RED),
            ),
            (
                "Show over a later Autopilot",
                Held::NONE,
                vec![
                    play(Show, 1, colour(GREEN)),
                    play(Autopilot, 2, colour(BLUE)),
                ],
                LATEST,
                colour(GREEN),
            ),
            (
                "Manual over Show over Autopilot",
                Held::NONE,
                vec![
                    play(Autopilot, 3, colour(BLUE)),
                    play(Manual, 1, colour(RED)),
                    play(Show, 2, colour(GREEN)),
                ],
                LATEST,
                colour(RED),
            ),
            (
                "a direct hold is Manual: over a later Show",
                held(colour(RED), 1),
                vec![play(Show, 2, colour(GREEN))],
                LATEST,
                colour(RED),
            ),
            // equal rank: the latest start
            (
                "two Shows, the later wins",
                Held::NONE,
                vec![play(Show, 1, colour(RED)), play(Show, 2, colour(BLUE))],
                LATEST,
                colour(BLUE),
            ),
            (
                "a later Manual play over a hold",
                held(colour(BLUE), 1),
                vec![play(Manual, 2, colour(RED))],
                LATEST,
                colour(RED),
            ),
            (
                "a later hold over a Manual play",
                held(colour(BLUE), 2),
                vec![play(Manual, 1, colour(RED))],
                LATEST,
                colour(BLUE),
            ),
            (
                "a later hold of one property only",
                held(intensity(5), 3),
                vec![play(Manual, 2, both(9, RED))],
                LATEST,
                both(5, RED),
            ),
            // zero and black take part, absent lets through
            (
                "Manual zero darkens a Show",
                Held::NONE,
                vec![
                    play(Show, 2, both(255, GREEN)),
                    play(Manual, 1, intensity(0)),
                ],
                LATEST,
                both(0, GREEN),
            ),
            (
                "Manual black claims colour",
                Held::NONE,
                vec![
                    play(Show, 2, colour(GREEN)),
                    play(Manual, 1, colour(Rgb::BLACK)),
                ],
                LATEST,
                colour(Rgb::BLACK),
            ),
            (
                "absent colour lets the hold through",
                held(colour(BLUE), 1),
                vec![play(Manual, 2, intensity(0))],
                LATEST,
                both(0, BLUE),
            ),
            // brightness: the latest, or the highest within the winning rank
            (
                "latest: the later dimmer Show",
                Held::NONE,
                vec![play(Show, 1, intensity(200)), play(Show, 2, intensity(100))],
                LATEST,
                intensity(100),
            ),
            (
                "highest: the earlier brighter Show",
                Held::NONE,
                vec![play(Show, 1, intensity(200)), play(Show, 2, intensity(100))],
                HIGHEST,
                intensity(200),
            ),
            (
                "highest: a brighter play over an earlier dimmer hold",
                held(intensity(100), 5),
                vec![play(Manual, 3, intensity(200))],
                HIGHEST,
                intensity(200),
            ),
            (
                "highest stays in the winning rank",
                Held::NONE,
                vec![
                    play(Autopilot, 1, intensity(255)),
                    play(Show, 2, intensity(10)),
                ],
                HIGHEST,
                intensity(10),
            ),
            (
                "highest: a dim Manual over a bright Autopilot",
                Held::NONE,
                vec![
                    play(Manual, 1, intensity(10)),
                    play(Autopilot, 2, intensity(255)),
                ],
                HIGHEST,
                intensity(10),
            ),
            (
                "highest is for brightness only: colour by the latest",
                Held::NONE,
                vec![
                    play(Show, 1, both(200, RED)),
                    play(Show, 2, both(100, BLUE)),
                ],
                HIGHEST,
                both(200, BLUE),
            ),
            // a kept play keeps its rank and start
            (
                "a kept Show over a later Autopilot",
                Held::NONE,
                vec![
                    kept(Show, 1, colour(GREEN)),
                    play(Autopilot, 2, colour(BLUE)),
                ],
                LATEST,
                colour(GREEN),
            ),
            (
                "a kept Show under a later Show",
                Held::NONE,
                vec![kept(Show, 1, colour(GREEN)), play(Show, 2, colour(BLUE))],
                LATEST,
                colour(BLUE),
            ),
            // each gobo wheel is its own claim
            (
                "two wheels from two plays, wheel 2 later",
                Held::NONE,
                vec![
                    play(Manual, 1, gobos([Some(3), None, None])),
                    play(Manual, 2, gobos([None, Some(7), None])),
                ],
                LATEST,
                gobos([Some(3), Some(7), None]),
            ),
            (
                "two wheels from two plays, wheel 1 later",
                Held::NONE,
                vec![
                    play(Manual, 2, gobos([Some(3), None, None])),
                    play(Manual, 1, gobos([None, Some(7), None])),
                ],
                LATEST,
                gobos([Some(3), Some(7), None]),
            ),
            (
                "a later start takes only its own wheel",
                Held::NONE,
                vec![
                    play(Show, 1, gobos([Some(3), Some(7), Some(2)])),
                    play(Show, 2, gobos([None, Some(5), None])),
                ],
                LATEST,
                gobos([Some(3), Some(5), Some(2)]),
            ),
            (
                "a hold of wheel 2 leaves wheel 1 to a Show",
                held(gobos([None, Some(7), None]), 1),
                vec![play(Show, 2, gobos([Some(3), Some(1), None]))],
                LATEST,
                gobos([Some(3), Some(7), None]),
            ),
        ];
        for (row, hold, sources, rules, expected) in cases {
            for first in 0..PLAYS {
                assert_eq!(
                    shown(hold, &sources, rules, first),
                    expected,
                    "{row}, from slot {first}"
                );
            }
            let mut reversed = sources.clone();
            reversed.reverse();
            assert_eq!(
                shown(hold, &reversed, rules, 0),
                expected,
                "{row}, reversed"
            );
        }
    }

    #[test]
    fn equal_brightness_is_won_by_the_later_start() {
        use Rank::{Manual, Show};
        let equal = vec![play(Show, 1, intensity(100)), play(Show, 2, intensity(100))];
        // (row, direct hold, plays, rules, the brightness winner's rank and start)
        let cases = [
            (
                "highest, equal",
                Held::NONE,
                equal.clone(),
                HIGHEST,
                (Show, 2),
            ),
            ("latest, equal", Held::NONE, equal, LATEST, (Show, 2)),
            (
                "highest, the earlier brighter",
                Held::NONE,
                vec![play(Show, 1, intensity(200)), play(Show, 2, intensity(100))],
                HIGHEST,
                (Show, 1),
            ),
            (
                "highest, a hold equal to an earlier play",
                held(intensity(100), 5),
                vec![play(Manual, 3, intensity(100))],
                HIGHEST,
                (Manual, 5),
            ),
        ];
        for (row, hold, sources, rules, (rank, order)) in cases {
            let won = |sources: &[Source], first| {
                let (_, claims) = mixed(hold, sources, rules, first);
                claims[Property::Intensity.index()]
            };
            let winner = Some(Claim { rank, order });
            for first in 0..PLAYS {
                assert_eq!(won(&sources, first), winner, "{row}, from slot {first}");
            }
            let mut reversed = sources.clone();
            reversed.reverse();
            assert_eq!(won(&reversed, 0), winner, "{row}, reversed");
        }
    }

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

    #[test]
    fn a_released_hold_lets_the_play_through() {
        for property in Property::ALL {
            let all = Light {
                position: Some(Position { pan: 1, tilt: 2 }),
                zoom: Some(3),
                gobos: [Some(2), Some(5), Some(1)],
                ..both(4, BLUE)
            };
            let mut hold = held(all, 3);
            hold.change(LightChange::Release { property }, 4);
            assert_eq!(
                hold.values,
                all.with(property, &Light::DARK),
                "{property:?}"
            );
            hold.change(LightChange::Release { property }, 5); // again: a no-op
            assert_eq!(
                hold.values,
                all.with(property, &Light::DARK),
                "{property:?}"
            );
            for other in Property::ALL {
                assert_eq!(hold.since(other), if other == property { 0 } else { 3 });
            }
        }
    }

    #[test]
    fn a_grab_is_reached_only_by_its_own_gestures() {
        use LightChange::{End as Ends, Grab, Release, Update};
        let one = GrabId { generation: 1 };
        let two = GrabId { generation: 2 };
        let green = Value::Colour(GREEN);
        let red = Value::Colour(RED);
        let white = Value::Colour(Rgb {
            red: u16::MAX,
            green: u16::MAX,
            blue: u16::MAX,
        });
        let until_stop = MixRules {
            manual_end: ManualEnd::UntilStop,
            ..LATEST
        };
        let end = |grab, end| Ends {
            property: Property::Colour,
            grab,
            end,
        };
        // (row, changes at start orders 1, 3, 5 and on, rules after, held colour; and
        // with a Manual blue play started at 2, what light 1 shows)
        type Row = (&'static str, Vec<LightChange>, MixRules, Option<Rgb>, Rgb);
        let cases: [Row; 14] = [
            (
                "a grab",
                vec![Grab {
                    value: green,
                    grab: one,
                }],
                LATEST,
                Some(GREEN),
                BLUE,
            ),
            (
                "an update keeps its start",
                vec![
                    Grab {
                        value: green,
                        grab: one,
                    },
                    Update {
                        value: red,
                        grab: one,
                    },
                ],
                LATEST,
                Some(RED),
                BLUE,
            ),
            (
                "a later grab wins over a play",
                vec![
                    Update {
                        value: red,
                        grab: one,
                    },
                    Grab {
                        value: green,
                        grab: one,
                    },
                ],
                LATEST,
                Some(GREEN),
                GREEN,
            ),
            (
                "an update of a replaced grab",
                vec![
                    Grab {
                        value: green,
                        grab: one,
                    },
                    Grab {
                        value: red,
                        grab: two,
                    },
                    Update {
                        value: white,
                        grab: one,
                    },
                ],
                LATEST,
                Some(RED),
                RED,
            ),
            (
                "an end of a replaced grab",
                vec![
                    Grab {
                        value: green,
                        grab: one,
                    },
                    Grab {
                        value: red,
                        grab: two,
                    },
                    end(one, End::Remove),
                ],
                LATEST,
                Some(RED),
                RED,
            ),
            (
                "an end of another property",
                vec![
                    Grab {
                        value: green,
                        grab: one,
                    },
                    Ends {
                        property: Property::Intensity,
                        grab: one,
                        end: End::Remove,
                    },
                ],
                LATEST,
                Some(GREEN),
                BLUE,
            ),
            (
                "an end kept",
                vec![
                    Grab {
                        value: green,
                        grab: one,
                    },
                    end(one, End::Keep),
                ],
                LATEST,
                Some(GREEN),
                BLUE,
            ),
            (
                "an update after the end",
                vec![
                    Grab {
                        value: green,
                        grab: one,
                    },
                    end(one, End::Keep),
                    Update {
                        value: red,
                        grab: one,
                    },
                ],
                LATEST,
                Some(GREEN),
                BLUE,
            ),
            (
                "an end removed",
                vec![
                    Grab {
                        value: green,
                        grab: one,
                    },
                    end(one, End::Remove),
                ],
                LATEST,
                None,
                BLUE,
            ),
            (
                "kept, then UntilStop",
                vec![
                    Grab {
                        value: green,
                        grab: one,
                    },
                    end(one, End::Keep),
                ],
                until_stop,
                None,
                BLUE,
            ),
            (
                "live, then UntilStop",
                vec![Grab {
                    value: green,
                    grab: one,
                }],
                until_stop,
                Some(GREEN),
                BLUE,
            ),
            (
                "kept, then a later grab, then UntilStop",
                vec![
                    Grab {
                        value: green,
                        grab: one,
                    },
                    end(one, End::Keep),
                    Grab {
                        value: red,
                        grab: two,
                    },
                ],
                until_stop,
                Some(RED),
                RED,
            ),
            (
                "released",
                vec![
                    Grab {
                        value: green,
                        grab: one,
                    },
                    Release {
                        property: Property::Colour,
                    },
                ],
                LATEST,
                None,
                BLUE,
            ),
            (
                "another property released",
                vec![
                    Grab {
                        value: green,
                        grab: one,
                    },
                    Release {
                        property: Property::Intensity,
                    },
                ],
                LATEST,
                Some(GREEN),
                BLUE,
            ),
        ];
        for (row, changes, rules, holds, shows) in cases {
            let mut hold = Held::NONE;
            for (n, change) in changes.into_iter().enumerate() {
                hold.change(change, 1 + 2 * n as u64);
            }
            hold.unkeep(rules);
            assert_eq!(hold.values.colour, holds, "{row}");
            let blue = [play(Rank::Manual, 2, colour(BLUE))];
            for first in 0..PLAYS {
                assert_eq!(
                    shown(hold, &blue, rules, first).colour,
                    Some(shows),
                    "{row}"
                );
            }
        }
    }
}
