//! The engine API as data: what a client sends and what it gets back, as JSON over
//! one WebSocket. `crates/engine-server` moves it; this crate says what it is. The core
//! knows only what a command carries (`engine_core::Command`, `Refusal`); the messages
//! around it live here.
//!
//! | Client sends | Engine answers |
//! |---|---|
//! | `{"Send": {"command": ...}}` | `"Accepted"`, `{"Patched": {"fixture": ...}}` for `Patch`, `{"Started": {"play": ...}}` for `StartLook`, `{"Grabbed": {"grab": ...}}` for a set, or `{"Refused": {"reason": ...}}`. `"Stopping"` once the engine stops: nothing changed. `"Failed"` when the server failed while working on it: the picture says what is live |
//! | `"Query"` | `{"Picture": ...}`: the state, the project, the patch, every level, every held light and output health |
//! | `"Subscribe"` | `{"Picture": ...}`, then `{"Changed": ...}` for new frames and `{"Health": ...}` for output timing and events |
//! | `"Stop"` | `"Stopping"`, then every connection closes and the engine process ends |
//! | `{"Search": {"text": ...}}` | `{"Found": ...}`: the best matches of the fixture library |
//! | `{"Fixture": {"maker": ..., "model": ...}}` | `{"Fixture": ...}`: its modes and channels, or `NoSuchFixture` |
//! | `"Failures"` | `{"Failures": ...}`: the definition files that could not be read |
//! | anything else | `{"Refused": {"reason": "NotACommand"}}` |

#![cfg_attr(not(test), no_std)] // the TypeScript generator test writes a file
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod health;
mod library;

pub use health::{
    Health, HealthEvent, LateReason, LatenessWindow, OutputHealth, OutputHealthEvent,
    OutputHealthState, OutputMeasurement, PressWindow, RecoveredReason,
};
pub use library::{
    Asked, ChannelFunction, Failure, FixtureKind, Library, Match, ModeChannels, ModeSize,
    NamedChannel, Why,
};

use engine_core::{
    ChannelLevel, Command, GrabId, Grouped, HeldLight, Levels, Patched, PlayId, Project, Refusal,
    SavedLook, ShowFile, State, Text,
};
use serde::{Deserialize, Serialize};
/// The version of this API. `GET /version` answers it as text. A client that speaks
/// another version does not talk to the engine. Raise it with every change a client of
/// the old version would get wrong.
pub const API_VERSION: u16 = 12;

/// What a client sends. A key it does not know, here or in any type it carries, refuses
/// the request: a misspelled field would otherwise be ignored without a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[expect(
    clippy::large_enum_variant,
    reason = "a client's command has no heap: `Patch` carries its four names in place, `SetGroup` its members. Moved once per request, off the tick thread"
)]
pub enum Request {
    /// Change the state. Answered at once, before the frame that carries it.
    Send { command: Command },
    /// The whole picture, once.
    Query,
    /// The whole picture, then what changed with every frame.
    Subscribe,
    /// Stop the engine in an orderly way: answer, close every connection, end.
    Stop,
    /// The fixtures of the library whose maker and model match `text`, best first.
    Search {
        #[cfg_attr(test, ts(type = "string"))]
        text: Text,
    },
    /// One fixture of the library: its modes, each channel by name and function.
    Fixture {
        #[cfg_attr(test, ts(type = "string"))]
        maker: Text,
        #[cfg_attr(test, ts(type = "string"))]
        model: Text,
    },
    /// The definition files that could not be read, and why.
    Failures,
}

