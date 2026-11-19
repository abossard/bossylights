//! Saved looks, off the tick thread: each checked against the patch when saved, and
//! compiled into a plan when it plays. A look is a step for now (`docs/design/looks-slice-3.md`).
//!
//! ```text
//! PutStep --check target--> Looks --compile against the patch--> plan (entries) --> tick
//! ```
//!
//! | A change | Refused when |
//! |---|---|
//! | save a step | its target names nothing patched or an empty group, or [`LOOKS`] are saved |
//! | change the patch or a group | a saved look's target would name nothing or no light, or a play's lights would change |
//! | remove a look | a play uses it |

#![no_std]
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use engine_core::{Entry, LOOKS, Refusal, Saved, Step};
use rig::Patch;

/// The saved looks, by number. A change makes new looks: a reader keeps the ones it has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Looks(BTreeMap<u16, Saved>);

impl Looks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every saved look, by number.
    pub fn all(&self) -> Vec<Saved> {
        self.0.values().copied().collect()
    }

    pub fn get(&self, look: u16) -> Option<&Saved> {
        self.0.get(&look)
    }

    /// The looks with `saved` in place of the one of its number. Refused when its target
    /// names nothing in `patch`, or a new number would make more than [`LOOKS`].
    pub fn put(&self, saved: Saved, patch: &Patch) -> Result<Self, Refusal> {
        if !self.0.contains_key(&saved.look) && self.0.len() >= LOOKS {
            return Err(Refusal::TooManyLooks {
                most: u16::try_from(LOOKS).unwrap_or(u16::MAX),
            });
        }
        compile(&saved.step, patch)?;
        let mut looks = self.0.clone();
        looks.insert(saved.look, saved);
        Ok(Self(looks))
    }

    /// The looks without `look`. The caller checks no play uses it.
    pub fn remove(&self, look: u16) -> Self {
        let mut looks = self.0.clone();
        looks.remove(&look);
        Self(looks)
    }

    /// Whether every saved look still names something in `patch`: `LookInUse` names the
    /// first that would not.
    pub fn check(&self, patch: &Patch) -> Result<(), Refusal> {
        match self
            .0
            .values()
            .find(|saved| compile(&saved.step, patch).is_err())
        {
            Some(saved) => Err(Refusal::LookInUse { look: saved.look }),
            None => Ok(()),
        }
    }
}

/// The plan of `step` in `patch`: its values on each run of lights its target names.
/// `EmptyGroup` when it names no light.
pub fn compile(step: &Step, patch: &Patch) -> Result<Box<[Entry]>, Refusal> {
    let runs = patch.runs(step.target)?;
    if runs.is_empty() {
        return Err(Refusal::EmptyGroup);
    }
    rig::supports(patch.writers(), &runs, &step.values)?;
    Ok(runs
        .into_iter()
        .map(|lights| Entry {
            lights,
            values: step.values,
        })
        .collect())
}
