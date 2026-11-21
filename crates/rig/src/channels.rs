//! The channel model (`docs/design/channel-model.md`): a mode at a channel becomes the
//! config of each channel, the pairs of coarse and fine bytes, and the compounds the
//! screens hold typed values for. A calculation off the tick: the tick sees bytes and
//! pairs, never a fixture.
//!
//! ```text
//! mode + first channel + channels of the engine --lower--> Lowered | PastTheEnd
//! ```
//!
//! The channels of a mode are `first`, `first + 1`, … in one flat space: no universe and
//! no stride. Defaults come from the declared colour, else the preset, else the group.
//! Group-only Intensity channels on a dimmer fixture are dimmers. On other fixtures,
//! group-only Intensity and Beam channels without a declared colour start independent.
//!
//! | Function | Follows | Dark | Change |
//! |---|---|---|---|
//! | dimmer | master and blackout | 0 when rising, first Closed byte when falling, else the top of its travel | fade |
//! | red, green, blue, white, amber, UV, lime, indigo | master and blackout, none when a dimmer carries it | 0 | fade |
//! | cyan, magenta, yellow | master and blackout, none when a dimmer carries it | 255 | fade |
//! | shutter | blackout, none without a range marked closed | the first byte of that range | jump |
//! | hue to colour temperature, pan, tilt, zoom, focus, iris, frost | none | 0 | fade |
//! | strobe, wheels, gobo, prism, speed, effect, control, nothing, other (including fog pumps) | none | 0 | jump |
//!
//! - [Roles](Channel::role) use only declarations. A v6 channel with none starts independent.
//! - On Smoke and Hazer fixtures a dimmer preset controls fog unless its head contains a
//!   declared emitter, or the mode has no heads and contains a declared emitter.
//! - A coarse dimmer carries the emitters in every head that lists it. A dimmer listed in
//!   no head carries the other emitters, including emitters listed in no head.
//! - A falling dimmer's dark word is the first Closed byte with a low byte of 0, or the
//!   high end of its travel with a low byte of 255 when it has no Closed range.
//! - `travel`: the first range marked as running gives its limits and direction, for both
//!   jumps and fades. Otherwise it spans every byte in the channel's direction.
//! - `rest`: the value that lets light out for a shutter or strobe, the bypass for a colour
//!   wheel over a colour mix, else the default, else 0. Ties: latest wins.
//! - Fines claim a free coarse in fine-channel order. Both bytes must have exactly the same
//!   heads and colour. A wheel selector alone cannot pair with an index alone.
//!
//! | Evidence for a fine | Coarse to join |
//! |---|---|
//! | Matching preset, ignoring travel direction | The coarse right before the fine, unless the preceding free coarse looks the same |
//! | Exactly one matching preset elsewhere | That coarse, unless a free coarse of the fine's group is right before the fine |
//! | No matching preset, or no preset on the fine | The coarse right before it, of the same explicit or preset-implied group, unless the preceding free coarse looks the same. A plain fine fits any ranges. A stepped fine requires the same range kinds in order |
//! | Any other case | The fine stays a single byte. A declared emitter follows its own head's dimmer rules |
//!
//! Two free coarses look the same when their group, full preset and range kinds match.
//! No ranges and one unnamed range both describe a plain byte.
//!
//! A joined fine keeps its rest, takes the low byte of the pair's dark, and inherits
//! the coarse byte's follows, change and travel.
//!
//! | Compound | One per |
//! |---|---|
//! | `Byte` | every channel, including both halves of a pair |
//! | `Word` | pair, immediately after its coarse channel's `Byte` |
//! | `Xy` | head with a pan and a tilt: the first of each |
//! | `Rgb`, `Rgbw`, `Rgba`, `Cmy` | head whose colour channels are exactly these (the first of each) |
//! | `Components` | head with 2 to 8 other colour roles, and separately with 2 or more of hue, saturation, lightness and value. First coarse byte of each role, in channel order |
//! | `Slot` | coarse channel that jumps and has ranges, including when paired. Each range is at [`slot_byte`] |

