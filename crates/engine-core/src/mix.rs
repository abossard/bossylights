//! Who wins: the one place that decides between two values for one channel
//! (`docs/design/who-wins.md`). Nothing else compares or overrides a level.
//!
//! | Rank | | Wins over |
//! |---|---|---|
//! | 1 | blackout | everything |
//! | 2 | a raw channel held on the DMX screen | the light that writes that channel, until released |
//! | 3 | a held value of a light | its default |
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
use crate::property::{Light, Property};
use serde::{Deserialize, Serialize};

/// The Mixer's three settings, saved with the show. An edit takes effect in the frame
/// that applies it.
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
/// One direct grab: the generation the front gave it. Its gestures reach only the light
/// properties it still holds: a later grab takes them away, and an old finger cannot
/// update or end its replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GrabId {
    pub generation: u32,
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
        *out = held.values;
        for (claim, property) in claims.iter_mut().zip(Property::ALL) {
            *claim = held.values.has(property).then(|| Claim {
                rank: Rank::Manual,
                order: held.since(property),
            });
        }
    }
    for play in plays.iter() {
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

/// What wants a channel this frame, and how strongly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// What the definition gives a channel no light value writes (a shutter held open),
    /// or 0 where nothing writes.
    Default(u8),
    /// The held values of a light, through its writer. A part not held is `Default`: its
    /// writer writes the channel's default from the definition.
    Light(u8),
    /// A raw channel held on the DMX screen.
    Raw(u8),
    /// Blackout: 0.
    Blackout,
}

impl Want {
    /// The byte that goes out.
    pub const fn level(self) -> u8 {
        match self {
            Self::Default(level) | Self::Light(level) | Self::Raw(level) => level,
            Self::Blackout => 0,
        }
    }
}

/// The winner of `first` and `then`, which came after it. The table of who wins, one
/// arm per row.
pub const fn mix(first: Want, then: Want) -> Want {
    use Want::{Blackout, Default, Light, Raw};
    match (first, then) {
        // 1: blackout wins over everything
        (Blackout, Default(_) | Light(_) | Raw(_) | Blackout)
        | (Default(_) | Light(_) | Raw(_), Blackout) => Blackout,
        // 2: a raw channel held wins over the light that writes that channel
        (Default(_) | Light(_), Raw(level)) | (Raw(level), Default(_) | Light(_)) => Raw(level),
        // 3: a held value of a light wins over its default
        (Default(_), Light(level)) | (Light(level), Default(_)) => Light(level),
        // the same rank: the later wins. Two lights never meet here (decision 48)
        (Default(_), Default(_)) | (Light(_), Light(_)) | (Raw(_), Raw(_)) => then,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Want::{Blackout, Default, Light, Raw};

    #[test]
    fn who_wins_row_by_row() {
        // (row, first, then, winner, level out)
        let cases = [
            (
                "1 blackout over a raw channel",
                Raw(9),
                Blackout,
                Blackout,
                0,
            ),
            (
                "1 blackout over a raw channel",
                Blackout,
                Raw(9),
                Blackout,
                0,
            ),
            ("1 blackout over a light", Light(255), Blackout, Blackout, 0),
            ("1 blackout over a light", Blackout, Light(255), Blackout, 0),
            (
                "1 blackout over a default",
                Default(255),
                Blackout,
                Blackout,
                0,
            ),
            (
                "1 blackout over a default",
                Blackout,
                Default(255),
                Blackout,
                0,
            ),
            ("1 blackout and blackout", Blackout, Blackout, Blackout, 0),
            ("2 raw over a light", Light(255), Raw(9), Raw(9), 9),
            ("2 raw over a light", Raw(9), Light(255), Raw(9), 9),
            ("2 raw at 0 over a light", Light(255), Raw(0), Raw(0), 0),
            ("2 raw over a default", Default(255), Raw(9), Raw(9), 9),
            ("2 raw over a default", Raw(9), Default(255), Raw(9), 9),
            ("2 raw over nothing", Default(0), Raw(9), Raw(9), 9),
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
        ];
        for (row, first, then, winner, level) in cases {
            assert_eq!(mix(first, then), winner, "{row}: {first:?} then {then:?}");
            assert_eq!(mix(first, then).level(), level, "{row}");
        }
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use crate::playback::{PLAYS, PlayId, Plays};
    use crate::property::{Gobo, Position, Rgb, Value};
    use crate::resolve::{LightNumber, LightRun, Lights};

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
            values.gobo.map(Value::Gobo),
        ];
        for value in each.into_iter().flatten() {
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
            plays.start(id, 7, *rank, *order);
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
        let cases: [(&str, Held, Vec<Source>, MixRules, Light); 22] = [
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
                gobo: Some(Gobo { wheel: 1, slot: 2 }),
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
