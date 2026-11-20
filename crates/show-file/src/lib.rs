//! The show file: the patch, the groups, the saved looks and the Mixer's settings as JSON text, and back. A calculation: the
//! engine process reads and writes the file (`engine-server`).
//!
//! ```text
//! patch + looks --write--> text
//! text + an empty patch --read--> the patch with its fixtures and groups, the looks | Unopened
//! ```
//!
//! What the text looks like. One line per fixture and per group, so moving one fixture
//! changes one line. Each mode is kept whole, so a show opens without the library.
//!
//! ```text
//! {
//!   "version": 6,
//!   "patch": [
//!     {"fixture":1,"name":"PAR 1","mode":"Generic / Generic RGB / RGB","at":{"universe":1,"address":37},"first":1}
//!   ],
//!   "groups": [
//!     {"name":"front","members":[{"Fixture":{"fixture":1}}]}
//!   ],
//!   "looks": [
//!     {"look":1,"name":"red","step":{"target":{"Fixture":{"fixture":1}},"values":{"colour":{"red":65535,"green":0,"blue":0}}}}
//!   ],
//!   "rules": {"brightness":"Latest","manual_end":"UntilRelease","other_end":"Reveal"},
//!   "modes": {
//!     "Generic / Generic RGB / RGB": {
//!       "maker": "Generic",
//!       "model": "Generic RGB",
//!       "mode": { "name": "RGB", "channels": [ ... ] }
//!     }
//!   }
//! }
//! ```
//!
//! | Key | Holds |
//! |---|---|
//! | `version` | [`VERSION`]. A newer one is not read |
//! | `patch` | each fixture by number: its name, its mode (a key of `modes`), where, its first light |
//! | `groups` | in the order they were made, each member a light or a fixture |
//! | `looks` | each saved look by number: its name and a Wave (v6), a sequence (v5), or a step. Steps can store up to four RGB swatches from v6 and the slot of each gobo wheel in `gobos`. Version 1 has none. Versions 2 and 3 hold one `gobo` of a wheel and a slot, read and checked as those engines did: read as that wheel's slot. A gobo a light with a gobo wheel cannot show is refused, a wheel past 3 among them (no wheel past 3 is shown now); on lights without a gobo wheel it showed nothing and is left out |
//! | `rules` | the Mixer's three settings. Versions 1 and 2 have none: the defaults |
//! | `modes` | each mode a fixture is patched in, with its maker and model, by key |
//!
//! Reading checks what the commands check: a fixture is placed as patch places it, a
//! group is set as set group sets it. A file that fails a check is not read at all.
//!
//! A file is at most [`MOST_BYTES`] long: one over it is not read, and neither is one
//! whose patch and looks would be written back over it. A patch kept in a file ([`LIMIT`])
//! reserves [`LOOKS_ROOM`] for steps and settings. Longer sequence rows share the
//! remaining space with the patch. Edits on either side keep the next save in bounds.