use crate::capabilities::Wheels;
use crate::heads;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::mem::{Discriminant, discriminant};
use fixture_library::{Byte, Channel, Degrees, Direction, Does, Function, Mode};

pub use crate::capabilities::slot_byte;

/// A mode at a channel, lowered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lowered {
    /// One per channel of the mode, in its order.
    pub channels: Vec<Config>,
    /// Each coarse byte with its fine byte, by coarse byte.
    pub pairs: Vec<Pair>,
    /// Bytes and words first, in channel order, then per head (the fixture's own first):
    /// position, colour, hue/saturation components, slots.
    pub compounds: Vec<Compound>,
}

/// The config of one channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub at: u32,
    /// What shows when nothing plays.
    pub rest: u8,
    /// Where it goes when what it follows is at 0, and the zero of its brightness.
    pub dark: u8,
    pub change: Change,
    pub ties: Ties,
    pub follows: Follows,
    /// Where a normalised value lands, for [`to_native`].
    pub travel: Travel,
}

/// How a channel goes from one value to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Fade,
    /// Wheels, shutters, modes: never through the values between.
    Jump,
}

/// Who wins among equals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ties {
    Latest,
    /// The greater brightness, the distance from `dark`.
    Highest,
}

/// The modifiers a channel follows, one bit per slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Follows(u8);

impl Follows {
    pub const NONE: Self = Self(0);
    /// Slot 0.
    pub const MASTER: Self = Self(1);
    /// Slot 1.
    pub const BLACKOUT: Self = Self(1 << 1);
    const BOTH: Self = Self(0b11);

    /// Whether every slot of `other` is followed.
    #[must_use]
    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The byte of the channel.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

/// The bytes a channel's values run over, and which way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Travel {
    pub low: u8,
    pub high: u8,
    pub sense: Sense,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sense {
    /// More of what it does as the value rises.
    Rising,
    Falling,
}

/// A coarse byte and its fine byte: one 16-bit value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub coarse: u32,
    pub fine: u32,
}

/// One value of a compound: a channel or a pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Byte(u32),
    Word(Pair),
}

/// A byte or a word, for [`to_native`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    Byte,
    Word,
}

/// Channels a typed value is set on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compound {
    /// The head, counted from 0. `None` for the fixture's own channels.
    pub head: Option<u16>,
    pub shape: Shape,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Byte(u32),
    Word(Pair),
    Xy {
        pan: Field,
        tilt: Field,
        degrees: Option<Degrees>,
    },
    Rgb([Field; 3]),
    Rgbw([Field; 4]),
    Rgba([Field; 4]),
    Cmy([Field; 3]),
    Slot {
        at: u32,
        slots: Vec<Slot>,
    },
    /// Named by what each does, in channel order.
    Components(Vec<(Function, Field)>),
}

/// A named value of a channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub label: String,
    pub byte: u8,
}

/// The mode's `channels` run past the last channel of the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PastTheEnd {
    pub channels: usize,
}

