//! Rejected input and output-opening failures, shared at checked boundaries.

use super::model_light::Property;
use super::model_output::Rate;
use serde::Serialize;

/// Why an output's device could not be opened ([`Refusal::CannotOpen`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum OpenFailure {
    /// The device or socket refused, `with` the system's error number when it gave one.
    Device { with: Option<i32> },
    /// The output's thread could not start.
    Thread,
    /// Nothing could be opened now: the engine stopped, or the old device was not given
    /// back in time.
    Unavailable,
}

/// Why a command was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Refusal {
    /// Not one of the listed requests or commands, or not in their shape.
    NotACommand,
    /// This engine's universes are 1 to `universes`.
    NoSuchUniverse {
        universes: u16,
    },
    /// Addresses are 1 to 512.
    NoSuchAddress,
    /// More commands are waiting than the engine takes in one frame. Send again.
    QueueFull,
    /// The output's device or socket could not be opened, and why.
    CannotOpen {
        why: OpenFailure,
    },
    /// The library has no fixture of that maker and model.
    NoSuchFixture,
    /// The output's device is being opened. Send again once it is open.
    Opening,
    /// This kind of output sends at most `most` frames a second (a USB DMX adapter 44).
    TooFast {
        most: Rate,
    },
    /// This kind of output takes at most `most` universes.
    TooManyUniverses {
        most: u16,
    },
    /// The fixture has no mode of that name.
    NoSuchMode,
    /// The mode's `channels` run past the engine's last universe from there.
    PastTheEnd {
        channels: u16,
    },
    /// The fixture would run over universes `first` to `last`, and no output streams
    /// them all as one run.
    NotStreamed {
        first: u16,
        last: u16,
    },
    /// Output `output` streams the fixture's channel at `universe`, `address` and never
    /// sends it: its run ends in channels that make no whole unit (a pixel, for some kinds).
    NotSent {
        output: u8,
        universe: u16,
        address: u16,
    },
    /// Then fixture `fixture` would lose a channel: no output would stream every universe
    /// it runs over, or an output would stream one of its channels and never send it.
    StreamsFixture {
        fixture: u16,
    },
    /// The fixture would be on a channel of fixture `fixture`: no two fixtures share one.
    Overlaps {
        fixture: u16,
    },
    /// The fixture's lights do not fit in the lights the engine has left.
    NoLightsLeft,
    /// No fixture is patched with that number.
    NotPatched,
    /// No patched fixture has that light.
    NoSuchLight,
    /// No group has that name.
    NoSuchGroup,
    /// A group has at most `most` members ([`crate::Members::MOST`] says why).
    TooManyMembers {
        most: u16,
    },
    /// Then the show file would be longer than its `most` bytes, and would not open.
    ShowFileFull {
        most: u32,
    },
    /// No look is saved with that number.
    NoSuchLook,
    WrongLookKind,
    LookCycle,
    InvalidTiming,
    /// A Wave needs at least one swatch in its inline or referenced palette.
    EmptyPalette,
    /// The fixture has no matrix layout matching its heads.
    NoMatrix,
    WorkLimit,
    /// Look `look` uses it: a saved look's target, or a play of the look.
    LookInUse {
        look: u16,
    },
    /// At most `most` looks are saved.
    TooManyLooks {
        most: u16,
    },
    /// Every play slot is taken. Release a play first.
    PlayFull,
    /// Light `light` (counted from 1) has a wheel for `property` but no slot that shows
    /// the value: black on a wheel without black. Or a fixture mixes colour beside a
    /// colour wheel that has no neutral slot to hold.
    Unsupported {
        light: u16,
        property: Property,
    },
    /// Another change came first: the command was checked against the project, the
    /// patch or the looks as they were before it. Nothing changed. Send it again.
    Stale,
    /// The group has no members: a saved look's target names at least one light.
    EmptyGroup,
    /// The grab holds `property`: an update gives a value of it.
    OtherProperty {
        property: Property,
    },
    /// The live grabs keep `most` runs of lights. End or release a grab first.
    GrabFull {
        most: u32,
    },
}
