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
//! | a direct hold and a play, or two plays | the latest start; absent lets the other through |
//! | nothing | absent: the writers write the definition's default |

use crate::playback::{Entry, PLAYS, Play, Plays};
use crate::property::{Light, Property};

/// A light's direct holds: the values held of it, and the start order each property was
/// held at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    pub values: Light,
    pub since: [u64; Property::ALL.len()],
}

impl Held {
    pub const NONE: Self = Self {
        values: Light::DARK,
        since: [0; Property::ALL.len()],
    };

    /// `property` held at `order` with the value `from` has.
    pub fn hold(&mut self, property: Property, from: &Light, order: u64) {
        self.values = self.values.with(property, from);
        if let Some(since) = self.since.get_mut(property.index()) {
            *since = order;
        }
    }

    /// `property` let go.
    pub fn release(&mut self, property: Property) {
        self.hold(property, &Light::DARK, 0);
    }

    fn since(&self, property: Property) -> u64 {
        self.since.get(property.index()).copied().unwrap_or(0)
    }
}

/// Whether a source started at `order` wins over one started at `over`: the latest start.
pub const fn wins(order: u64, over: u64) -> bool {
    order > over
}

/// What each light shows this frame, written into `out`: its direct holds, and over them
/// the values of each play by start order, where the play started later than the hold.
/// `plan` gives a play's entries by slot. Allocates nothing.
pub fn show<'a>(
    held: &[Held],
    plays: &Plays,
    plan: impl Fn(usize) -> &'a [Entry],
    out: &mut [Light],
) {
    for (out, held) in out.iter_mut().zip(held) {
        *out = held.values;
    }
    let mut ordered = [None::<Play>; PLAYS];
    for (slot, play) in ordered.iter_mut().zip(plays.iter()) {
        *slot = Some(*play);
    }
    ordered.sort_unstable_by_key(|play| play.map(|p| p.order));
    for play in ordered.iter().flatten() {
        for entry in plan(play.play.slot()) {
            let first = usize::from(entry.lights.first().index());
            let run = first..first + usize::from(entry.lights.count());
            let (Some(out), Some(held)) = (out.get_mut(run.clone()), held.get(run)) else {
                continue;
            };
            for (light, held) in out.iter_mut().zip(held) {
                for property in Property::ALL {
                    if entry.values.has(property) && wins(play.order, held.since(property)) {
                        *light = light.with(property, &entry.values);
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
    use crate::playback::{PlayId, Plays};
    use crate::property::Rgb;
    use crate::resolve::{LightNumber, LightRun, Lights};

    const RED: Rgb = Rgb {
        red: u16::MAX,
        green: 0,
        blue: 0,
    };
    const BLUE: Rgb = Rgb {
        red: 0,
        green: 0,
        blue: u16::MAX,
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

    fn held(values: Light, order: u64) -> Held {
        let mut held = Held::NONE;
        for property in Property::ALL {
            if values.has(property) {
                held.hold(property, &values, order);
            }
        }
        held
    }

    #[test]
    fn the_latest_start_wins_each_property_and_absent_lets_through() {
        let run =
            LightRun::new(LightNumber::new(0, Lights::MOST).unwrap(), 1, Lights::MOST).unwrap();
        let both = Light {
            intensity: Some(9),
            colour: Some(RED),
            ..Light::DARK
        };
        // (row, direct hold, its order, the play's values (None: no play), its order, shown)
        let cases = [
            ("nothing", Held::NONE, None, 0, Light::DARK),
            ("a hold alone", held(colour(BLUE), 1), None, 0, colour(BLUE)),
            (
                "a play alone",
                Held::NONE,
                Some(colour(RED)),
                1,
                colour(RED),
            ),
            (
                "a later play over a hold",
                held(colour(BLUE), 1),
                Some(colour(RED)),
                2,
                colour(RED),
            ),
            (
                "a later hold over a play",
                held(colour(BLUE), 2),
                Some(colour(RED)),
                1,
                colour(BLUE),
            ),
            (
                "black claims colour",
                held(colour(BLUE), 1),
                Some(colour(Rgb::BLACK)),
                2,
                colour(Rgb::BLACK),
            ),
            (
                "zero claims intensity",
                held(intensity(9), 1),
                Some(intensity(0)),
                2,
                intensity(0),
            ),
            (
                "absent colour lets the hold through",
                held(colour(BLUE), 1),
                Some(intensity(0)),
                2,
                Light {
                    intensity: Some(0),
                    colour: Some(BLUE),
                    ..Light::DARK
                },
            ),
            (
                "a later hold of one property only",
                held(intensity(5), 3),
                Some(both),
                2,
                Light {
                    intensity: Some(5),
                    colour: Some(RED),
                    ..Light::DARK
                },
            ),
        ];
        for (row, hold, values, order, shown) in cases {
            let mut plays = Plays::NONE;
            let entries = values.map(|values| {
                [Entry {
                    lights: run,
                    values,
                }]
            });
            if entries.is_some() {
                plays.start(PlayId::new(0, 1).unwrap(), 1, order);
            }
            let mut out = [Light::DARK];
            show(
                &[hold],
                &plays,
                |_| entries.as_ref().map_or(&[][..], |e| &e[..]),
                &mut out,
            );
            assert_eq!(out, [shown], "{row}");
        }
    }

    #[test]
    fn between_two_plays_the_later_start_wins_in_either_slot() {
        let run =
            LightRun::new(LightNumber::new(0, Lights::MOST).unwrap(), 1, Lights::MOST).unwrap();
        let red = [Entry {
            lights: run,
            values: colour(RED),
        }];
        let blue = [Entry {
            lights: run,
            values: colour(BLUE),
        }];
        // (red's slot and order, blue's slot and order, shown)
        let cases = [
            ((0, 1), (1, 2), BLUE),
            ((0, 2), (1, 1), RED),
            ((1, 1), (0, 2), BLUE),
            ((1, 2), (0, 1), RED),
        ];
        for ((red_slot, red_order), (blue_slot, blue_order), shown) in cases {
            let mut plays = Plays::NONE;
            plays.start(PlayId::new(red_slot, 1).unwrap(), 1, red_order);
            plays.start(PlayId::new(blue_slot, 1).unwrap(), 2, blue_order);
            let mut out = [Light::DARK];
            show(
                &[Held::NONE],
                &plays,
                |slot| {
                    if slot == red_slot {
                        &red[..]
                    } else {
                        &blue[..]
                    }
                },
                &mut out,
            );
            assert_eq!(out, [colour(shown)], "red {red_slot} at {red_order}");
        }
    }

    #[test]
    fn a_released_hold_lets_the_play_through() {
        let mut hold = held(colour(BLUE), 3);
        hold.release(Property::Colour);
        assert_eq!(hold, Held::NONE);
    }
}