/// The channels of `mode` from `first`, checked against the `space` channels of the engine.
pub fn lower(mode: &Mode, first: u32, space: u32) -> Result<Lowered, PastTheEnd> {
    let count = mode.channels.len();
    let past = PastTheEnd { channels: count };
    let end = u32::try_from(count)
        .ok()
        .and_then(|count| first.checked_add(count))
        .ok_or(past)?;
    if end > space {
        return Err(past);
    }
    // below `end`, so it fits
    let at = |index: usize| first.saturating_add(u32::try_from(index).unwrap_or(u32::MAX));
    let carried = carried_by_dimmers(&mode.channels, &mode.pixels);
    let heads = heads(mode);
    let mut groups: BTreeMap<Option<usize>, Vec<(usize, &Channel)>> = BTreeMap::new();
    for ((index, channel), head) in mode.channels.iter().enumerate().zip(&heads) {
        groups.entry(*head).or_default().push((index, channel));
    }
    let fine_of = pair_bytes(mode);
    let joined: BTreeMap<usize, usize> = fine_of.iter().map(|(&c, &f)| (f, c)).collect();
    let field = |index: usize| match fine_of.get(&index) {
        Some(&fine) => Field::Word(Pair {
            coarse: at(index),
            fine: at(fine),
        }),
        None => Field::Byte(at(index)),
    };

    // every channel but a joined fine byte, with the dark of its pair
    let mut own: BTreeMap<usize, (Config, u16)> = BTreeMap::new();
    let mut compounds = Vec::new();
    let mut by_head = Vec::new();
    for (head, group) in &groups {
        let head = head.and_then(|head| u16::try_from(head).ok());
        for (&(index, channel), rest) in group.iter().zip(rests(group)) {
            if joined.contains_key(&index) {
                continue;
            }
            let role = channel.role();
            let change = change(&role);
            let travel = travel(channel);
            let (follows, dark) = rule(
                channel,
                &role,
                carried.get(index).copied().unwrap_or(false),
                travel,
            );
            let [dark_coarse, _] = dark.to_be_bytes();
            let config = Config {
                at: at(index),
                rest,
                dark: dark_coarse,
                change,
                ties: Ties::Latest,
                follows,
                travel,
            };
            own.insert(index, (config, dark));
        }
        let first_of = |function: &Function| {
            group
                .iter()
                .find(|(_, c)| c.byte == Byte::Coarse && c.role() == *function)
                .map(|&(index, _)| field(index))
        };
        if let (Some(pan), Some(tilt)) = (first_of(&Function::Pan), first_of(&Function::Tilt)) {
            by_head.push(Compound {
                head,
                shape: Shape::Xy {
                    pan,
                    tilt,
                    degrees: mode.degrees,
                },
            });
        }
        if let Some(shape) = colour(components(group, field, is_colour)) {
            by_head.push(Compound { head, shape });
        }
        let hsv = components(group, field, |role| {
            matches!(
                role,
                Function::Hue | Function::Saturation | Function::Lightness | Function::Value
            )
        });
        if hsv.len() >= 2 {
            by_head.push(Compound {
                head,
                shape: Shape::Components(hsv),
            });
        }
        for &(index, channel) in group {
            let jumps = own
                .get(&index)
                .is_some_and(|(config, _)| config.change == Change::Jump);
            if channel.byte == Byte::Coarse && jumps && !channel.ranges.is_empty() {
                by_head.push(Compound {
                    head,
                    shape: Shape::Slot {
                        at: at(index),
                        slots: channel
                            .ranges
                            .iter()
                            .map(|range| Slot {
                                label: range.label.clone(),
                                byte: slot_byte(range),
                            })
                            .collect(),
                    },
                });
            }
        }
    }

    let mut channels = Vec::with_capacity(count);
    for (index, channel) in mode.channels.iter().enumerate() {
        let config = match joined.get(&index).and_then(|coarse| own.get(coarse)) {
            Some(&(coarse, dark)) => {
                let [_, dark_fine] = dark.to_be_bytes();
                Config {
                    at: at(index),
                    rest: channel.default.unwrap_or(0),
                    dark: dark_fine,
                    ..coarse
                }
            }
            None => match own.get(&index) {
                Some(&(config, _)) => config,
                None => continue,
            },
        };
        channels.push(config);
        let head = heads
            .get(index)
            .copied()
            .flatten()
            .and_then(|head| u16::try_from(head).ok());
        compounds.push(Compound {
            head,
            shape: Shape::Byte(config.at),
        });
        if let Field::Word(pair) = field(index) {
            compounds.push(Compound {
                head,
                shape: Shape::Word(pair),
            });
        }
    }
    compounds.extend(by_head);
    Ok(Lowered {
        channels,
        pairs: fine_of
            .iter()
            .map(|(&coarse, &fine)| Pair {
                coarse: at(coarse),
                fine: at(fine),
            })
            .collect(),
        compounds,
    })
}

