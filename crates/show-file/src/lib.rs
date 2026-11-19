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
//!   "version": 4,
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
//! | `looks` | each saved look by number: its name and its step, the slot of each gobo wheel in `gobos`. Version 1 has none. Versions 2 and 3 hold one `gobo` of a wheel and a slot: read as that wheel's slot, a wheel no light has (past 3) left out, as it showed nothing |
//! | `rules` | the Mixer's three settings. Versions 1 and 2 have none: the defaults |
//! | `modes` | each mode a fixture is patched in, with its maker and model, by key |
//!
//! Reading checks what the commands check: a fixture is placed as patch places it, a
//! group is set as set group sets it. A file that fails a check is not read at all.
//!
//! A file is at most [`MOST_BYTES`] long: one over it is not read, and neither is one
//! whose patch and looks would be written back over it. A patch kept in a file ([`LIMIT`])
//! refuses the command that would take its file over it, beside the room every look may
//! take ([`LOOKS_ROOM`]), so every save writes, and the file on disk always opens.

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
use alloc::vec;
use alloc::vec::Vec;
use core::num::{NonZeroU16, NonZeroU32};
use engine_core::{
    ChannelNumbers, LOOKS, Member, Members, MixRules, PatchCommand, Refusal, Saved, Text, Unopened,
    Wheel,
};
use fixture_library::Mode;
use looks::Looks;
use rig::{Limit, Patch, Placed, Unplaced};
use serde::{Deserialize, Serialize};

/// The version of the file this engine writes and reads. Raise it with every change an
/// engine of the old version would read wrong. An engine never reads a newer file.
pub const VERSION: u32 = 4;

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
    fits: |patch| parts(patch).is_some_and(|parts| parts.len() <= MOST_BYTES - LOOKS_ROOM),
    encode,
};

/// The longest line of a look: a name and a group of [`Text::MOST`] bytes, each byte
/// escaped, and every property given.
pub const LOOK_MOST: usize = 2048;

/// How much longer the line of the Mixer's settings may be than the defaults' line,
/// which a patch is measured with.
pub const RULES_MOST: usize = 128;

/// The room the looks and the Mixer's settings may take: [`LOOKS`] of [`LOOK_MOST`]
/// bytes and [`RULES_MOST`]. A patch leaves it.
pub const LOOKS_ROOM: usize = LOOKS * (LOOK_MOST + 6) + RULES_MOST;

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
    parts.looks = looks.all().iter().map(line).collect::<Option<Vec<_>>>()?;
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
    let Head { version } = serde_json::from_str(text).map_err(broken)?;
    if version.get() > VERSION {
        return Err(Unopened::Newer {
            version: version.get(),
        });
    }
    let file = if version.get() < 4 {
        let mut file: serde_json::Value = serde_json::from_str(text).map_err(broken)?;
        one_gobo_per_wheel(&mut file);
        // broken after all: where the text is
        serde_json::from_value(file)
            .or_else(|_| serde_json::from_str(text))
            .map_err(broken)?
    } else {
        serde_json::from_str(text).map_err(broken)?
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
    let mut looks = Looks::new().with_rules(rules);
    for saved in saved {
        let refused = |reason| Unopened::Refused { reason };
        if looks.get(saved.look).is_some() {
            return Err(refused(Refusal::LookInUse { look: saved.look }));
        }
        looks = looks.put(saved, &patch).map_err(refused)?;
    }
    write(&patch, &looks).map(|_| (patch, looks)).ok_or(too_big)
}

/// The looks of a file before version 4, each `"gobo": {"wheel": 1, "slot": 3}` as
/// `"gobos": [3, null, null]`. A wheel past [`Wheel::COUNT`] no light has: left out. A
/// `gobo` that is not a wheel and a slot stays, so the file is broken as it was.
fn one_gobo_per_wheel(file: &mut serde_json::Value) {
    /// A gobo as versions 2 and 3 have it.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct OneGobo {
        wheel: u8,
        slot: u8,
    }
    let Some(looks) = file
        .get_mut("looks")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    let values = looks
        .iter_mut()
        .filter_map(|look| look.pointer_mut("/step/values"))
        .filter_map(serde_json::Value::as_object_mut);
    for values in values {
        let Some(gobo) = values.remove("gobo") else {
            continue;
        };
        let Ok(one) = serde_json::from_value::<Option<OneGobo>>(gobo.clone()) else {
            values.insert("gobo".to_string(), gobo);
            continue;
        };
        let mut gobos = vec![serde_json::Value::Null; Wheel::COUNT];
        if let Some(OneGobo { wheel, slot }) = one
            && let Some(held) = Wheel::new(wheel).and_then(|wheel| gobos.get_mut(wheel.index()))
        {
            *held = slot.into();
        }
        values.insert("gobos".to_string(), gobos.into());
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

/// The whole file as it is read. A key it does not know: the file is broken.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[expect(dead_code, reason = "read first, by `Head`: here so the key is known")]
    version: NonZeroU32,
    patch: Vec<RowIn>,
    groups: Vec<GroupIn>,
    /// Version 1 has none.
    #[serde(default)]
    looks: Vec<Saved>,
    /// Versions 1 and 2 have none: the defaults.
    #[serde(default)]
    rules: MixRules,
    modes: BTreeMap<String, DefinitionIn>,
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
