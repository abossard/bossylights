//! The data the API carries that the core itself checks or keeps: timing, saved looks,
//! why a command is refused. The messages around it (`Request`, `Reply`, the health and
//! library views, `API_VERSION`) are in `crates/wire`.

use crate::patch::Target;
use crate::{Light, OutputNumber, Rgb, Text};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A step: sparse values for the lights a target names. An absent property lets
/// another source through; zero and black claim it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Step {
    pub target: Target,
    pub values: Light,
}
/// A saved step row, also read from earlier show files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Saved {
    pub look: u16,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub name: Text,
    pub step: Step,
}

/// A number outside the range its type holds: the request is no command
/// (`NotACommand`, docs/coding.md rule 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutOfRange;
impl core::fmt::Display for OutOfRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("out of range")
    }
}

/// A whole number from `$least` to `$most`, checked when it is built or decoded.
macro_rules! bounded {
    ($(#[$doc:meta])* $name:ident($int:ty, $repr:literal), $least:expr, $most:expr) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = $repr, into = $repr)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "number"))]
        pub struct $name($int);

        impl $name {
            pub const LEAST: Self = Self($least);
            pub const MOST: Self = Self($most);

            /// `None` outside `LEAST` to `MOST`.
            #[allow(
                unused_comparisons,
                clippy::absurd_extreme_comparisons,
                reason = "some bounds are the integer's own"
            )]
            pub const fn new(value: $int) -> Option<Self> {
                if value >= $least && value <= $most {
                    Some(Self(value))
                } else {
                    None
                }
            }

            pub const fn get(self) -> $int {
                self.0
            }
        }

        impl TryFrom<$int> for $name {
            type Error = OutOfRange;
            fn try_from(value: $int) -> Result<Self, OutOfRange> {
                Self::new(value).ok_or(OutOfRange)
            }
        }

        impl From<$name> for $int {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

bounded!(
    /// A percentage, 0 to 100: the spread of a play, the transition of a sequence item.
    Percent(u8, "u8"), 0, 100
);
bounded!(
    /// The width of a sequence item in sixteenths of a beat: at least one.
    Width(u16, "u16"), 1, u16::MAX
);
bounded!(
    /// The period of a wave in beats, 1 to 32.
    Period(u8, "u8"), 1, 32
);

impl Default for Percent {
    fn default() -> Self {
        Self::LEAST
    }
}

/// Local time and phase of a root or a reference. Spread is a percentage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Use {
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub speed: Speed,
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub repeat: Repeat,
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub spread: Percent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Speed {
    Half,
    #[default]
    Normal,
    Double,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Repeat {
    #[default]
    Loop,
    Once,
}

/// Width is counted in sixteenths of a beat. Transition is the percentage of this
/// item's width spent moving from the preceding item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SequenceItem {
    pub look: u16,
    #[serde(default = "one_beat")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub width: Width,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub transition: Percent,
    #[serde(default, rename = "use")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub usage: Use,
}

const fn one_beat() -> Width {
    Width(16)
}

impl Default for SequenceItem {
    fn default() -> Self {
        Self {
            look: 0,
            width: one_beat(),
            transition: Percent::LEAST,
            usage: Use::default(),
        }
    }
}

/// A bounded wire list, carried in the same fixed-size command as groups. An
/// oversized command reaches admission as WorkLimit without allocating a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Sequence {
    #[cfg_attr(feature = "ts", ts(type = "Array<SequenceItem>"))]
    items: Items,
}

impl Sequence {
    pub const MOST: usize = 64;

    pub fn new(items: &[SequenceItem]) -> Option<Self> {
        let mut storage = Items::default();
        storage.items.get_mut(..items.len())?.copy_from_slice(items);
        storage.len = u8::try_from(items.len()).ok()?;
        Some(Self { items: storage })
    }

    pub fn as_slice(&self) -> &[SequenceItem] {
        self.items
            .items
            .get(..usize::from(self.items.len))
            .unwrap_or_default()
    }

    pub fn too_many(&self) -> bool {
        usize::from(self.items.len) > Self::MOST
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Items {
    len: u8,
    items: [SequenceItem; Sequence::MOST],
}

impl Default for Items {
    fn default() -> Self {
        Self {
            len: 0,
            items: [SequenceItem::default(); Sequence::MOST],
        }
    }
}

impl Serialize for Items {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.items.iter().take(usize::from(self.len)))
    }
}

impl<'de> Deserialize<'de> for Items {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct List;
        impl<'de> serde::de::Visitor<'de> for List {
            type Value = Items;
            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("a list of sequence items")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut list: A,
            ) -> Result<Items, A::Error> {
                let mut storage = Items::default();
                while let Some(item) = list.next_element()? {
                    if let Some(slot) = storage.items.get_mut(usize::from(storage.len)) {
                        *slot = item;
                    }
                    storage.len = storage.len.saturating_add(1);
                }
                Ok(storage)
            }
        }
        d.deserialize_seq(List)
    }
}

/// Ordered RGB16 stops. The decoder rejects a fifth swatch before storing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "Array<Rgb>"))]
pub struct Swatches {
    colours: [Rgb; Self::MOST],
    len: u8,
}