/// The shared shutter and wheel helpers take their role in the v11 function field.
/// Adapt just their inputs so each head gets the rest bytes of its declared roles.
fn rests(group: &[(usize, &Channel)]) -> Vec<u8> {
    let channels: Vec<_> = group
        .iter()
        .map(|&(_, c)| Channel {
            function: c.role(),
            ..c.clone()
        })
        .collect();
    let numbered: Vec<_> = group
        .iter()
        .zip(&channels)
        .map(|(&(i, _), c)| (i, c))
        .collect();
    let wheels = Wheels::of(&numbered);
    channels
        .iter()
        .map(|c| {
            let rest = match c.byte {
                Byte::Coarse => c.open().or_else(|| wheels.rest(c)),
                Byte::Fine => None,
            };
            rest.or(c.default).unwrap_or(0)
        })
        .collect()
}

/// Declared facts used to find a fine byte's partner. A channel can belong to several heads.
struct PairChannel<'a> {
    byte: Byte,
    colour: Option<Function>,
    preset: Option<&'a str>,
    group: Option<&'a str>,
    ranges: Vec<Option<Discriminant<Does>>>,
    wheel: Wheel,
    heads: Vec<usize>,
}

#[derive(Clone, Copy)]
enum Wheel {
    Neither,
    Index,
    Slots,
    IndexAndSlots,
}

impl Wheel {
    fn of(channel: &Channel, stem: Option<&str>) -> Self {
        let index = stem.is_some_and(|s| s.ends_with("Index"))
            || channel
                .ranges
                .iter()
                .any(|r| matches!(r.does, Some(Does::Rotation(_))));
        let slots = channel.ranges.iter().any(|r| {
            matches!(
                r.does,
                Some(
                    Does::Gobo { .. }
                        | Does::Color(_)
                        | Does::Colors(_, _)
                        | Does::Prism { .. }
                        | Does::PrismOff
                )
            )
        });
        match (index, slots) {
            (false, false) => Self::Neither,
            (true, false) => Self::Index,
            (false, true) => Self::Slots,
            (true, true) => Self::IndexAndSlots,
        }
    }

    fn can_pair(self, other: Self) -> bool {
        !matches!(
            (self, other),
            (Self::Slots, Self::Index) | (Self::Index, Self::Slots)
        )
    }
}

impl<'a> PairChannel<'a> {
    fn new(channel: &'a Channel) -> Self {
        let declared = &channel.declared;
        let stem = declared.preset_stem();
        let ranges = match channel.ranges.as_slice() {
            [only] if only.does.is_none() => Vec::new(),
            steps => steps
                .iter()
                .map(|r| r.does.as_ref().map(discriminant))
                .collect(),
        };
        Self {
            byte: channel.byte,
            colour: declared.emitter_colour(),
            preset: stem,
            group: declared
                .group
                .as_deref()
                .or_else(|| stem.and_then(preset_group)),
            ranges,
            wheel: Wheel::of(channel, stem),
            heads: Vec::new(),
        }
    }

