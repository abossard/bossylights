//! The patch as the API shows it: the commands the patch checks, and a fixture as the
//! picture shows it. The patch itself has names and lists, so it lives off the tick thread
//! (`crates/rig`); this file says what goes in and out.
//!
//! ```text
//! Command --Project::update--> PatchCommand --rig: Patch::update--> new patch, writers,
//!                                                                    EngineCommand
//! ```

use crate::{ChannelNumbers, Color, Text};
use core::fmt;
use serde::de::{Deserializer, Error, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

/// What a light command names: one light, every light of a fixture, or every member of a
/// group. Counted from 1, as a client sends them. Not checked yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Target {
    Light {
        light: u16,
    },
    Fixture {
        fixture: u16,
    },
    Group {
        #[cfg_attr(test, ts(type = "string"))]
        group: Text,
    },
}

/// One member of a group: a light, or every light of a fixture. Counted from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Member {
    Light { light: u16 },
    Fixture { fixture: u16 },
}

/// The members of a group in their order, in place: at most [`Members::MOST`]. A client
/// sends them as a list; a longer one is not a command.
#[derive(Clone, Copy)]
pub struct Members {
    len: u16,
    members: [Member; Members::MOST],
}

impl Members {
    /// The most members of one group. A member that is a fixture counts once, however
    /// many lights it has.
    pub const MOST: usize = 512;

    /// `None` when there are more than [`Members::MOST`].
    pub fn new(members: &[Member]) -> Option<Self> {
        let mut all = [Member::Light { light: 0 }; Self::MOST];
        all.get_mut(..members.len())?.copy_from_slice(members);
        Some(Self {
            len: u16::try_from(members.len()).ok()?,
            members: all,
        })
    }

    pub fn as_slice(&self) -> &[Member] {
        self.members
            .get(..usize::from(self.len))
            .unwrap_or_default()
    }
}

impl PartialEq for Members {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for Members {}

impl fmt::Debug for Members {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_slice(), f)
    }
}

impl<'de> Deserialize<'de> for Members {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct List;
        impl<'de> Visitor<'de> for List {
            type Value = Members;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a list of at most {} members", Members::MOST)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Members, A::Error> {
                let mut members = Members::new(&[]).ok_or_else(|| A::Error::custom("empty"))?;
                while let Some(member) = seq.next_element::<Member>()? {
                    let slot = members
                        .members
                        .get_mut(usize::from(members.len))
                        .ok_or_else(|| A::Error::invalid_length(Members::MOST + 1, &self))?;
                    *slot = member;
                    members.len += 1;
                }
                Ok(members)
            }
        }
        deserializer.deserialize_seq(List)
    }
}

/// A client's command that the patch checks: it changes the patch, or names a light the
/// patch has. Made from a [`crate::Command`] by [`crate::Project::update`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a client's command has no heap: `Patch` carries its four names in place, `SetGroup` its members. Moved once per request, off the tick thread"
)]
pub enum PatchCommand {
    Patch {
        name: Text,
        maker: Text,
        model: Text,
        mode: Text,
        at: ChannelNumbers,
    },
    PatchStrip {
        name: Text,
        pixels: Pixels,
        color: Color,
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
    SetGroup {
        name: Text,
        members: Members,
    },
    RemoveGroup {
        name: Text,
    },
}

/// How many pixels a generic strip has: 1 or more. 0 does not parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u16")]
#[cfg_attr(test, derive(ts_rs::TS), ts(type = "number"))]
pub struct Pixels(u16);

impl Pixels {
    pub const fn count(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for Pixels {
    type Error = &'static str;
    fn try_from(count: u16) -> Result<Self, Self::Error> {
        if count >= 1 {
            Ok(Self(count))
        } else {
            Err("a strip has 1 pixel or more")
        }
    }
}

/// The heads of a bar or a panel in a grid: `width` by `height` cells, one per head.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Layout {
    pub width: u16,
    pub height: u16,
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
    /// The grid of its heads, `None` for a fixture without heads or a definition that
    /// gives none that fits. Its heads are its last lights, row by row.
    pub layout: Option<Layout>,
    /// The fixtures that share at least one of its channels. Allowed: the patch shows it.
    pub clashes: &'a [u16],
}

/// A group, as the picture shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Grouped<'a> {
    pub name: &'a str,
    /// In the order they were set.
    pub members: &'a [Member],
}
