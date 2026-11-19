//! The show file: the patch and the groups as JSON text, and back. A calculation: the
//! engine process reads and writes the file (`engine-server`).
//!
//! ```text
//! patch --write--> text
//! text + an empty patch --read--> the patch with its fixtures and groups | Unopened
//! ```
//!
//! What the text looks like. One line per fixture and per group, so moving one fixture
//! changes one line. Each mode is kept whole, so a show opens without the library.
//!
//! ```text
//! {
//!   "version": 1,
//!   "patch": [
//!     {"fixture":1,"name":"PAR 1","mode":"Generic / Generic RGB / RGB","at":{"universe":1,"address":37},"first":1}
//!   ],
//!   "groups": [
//!     {"name":"front","members":[{"Fixture":{"fixture":1}}]}
//!   ],
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
//! | `modes` | each mode a fixture is patched in, with its maker and model, by key |
//!
//! Reading checks what the commands check: a fixture is placed as patch places it, a
//! group is set as set group sets it. A file that fails a check is not read at all.
//!
//! A file is at most [`MOST_BYTES`] long: one over it is not read, and neither is one
//! whose patch would be written back over it. A patch kept in a file ([`LIMIT`]) refuses
//! the command that would take its file over it, so every save writes, and the file on
//! disk always opens.

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
use alloc::vec::Vec;
use core::num::{NonZeroU16, NonZeroU32};
use engine_core::{ChannelNumbers, Member, Members, PatchCommand, Refusal, Text, Unopened};
use fixture_library::Mode;
use rig::{Limit, Patch, Placed, Unplaced};
use serde::{Deserialize, Serialize};

/// The version of the file this engine writes and reads. Raise it with every change an
/// engine of the old version would read wrong. An engine never reads a newer file.
pub const VERSION: u32 = 1;

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
/// whose file would be over [`MOST_BYTES`] is refused. Each change writes the whole file
/// once to measure it, off the tick thread, as the save after it does.
pub const LIMIT: Limit = Limit {
    most: MOST_BYTES,
    fits: |patch| write(patch).is_some(),
};

/// The text of `patch`: its fixtures, their modes and its groups. `None` when it would
/// be over [`MOST_BYTES`]: that file would not open.
pub fn write(patch: &Patch) -> Option<String> {
    // each different mode under its own key, in the order the fixtures name them
    let mut modes: Vec<(String, Definition<'_>)> = Vec::new();
    let mut rows = Vec::new();
    for fixture in patch.fixtures() {
        let mode = patch.mode(fixture.fixture)?;
        let definition = Definition {
            maker: fixture.maker,
            model: fixture.model,
            mode,
        };
        let key = match modes.iter().find(|(_, d)| *d == definition) {
            Some((key, _)) => key.clone(),
            None => {
                let key = free_key(&modes, &definition);
                modes.push((key.clone(), definition));
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
    modes.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
    let modes = modes
        .iter()
        .map(|(key, definition)| {
            let text = serde_json::to_string_pretty(definition).ok()?;
            Some(format!("{}: {}", line(key)?, text.replace('\n', "\n    ")))
        })
        .collect::<Option<Vec<_>>>()?;
    let text = format!(
        "{{\n  \"version\": {VERSION},\n  \"patch\": {},\n  \"groups\": {},\n  \"modes\": {}\n}}\n",
        list(&rows, '[', ']'),
        list(&groups, '[', ']'),
        list(&modes, '{', '}'),
    );
    (text.len() <= MOST_BYTES).then_some(text)
}

/// The patch of `text`: `empty` with the file's fixtures and groups, or why not. `empty`
/// is the engine's patch with nothing in it, so the file is checked against its
/// universes and lights. A file whose patch would be written back over [`MOST_BYTES`]
/// (one with less space in it than this engine writes) is too big too: it could not be
/// saved.
pub fn read(text: &str, empty: &Patch) -> Result<Patch, Unopened> {
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
    let File {
        version: _,
        patch,
        groups,
        modes,
    } = serde_json::from_str(text).map_err(broken)?;
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
    write(&patch).map(|_| patch).ok_or(too_big)
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
fn free_key(modes: &[(String, Definition<'_>)], definition: &Definition<'_>) -> String {
    let base = format!(
        "{} / {} / {}",
        definition.maker, definition.model, definition.mode.name
    );
    let mut key = base.clone();
    let mut n = 2_u32;
    while modes.iter().any(|(k, _)| *k == key) {
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