    fn partner(
        &self,
        fine: usize,
        channels: &[Self],
        taken: &BTreeMap<usize, usize>,
    ) -> Option<usize> {
        let free = |i, c: &Self| {
            c.byte == Byte::Coarse
                && c.heads == self.heads
                && c.colour == self.colour
                && !taken.contains_key(&i)
        };
        let free_before = |i: usize| {
            let i = i.checked_sub(1)?;
            let c = channels.get(i)?;
            free(i, c).then_some((i, c))
        };
        let same_group = |c: &Self| self.group.is_some() && c.group == self.group;
        let adjacent = free_before(fine);
        let distinct_adjacent = adjacent.filter(|&(i, c)| {
            free_before(i).is_none_or(|(_, prev)| {
                prev.group != c.group || prev.preset != c.preset || prev.ranges != c.ranges
            })
        });

        if let Some(preset) = self.preset.map(preset_family) {
            let matches_preset = |c: &Self| {
                c.preset.map(preset_family) == Some(preset) && c.wheel.can_pair(self.wheel)
            };
            if let Some((i, _)) = distinct_adjacent.filter(|(_, c)| matches_preset(c)) {
                return Some(i);
            }
            let mut candidates = channels
                .iter()
                .enumerate()
                .filter(|&(i, c)| free(i, c) && matches_preset(c));
            if let Some((i, _)) = candidates.next() {
                // Even a selector beside an index fine casts doubt on a distant preset.
                return (candidates.next().is_none()
                    && adjacent.is_none_or(|(_, c)| !same_group(c)))
                .then_some(i);
            }
        }

        let (i, c) = distinct_adjacent?;
        (same_group(c)
            && c.wheel.can_pair(self.wheel)
            && (self.ranges.is_empty() || self.ranges == c.ranges))
            .then_some(i)
    }
}

/// A QLC+ preset identifies a capability even when its travel direction differs.
fn preset_family(stem: &str) -> &str {
    [
        "SlowFast", "FastSlow", "SmallBig", "BigSmall", "NearFar", "FarNear", "MinToMax",
        "MaxToMin",
    ]
    .into_iter()
    .find_map(|way| stem.strip_suffix(way))
    .unwrap_or(stem)
}

/// QLC+ group of a preset on a channel without an explicit group.
fn preset_group(stem: &str) -> Option<&'static str> {
    match stem {
        "PositionXAxis" => Some("Pan"),
        "PositionYAxis" => Some("Tilt"),
        "NoFunction" => Some("Nothing"),
        _ => [
            ("Intensity", "Intensity"),
            ("PositionPan", "Pan"),
            ("PositionTilt", "Tilt"),
            ("Speed", "Speed"),
            ("Color", "Colour"),
            ("Gobo", "Gobo"),
            ("Shutter", "Shutter"),
            ("Beam", "Beam"),
            ("Prism", "Prism"),
        ]
        .into_iter()
        .find_map(|(prefix, group)| stem.starts_with(prefix).then_some(group)),
    }
}

/// Coarse index to fine index. Only a fine that claims a coarse removes it from later choices.
fn pair_bytes(mode: &Mode) -> BTreeMap<usize, usize> {
    let mut fine_of = BTreeMap::new();
    let mut channels: Vec<_> = mode.channels.iter().map(PairChannel::new).collect();
    for (head, numbers) in mode.pixels.iter().enumerate() {
        for &number in numbers {
            if let Some(channel) = channels.get_mut(usize::from(number)) {
                channel.heads.push(head);
            }
        }
    }
    for (fine, channel) in channels
        .iter()
        .enumerate()
        .filter(|(_, c)| c.byte == Byte::Fine)
    {
        if let Some(coarse) = channel.partner(fine, &channels, &fine_of) {
            fine_of.insert(coarse, fine);
        }
    }
    fine_of
}

/// Every channel listed alongside a coarse dimmer, or carried by the shared dimmer.
fn carried_by_dimmers(channels: &[Channel], pixels: &[Vec<u16>]) -> Vec<bool> {
    let dimmer = |c: &Channel| c.byte == Byte::Coarse && c.role() == Function::Dimmer;
    let mut carried = vec![false; channels.len()];
    for head in pixels {
        if head
            .iter()
            .any(|&index| channels.get(usize::from(index)).is_some_and(dimmer))
        {
            for &index in head {
                if let Some(in_head) = carried.get_mut(usize::from(index)) {
                    *in_head = true;
                }
            }
        }
    }
    // A dimmer in any head marked its own channel above. An unmarked dimmer is shared
    // and carries every emitter that a head dimmer did not already carry.
    if channels
        .iter()
        .zip(&carried)
        .any(|(c, in_head)| dimmer(c) && !in_head)
    {
        carried.fill(true);
    }
    carried
}

