//! Wheels: the colours and gobos a light can choose, each bound to the byte of its slot,
//! and whether a light can show a value it is given.
//!
//! | Channel of a light | Binding |
//! |---|---|
//! | a colour wheel, and the light mixes colour (hybrid) | fixed at its bypass: the colour is mixed. No bypass: refused |
//! | a colour wheel, nothing mixes | picks each colour of the light's wheels: its own slot, else its neutral slot. Neither: refused |
//! | a gobo wheel | picks each gobo of the wheel and open, slot 1 first |
//!
//! | Word | Is |
//! |---|---|
//! | colour wheel | a colour channel with at least one static colour (`ColorMacro`) |
//! | gobo wheel | a gobo channel with at least one gobo that does not shake |
//! | neutral slot | white, or a range without a preset named open, white, no function, no colour or off. It shows white |
//! | bypass | a range without a preset that is neutral or names the mix (RGB, CMY, mix), else white: a static white can be a fixed white that overrides the mix |
//! | slot byte | the lower midpoint of its range: 20 to 29 is 24 |
//!
//! A light refuses a colour or a gobo when it has a wheel for it but no slot that shows it
//! (black on a wheel without black). A light without a wheel or mixing for a property
//! lets the value through: it has nothing to show it with.

use crate::Unfit;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use engine_core::{
    Channel, Gobo, Light, LightNumber, LightRun, Part, Property, Refusal, Rgb, Writer,
};
use fixture_library::{Byte, Does, Function, Range};

/// The wheels of one light.
pub(crate) struct Wheels {
    /// The light mixes colour: red, green, blue, white, cyan, magenta or yellow.
    mixes: bool,
    /// Every colour of the light's colour wheels, each once, in channel and range order.
    colours: Vec<Rgb>,
    /// The mode's channel index of each gobo wheel, wheel 1 first.
    gobos: Vec<usize>,
}

impl Wheels {
    /// The wheels of a light with these channels, each with its index in the mode.
    pub(crate) fn of(channels: &[(usize, &fixture_library::Channel)]) -> Self {
        let coarse = || channels.iter().filter(|(_, c)| c.byte == Byte::Coarse);
        let mut colours: Vec<Rgb> = Vec::new();
        for (_, channel) in coarse().filter(|(_, c)| is_colour_wheel(c)) {
            for (colour, _) in colour_slots(channel) {
                if !colours.contains(&colour) {
                    colours.push(colour);
                }
            }
        }
        Self {
            mixes: coarse().any(|(_, c)| mixes(&c.function)),
            colours,
            gobos: coarse()
                .filter(|(_, c)| c.function == Function::Gobo && !gobo_slots(c).is_empty())
                .map(|&(index, _)| index)
                .collect(),
        }
    }

    /// The writers of the coarse channel `index` of the mode, at `at`, when it is a wheel
    /// of `light`. `None` when it is not a wheel.
    pub(crate) fn bind(
        &self,
        index: usize,
        channel: &fixture_library::Channel,
        light: LightNumber,
        at: Channel,
    ) -> Option<Result<Vec<Writer>, Unfit>> {
        let unheld = channel.default.unwrap_or(0);
        if is_colour_wheel(channel) {
            let refused = Unfit::Refused(unsupported(
                light.index().saturating_add(1),
                Property::Colour,
            ));
            if self.mixes {
                return Some(
                    bypass(channel)
                        .map(|value| vec![Writer::Fixed { at, value }])
                        .ok_or(refused),
                );
            }
            let own = colour_slots(channel);
            let neutral = neutral(channel);
            let slots = self
                .colours
                .iter()
                .map(|&colour| {
                    own.iter()
                        .find(|(slot, _)| *slot == colour)
                        .map(|&(_, value)| value)
                        .or(neutral)
                        .map(|value| Writer::ColourSlot { colour, value })
                })
                .collect::<Option<Vec<_>>>();
            return Some(
                slots
                    .ok_or(refused)
                    .and_then(|slots| picks(light, at, unheld, slots)),
            );
        }
        let wheel = self.gobos.iter().position(|&gobo| gobo == index)?;
        let wheel = u8::try_from(wheel + 1).ok()?;
        let slots = gobo_slots(channel)
            .into_iter()
            .zip(1..=u8::MAX)
            .map(|(value, slot)| Writer::GoboSlot {
                gobo: Gobo { wheel, slot },
                value,
            })
            .collect();
        Some(picks(light, at, unheld, slots).map_err(|_| {
            Unfit::Refused(unsupported(light.index().saturating_add(1), Property::Gobo))
        }))
    }
}

/// A `Picks` writer, then its slots.
fn picks(
    light: LightNumber,
    at: Channel,
    unheld: u8,
    slots: Vec<Writer>,
) -> Result<Vec<Writer>, Unfit> {
    let count = u8::try_from(slots.len()).map_err(|_| {
        Unfit::Refused(unsupported(
            light.index().saturating_add(1),
            Property::Colour,
        ))
    })?;
    let mut writers = vec![Writer::Picks {
        light,
        at,
        slots: count,
        unheld,
    }];
    writers.extend(slots);
    Ok(writers)
}