impl Default for Swatches {
    fn default() -> Self {
        Self {
            colours: [Rgb::BLACK; Self::MOST],
            len: 0,
        }
    }
}

impl Swatches {
    pub const MOST: usize = 4;

    pub fn new(colours: &[Rgb]) -> Option<Self> {
        let mut swatches = Self::default();
        swatches
            .colours
            .get_mut(..colours.len())?
            .copy_from_slice(colours);
        swatches.len = u8::try_from(colours.len()).ok()?;
        Some(swatches)
    }

    pub fn as_slice(&self) -> &[Rgb] {
        self.colours
            .get(..usize::from(self.len))
            .unwrap_or_default()
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Serialize for Swatches {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.as_slice().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Swatches {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct List;
        impl<'de> serde::de::Visitor<'de> for List {
            type Value = Swatches;

            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("up to four RGB swatches")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut list: A,
            ) -> Result<Swatches, A::Error> {
                let mut swatches = Swatches::default();
                while let Some(colour) = list.next_element()? {
                    let slot = swatches
                        .colours
                        .get_mut(usize::from(swatches.len))
                        .ok_or_else(|| {
                            serde::de::Error::invalid_length(Swatches::MOST + 1, &self)
                        })?;
                    *slot = colour;
                    swatches.len += 1;
                }
                Ok(swatches)
            }
        }
        d.deserialize_seq(List)
    }
}

/// A step may also lend its swatches to a Wave. Only its target and values play directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AuthoredStep {
    pub target: Target,
    pub values: Light,
    #[serde(default, skip_serializing_if = "Swatches::is_empty")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub swatches: Swatches,
}

impl AuthoredStep {
    pub const fn step(&self) -> Step {
        Step {
            target: self.target,
            values: self.values,
        }
    }
}

impl From<Step> for AuthoredStep {
    fn from(step: Step) -> Self {
        Self {
            target: step.target,
            values: step.values,
            swatches: Swatches::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Pattern {
    Wave {
        target: WaveTarget,
        period: Period,
        palette: Palette,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum WaveTarget {
    Group {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        group: Text,
    },
    Matrix {
        fixture: u16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Palette {
    Inline(Swatches),
    Step { look: u16 },
}

/// A definition has exactly one kind for its lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "the wire definitions are bounded and travel inline with the command"
)]
pub enum Definition {
    Step(AuthoredStep),
    Sequence(Sequence),
    Pattern(Pattern),
}

/// Each saved row contains exactly one definition, under `step`, `sequence` or `pattern`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        type = "{ look: number, name: string, step: AuthoredStep } | { look: number, name: string, sequence: Sequence } | { look: number, name: string, pattern: Pattern }"
    )
)]
pub struct SavedLook {
    pub look: u16,
    pub name: Text,
    pub definition: Definition,
}

impl From<Saved> for SavedLook {
    fn from(saved: Saved) -> Self {
        Self {
            look: saved.look,
            name: saved.name,
            definition: Definition::Step(saved.step.into()),
        }
    }
}

impl Serialize for SavedLook {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut row = s.serialize_struct("SavedLook", 3)?;
        row.serialize_field("look", &self.look)?;
        row.serialize_field("name", &self.name)?;
        match &self.definition {
            Definition::Step(step) => row.serialize_field("step", step)?,
            Definition::Sequence(sequence) => row.serialize_field("sequence", sequence)?,
            Definition::Pattern(pattern) => row.serialize_field("pattern", pattern)?,
        }
        row.end()
    }
}

// A present key must contain a definition, even when another key is present too.
fn present<'de, T: Deserialize<'de>, D: Deserializer<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "S: Deserialize<'de>"))]
struct Row<S> {
    look: u16,
    name: Text,
    #[serde(default, deserialize_with = "present")]
    step: Option<S>,
    #[serde(default, deserialize_with = "present")]
    sequence: Option<Sequence>,
    #[serde(default, deserialize_with = "present")]
    pattern: Option<Pattern>,
}

impl<S: Into<AuthoredStep>> Row<S> {
    fn checked<E: serde::de::Error>(self) -> Result<SavedLook, E> {
        let definition = match (self.step, self.sequence, self.pattern) {
            (Some(step), None, None) => Definition::Step(step.into()),
            (None, Some(sequence), None) => Definition::Sequence(sequence),
            (None, None, Some(pattern)) => Definition::Pattern(pattern),
            _ => return Err(E::custom("a look has exactly one definition")),
        };
        Ok(SavedLook {
            look: self.look,
            name: self.name,
            definition,
        })
    }
}

impl SavedLook {
    pub fn has_swatches(&self) -> bool {
        matches!(&self.definition, Definition::Step(step) if !step.swatches.is_empty())
    }
}

impl<'de> Deserialize<'de> for SavedLook {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Row::<AuthoredStep>::deserialize(d)?.checked()
    }
}

// Commands read a one-based output number. Replies write it with the same numbering.
impl Serialize for OutputNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (self.slot() + 1).serialize(serializer)
    }
}