/// What a channel follows and its dark word. A joined fine inherits its coarse rule.
/// Of the unpaired fines only a declared emitter carries light on its own.
fn rule(channel: &Channel, role: &Function, carried: bool, travel: Travel) -> (Follows, u16) {
    if channel.byte == Byte::Fine && !is_colour(role) {
        return (Follows::NONE, 0);
    }
    let unless_carried = if carried {
        Follows::NONE
    } else {
        Follows::BOTH
    };
    match role {
        Function::Dimmer => (
            Follows::BOTH,
            match travel.sense {
                Sense::Rising => 0,
                Sense::Falling => u16::from_be_bytes(
                    channel
                        .closed()
                        .map_or([travel.high, u8::MAX], |closed| [closed, 0]),
                ),
            },
        ),
        Function::Red
        | Function::Green
        | Function::Blue
        | Function::White
        | Function::Amber
        | Function::Uv
        | Function::Lime
        | Function::Indigo => (unless_carried, 0),
        Function::Cyan | Function::Magenta | Function::Yellow => (unless_carried, u16::MAX),
        Function::Shutter => channel.closed().map_or((Follows::NONE, 0), |closed| {
            (Follows::BLACKOUT, u16::from_be_bytes([closed, 0]))
        }),
        Function::Hue
        | Function::Saturation
        | Function::Lightness
        | Function::Value
        | Function::ColorTemperature
        | Function::Pan
        | Function::Tilt
        | Function::Zoom
        | Function::Focus
        | Function::Iris
        | Function::Frost
        | Function::Gobo
        | Function::ColorWheel
        | Function::Strobe
        | Function::Prism
        | Function::Speed
        | Function::Effect
        | Function::Control
        | Function::NoFunction
        | Function::Other(_) => (Follows::NONE, 0),
    }
}

/// Whether a channel that does this fades or jumps.
fn change(function: &Function) -> Change {
    match function {
        Function::Dimmer
        | Function::Red
        | Function::Green
        | Function::Blue
        | Function::White
        | Function::Amber
        | Function::Uv
        | Function::Indigo
        | Function::Lime
        | Function::Cyan
        | Function::Magenta
        | Function::Yellow
        | Function::Hue
        | Function::Saturation
        | Function::Lightness
        | Function::Value
        | Function::ColorTemperature
        | Function::Pan
        | Function::Tilt
        | Function::Zoom
        | Function::Focus
        | Function::Iris
        | Function::Frost => Change::Fade,
        Function::Gobo
        | Function::ColorWheel
        | Function::Strobe
        | Function::Shutter
        | Function::Prism
        | Function::Speed
        | Function::Effect
        | Function::Control
        | Function::NoFunction
        | Function::Other(_) => Change::Jump,
    }
}

/// The first range that says which way it runs, else every byte the way the channel runs.
fn travel(channel: &Channel) -> Travel {
    let sense = |direction: Option<Direction>| match direction {
        Some(Direction::Down) => Sense::Falling,
        Some(Direction::Up) | None => Sense::Rising,
    };
    channel
        .ranges
        .iter()
        .find_map(|range| match &range.does {
            Some(Does::Runs(direction)) => Some(Travel {
                low: range.min,
                high: range.max,
                sense: sense(Some(*direction)),
            }),
            _ => None,
        })
        .unwrap_or(Travel {
            low: 0,
            high: u8::MAX,
            sense: sense(channel.direction),
        })
}

/// The first coarse channel of each selected role in a head, in channel order.
fn components(
    group: &[(usize, &Channel)],
    field: impl Fn(usize) -> Field,
    includes: impl Fn(&Function) -> bool,
) -> Vec<(Function, Field)> {
    let mut found = Vec::new();
    for &(index, channel) in group {
        let role = channel.role();
        if channel.byte == Byte::Coarse && includes(&role) && !found.iter().any(|(f, _)| *f == role)
        {
            found.push((role, field(index)));
        }
    }
    found
}