#![no_std]
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::num::{NonZeroU16, NonZeroU32};
use engine_core::{
    ChannelNumbers, LOOKS, Light, Member, Members, MixRules, PatchCommand, Position, Refusal, Rgb,
    Saved, SavedLook, Sequence, Step, Target, Text, Unopened, Wheel,
};
use fixture_library::Mode;
use looks::Looks;
use rig::{Limit, Patch, Placed, Unplaced};
use serde::de::{IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

/// The version of the file this engine writes and reads. Raise it with every change an
/// engine of the old version would read wrong. An engine never reads a newer file.
pub const VERSION: u32 = 6;

/// The longest file, in bytes: 64 MiB. Far over any show, so no show meets it, and a file
/// of gigabytes is not read whole at start.
///
/// No count of the patch has a limit of its own: a mode's size comes from its definition,
/// and fixtures, modes and groups share the bytes. What holds it is [`LIMIT`]: each
/// command that changes the patch measures its file, and is refused at one byte over.
///
/// | Part of the file | Measured | Fits beside the rest |
/// |---|---|---|
/// | a fixture's line, a name of 128 bytes | 244 bytes | 16 384 lines, one per light: 4 MB |
/// | a mode, pretty | 0.2 to 17 KB (the test definitions) | 3 500 modes of 17 KB beside the lines |
/// | a group of 512 lights | 14 KB | 4 000 groups beside the lines |
pub const MOST_BYTES: usize = 64 * 1024 * 1024;

/// The show file to keep a patch in ([`rig::Patch::kept_in`]): a change of the patch
/// whose file would be over [`MOST_BYTES`] is refused. A change sums the lengths of the
/// parts of its file and encodes none of a mode: each definition is encoded once, when its
/// fixture is patched ([`encode`]), and kept with the fixture. The save after the change
/// joins the same parts into the text.
pub const LIMIT: Limit = Limit {
    most: MOST_BYTES - LOOKS_ROOM,
    fits,
    encode,
};

/// Version 4's allowance per step, including its escaped name, target and properties.
const STEP_MOST: usize = 2048;

/// Room for a look's escaped name, target and properties, or its 64 full references.
/// Each reference takes under 112 bytes even with the longest valid numeric fields.
pub const LOOK_MOST: usize = STEP_MOST + 112 * Sequence::MOST;

/// How much longer the line of the Mixer's settings may be than the defaults' line,
/// which a patch is measured with.
pub const RULES_MOST: usize = 128;

/// The space guaranteed in version 4 for every step row, line separators and settings.
pub const LOOKS_ROOM: usize = LOOKS * (STEP_MOST + 6) + RULES_MOST;

fn fits(patch: &Patch, looks: &mut dyn Iterator<Item = &SavedLook>) -> bool {
    let Some(room) = parts(patch).and_then(|parts| LIMIT.most.checked_sub(parts.len())) else {
        return false;
    };
    // Most patches have room for all the longest sequences without measuring a row.
    if room >= LOOKS * (LOOK_MOST - STEP_MOST) {
        return true;
    }
    // Even four swatches and two fully escaped names fit a step's guaranteed room.
    looks
        .filter(|look| matches!(look.definition, engine_core::Definition::Sequence(_)))
        .try_fold(room, |left, look| {
            left.checked_sub(line(look)?.len().saturating_sub(STEP_MOST))
        })
        .is_some()
}

/// The text of a definition as the file has it, under the key of its mode, indented to sit
/// in the file.
fn encode(maker: &str, model: &str, mode: &Mode) -> Option<Arc<str>> {
    let text = serde_json::to_string_pretty(&Definition { maker, model, mode }).ok()?;
    Some(Arc::from(text.replace('\n', "\n    ")))
}

/// The parts of a file: its lines and its modes' texts, before they are joined.
struct Parts {
    rows: Vec<String>,
    groups: Vec<String>,
    looks: Vec<String>,
    /// The Mixer's settings, one line: the defaults while only the patch is measured.
    rules: String,
    /// Each different mode, sorted by key.
    modes: Vec<Mode3>,
}

/// A mode's key, the key as a line of JSON, and its text.
type Mode3 = (String, String, Arc<str>);

impl Parts {
    const fn join_len(count: usize, sum: usize) -> usize {
        if count == 0 {
            2
        } else {
            sum + 6 * (count - 1) + 10
        }
    }

    /// The length of the file: the same as `self.text().len()`, without making it.
    fn len(&self) -> usize {
        let base = Self::frame("", "", "", "", "").len();
        base + self.rules.len()
            + Self::join_len(
                self.rows.len(),
                self.rows.iter().map(String::len).sum::<usize>(),
            )
            + Self::join_len(
                self.groups.len(),
                self.groups.iter().map(String::len).sum::<usize>(),
            )
            + Self::join_len(
                self.looks.len(),
                self.looks.iter().map(String::len).sum::<usize>(),
            )
            + Self::join_len(
                self.modes.len(),
                self.modes
                    .iter()
                    .map(|(_, k, t)| k.len() + 2 + t.len())
                    .sum::<usize>(),
            )
    }

    fn frame(patch: &str, groups: &str, looks: &str, rules: &str, modes: &str) -> String {
        format!(
            "{{\n  \"version\": {VERSION},\n  \"patch\": {patch},\n  \"groups\": {groups},\n  \"looks\": {looks},\n  \"rules\": {rules},\n  \"modes\": {modes}\n}}\n",
        )
    }

    fn text(&self) -> String {
        let modes: Vec<String> = self
            .modes
            .iter()
            .map(|(_, k, t)| format!("{k}: {t}"))
            .collect();
        Self::frame(
            &list(&self.rows, '[', ']'),
            &list(&self.groups, '[', ']'),
            &list(&self.looks, '[', ']'),
            &self.rules,
            &list(&modes, '{', '}'),
        )
    }
}

/// The text of `patch` and `looks`: its fixtures, their modes, its groups and the saved
/// looks. `None` when it would be over [`MOST_BYTES`]: that file would not open.
pub fn write(patch: &Patch, looks: &Looks) -> Option<String> {
    let mut parts = parts(patch)?;
    parts.looks = looks.rows().map(line).collect::<Option<Vec<_>>>()?;
    parts.rules = line(&looks.rules())?;
    (parts.len() <= MOST_BYTES).then(|| parts.text())
}

fn parts(patch: &Patch) -> Option<Parts> {
    // each different mode under its own key, in the order the fixtures name them
    let mut modes: Vec<Mode3> = Vec::new();
    let mut rows = Vec::new();
    for fixture in patch.fixtures() {
        let text = match patch.definition_text(fixture.fixture) {
            Some(text) => Arc::clone(text),
            None => encode(fixture.maker, fixture.model, patch.mode(fixture.fixture)?)?,
        };
        let same = modes
            .iter()
            .find(|(_, _, t)| Arc::ptr_eq(t, &text) || **t == *text)
            .map(|(key, ..)| key.clone());
        let key = match same {
            Some(key) => key,
            None => {
                let key = free_key(&modes, fixture.maker, fixture.model, fixture.mode);
                modes.push((key.clone(), line(&key)?, text));
                key
            }
        };
        rows.push(line(&Row {
            fixture: fixture.fixture,
            name: fixture.name,
            mode: &key,
            at: At {
                universe: fixture.universe,
                address: fixture.address,
            },
            first: fixture.first,
        })?);
    }
    let groups = patch
        .groups()
        .iter()
        .map(|g| {
            line(&Group {
                name: g.name,
                members: g.members,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    modes.sort_unstable_by(|(a, ..), (b, ..)| a.cmp(b));
    Some(Parts {
        rows,
        groups,
        looks: Vec::new(),
        rules: line(&MixRules::default())?,
        modes,
    })
}

/// The patch and the looks of `text`: `empty` with the file's fixtures and groups, and
/// the file's looks, each checked as `PutStep` checks it, or why not. Two looks of one
/// number: `LookInUse`. `empty`
/// is the engine's patch with nothing in it, so the file is checked against its
/// universes and lights. A file whose patch would be written back over [`MOST_BYTES`]
/// (one with less space in it than this engine writes) is too big too: it could not be
/// saved.
pub fn read(text: &str, empty: &Patch) -> Result<(Patch, Looks), Unopened> {
    let too_big = Unopened::TooBig {
        most: u32::try_from(MOST_BYTES).unwrap_or(u32::MAX),
    };
    if text.len() > MOST_BYTES {
        return Err(too_big);
    }
    let (version, look_count) = match serde_json::from_str::<Counts>(text) {
        Ok(head) => (head.version, head.looks),
        Err(error) => {
            // A newer version may give "looks" an entirely different shape.
            let Head { version } = serde_json::from_str(text).map_err(broken)?;
            if version.get() <= VERSION {
                return Err(broken(error));
            }
            (version, 0)
        }
    };
    if version.get() > VERSION {
        return Err(Unopened::Newer {
            version: version.get(),
        });
    }
    if look_count > LOOKS {
        return Err(Unopened::Refused {
            reason: Refusal::TooManyLooks { most: LOOKS as u16 },
        });
    }
    let file = if version.get() < 4 {
        serde_json::from_str::<File<Before4>>(text)
            .map_err(broken)?
            .with_looks(|old| {
                let (saved, gobo) = old.now();
                (saved.into(), gobo.map(|one| (saved.step.target, one)))
            })
    } else if version.get() == 4 {
        serde_json::from_str::<File<Saved>>(text)
            .map_err(broken)?
            .with_looks(|saved| (saved.into(), None))
    } else if version.get() == 5 {
        serde_json::from_str::<File<Before6>>(text)
            .map_err(broken)?
            .with_looks(|old| (old.0, None))
    } else {
        serde_json::from_str::<File<SavedLook>>(text)
            .map_err(broken)?
            .with_looks(|saved| (saved, None))
    };
    let File {
        version: _,
        patch,
        groups,
        looks: saved,
        rules,
        modes,
    } = file;
    let placed = patch
        .into_iter()
        .map(|row| {
            let definition = modes.get(&row.mode).ok_or(Unopened::Refused {
                reason: Refusal::NoSuchMode,
            })?;
            Ok(Placed {
                fixture: row.fixture,
                name: row.name.as_str().to_string(),
                maker: definition.maker.as_str().to_string(),
                model: definition.model.as_str().to_string(),
                mode: definition.mode.clone(),
                at: row.at,
                first: row.first,
            })
        })
        .collect::<Result<Vec<_>, Unopened>>()?;
    let mut patch = empty.restore(placed).map_err(|(fixture, why)| match why {
        Unplaced::Taken => Unopened::Taken { fixture },
        Unplaced::Refused(reason) => Unopened::Refused { reason },
    })?;
    for group in groups {
        let set = PatchCommand::SetGroup {
            name: group.name,
            members: group.members,
        };
        let (changed, _) = patch
            .update(set, &[])
            .map_err(|reason| Unopened::Refused { reason })?;
        if let Some(changed) = changed {
            patch = changed;
        }
    }
    let refused = |reason| Unopened::Refused { reason };
    let mut rows = Vec::with_capacity(saved.len());
    for (row, legacy) in saved {
        if let Some((target, OneGobo { wheel, slot })) = legacy {
            let runs = patch.runs(target).map_err(refused)?;
            rig::shows_one_gobo(patch.writers(), &runs, wheel, slot).map_err(refused)?;
        }
        rows.push(row);
    }
    let looks = Looks::restore(rows, rules, &patch).map_err(refused)?;
    write(&patch, &looks).map(|_| (patch, looks)).ok_or(too_big)
}

/// V5 accepted sequences and plain steps. Both decode into the current model.
#[derive(Deserialize)]
struct Before6(#[serde(deserialize_with = "SavedLook::deserialize_v5")] SavedLook);

/// A look of a file before version 4: one `gobo`, a wheel and a slot, read as that
/// engine read it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Before4 {
    look: u16,
    name: Text,
    step: StepBefore4,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepBefore4 {
    target: Target,
    values: LightBefore4,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LightBefore4 {
    #[serde(default)]
    intensity: Option<u16>,
    #[serde(default)]
    colour: Option<Rgb>,
    #[serde(default)]
    position: Option<Position>,
    #[serde(default)]
    zoom: Option<u16>,
    #[serde(default)]
    gobo: Option<OneGobo>,
}

/// The one gobo of a look before version 4.
#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct OneGobo {
    wheel: u8,
    slot: u8,
}

impl Before4 {
    /// The look now, its gobo as the slot of its wheel, and the gobo to check as that
    /// engine checked it. A gobo no wheel can hold now (a wheel past [`Wheel::COUNT`],
    /// slot 0) is left out: the check refuses it on a light with a gobo wheel.
    fn now(self) -> (Saved, Option<OneGobo>) {
        let LightBefore4 {
            intensity,
            colour,
            position,
            zoom,
            gobo,
        } = self.step.values;
        let mut gobos = [None; Wheel::COUNT];
        if let Some(OneGobo { wheel, slot }) = gobo
            && slot > 0
            && let Some(held) = Wheel::new(wheel).and_then(|wheel| gobos.get_mut(wheel.index()))
        {
            *held = Some(slot);
        }
        let values = Light {
            intensity,
            colour,
            position,
            zoom,
            gobos,
        };
        let step = Step {
            target: self.step.target,
            values,
        };
        let saved = Saved {
            look: self.look,
            name: self.name,
            step,
        };
        (saved, gobo)
    }
}

/// A fixture as the file has it.
#[derive(Serialize)]
struct Row<'a> {
    fixture: u16,
    name: &'a str,
    mode: &'a str,
    at: At,
    first: u16,
}

#[derive(Serialize)]
struct At {
    universe: u16,
    address: u16,
}

#[derive(Serialize)]
struct Group<'a> {
    name: &'a str,
    members: &'a [Member],
}

/// A mode as the file keeps it, with its fixture's maker and model.
#[derive(Serialize, PartialEq, Eq)]
struct Definition<'a> {
    maker: &'a str,
    model: &'a str,
    mode: &'a Mode,
}

/// Only the version: read before the rest, so a newer file is told apart from a broken one.
#[derive(Deserialize)]
struct Head {
    version: NonZeroU32,
}

/// Count the rows while inspecting the version. All other fields are skipped during
/// this pass, so an oversized look list never materializes a typed project.
#[derive(Deserialize)]
struct Counts {
    version: NonZeroU32,
    #[serde(default, deserialize_with = "count_looks")]
    looks: usize,
}

fn count_looks<'de, D: Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
    struct Count;
    impl<'de> Visitor<'de> for Count {
        type Value = usize;
        fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("an array of saved looks")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut rows: A) -> Result<usize, A::Error> {
            let mut count = 0;
            while rows.next_element::<IgnoredAny>()?.is_some() {
                count += 1;
            }
            Ok(count)
        }
    }
    d.deserialize_seq(Count)
}

/// The whole file as it is read, each look an `L`. A key it does not know: the file is
/// broken.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File<L> {
    /// Read first, by `Head`: here so the key is known.
    version: NonZeroU32,
    patch: Vec<RowIn>,
    groups: Vec<GroupIn>,
    /// Version 1 has none.
    #[serde(default = "Vec::new")]
    looks: Vec<L>,
    /// Versions 1 and 2 have none: the defaults.
    #[serde(default)]
    rules: MixRules,
    modes: BTreeMap<String, DefinitionIn>,
}

