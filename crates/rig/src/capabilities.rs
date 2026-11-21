//! Wheels: the colours and gobos a light can choose, each bound to the byte of its slot,
//! and whether a light can show a value it is given.
//!
//! | Channel of a light | Binding |
//! |---|---|
//! | a colour wheel, and the light mixes colour (hybrid) | fixed at its bypass: the colour is mixed. No bypass: refused, a static white over an added mix (RGB, white) among them |
//! | a colour wheel, nothing mixes | picks each colour of the light's wheels: its own slot, else its neutral slot. Neither: refused |
//! | a gobo wheel, one of the first three | picks each gobo of the wheel and open, slot 1 first. Each wheel is its own property |
//! | a fourth gobo wheel | its default: no mode of the QLC+ library has one |
//!
//! | Word | Is |
//! |---|---|
//! | colour wheel | a colour channel with at least one static colour (`ColorMacro`) |
//! | gobo wheel | a gobo channel with at least one gobo that does not shake: a gobo preset, or a range without a preset named "Gobo" and a number |
//! | open gobo | a range without a preset named open, no gobo, open gobo, gobo open or "open - gobo selection" |
//! | neutral slot | white, or a range without a preset named open, white, no function, no colour or off. It shows white |
//! | bypass | a range without a preset that is neutral, or starts the channel and names the mix (RGB, CMY, mix) and no effect (random, cycle, fast, slow, speed, sound, music), or names the channels the light mixes with ("Channel 11-14"). Or the range at 0, whatever its preset, that turns the macro off (off, none, nothing, no function, not functional, inactive), never one that names black, blackout or the LEDs, two things ("OFF/Color") or a slot ("Color 1 (off)"). Else, when the light mixes only cyan, magenta and yellow, its white slot: open. Over red, green, blue or white a static white is a fixed white that overrides the mix |
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
    Channel, Gobo, Light, LightNumber, LightRun, Part, Property, Refusal, Rgb, Wheel, Writer,
};
use fixture_library::{Byte, Does, Function, Range};