/// Whether every light of `runs` can show `values`: `Unsupported` names the first light
/// and property that cannot. `writers` are the writers of the whole patch.
pub fn supports(writers: &[Writer], runs: &[LightRun], values: &Light) -> Result<(), Refusal> {
    /// What a light's writers say of one property: bound by a wheel, shown.
    #[derive(Clone, Copy, Default)]
    struct Seen {
        wheel: bool,
        shown: bool,
    }
    let within = |light: LightNumber| {
        runs.iter().any(|run| {
            let first = run.first().index();
            (first..first.saturating_add(run.count())).contains(&light.index())
        })
    };
    // by light and property index: the property and what was seen of it
    let mut seen: BTreeMap<(u16, usize), (Property, Seen)> = BTreeMap::new();
    let mut note = |light: LightNumber, property: Property, wheel: bool, shown: bool| {
        let (_, entry) = seen
            .entry((light.index(), property.index()))
            .or_insert((property, Seen::default()));
        entry.wheel |= wheel;
        entry.shown |= shown;
    };
    // the light of the `Picks` whose slots follow, when it is one of `runs`
    let mut wheel: Option<LightNumber> = None;
    for writer in writers {
        let slot_of = match *writer {
            Writer::Takes { light, take, .. } => {
                wheel = None;
                let mixed = matches!(take.of, Part::Red | Part::Green | Part::Blue | Part::Shared);
                if mixed && within(light) {
                    note(light, Property::Colour, false, true);
                }
                None
            }
            Writer::Picks { light, .. } => {
                wheel = Some(light).filter(|&light| within(light));
                None
            }
            Writer::Fixed { .. } => {
                wheel = None;
                None
            }
            Writer::ColourSlot { .. } => Some(Property::Colour),
            Writer::GoboSlot { .. } => Some(Property::Gobo),
        };
        if let (Some(property), Some(light)) = (slot_of, wheel) {
            note(light, property, true, writer.slot(values).is_some());
        }
    }
    match seen
        .into_iter()
        .find(|&(_, (property, seen))| values.has(property) && seen.wheel && !seen.shown)
    {
        Some(((light, _), (property, _))) => Err(unsupported(light.saturating_add(1), property)),
        None => Ok(()),
    }
}

/// Whether a channel that does this mixes colour.
fn mixes(function: &Function) -> bool {
    matches!(
        function,
        Function::Red
            | Function::Green
            | Function::Blue
            | Function::White
            | Function::Cyan
            | Function::Magenta
            | Function::Yellow
    )
}

fn is_colour_wheel(channel: &fixture_library::Channel) -> bool {
    channel.byte == Byte::Coarse
        && channel.function == Function::ColorWheel
        && channel.ranges.iter().any(|r| static_colour(r).is_some())
}

/// Each colour of a colour wheel and its byte, in range order, each colour once: its
/// neutral slot is white.
fn colour_slots(channel: &fixture_library::Channel) -> Vec<(Rgb, u8)> {
    let mut slots: Vec<(Rgb, u8)> = Vec::new();
    for range in &channel.ranges {
        let colour = static_colour(range).or_else(|| is_neutral(range).then_some(WHITE));
        if let Some(colour) = colour
            && !slots.iter().any(|(slot, _)| *slot == colour)
        {
            slots.push((colour, midpoint(range)));
        }
    }
    slots
}

/// The byte of each slot of a gobo wheel, slot 1 first: each gobo that does not shake,
/// and open without a preset. None without a gobo.
fn gobo_slots(channel: &fixture_library::Channel) -> Vec<u8> {
    let gobo = |r: &&Range| matches!(r.does, Some(Does::Gobo { shakes: false, .. }));
    let open = |r: &&Range| r.does.is_none() && r.label.trim().eq_ignore_ascii_case("open");
    if !channel.ranges.iter().any(|r| gobo(&r)) {
        return Vec::new();
    }
    channel
        .ranges
        .iter()
        .filter(|r| gobo(r) || open(r))
        .map(midpoint)
        .collect()
}

/// The byte of the neutral slot of a colour wheel: white, or open.
fn neutral(channel: &fixture_library::Channel) -> Option<u8> {
    colour_slots(channel)
        .into_iter()
        .find(|(colour, _)| *colour == WHITE)
        .map(|(_, value)| value)
}

/// The byte where a hybrid's colour wheel lets the mix through: a range without a preset
/// that is neutral or names the mix, else its neutral slot.
fn bypass(channel: &fixture_library::Channel) -> Option<u8> {
    const MIXES: [&str; 3] = ["rgb", "cmy", "mix"];
    channel
        .ranges
        .iter()
        .find(|range| {
            let label = range.label.to_ascii_lowercase();
            range.does.is_none()
                && (is_neutral(range) || MIXES.iter().any(|mix| label.contains(mix)))
        })
        .map(midpoint)
        .or_else(|| neutral(channel))
}

const WHITE: Rgb = Rgb {
    red: u16::MAX,
    green: u16::MAX,
    blue: u16::MAX,
};

/// The colour of a range that shows one colour, at full 16 bits.
fn static_colour(range: &Range) -> Option<Rgb> {
    let Some(Does::Color(rgb)) = range.does else {
        return None;
    };
    Some(Rgb {
        red: u16::from(rgb.red) * 257,
        green: u16::from(rgb.green) * 257,
        blue: u16::from(rgb.blue) * 257,
    })
}

/// A range without a preset whose name says the wheel lets white through.
fn is_neutral(range: &Range) -> bool {
    const NAMES: [&str; 6] = [
        "open",
        "white",
        "no function",
        "no colour",
        "no color",
        "off",
    ];
    range.does.is_none()
        && NAMES
            .iter()
            .any(|name| range.label.trim().eq_ignore_ascii_case(name))
}

/// The refusal of a value `light` (counted from 1) cannot show.
const fn unsupported(light: u16, property: Property) -> Refusal {
    Refusal::Unsupported { light, property }
}

/// The lower midpoint of a range: 20 to 29 is 24.
fn midpoint(range: &Range) -> u8 {
    let mid = (u16::from(range.min) + u16::from(range.max)) / 2;
    u8::try_from(mid).unwrap_or(range.min)
}
