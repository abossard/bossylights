//! Lights to bytes. The Rig compiles a mode at an address into writers, off the tick
//! thread. [`resolve`] runs them every frame. A writer says which light, what to take,
//! how, and where it goes. Nothing here knows a fixture, a mode, a maker or a colour
//! model: only the values of a light and arithmetic on them.
//!
//! ```text
//! lights (values per light) + writers --resolve--> bytes in the universes
//! ```
//!
//! | A light | 16 bits, 0 to 65 535 is 0 to 1, or not held |
//! |---|---|
//! | intensity | how bright |
//! | red, green, blue | its colour, one part |
//!
//! A part nothing holds shows the definition's default: its writers write the default of
//! their channel, not a value of the light.
//!
//! A value of 16 bits goes out as one byte (its high byte) or as a coarse and a fine
//! byte. So an 8-bit channel always shows the coarse byte of the same value.

use crate::channel::{Channel, ChannelSet, Levels, MAX_UNIVERSES};
use crate::mix::{Want, mix};
use crate::property::{Light, Rgb};
use serde::Serialize;

/// The most lights an engine can have: 64 universes of 170 RGB pixels and more.
pub const MAX_LIGHTS: u16 = 16_384;

/// How many lights this engine has, 1 to [`MAX_LIGHTS`]. A setting, fixed while the
/// engine runs: the buffer of light values has this length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lights(u16);

impl Lights {
    /// [`MAX_LIGHTS`].
    pub const MOST: Self = Self(MAX_LIGHTS);

    /// `None` outside 1 to [`MAX_LIGHTS`].
    pub const fn new(count: u16) -> Option<Self> {
        if count >= 1 && count <= MAX_LIGHTS {
            Some(Self(count))
        } else {
            None
        }
    }

    pub const fn count(self) -> u16 {
        self.0
    }
}

/// One light of this engine, counted from 0, below its [`Lights`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightNumber(u16);

impl LightNumber {
    /// `None` at or past `lights`.
    pub const fn new(index: u16, lights: Lights) -> Option<Self> {
        if index < lights.0 {
            Some(Self(index))
        } else {
            None
        }
    }

    pub const fn index(self) -> u16 {
        self.0
    }
}

/// Lights one after the other, from `first`: a fixture's, or one. Checked against
/// [`Lights`] when made, so it never runs past the engine's last light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightRun {
    first: u16,
    count: u16,
}

impl LightRun {
    /// `None` when it is empty or runs past `lights`.
    pub fn new(first: LightNumber, count: u16, lights: Lights) -> Option<Self> {
        let end = first.0.checked_add(count)?;
        (count > 0 && end <= lights.0).then_some(Self {
            first: first.0,
            count,
        })
    }

    pub const fn first(self) -> LightNumber {
        LightNumber(self.first)
    }

    pub const fn count(self) -> u16 {
        self.count
    }

    /// These lights of `lights`, the values by number. Fewer when `lights` is shorter.
    pub fn of<T>(self, lights: &mut [T]) -> &mut [T] {
        let first = usize::from(self.first).min(lights.len());
        let end = (first + usize::from(self.count)).min(lights.len());
        lights.get_mut(first..end).unwrap_or_default()
    }
}

/// A light's held parts, as the picture shows them: its number, counted from 1, and each
/// part's value, `None` where nothing holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HeldLight {
    pub light: u16,
    pub intensity: Option<u16>,
    pub colour: Option<Rgb>,
}

/// Every light of `lights` that holds a part, by number.
pub fn held_lights(lights: &[Light]) -> impl Iterator<Item = HeldLight> + '_ {
    numbered(lights)
        .filter(|(_, light)| **light != Light::DARK)
        .map(held)
}

/// Every light whose held parts differ between `before` and `after`, as `after` holds them.
pub fn light_changes<'a>(
    before: &'a [Light],
    after: &'a [Light],
) -> impl Iterator<Item = HeldLight> + 'a {
    numbered(after)
        .zip(before)
        .filter(|((_, new), old)| new != old)
        .map(|(new, _)| held(new))
}

fn numbered(lights: &[Light]) -> impl Iterator<Item = (u16, &Light)> {
    (1..=MAX_LIGHTS).zip(lights)
}

const fn held((light, values): (u16, &Light)) -> HeldLight {
    HeldLight {
        light,
        intensity: values.intensity,
        colour: values.colour,
    }
}

