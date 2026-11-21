//! The Rig: a mode of a fixture at an address becomes the config of its channels and,
//! for the v11 light commands until T7, its lights. The only crate that knows an address
//! belongs to a fixture. A calculation: it runs off the tick thread and may use the heap.
//! The tick sees only channels and the words offered on them.
//!
//! ```text
//! mode + address + first light --compile--> lowered channels + a target per light | Unfit
//! patch + command + library --Patch::update--> new patch + effects | Refusal
//! ```
//!
//! A fixture is one or more lights. A mode without heads is one light. A mode with N
//! heads (pixels) is one light per head in the order of the definition, after the fixture
//! itself when some channel is in no head or in more than one (a master dimmer): N + 1,
//! else N.
//!
//! Each coarse channel of a light, by what it does and what else the light has, is a
//! field of the light's [`LightFields`]:
//!
//! | Channel | Field |
//! |---|---|
//! | dimmer | intensity |
//! | red, green, blue | its part of the colour, less the shared part when the light has white, times intensity when it has no dimmer and the value has one (cx-4) |
//! | white | the part red, green and blue share, times intensity as red |
//! | cyan, magenta, yellow | full less red, green, blue, times intensity as red |
//! | pan, tilt, zoom | its part of the position, the zoom; full less it when the channel runs down |
//! | a colour or gobo wheel | the slot of the colour or gobo the light holds (`capabilities.rs`) |
//! | a coarse with a fine (`channels::lower`) | its field is the pair: a 16-bit word |
//! | anything else | no field: it shows its rest |
//!
//! A light value offers a word on each field it has a value for; a field it has none for
//! shows what is underneath, else the channel's rest (`channels::lower`).
//!
//! Every channel of the mode is in at most one light. A channel two heads name is the
//! fixture's own (a shared dimmer, a strobe): the fixture's light has it, so no two
//! lights offer one channel (decision 48, `docs/design/who-wins.md`).

#![no_std]
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

extern crate alloc;

mod capabilities;
pub mod channels;
mod light;
mod patch;

pub use capabilities::{shows_one_gobo, supports};
pub use light::{Choice, Feed, Less, LightFields, Part, Scale, Sense, Take, Travel};
pub use patch::{Limit, Patch, PatchEffect, Placed, Unplaced, Whole};

use capabilities::Wheels;

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use channels::Lowered;
use engine_core::{
    ADDRESSES, CHANNELS, ChannelNumbers, Color, Field, LightNumber, Lights, Pixels, Refusal,
    Universes,
};
use fixture_library::{Byte, Direction, Does, Function, Layout, Mode};

/// A mode at an address, compiled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rigged {
    /// One per light the mode makes, numbered on from the first.
    pub targets: Vec<LightFields>,
    /// The config of each of its channels, its pairs and compounds.
    pub lowered: Lowered,
}

/// Why a mode does not fit where it was put.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unfit {
    /// The address is not one of this engine.
    Refused(Refusal),
    /// The mode's `channels` run past the end of the universe.
    PastTheEnd { channels: usize },
    /// The mode makes `needs` lights from the first: past the engine's last light.
    TooManyLights { needs: usize },
}