/// The colour compound for these emitters or filters.
fn colour(found: Vec<(Function, Field)>) -> Option<Shape> {
    let of = |function: Function| {
        found
            .iter()
            .find(|(f, _)| *f == function)
            .map(|&(_, field)| field)
    };
    let shape = match found.len() {
        3 => match (
            of(Function::Red),
            of(Function::Green),
            of(Function::Blue),
            of(Function::Cyan),
            of(Function::Magenta),
            of(Function::Yellow),
        ) {
            (Some(r), Some(g), Some(b), ..) => Some(Shape::Rgb([r, g, b])),
            (.., Some(c), Some(m), Some(y)) => Some(Shape::Cmy([c, m, y])),
            _ => None,
        },
        4 => match (
            of(Function::Red),
            of(Function::Green),
            of(Function::Blue),
            of(Function::White),
            of(Function::Amber),
        ) {
            (Some(r), Some(g), Some(b), Some(w), _) => Some(Shape::Rgbw([r, g, b, w])),
            (Some(r), Some(g), Some(b), _, Some(a)) => Some(Shape::Rgba([r, g, b, a])),
            _ => None,
        },
        _ => None,
    };
    shape.or_else(|| {
        (2..=8)
            .contains(&found.len())
            .then_some(Shape::Components(found))
    })
}

/// Whether a channel that does this makes or filters colour.
fn is_colour(function: &Function) -> bool {
    matches!(
        function,
        Function::Red
            | Function::Green
            | Function::Blue
            | Function::White
            | Function::Amber
            | Function::Uv
            | Function::Lime
            | Function::Indigo
            | Function::Cyan
            | Function::Magenta
            | Function::Yellow
    )
}

/// A 16-bit value as its coarse and fine bytes.
#[must_use]
pub const fn word_split(word: u16) -> (u8, u8) {
    let [coarse, fine] = word.to_be_bytes();
    (coarse, fine)
}

/// A coarse and a fine byte as one 16-bit value.
#[must_use]
pub const fn word_join(coarse: u8, fine: u8) -> u16 {
    u16::from_be_bytes([coarse, fine])
}

/// A normalised `value` (0 to 65535) as the native byte or word of a channel that runs over
/// `travel`, rounded half up. A word runs from the low coarse byte to the high one's top.
#[must_use]
pub fn to_native(value: u16, travel: Travel, width: Width) -> u16 {
    let (low, high) = (travel.low.min(travel.high), travel.low.max(travel.high));
    let (low, high) = match width {
        Width::Byte => (u32::from(low), u32::from(high)),
        Width::Word => (
            u32::from(word_join(low, 0)),
            u32::from(word_join(high, u8::MAX)),
        ),
    };
    let full = u32::from(u16::MAX);
    let step = (u32::from(value) * (high - low) + full / 2) / full;
    let native = match travel.sense {
        Sense::Rising => low + step,
        Sense::Falling => high - step,
    };
    u16::try_from(native).unwrap_or(u16::MAX)
}

/// Red, green and blue as red, green and blue less the part they share, and that part as
/// white.
#[must_use]
pub fn rgb_to_rgbw([red, green, blue]: [u16; 3]) -> [u16; 4] {
    let white = red.min(green).min(blue);
    [red - white, green - white, blue - white, white]
}

/// Red, green, blue and white as red, green and blue, white added to each, at most full.
#[must_use]
pub const fn rgbw_to_rgb([red, green, blue, white]: [u16; 4]) -> [u16; 3] {
    [
        red.saturating_add(white),
        green.saturating_add(white),
        blue.saturating_add(white),
    ]
}

/// `at` degrees of a pan or tilt that runs `of` degrees as a word, rounded half up. `None`
/// past the end, or when it runs 0 degrees.
#[must_use]
pub fn degrees(at: u16, of: u16) -> Option<u16> {
    if of == 0 || at > of {
        return None;
    }
    let (at, of) = (u32::from(at), u32::from(of));
    u16::try_from((at * u32::from(u16::MAX) + of / 2) / of).ok()
}