/// What the engine sends back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Reply<'a> {
    /// The command is queued for the very next frame.
    Accepted,
    /// The fixture is patched as `fixture`; its writers are queued for the very next frame.
    Patched { fixture: u16 },
    /// The look plays as `play` from the very next frame.
    Started { play: PlayId },
    /// The grab holds from the very next frame.
    Grabbed { grab: GrabId },
    /// The engine stops: every connection closes, then the process ends.
    Stopping,
    /// The server failed while working on it. The picture says what is live.
    Failed,
    /// The command was not taken. Nothing changed.
    Refused { reason: Refusal },
    /// The state, the project, the held levels of every universe and the held parts of
    /// every light.
    Picture {
        state: State,
        project: Project,
        library: Library,
        /// Every patched fixture, by number.
        patch: &'a [Patched<'a>],
        /// Every group, in the order they were made.
        groups: &'a [Grouped<'a>],
        /// Every saved look, by number.
        looks: &'a [SavedLook],
        /// Whether the patch, the groups and the looks are kept in a show file.
        show: ShowFile,
        universes: &'a [Levels],
        /// Every light that holds a part, by number.
        lights: &'a [HeldLight],
        health: Health<'a>,
    },
    /// Current rows and the events since this subscriber's last health delivery.
    Health {
        health: Health<'a>,
        events: &'a [OutputHealthEvent],
        missed: u32,
    },
    /// A new frame: the state, and the channels and the lights whose held values changed
    /// since the last message.
    Changed {
        state: State,
        project: Project,
        library: Library,
        /// The whole patch when it changed since the last message, else `None`.
        patch: Option<&'a [Patched<'a>]>,
        /// Every group when the patch changed since the last message, else `None`.
        groups: Option<&'a [Grouped<'a>]>,
        /// Every saved look when they changed since the last message, else `None`.
        looks: Option<&'a [SavedLook]>,
        /// Whether the patch, the groups and the looks are kept in a show file.
        show: ShowFile,
        channels: &'a [ChannelLevel],
        /// Every light whose held parts changed, as it holds them now: a part let go is
        /// `None`.
        lights: &'a [HeldLight],
    },
    /// The answer to `Search`: at most [`MATCHES`], best first. Empty while the library
    /// is not read.
    Found { fixtures: &'a [Match<'a>] },
    /// The answer to `Fixture`.
    Fixture {
        maker: &'a str,
        model: &'a str,
        kind: FixtureKind<'a>,
        modes: &'a [ModeChannels<'a>],
    },
    /// The answer to `Failures`, in the order of the files' names.
    Failures { failures: &'a [Failure<'a>] },
}

