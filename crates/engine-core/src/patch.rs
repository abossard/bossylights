//! The patch as the API shows it: the commands the patch checks, and a fixture as the
//! picture shows it. The patch itself has names and lists, so it lives off the tick thread
//! (`crates/rig`); this file says what goes in and out.
//!
//! ```text
//! Command --Project::update--> PatchCommand --rig: Patch::update--> new patch, writers,
//!                                                                    EngineCommand
//! ```

use crate::{ChannelNumbers, Text};
use serde::{Deserialize, Serialize};

/// What a light command names: one light, or every light of a fixture. Counted from 1,
/// as a client sends them. Not checked yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Target {
    Light { light: u16 },
    Fixture { fixture: u16 },
}

/// A client's command that the patch checks: it changes the patch, or names a light the
/// patch has. Made from a [`crate::Command`] by [`crate::Project::update`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a client's command has no heap: `Patch` carries its four names in place. Moved once per request, off the tick thread"
)]
pub enum PatchCommand {
    Patch {
        name: Text,
        maker: Text,
        model: Text,
        mode: Text,
        at: ChannelNumbers,
    },
    Move {
        fixture: u16,
        at: ChannelNumbers,
    },
    Unpatch {
        fixture: u16,
    },
    Rename {
        fixture: u16,
        name: Text,
    },
    SetIntensity {
        target: Target,
        intensity: u16,
    },
    SetColour {
        target: Target,
        red: u16,
        green: u16,
        blue: u16,
    },
    ReleaseIntensity {
        target: Target,
    },
    ReleaseColour {
        target: Target,
    },
}

/// A patched fixture, as the picture shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Patched<'a> {
    /// Its number, from 1. Kept while it is patched.
    pub fixture: u16,
    pub name: &'a str,
    pub maker: &'a str,
    pub model: &'a str,
    pub mode: &'a str,
    /// Its first channel.
    pub universe: u16,
    pub address: u16,
    /// How many channels it takes from its address on.
    pub channels: u16,
    /// Its lights, numbered from `first` (counted from 1).
    pub first: u16,
    pub lights: u16,
    /// The fixtures that share at least one of its channels. Allowed: the patch shows it.
    pub clashes: &'a [u16],
}