/// One channel, or a coarse and fine pair, and what goes there each frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    /// The same byte every frame: a shutter held open, the default of a channel no
    /// light writes.
    Fixed { at: Channel, value: u8 },
    /// A value taken from a light, or `unheld` when nothing holds the part it takes: the
    /// default of its channel from the definition, the coarse byte high.
    Takes {
        light: LightNumber,
        take: Take,
        at: Place,
        unheld: u16,
    },
}

/// Where a value of 16 bits goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Its high byte only.
    Byte(Channel),
    /// The high byte to `coarse`, the low byte to `fine`.
    Pair { coarse: Channel, fine: Channel },
}

/// What a writer takes from its light, and how: the part `of` the light, less `less`,
/// then scaled, then turned.
///
/// | Channel | Take |
/// |---|---|
/// | a dimmer | intensity |
/// | red beside a dimmer | red |
/// | red without a dimmer | red, times intensity |
/// | red beside white | red, less shared |
/// | white | shared: the part red, green and blue all have |
/// | cyan | red, falling: full less red |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Take {
    pub of: Part,
    pub less: Less,
    pub scale: Scale,
    pub sense: Sense,
}

impl Take {
    /// Intensity as it is: a dimmer.
    pub const INTENSITY: Self = Self {
        of: Part::Intensity,
        less: Less::Nothing,
        scale: Scale::Whole,
        sense: Sense::Rising,
    };