/// The lights of `mode` at `at`, numbered from `first`, and its channels lowered. The
/// address and the light numbers are checked against this engine's `universes` and
/// `lights`. A coarse channel that takes from a light and is paired with a fine one
/// (`channels::lower`, the definition is the truth) takes a 16-bit word.
pub fn compile(
    mode: &Mode,
    at: ChannelNumbers,
    first: LightNumber,
    universes: Universes,
    lights: Lights,
) -> Result<Rigged, Unfit> {
    let places = places(mode.channels.len(), at, universes)?;
    let needs = lights_of(mode);
    (0..needs)
        .map(|offset| {
            u16::try_from(offset)
                .ok()
                .and_then(|offset| first.index().checked_add(offset))
                .and_then(|index| LightNumber::new(index, lights))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(Unfit::TooManyLights { needs })?;
    let space = u32::try_from(CHANNELS).unwrap_or(u32::MAX);
    let lowered = places
        .first()
        .map_or(Ok(None), |&start| {
            channels::lower(mode, start, space).map(Some)
        })
        .map_err(|past| Unfit::PastTheEnd {
            channels: past.channels,
        })?
        .unwrap_or(Lowered {
            channels: vec![],
            pairs: vec![],
            compounds: vec![],
        });
    let fine = |coarse: u32| {
        lowered
            .pairs
            .iter()
            .find(|pair| pair.coarse == coarse)
            .map(|pair| pair.fine)
    };
    let mut of_light: Vec<Vec<(usize, &fixture_library::Channel)>> = vec![vec![]; needs];
    for ((index, channel), owner) in mode.channels.iter().enumerate().zip(owners(mode)) {
        if let Some(channels) = of_light.get_mut(owner) {
            channels.push((index, channel));
        }
    }
    let mut targets = Vec::with_capacity(needs);
    for (light, channels) in of_light.iter().enumerate() {
        let number = u16::try_from(light)
            .ok()
            .and_then(|offset| first.index().checked_add(offset))
            .unwrap_or(u16::MAX);
        let has = Has::of(channels.iter().map(|(_, c)| *c));
        let wheels = Wheels::of(channels);
        let mut target = LightFields::default();
        for &(index, channel) in channels {
            let (Byte::Coarse, Some(&place)) = (channel.byte, places.get(index)) else {
                continue;
            };
            let feed = match wheels.bind(index, channel, number) {
                Some(bound) => bound?,
                None => take(channel, has).map(Feed::Takes),
            };
            let Some(feed) = feed else {
                continue;
            };
            let at = match (&feed, fine(place)) {
                (Feed::Takes(_), Some(fine)) => Field::Word {
                    coarse: place,
                    fine,
                },
                (Feed::Takes(_) | Feed::Picks(_), _) => Field::Byte(place),
            };
            target.push(at, feed);
        }
        targets.push(target);
    }
    Ok(Rigged { targets, lowered })
}

/// How many lights `mode` makes: one per head, and the fixture itself when it has a
/// channel of its own.
fn lights_of(mode: &Mode) -> usize {
    mode.pixels.len() + usize::from(has_own(mode))
}

/// True when the fixture itself is a light: it has no heads, or a channel in none.
fn has_own(mode: &Mode) -> bool {
    mode.pixels.is_empty() || heads(mode).iter().any(Option::is_none)
}

/// The one head of each channel. `None` for a channel in no head, and for one two heads
/// name: it is the fixture's own.
fn heads(mode: &Mode) -> Vec<Option<usize>> {
    let mut heads: Vec<Vec<usize>> = vec![Vec::new(); mode.channels.len()];
    for (head, channels) in mode.pixels.iter().enumerate() {
        for &channel in channels {
            if let Some(slot) = heads.get_mut(usize::from(channel))
                && slot.last() != Some(&head)
            {
                slot.push(head);
            }
        }
    }
    heads
        .into_iter()
        .map(|heads| match heads.as_slice() {
            &[head] => Some(head),
            _ => None,
        })
        .collect()
}

/// A generic strip of `pixels` pixels without a definition file: its model's name, and a
/// mode of one head per pixel (red, green, blue, and white for `Rgbw`) in one row.
fn strip(pixels: Pixels, color: Color) -> (&'static str, Mode) {
    let (model, functions): (_, &[Function]) = match color {
        Color::Rgb => (
            "RGB strip",
            &[Function::Red, Function::Green, Function::Blue],
        ),
        Color::Rgbw => (
            "RGBW strip",
            &[
                Function::Red,
                Function::Green,
                Function::Blue,
                Function::White,
            ],
        ),
    };
    let channel = |function: &Function| {
        let mut channel = fixture_library::Channel {
            name: format!("{function:?}"),
            function: function.clone(),
            declared: Default::default(),
            byte: Byte::Coarse,
            default: None,
            direction: None,
            ranges: vec![],
        };
        channel.declared.colour = Some(function.clone());
        channel
    };
    let size = functions.len();
    let mode = Mode {
        name: format!("{} pixels", pixels.count()),
        channels: (0..pixels.count())
            .flat_map(|_| functions.iter().map(channel))
            .collect(),
        pixels: (0..usize::from(pixels.count()))
            .map(|pixel| {
                (pixel * size..(pixel + 1) * size)
                    .filter_map(|c| u16::try_from(c).ok())
                    .collect()
            })
            .collect(),
        layout: Some(Layout {
            width: pixels.count(),
            height: 1,
        }),
        degrees: None,
    };
    (model, mode)
}

/// The strip mode the rig makes that `mode` is, with its declarations: a strip has no
/// definition file to take them from. `None` when `maker` and `model` name no strip, or
/// `mode` (declarations aside) is none of its modes.
pub fn strip_mode(maker: &str, model: &str, mode: &Mode) -> Option<Mode> {
    let pixels = u16::try_from(mode.pixels.len())
        .ok()
        .and_then(|count| Pixels::try_from(count).ok())?;
    let (_, made) = [Color::Rgb, Color::Rgbw]
        .into_iter()
        .map(|color| strip(pixels, color))
        .find(|&(strip, _)| maker == "Generic" && strip == model)?;
    let mut plain = made.clone();
    for channel in &mut plain.channels {
        channel.declared = fixture_library::Declared::default();
    }
    (plain == *mode).then_some(made)
}

/// The channel of each of the `count` channels from `at`, counted from 0 over the one
/// space. Past address 512 a fixture runs on at address 1 of the next universe, as an
/// output streams a run of universes.
fn places(count: usize, at: ChannelNumbers, universes: Universes) -> Result<Vec<u32>, Unfit> {
    at.check(universes).map_err(Unfit::Refused)?;
    let start = flat(at);
    if start + count > usize::from(universes.count()) * ADDRESSES {
        return Err(Unfit::PastTheEnd { channels: count });
    }
    (start..start + count)
        .map(|channel| u32::try_from(channel).map_err(|_| Unfit::PastTheEnd { channels: count }))
        .collect()
}

/// A channel counted from 0 over every universe in a row: `at` must be checked.
pub(crate) fn flat(at: ChannelNumbers) -> usize {
    usize::from(at.universe.saturating_sub(1)) * ADDRESSES
        + usize::from(at.address.saturating_sub(1))
}

/// The channel `flat` counts, as a client counts it.
pub(crate) fn unflat(flat: usize) -> Option<ChannelNumbers> {
    Some(ChannelNumbers {
        universe: u16::try_from(flat / ADDRESSES + 1).ok()?,
        address: u16::try_from(flat % ADDRESSES + 1).ok()?,
    })
}

/// The light that writes each channel, counted from the fixture's first: the fixture
/// itself is 0 when it is a light, then each head in order.
fn owners(mode: &Mode) -> Vec<usize> {
    let first_head = usize::from(has_own(mode));
    heads(mode)
        .into_iter()
        .map(|head| head.map_or(0, |head| first_head + head))
        .collect()
}

/// What else a light's channels do, which changes how each is written.
#[derive(Clone, Copy)]
struct Has {
    /// A dimmer: the colour is not scaled by intensity.
    dimmer: bool,
    /// White: red, green and blue leave out the part they share.
    white: bool,
}

impl Has {
    fn of<'a>(channels: impl Iterator<Item = &'a fixture_library::Channel>) -> Self {
        let mut has = Self {
            dimmer: false,
            white: false,
        };
        for channel in channels.filter(|c| c.byte == Byte::Coarse) {
            has.dimmer |= channel.function == Function::Dimmer;
            has.white |= channel.function == Function::White;
        }
        has
    }
}

/// What a channel takes from its light. `None` for what this slice does not write. Pan,
/// tilt and zoom travel over the first range that says which way it runs, that way, else
/// over every byte the way the channel runs.
fn take(channel: &fixture_library::Channel, has: Has) -> Option<Take> {
    let runs = channel.ranges.iter().find_map(|range| match range.does {
        Some(Does::Runs(direction)) => Some((range, direction)),
        _ => None,
    });
    let (travel, direction) =
        runs.map_or((Travel::FULL, channel.direction), |(range, direction)| {
            (
                Travel {
                    low: range.min,
                    high: range.max,
                },
                Some(direction),
            )
        });
    let scale = if has.dimmer {
        Scale::Whole
    } else {
        Scale::ByIntensity
    };
    let primary = |of| Take {
        of,
        less: if has.white {
            Less::Shared
        } else {
            Less::Nothing
        },
        scale,
        sense: Sense::Rising,
        travel: Travel::FULL,
    };
    let filter = |of| Take {
        of,
        less: Less::Nothing,
        scale,
        sense: Sense::Falling,
        travel: Travel::FULL,
    };
    let travels = |of| Take {
        of,
        less: Less::Nothing,
        scale: Scale::Whole,
        sense: match direction {
            Some(Direction::Down) => Sense::Falling,
            Some(Direction::Up) | None => Sense::Rising,
        },
        travel,
    };
    let take = match &channel.function {
        Function::Dimmer => Take::INTENSITY,
        Function::Pan => travels(Part::Pan),
        Function::Tilt => travels(Part::Tilt),
        Function::Zoom => travels(Part::Zoom),
        Function::Red => primary(Part::Red),
        Function::Green => primary(Part::Green),
        Function::Blue => primary(Part::Blue),
        Function::White => Take {
            of: Part::Shared,
            less: Less::Nothing,
            scale,
            sense: Sense::Rising,
            travel: Travel::FULL,
        },
        Function::Cyan => filter(Part::Red),
        Function::Magenta => filter(Part::Green),
        Function::Yellow => filter(Part::Blue),
        // the next slices: beam, and colours past red, green, blue and white. Wheels are
        // bound in `capabilities.rs`
        Function::Amber
        | Function::Uv
        | Function::Indigo
        | Function::Lime
        | Function::Hue
        | Function::Saturation
        | Function::Lightness
        | Function::Value
        | Function::ColorTemperature
        | Function::Focus
        | Function::Iris
        | Function::Frost
        | Function::Gobo
        | Function::ColorWheel
        | Function::Strobe
        | Function::Shutter
        | Function::Prism
        | Function::Speed
        | Function::Effect
        | Function::Control
        | Function::NoFunction
        | Function::Other(_) => return None,
    };
    Some(take)
}