impl<L> File<L> {
    /// The file with each look made by `now`.
    fn with_looks<M>(self, now: impl Fn(L) -> M) -> File<M> {
        File {
            version: self.version,
            patch: self.patch,
            groups: self.groups,
            looks: self.looks.into_iter().map(now).collect(),
            rules: self.rules,
            modes: self.modes,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RowIn {
    fixture: NonZeroU16,
    name: Text,
    mode: String,
    at: ChannelNumbers,
    first: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupIn {
    name: Text,
    members: Members,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefinitionIn {
    maker: Text,
    model: Text,
    mode: Mode,
}

/// `maker / model / mode`, with ` (2)`, ` (3)` and on when another mode has it: the
/// library may have changed between two patches of one fixture.
fn free_key(modes: &[Mode3], maker: &str, model: &str, mode: &str) -> String {
    let base = format!("{maker} / {model} / {mode}");
    let mut key = base.clone();
    let mut n = 2_u32;
    while modes.iter().any(|(k, ..)| *k == key) {
        key = format!("{base} ({n})");
        n = n.saturating_add(1);
    }
    key
}

/// `value` as JSON on one line.
fn line<T: Serialize + ?Sized>(value: &T) -> Option<String> {
    serde_json::to_string(value).ok()
}

/// `lines` between `open` and `close`, one per line, or `[]` when there are none.
fn list(lines: &[String], open: char, close: char) -> String {
    if lines.is_empty() {
        return format!("{open}{close}");
    }
    format!("{open}\n    {}\n  {close}", lines.join(",\n    "))
}

/// Where the text stopped being a show file.
fn broken(error: serde_json::Error) -> Unopened {
    Unopened::Broken {
        line: u32::try_from(error.line()).unwrap_or(u32::MAX),
        column: u32::try_from(error.column()).unwrap_or(u32::MAX),
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "each checked fixture and canonical file is required by the test"
)]
mod sequence_tests {
    use super::*;
    use engine_core::{Color, Lights, Pixels, Universes};
    use rig::Whole;

    #[test]
    fn the_reader_stops_decoding_looks_at_the_count_limit() {
        let empty = Patch::new(
            Universes::new(1).unwrap(),
            Lights::new(8).unwrap(),
            Whole(|_| NonZeroU16::MIN),
        );
        let file = write(&empty, &Looks::new()).unwrap();
        // The next element is valid JSON, but could never deserialize as a look.
        // Only a reader that stops decoding at the limit can report TooManyLooks.
        for count in [LOOKS - 1, LOOKS, LOOKS + 1] {
            let mut rows: Vec<_> = (1..=count)
                .map(|id| format!(r#"{{"look":{id},"name":"rest","step":{{"target":{{"Light":{{"light":1}}}},"values":{{}}}}}}"#))
                .collect();
            rows.push("null".into());
            let text = file.replace(
                r#""looks": []"#,
                &format!(r#""looks": [{}]"#, rows.join(",")),
            );
            let result = read(&text, &empty);
            if count < LOOKS {
                assert!(matches!(result, Err(Unopened::Broken { .. })));
            } else {
                assert_eq!(
                    result,
                    Err(Unopened::Refused {
                        reason: Refusal::TooManyLooks { most: LOOKS as u16 }
                    })
                );
                // Count before decoding any other part of an oversized project.
                let invalid_modes = text.replace(r#""modes": {}"#, r#""modes": false"#);
                assert_eq!(read(&invalid_modes, &empty), result);
            }
        }
        let newer = VERSION + 1;
        let many = alloc::vec!["null"; LOOKS + 1].join(",");
        for looks in [format!("[{many}]"), r#"{"future":"shape"}"#.into()] {
            let future = format!(r#"{{"looks":{looks},"version":{newer}}}"#);
            assert_eq!(
                read(&future, &empty),
                Err(Unopened::Newer { version: newer })
            );
        }
    }

    #[test]
    fn a_saved_row_with_two_definition_keys_is_broken_even_if_one_is_null() {
        let empty = Patch::new(
            Universes::new(1).unwrap(),
            Lights::new(8).unwrap(),
            Whole(|_| NonZeroU16::MIN),
        );
        let file = write(&empty, &Looks::new()).unwrap();
        for fields in [
            r#""step":{"target":{"Light":{"light":1}},"values":{}},"sequence":null"#,
            r#""step":null,"sequence":{"items":[{"look":3}]}"#,
        ] {
            let row = format!(r#""looks": [{{"look":1,"name":"two",{fields}}}]"#);
            let both = file.replace(r#""looks": []"#, &row);
            assert!(
                matches!(read(&both, &empty), Err(Unopened::Broken { .. })),
                "{fields}"
            );
        }
    }

    #[test]
    fn four_swatches_fit_the_space_guaranteed_in_existing_files() {
        use engine_core::{AuthoredStep, Definition, Palette, Pattern, Swatches, WaveTarget};
        let name = Text::new(&"\u{1}".repeat(Text::MOST)).unwrap();
        let white = Rgb {
            red: u16::MAX,
            green: u16::MAX,
            blue: u16::MAX,
        };
        let swatches = Swatches::new(&[white; Swatches::MOST]).unwrap();
        for definition in [
            Definition::Step(AuthoredStep {
                target: Target::Group { group: name },
                values: Light {
                    intensity: Some(u16::MAX),
                    colour: Some(white),
                    zoom: Some(u16::MAX),
                    position: Some(Position {
                        pan: u16::MAX,
                        tilt: u16::MAX,
                    }),
                    gobos: [Some(u8::MAX); Wheel::COUNT],
                },
                swatches,
            }),
            Definition::Pattern(Pattern::Wave {
                target: WaveTarget::Group { group: name },
                period: 32,
                palette: Palette::Inline(swatches),
            }),
        ] {
            let row = line(&SavedLook {
                look: u16::MAX,
                name,
                definition,
            })
            .unwrap();
            assert!(row.len() <= STEP_MOST, "{} bytes", row.len());
        }
    }

    #[test]
    fn the_longest_sequence_fits_its_reserved_room() {
        use engine_core::{Definition, Repeat, SequenceItem, Speed, Use};
        let item = SequenceItem {
            look: u16::MAX,
            width: u16::MAX,
            transition: 100,
            usage: Use {
                speed: Speed::Double,
                repeat: Repeat::Once,
                spread: 100,
            },
        };
        let saved = SavedLook {
            look: u16::MAX,
            name: Text::new(&"\u{1}".repeat(Text::MOST)).unwrap(),
            definition: Definition::Sequence(Sequence::new(&[item; Sequence::MOST]).unwrap()),
        };
        let line = serde_json::to_string(&saved).unwrap();
        assert!(line.len() <= LOOK_MOST, "{} bytes", line.len());
    }

    #[test]
    #[expect(
        clippy::indexing_slicing,
        reason = "the JSON fixture uses named fields"
    )]
    fn waves_and_swatches_first_open_in_v6_and_round_trip_together() {
        let empty = Patch::new(
            Universes::new(1).unwrap(),
            Lights::new(8).unwrap(),
            Whole(|_| NonZeroU16::MIN),
        );
        // A one-cell RGB strip. Parent rows occur before both of their children.
        let patch = empty
            .update(
                PatchCommand::PatchStrip {
                    name: Text::new("cell").unwrap(),
                    pixels: Pixels::try_from(1).unwrap(),
                    color: Color::Rgb,
                    at: ChannelNumbers {
                        universe: 1,
                        address: 1,
                    },
                },
                &[],
            )
            .unwrap()
            .0
            .unwrap();
        let mut file: serde_json::Value =
            serde_json::from_str(&write(&patch, &Looks::new()).unwrap()).unwrap();
        file["groups"] =
            serde_json::json!([{"name":"front","members":[{"Fixture":{"fixture":1}}]}]);
        let step = serde_json::json!({"look":3,"name":"red","step":{
            "target":{"Light":{"light":1}},"values":{},"swatches":[{"red":65535,"green":0,"blue":0}]
        }});
        let wave = serde_json::json!({"look":2,"name":"wave","pattern":{"Wave":{
            "target":{"Group":{"group":"front"}},"period":2,"palette":{"Step":{"look":3}}
        }}});
        let parent =
            serde_json::json!({"look":1,"name":"parent","sequence":{"items":[{"look":2}]}});

        let inline = serde_json::json!({"look":2,"name":"inline","pattern":{"Wave":{
            "target":{"Group":{"group":"front"}},"period":1,"palette":{"Inline":[{"red":1,"green":2,"blue":3}]}
        }}});
        for looks in [
            serde_json::json!([step]),
            serde_json::json!([inline]),
            serde_json::json!([parent, wave, step]),
        ] {
            file["looks"] = looks;
            for version in [4, 5, 6] {
                file["version"] = version.into();
                let opened = read(&file.to_string(), &empty);
                if version == 6 {
                    let (rig, looks) = opened.unwrap();
                    let canonical = write(&rig, &looks).unwrap();
                    assert!(canonical.contains(r#""version": 6"#));
                    assert_eq!(read(&canonical, &empty), Ok((rig, looks)));
                } else {
                    assert!(matches!(opened, Err(Unopened::Broken { .. })), "v{version}");
                }
            }
        }
    }

    #[test]
    fn saved_sequences_can_refer_to_later_rows() {
        let empty = Patch::new(
            Universes::new(1).unwrap(),
            Lights::new(8).unwrap(),
            Whole(|_| NonZeroU16::MIN),
        );
        let patch = empty
            .update(
                PatchCommand::PatchStrip {
                    name: Text::new("PAR").unwrap(),
                    pixels: Pixels::try_from(1).unwrap(),
                    color: Color::Rgb,
                    at: ChannelNumbers {
                        universe: 1,
                        address: 1,
                    },
                },
                &[],
            )
            .unwrap()
            .0
            .unwrap();
        let step = Saved {
            look: 9,
            name: Text::new("rest").unwrap(),
            step: Step {
                target: Target::Fixture { fixture: 1 },
                values: Light::DARK,
            },
        };
        let step_file = write(&patch, &Looks::new().put(step, &patch).unwrap()).unwrap();
        let sequence = r#"{"look":1,"name":"parent","sequence":{"items":[{"look":9}]}}"#;
        let whole = step_file.replace("\"looks\": [", &format!("\"looks\": [\n    {sequence},"));
        let (_, read_back) = read(&whole, &empty).unwrap();
        assert_eq!(read_back.all().len(), 2);
        let canonical = write(&patch, &read_back).unwrap();
        assert_eq!(read(&canonical, &empty), Ok((patch, read_back)));
    }
}