/// The wheels of one light.
pub(crate) struct Wheels {
    /// The mode's channel index of each channel the light mixes colour with: red, green,
    /// blue, white, cyan, magenta or yellow. None when it does not mix.
    mixers: Vec<usize>,
    /// The light adds light to mix: red, green, blue or white.
    adds: bool,
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
            mixers: coarse()
                .filter(|(_, c)| mixes(&c.function))
                .map(|&(index, _)| index)
                .collect(),
            adds: coarse().any(|(_, c)| adds(&c.function)),
            colours,
            gobos: coarse()
                .filter(|(_, c)| c.function == Function::Gobo && !gobo_slots(c).is_empty())
                .map(|&(index, _)| index)
                .collect(),
        }
    }

    /// The byte a colour wheel of a light that mixes colour rests at: its bypass, so the
    /// mix shows. `None` for any other channel, and for a wheel without a bypass.
    pub(crate) fn rest(&self, channel: &fixture_library::Channel) -> Option<u8> {
        (is_colour_wheel(channel) && !self.mixers.is_empty())
            .then(|| bypass(channel, self.adds, &self.mixers))
            .flatten()
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
            if !self.mixers.is_empty() {
                return Some(
                    bypass(channel, self.adds, &self.mixers)
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
        let wheel = u8::try_from(wheel + 1).ok().and_then(Wheel::new)?;
        let slots = gobo_slots(channel)
            .into_iter()
            .zip(1..=u8::MAX)
            .map(|(value, slot)| Writer::GoboSlot {
                gobo: Gobo { wheel, slot },
                value,
            })
            .collect();
        Some(picks(light, at, unheld, slots).map_err(|_| {
            Unfit::Refused(unsupported(
                light.index().saturating_add(1),
                Property::Gobo(wheel),
            ))
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
    // by light and property index: the property and what was seen of it
    let mut seen: BTreeMap<(u16, usize), (Property, Seen)> = BTreeMap::new();
    let mut note = |light: LightNumber, property: Property, wheel: bool, shown: bool| {
        let (_, entry) = seen
            .entry((light.index(), property.index()))
            .or_insert((property, Seen::default()));
        entry.wheel |= wheel;
        entry.shown |= shown;
    };
    for writer in writers {
        if let Writer::Takes { light, take, .. } = *writer
            && matches!(take.of, Part::Red | Part::Green | Part::Blue | Part::Shared)
            && within(runs, light)
        {
            note(light, Property::Colour, false, true);
        }
    }
    for (light, writer) in slots(writers, runs) {
        let property = match *writer {
            Writer::GoboSlot { gobo, .. } => Property::Gobo(gobo.wheel),
            // `slots` gives slots only
            Writer::ColourSlot { .. }
            | Writer::Takes { .. }
            | Writer::Picks { .. }
            | Writer::Fixed { .. } => Property::Colour,
        };
        note(light, property, true, writer.slot(values).is_some());
    }
    match seen
        .into_iter()
        .find(|&(_, (property, seen))| values.has(property) && seen.wheel && !seen.shown)
    {
        Some(((light, _), (property, _))) => Err(unsupported(light.saturating_add(1), property)),
        None => Ok(()),
    }
}

/// The check of a look of a show file before version 4, which held one gobo, a wheel and a
/// slot, as that engine checked it: refused on the first light of `runs` with a gobo wheel
/// and no slot `slot` on wheel `wheel`. No wheel past [`Wheel::COUNT`] is shown now: on a
/// light with a gobo wheel it is refused, named as [`Wheel::LAST`].
pub fn shows_one_gobo(
    writers: &[Writer],
    runs: &[LightRun],
    wheel: u8,
    slot: u8,
) -> Result<(), Refusal> {
    // by light: whether a slot of a gobo wheel shows it
    let mut shown: BTreeMap<u16, bool> = BTreeMap::new();
    for (light, writer) in slots(writers, runs) {
        if let Writer::GoboSlot { gobo, .. } = *writer {
            let held = u8::from(gobo.wheel) == wheel && gobo.slot == slot;
            *shown.entry(light.index()).or_default() |= held;
        }
    }
    match shown.into_iter().find(|&(_, shown)| !shown) {
        Some((light, _)) => Err(unsupported(
            light.saturating_add(1),
            Property::Gobo(Wheel::new(wheel).unwrap_or(Wheel::LAST)),
        )),
        None => Ok(()),
    }
}

/// Whether `light` is one of `runs`.
fn within(runs: &[LightRun], light: LightNumber) -> bool {
    runs.iter().any(|run| {
        let first = run.first().index();
        (first..first.saturating_add(run.count())).contains(&light.index())
    })
}

/// Each slot of a wheel of a light of `runs`, with that light: the writers after its
/// `Picks`.
fn slots<'a>(
    writers: &'a [Writer],
    runs: &'a [LightRun],
) -> impl Iterator<Item = (LightNumber, &'a Writer)> + 'a {
    writers
        .iter()
        .scan(None, move |wheel: &mut Option<LightNumber>, writer| {
            Some(match *writer {
                Writer::Picks { light, .. } => {
                    *wheel = Some(light).filter(|&light| within(runs, light));
                    None
                }
                Writer::Takes { .. } | Writer::Fixed { .. } => {
                    *wheel = None;
                    None
                }
                Writer::ColourSlot { .. } | Writer::GoboSlot { .. } => {
                    wheel.map(|light| (light, writer))
                }
            })
        })
        .flatten()
}

/// Whether a channel that does this adds light to a mix.
fn adds(function: &Function) -> bool {
    matches!(
        function,
        Function::Red | Function::Green | Function::Blue | Function::White
    )
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
            slots.push((colour, slot_byte(range)));
        }
    }
    slots
}

/// The byte of each slot of a gobo wheel, slot 1 first: each gobo that does not shake,
/// and open without a preset. None without a gobo.
fn gobo_slots(channel: &fixture_library::Channel) -> Vec<u8> {
    /// The names of open without a preset.
    const OPEN: [&str; 5] = [
        "open",
        "no gobo",
        "open gobo",
        "gobo open",
        "open - gobo selection",
    ];
    let gobo = |r: &&Range| {
        matches!(r.does, Some(Does::Gobo { shakes: false, .. }))
            || r.does.is_none() && names_a_gobo(&r.label)
    };
    let open = |r: &&Range| {
        r.does.is_none()
            && OPEN
                .iter()
                .any(|name| r.label.trim().eq_ignore_ascii_case(name))
    };
    if !channel.ranges.iter().any(|r| gobo(&r)) {
        return Vec::new();
    }
    channel
        .ranges
        .iter()
        .filter(|r| gobo(r) || open(r))
        .map(slot_byte)
        .collect()
}

/// Whether a label names one gobo: "Gobo 2" or "Gobo2", not "Gobo 2 shake".
fn names_a_gobo(label: &str) -> bool {
    let label = label.trim();
    label
        .get(..4)
        .filter(|word| word.eq_ignore_ascii_case("gobo"))
        .and_then(|_| label.get(4..))
        .map(str::trim_start)
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// The byte of the neutral slot of a colour wheel: white, or open.
fn neutral(channel: &fixture_library::Channel) -> Option<u8> {
    colour_slots(channel)
        .into_iter()
        .find(|(colour, _)| *colour == WHITE)
        .map(|(_, value)| value)
}

/// The byte where a hybrid's colour wheel lets the mix of `mixers` through: a range
/// without a preset that is neutral or lets the mix through, or the range at 0 that turns
/// the macro off. Else its neutral slot, unless the light `adds` light to mix: then a static
/// white is a fixed white over the mix, and none.
fn bypass(channel: &fixture_library::Channel, adds: bool, mixers: &[usize]) -> Option<u8> {
    channel
        .ranges
        .iter()
        .find(|range| {
            is_neutral(range)
                || lets_the_mix_through(range)
                || names_the_mix(range, mixers)
                || turns_the_macro_off(range)
        })
        .map(slot_byte)
        .or_else(|| neutral(channel).filter(|_| !adds))
}

/// The range at 0 of a colour macro, whatever its preset (white, black or none), when its
/// label says the macro is off: off, none, nothing, no function, not functional or
/// inactive. Never when it names black, blackout or the LEDs ("All LEDs off" is dark), more
/// than one thing ("OFF/Color") or a slot ("Color 1 (off)").
fn turns_the_macro_off(range: &Range) -> bool {
    const OFF: [&[&str]; 6] = [
        &["off"],
        &["none"],
        &["nothing"],
        &["no", "function"],
        &["not", "functional"],
        &["inactive"],
    ];
    const SLOTS: [&str; 3] = ["color", "colour", "macro"];
    let words = range.words();
    let says = |phrase: &[&str]| words.windows(phrase.len()).any(|seen| seen == phrase);
    let names_a_slot = words.windows(2).any(|pair| match pair {
        [word, number] => {
            SLOTS.contains(&word.as_str()) && number.bytes().all(|b| b.is_ascii_digit())
        }
        _ => false,
    });
    range.min == 0
        && !range.label.contains('/')
        && !range.names_black_or_leds()
        && !names_a_slot
        && OFF.iter().any(|phrase| says(phrase))
}

/// A range at 0 without a preset that names channels of the mode, each one of `mixers`:
/// "Use channels 29-32 to control colour", "All LEDs Set via Channels 1-3".
fn names_the_mix(range: &Range, mixers: &[usize]) -> bool {
    range.min == 0
        && range.does.is_none()
        && channels_named(&range.label).is_some_and(|(first, last)| {
            first <= last
                && (first..=last).all(|number| {
                    number
                        .checked_sub(1)
                        .is_some_and(|index| mixers.contains(&index))
                })
        })
}

/// The channels a label names, counted from 1: "Channels 1-3" is 1 to 3.
fn channels_named(label: &str) -> Option<(usize, usize)> {
    let label = label.to_ascii_lowercase();
    let (_, after) = label.split_once("channel")?;
    let after = after.strip_prefix('s').unwrap_or(after);
    let (first, rest) = after.split_once('-')?;
    let last = rest.split(|c: char| !c.is_ascii_digit()).next()?;
    Some((first.trim().parse().ok()?, last.parse().ok()?))
}

/// A range without a preset at the start of its channel that names the mix (RGB, CMY,
/// mix) and no effect: "Colors adjustable via RGBW", not "Random color CMY, fast" at 244
/// or "CYM mixing from slow to fast".
fn lets_the_mix_through(range: &Range) -> bool {
    const MIXES: [&str; 3] = ["rgb", "cmy", "mix"];
    const EFFECTS: [&str; 7] = ["random", "cycle", "fast", "slow", "speed", "sound", "music"];
    let label = range.label.to_ascii_lowercase();
    let names = |words: &[&str]| words.iter().any(|word| label.contains(word));
    range.does.is_none() && range.min == 0 && names(&MIXES) && !names(&EFFECTS)
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

/// The byte of a slot: the lower midpoint of its range, 20 to 29 is 24.
#[must_use]
pub fn slot_byte(range: &Range) -> u8 {
    let mid = (u16::from(range.min) + u16::from(range.max)) / 2;
    u8::try_from(mid).unwrap_or(range.min)
}