    /// The value of this take for `light`, 16 bits. `None` when nothing holds the part it
    /// takes. Intensity not held scales as 0.
    pub fn value(self, light: &Light) -> Option<u16> {
        let part = match self.of {
            Part::Intensity => light.intensity,
            Part::Red => light.colour.map(|c| c.red),
            Part::Green => light.colour.map(|c| c.green),
            Part::Blue => light.colour.map(|c| c.blue),
            Part::Shared => light.colour.map(Rgb::shared),
        }?;
        let shared = light.colour.map_or(0, Rgb::shared);
        let less = match self.less {
            Less::Nothing => part,
            Less::Shared => part.saturating_sub(shared),
        };
        let scaled = match self.scale {
            Scale::Whole => less,
            Scale::ByIntensity => times(less, light.intensity.unwrap_or(0)),
        };
        Some(match self.sense {
            Sense::Rising => scaled,
            Sense::Falling => u16::MAX - scaled,
        })
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

/// Every writer's byte for this frame, written into `out`, a buffer the caller owns.
/// `lights` are the values of the lights by number. A channel no writer names keeps what
/// `out` holds. A writer whose light is past `lights` writes nothing. Where writers share
/// a channel, `mix` decides. Allocates nothing, cannot fail.
pub fn resolve(lights: &[Light], writers: &[Writer], out: &mut [Levels]) {
    write(
        lights,
        writers,
        out,
        &mut [ChannelSet::NONE; MAX_UNIVERSES as usize],
    );
}

/// [`resolve`], and the channels a light wrote go into `lit`.
pub(crate) fn write(
    lights: &[Light],
    writers: &[Writer],
    out: &mut [Levels],
    lit: &mut [ChannelSet],
) {
    for writer in writers {
        match *writer {
            Writer::Fixed { at, value } => put(at, Want::Default(value), out, lit),
            Writer::Takes {
                light,
                take,
                at,
                unheld,
            } => {
                let Some(light) = lights.get(usize::from(light.0)) else {
                    continue;
                };
                let (want, value): (fn(u8) -> Want, u16) = match take.value(light) {
                    Some(value) => (Want::Light, value),
                    None => (Want::Default, unheld),
                };
                let [high, low] = value.to_be_bytes();
                match at {
                    Place::Byte(at) => put(at, want(high), out, lit),
                    Place::Pair { coarse, fine } => {
                        put(coarse, want(high), out, lit);
                        put(fine, want(low), out, lit);
                    }
                }
            }
        }
    }
}

/// `want` against what `at` has: `mix` decides.
fn put(at: Channel, want: Want, out: &mut [Levels], lit: &mut [ChannelSet]) {
    let was_lit = at.marked(lit);
    if let Some(level) = at.level(out) {
        let had = if was_lit {
            Want::Light(*level)
        } else {
            Want::Default(*level)
        };
        let won = mix(had, want);
        *level = won.level();
        at.mark(lit, matches!(won, Want::Light(_)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChannelNumbers, Universes};

    const FULL: u16 = u16::MAX;
    const HALF: u16 = 0x8000;

    fn light(intensity: u16, (red, green, blue): (u16, u16, u16)) -> Light {
        Light {
            intensity: Some(intensity),
            colour: Some(Rgb { red, green, blue }),
            ..Light::DARK
        }
    }

    fn take(of: Part, less: Less, scale: Scale, sense: Sense) -> Take {
        Take {
            of,
            less,
            scale,
            sense,
        }
    }

    #[test]
    fn a_take_is_a_part_less_scaled_turned() {
        use Less::{Nothing, Shared as LessShared};
        use Part::{Blue, Green, Intensity, Red, Shared};
        use Scale::{ByIntensity, Whole};
        use Sense::{Falling, Rising};
        let orange = (FULL, HALF, 0x1000);
        let grey = (HALF, HALF, HALF);
        // (take, light, value)
        let cases = [
            (
                take(Intensity, Nothing, Whole, Rising),
                light(HALF, orange),
                Some(HALF),
            ),
            (
                take(Red, Nothing, Whole, Rising),
                light(0, orange),
                Some(FULL),
            ),
            (
                take(Green, Nothing, Whole, Rising),
                light(0, orange),
                Some(HALF),
            ),
            (
                take(Blue, Nothing, Whole, Rising),
                light(0, orange),
                Some(0x1000),
            ),
            (
                take(Shared, Nothing, Whole, Rising),
                light(0, orange),
                Some(0x1000),
            ),
            (
                take(Red, LessShared, Whole, Rising),
                light(0, orange),
                Some(FULL - 0x1000),
            ),
            (
                take(Shared, LessShared, Whole, Rising),
                light(0, orange),
                Some(0),
            ),
            (
                take(Red, Nothing, ByIntensity, Rising),
                light(HALF, orange),
                Some(HALF),
            ),
            (
                take(Red, Nothing, ByIntensity, Rising),
                light(0, orange),
                Some(0),
            ),
            (
                take(Red, Nothing, ByIntensity, Rising),
                light(FULL, orange),
                Some(FULL),
            ),
            // 0.5 × 0.5 is 0.25, rounded to the nearest
            (
                take(Green, Nothing, ByIntensity, Rising),
                light(HALF, grey),
                Some(0x4000),
            ),
            (
                take(Red, Nothing, Whole, Falling),
                light(0, orange),
                Some(0),
            ),
            (
                take(Blue, Nothing, Whole, Falling),
                light(0, orange),
                Some(FULL - 0x1000),
            ),
            (
                take(Red, Nothing, ByIntensity, Falling),
                light(0, orange),
                Some(FULL),
            ),
            (Take::INTENSITY, light(0x1234, grey), Some(0x1234)),
            // a part nothing holds: no value, its writer writes the default
            (
                Take::INTENSITY,
                Light {
                    intensity: None,
                    ..light(FULL, grey)
                },
                None,
            ),
            (
                take(Red, Nothing, Whole, Rising),
                Light {
                    colour: None,
                    ..light(FULL, grey)
                },
                None,
            ),
            (take(Shared, Nothing, Whole, Rising), Light::DARK, None),
            // intensity not held scales as 0
            (
                take(Red, Nothing, ByIntensity, Rising),
                Light {
                    intensity: None,
                    ..light(FULL, orange)
                },
                Some(0),
            ),
        ];
        for (take, light, value) in cases {
            assert_eq!(take.value(&light), value, "{take:?} of {light:?}");
        }
    }

    fn channel(address: u16) -> Channel {
        ChannelNumbers {
            universe: 1,
            address,
        }
        .check(Universes::ONE)
        .unwrap()
    }

    fn number(index: u16) -> LightNumber {
        LightNumber::new(index, Lights::new(4).unwrap()).unwrap()
    }

    #[test]
    fn resolve_writes_each_writer_and_leaves_the_rest() {
        let lights = [
            light(0x1234, (FULL, 0, 0)),
            light(HALF, (0, HALF, 0)),
            Light::DARK,
        ];
        // the default of the channels from the definition, coarse 0xc8, fine 0x05
        let takes = |index, take, at| Writer::Takes {
            light: number(index),
            take,
            at,
            unheld: 0xc805,
        };
        let green = take(Part::Green, Less::Nothing, Scale::Whole, Sense::Rising);
        // (writers, the bytes at addresses 1 to 6, the rest of `out` holding 7)
        let cases = [
            (vec![], [7, 7, 7, 7, 7, 7]),
            (
                vec![Writer::Fixed {
                    at: channel(2),
                    value: 8,
                }],
                [7, 8, 7, 7, 7, 7],
            ),
            (
                vec![takes(0, Take::INTENSITY, Place::Byte(channel(1)))],
                [0x12, 7, 7, 7, 7, 7],
            ),
            (
                vec![takes(
                    0,
                    Take::INTENSITY,
                    Place::Pair {
                        coarse: channel(3),
                        fine: channel(6),
                    },
                )],
                [7, 7, 0x12, 7, 7, 0x34],
            ),
            (
                vec![
                    takes(0, green, Place::Byte(channel(4))),
                    takes(1, green, Place::Byte(channel(5))),
                ],
                [7, 7, 7, 0, 0x80, 7],
            ),
            // light 2 holds nothing: the default
            (
                vec![
                    takes(2, green, Place::Byte(channel(1))),
                    takes(
                        2,
                        Take::INTENSITY,
                        Place::Pair {
                            coarse: channel(3),
                            fine: channel(6),
                        },
                    ),
                ],
                [0xc8, 7, 0xc8, 7, 7, 0x05],
            ),
            // light 3 is a number of the engine, but past the values given: nothing
            (
                vec![takes(3, Take::INTENSITY, Place::Byte(channel(1)))],
                [7, 7, 7, 7, 7, 7],
            ),
        ];
        for (writers, expected) in cases {
            let mut out = [Levels([7; crate::ADDRESSES])];
            resolve(&lights, &writers, &mut out);
            assert_eq!(out[0].0[..6], expected, "{writers:?}");
            assert!(out[0].0[6..].iter().all(|&b| b == 7), "{writers:?}");
        }
    }

    #[test]
    fn a_channel_past_the_buffer_is_not_written() {
        let two = Universes::new(2).unwrap();
        let second = ChannelNumbers {
            universe: 2,
            address: 1,
        }
        .check(two)
        .unwrap();
        let mut out = [Levels::ZERO];
        resolve(
            &[],
            &[Writer::Fixed {
                at: second,
                value: 9,
            }],
            &mut out,
        );
        assert_eq!(out, [Levels::ZERO]);
    }

    #[test]
    fn lights_are_1_to_16_384_and_a_number_is_below_the_count() {
        // (count, valid)
        for (count, valid) in [
            (0, false),
            (1, true),
            (MAX_LIGHTS, true),
            (MAX_LIGHTS + 1, false),
        ] {
            assert_eq!(Lights::new(count).is_some(), valid, "{count}");
        }
        let four = Lights::new(4).unwrap();
        // (index, valid)
        for (index, valid) in [(0, true), (3, true), (4, false), (u16::MAX, false)] {
            assert_eq!(LightNumber::new(index, four).is_some(), valid, "{index}");
        }
    }

    #[test]
    fn the_picture_shows_the_held_parts_of_each_light_and_what_changed() {
        let red = Rgb {
            red: FULL,
            green: 0,
            blue: 0,
        };
        let coloured = Light {
            colour: Some(red),
            ..Light::DARK
        };
        let bright = Light {
            intensity: Some(HALF),
            ..Light::DARK
        };
        let both = light(HALF, (FULL, 0, 0));
        let shown = |light, values: Light| HeldLight {
            light,
            intensity: values.intensity,
            colour: values.colour,
        };
        let dark = Light::DARK;
        // (before, after, held after, changes)
        let cases = [
            (vec![dark, dark], vec![dark, dark], vec![], vec![]),
            (
                vec![dark, dark],
                vec![dark, coloured],
                vec![shown(2, coloured)],
                vec![shown(2, coloured)],
            ),
            (
                vec![coloured, dark],
                vec![both, bright],
                vec![shown(1, both), shown(2, bright)],
                vec![shown(1, both), shown(2, bright)],
            ),
            (
                vec![both, coloured],
                vec![bright, coloured],
                vec![shown(1, bright), shown(2, coloured)],
                vec![shown(1, bright)],
            ),
            // let go: the light shows nothing held, and changed to nothing
            (
                vec![bright, coloured],
                vec![dark, coloured],
                vec![shown(2, coloured)],
                vec![shown(1, dark)],
            ),
            // held black is held, not let go
            (
                vec![dark],
                vec![light(0, (0, 0, 0))],
                vec![shown(1, light(0, (0, 0, 0)))],
                vec![shown(1, light(0, (0, 0, 0)))],
            ),
        ];
        for (before, after, held, changes) in cases {
            assert_eq!(held_lights(&after).collect::<Vec<_>>(), held, "{after:?}");
            assert_eq!(
                light_changes(&before, &after).collect::<Vec<_>>(),
                changes,
                "{before:?} to {after:?}"
            );
        }
    }
}
