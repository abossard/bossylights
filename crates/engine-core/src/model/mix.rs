//! Plain Mixer settings and grab identity.

use serde::{Deserialize, Serialize};

/// The Mixer's three settings, saved with the show. An edit takes effect in the frame
/// that applies it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MixRules {
    pub brightness: Brightness,
    pub manual_end: ManualEnd,
    pub other_end: OtherEnd,
}

/// Between two brightness values of one rank: the latest start, or the highest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Brightness {
    #[default]
    Latest,
    Highest,
}

/// What a stopped Manual play leaves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum ManualEnd {
    /// Its values, until released.
    #[default]
    UntilRelease,
    /// Nothing: the stop ends it.
    UntilStop,
}

/// What a stopped Show or Autopilot play leaves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum OtherEnd {
    /// Nothing: what is underneath shows.
    #[default]
    Reveal,
    /// Its values, at its rank and start, until released or started again.
    Stay,
}

/// One direct grab, by the generation the front gave it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GrabId {
    pub generation: u32,
}
