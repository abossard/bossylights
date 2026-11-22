//! The show file as the picture shows it: whether the engine keeps its patch and groups
//! in one, and why not. A state machine: [`ShowFile::saved`] holds every transition.
//!
//! | State | A change of the patch | Then, saved whole | Then, not |
//! |---|---|---|---|
//! | `None` (no `--show`) | not saved | | |
//! | `Kept` | saved | `Kept` | `NotSaved` |
//! | `NotSaved` | saved | `Kept` | `NotSaved` |
//! | `LeftAlone` | not saved: the file is never written | | |

use crate::Refusal;
use serde::Serialize;

/// Whether the engine keeps its patch in a show file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum ShowFile {
    /// The engine was started without one: nothing is kept.
    None,
    /// The file holds the patch and the groups, or gets them with the save under way.
    Kept,
    /// The last save failed: the file holds an older patch, or the new one not yet safe
    /// from a power cut. The next change saves again.
    NotSaved,
    /// The file was not opened: the engine started with an empty patch and never writes it.
    LeftAlone { why: Unopened },
}

/// Why a show file was not opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Unopened {
    /// It is there, but could not be read.
    CannotRead,
    /// Not a show file: not JSON, or not in its shape. Where it went wrong, from 1.
    Broken { line: u32, column: u32 },
    /// Written by a newer engine, in this `version` of the file.
    Newer { version: u32 },
    /// Written by an older engine, in this `version` of the file: not read.
    Older { version: u32 },
    /// The fixture has the number or one of the lights of another fixture.
    Taken { fixture: u16 },
    /// Another saved look has this number.
    LookTaken { look: u16 },
    /// A fixture or a group of the file is refused as its command would be.
    Refused { reason: Refusal },
    /// Longer than the `most` bytes a show file has.
    TooBig { most: u32 },
}

impl ShowFile {
    /// The state after a save: `whole` when the file was written whole.
    pub const fn saved(self, whole: bool) -> Self {
        match self {
            Self::Kept | Self::NotSaved if whole => Self::Kept,
            Self::Kept | Self::NotSaved => Self::NotSaved,
            Self::None | Self::LeftAlone { .. } => self,
        }
    }

    /// Whether a change of the patch is saved.
    pub const fn saves(self) -> bool {
        match self {
            Self::Kept | Self::NotSaved => true,
            Self::None | Self::LeftAlone { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_moves_a_kept_file_between_kept_and_not_saved_and_nothing_else() {
        let left = ShowFile::LeftAlone {
            why: Unopened::Newer { version: 2 },
        };
        // (state, saves, saved whole, saved not whole)
        let cases = [
            (ShowFile::None, false, ShowFile::None, ShowFile::None),
            (ShowFile::Kept, true, ShowFile::Kept, ShowFile::NotSaved),
            (ShowFile::NotSaved, true, ShowFile::Kept, ShowFile::NotSaved),
            (left, false, left, left),
        ];
        for (state, saves, whole, broken) in cases {
            assert_eq!(state.saves(), saves, "{state:?} saves");
            assert_eq!(state.saved(true), whole, "{state:?} saved whole");
            assert_eq!(state.saved(false), broken, "{state:?} not saved");
        }
    }
}