/// The most fixtures one `Found` carries.
pub const MATCHES: usize = 50;

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::{
        Adapter, AuthoredStep, BestEffort, Bpm, Brightness, ChannelNumbers, Color, Counters, Gobo,
        Layout, Light, ManualEnd, Member, MixRules, OpenFailure, OtherEnd, Output, OutputKind,
        OutputNumber, Palette, Pattern, Pixels, Play, PlayState, Position, Priority, ProjectOutput,
        Property, Rank, Rate, Reading, Repeat, Rgb, Run, RunNumbers, Saved, Sequence, SequenceItem,
        Speed, Status, Step, Swatches, Target, Unopened, Use, Value, WaveTarget, Wheel, When,
    };
    use engine_core::{Percent, Period, Width};
    use fixture_library::{Problem, Refused};
    use ts_rs::{Config, TS};

    /// Expected answers: docs/engine.md command rows (spread and transition 0 to 100,
    /// width at least one sixteenth, period 1 to 32 beats). A number out of range is no
    /// command at all.
    #[test]
    fn a_timing_number_out_of_range_is_no_command() {
        let start = |spread: u32| format!(r#"{{"Send":{{"command":{{"StartLook":{{"look":1,"use":{{"spread":{spread}}}}}}}}}}}"#);
        let item = |field: &str, value: u32| {
            format!(r#"{{"Send":{{"command":{{"PutSequence":{{"look":3,"name":"s","sequence":{{"items":[{{"look":1,"{field}":{value}}}]}}}}}}}}}}"#)
        };
        let wave = |period: u32| {
            format!(r#"{{"Send":{{"command":{{"PutPattern":{{"look":7,"name":"w","pattern":{{"Wave":{{"target":{{"Matrix":{{"fixture":1}}}},"period":{period},"palette":{{"Step":{{"look":3}}}}}}}}}}}}}}}}"#)
        };
        let cases = [
            (start(0), true),
            (start(100), true),
            (start(101), false),
            (item("width", 0), false),
            (item("width", 1), true),
            (item("width", 65_535), true),
            (item("transition", 100), true),
            (item("transition", 101), false),
            (wave(0), false),
            (wave(1), true),
            (wave(32), true),
            (wave(33), false),
        ];
        for (text, command) in cases {
            assert_eq!(serde_json::from_str::<Request>(&text).is_ok(), command, "{text}");
        }
    }

    /// The UI's types for the engine API, generated from these types and the core's.
    const TYPESCRIPT: &str = "../../ui/src/engine-api.ts";

    #[test]
    fn typescript_types_match_the_rust_types() {
        let config = Config::default();
        let generated = format!(
            "// Generated from crates/wire. Do not edit.\n\
             // Update: UPDATE_TS=1 cargo test -p wire\n\n\
             {}",
            [
                Request::decl(&config),
                Command::decl(&config),
                ChannelNumbers::decl(&config),
                RunNumbers::decl(&config),
                OutputNumber::decl(&config),
                OutputKind::decl(&config),
                Adapter::decl(&config),
                Color::decl(&config),
                Rate::decl(&config),
                Reply::decl(&config),
                Health::decl(&config),
                OutputHealth::decl(&config),
                OutputHealthState::decl(&config),
                OutputMeasurement::decl(&config),
                LatenessWindow::decl(&config),
                PressWindow::decl(&config),
                LateReason::decl(&config),
                RecoveredReason::decl(&config),
                OutputHealthEvent::decl(&config),
                HealthEvent::decl(&config),
                Refusal::decl(&config),
                OpenFailure::decl(&config),
                State::decl(&config),
                Priority::decl(&config),
                BestEffort::decl(&config),
                Levels::decl(&config),
                ChannelLevel::decl(&config),
                HeldLight::decl(&config),
                Rgb::decl(&config),
                Light::decl(&config),
                Position::decl(&config),
                Gobo::decl(&config),
                Wheel::decl(&config),
                Property::decl(&config),
                Value::decl(&config),
                GrabId::decl(&config),
                Step::decl(&config),
                AuthoredStep::decl(&config),
                Swatches::decl(&config),
                Pattern::decl(&config),
                WaveTarget::decl(&config),
                Palette::decl(&config),
                Saved::decl(&config),
                SavedLook::decl(&config),
                Sequence::decl(&config),
                SequenceItem::decl(&config),
                Use::decl(&config),
                Percent::decl(&config),
                Width::decl(&config),
                Period::decl(&config),
                Speed::decl(&config),
                Repeat::decl(&config),
                PlayId::decl(&config),
                Play::decl(&config),
                Rank::decl(&config),
                PlayState::decl(&config),
                When::decl(&config),
                Bpm::decl(&config),
                Reading::decl(&config),
                MixRules::decl(&config),
                Brightness::decl(&config),
                ManualEnd::decl(&config),
                OtherEnd::decl(&config),
                Output::decl(&config),
                Run::decl(&config),
                Status::decl(&config),
                Counters::decl(&config),
                Library::decl(&config),
                Match::decl(&config),
                ModeSize::decl(&config),
                ModeChannels::decl(&config),
                NamedChannel::decl(&config),
                Failure::decl(&config),
                Why::decl(&config),
                Asked::decl(&config),
                Problem::decl(&config),
                Refused::decl(&config),
                FixtureKind::decl(&config),
                ChannelFunction::decl(&config),
                Project::decl(&config),
                ProjectOutput::decl(&config),
                Target::decl(&config),
                Patched::decl(&config),
                Pixels::decl(&config),
                Layout::decl(&config),
                Member::decl(&config),
                Grouped::decl(&config),
                ShowFile::decl(&config),
                Unopened::decl(&config),
            ]
            .map(|declaration| format!("export {declaration}\n"))
            .join("\n"),
        ) + &format!("\nexport const API_VERSION = {API_VERSION};\n");
        if std::env::var_os("UPDATE_TS").is_some() {
            std::fs::write(TYPESCRIPT, &generated).expect("write the TypeScript types");
        }
        let committed = std::fs::read_to_string(TYPESCRIPT).unwrap_or_else(|_| String::new());
        assert_eq!(
            committed, generated,
            "{TYPESCRIPT} is out of date: UPDATE_TS=1 cargo test -p wire"
        );
    }
}
