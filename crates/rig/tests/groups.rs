//! Groups, by command: patch and command in, effects and the groups after out.

mod common;

use common::fixture;
use engine_core::{
    ChannelNumbers, EngineCommand, LightRun, Lights, Member, Members, PatchCommand, Refusal,
    Target, Text, Universes,
};
use fixture_library::Fixture;
use rig::{Patch, PatchEffect};

fn text(text: &str) -> Text {
    Text::new(text).unwrap()
}

fn par(name: &str, universe: u16, address: u16) -> PatchCommand {
    PatchCommand::Patch {
        name: text(name),
        maker: text("Generic"),
        model: text("Generic RGB"),
        mode: text("RGB"),
        at: ChannelNumbers { universe, address },
    }
}

/// A bar of 8 pixels at 2.1: 9 lights, the bar first.
fn bar() -> PatchCommand {
    PatchCommand::Patch {
        name: text("Bar"),
        maker: text("XStatic"),
        model: text("X-240Bar RGB"),
        mode: text("26 Channel"),
        at: ChannelNumbers {
            universe: 2,
            address: 1,
        },
    }
}

const fn f(fixture: u16) -> Member {
    Member::Fixture { fixture }
}

const fn l(light: u16) -> Member {
    Member::Light { light }
}

fn set(name: &str, members: &[Member]) -> PatchCommand {
    PatchCommand::SetGroup {
        name: text(name),
        members: Members::new(members).unwrap(),
    }
}

fn remove(name: &str) -> PatchCommand {
    PatchCommand::RemoveGroup { name: text(name) }
}

fn red(group: &str) -> PatchCommand {
    PatchCommand::SetColour {
        target: Target::Group { group: text(group) },
        red: 0xffff,
        green: 0,
        blue: 0,
    }
}

fn library() -> Vec<Fixture> {
    ["Generic-Generic-RGB.qxf", "XStatic-X-240Bar-RGB.qxf"]
        .map(fixture)
        .into()
}

/// An effect in words: `colour 0+3` (first light from 0, count), `writers 6`, `darken 1+1`.
/// The runs of a group sent as one: `colour 0+1 2+1`.
fn said(effect: &PatchEffect) -> String {
    let run = |run: LightRun| format!("{}+{}", run.first().index(), run.count());
    let word = |command: EngineCommand| match command {
        EngineCommand::Darken { lights } => format!("darken {}", run(lights)),
        EngineCommand::SetColour { lights, .. } => format!("colour {}", run(lights)),
        EngineCommand::SetIntensity { lights, .. } => format!("intensity {}", run(lights)),
        EngineCommand::ReleaseIntensity { lights } => {
            format!("release intensity {}", run(lights))
        }
        other @ (EngineCommand::Tick { .. }
        | EngineCommand::Handed { .. }
        | EngineCommand::SetChannel { .. }
        | EngineCommand::ReleaseChannel { .. }
        | EngineCommand::ReleaseAll
        | EngineCommand::Blackout { .. }
        | EngineCommand::Attach { .. }
        | EngineCommand::RemoveOutput { .. }
        | EngineCommand::Writers
        | EngineCommand::ReleaseColour { .. }
        | EngineCommand::Runs { .. }) => format!("{other:?}"),
    };
    match effect {
        PatchEffect::Patched { fixture } => format!("patched {fixture}"),
        PatchEffect::Writers(writers) => format!("writers {}", writers.len()),
        PatchEffect::Send(command) => word(*command),
        PatchEffect::Runs { change, runs } => {
            let rest: Vec<String> = runs.iter().skip(1).map(|r| run(*r)).collect();
            let first = runs
                .first()
                .map(|r| word(change.on(*r)))
                .unwrap_or_default();
            [first]
                .into_iter()
                .chain(rest)
                .collect::<Vec<_>>()
                .join(" ")
        }
    }
}

/// The groups in words: `front: F3 F1 L5`.
fn groups(patch: &Patch) -> Vec<String> {
    patch
        .groups()
        .iter()
        .map(|g| {
            let members: Vec<String> = g
                .members
                .iter()
                .map(|m| match *m {
                    Member::Fixture { fixture } => format!(" F{fixture}"),
                    Member::Light { light } => format!(" L{light}"),
                })
                .collect();
            format!("{}:{}", g.name, members.concat())
        })
        .collect()
}

#[test]
fn a_group_command_makes_new_groups_and_says_what_follows() {
    let library = library();
    // PARs 1, 2, 3 are fixtures 1, 2, 3 with lights 1, 2, 3. The bar is fixture 4, lights
    // 4 (the bar) and 5 to 12 (its pixels).
    let rig = vec![
        par("PAR 1", 1, 1),
        par("PAR 2", 1, 4),
        par("PAR 3", 1, 7),
        bar(),
    ];
    let front = set("front", &[f(3), f(1), f(2)]);
    let back = set("back", &[l(6), l(5), f(1)]);
    let with = |more: &[PatchCommand]| [rig.as_slice(), more].concat();
    // (patch before, command, effects or why not, groups after)
    type Done = Result<&'static [&'static str], Refusal>;
    type Case = (
        Vec<PatchCommand>,
        PatchCommand,
        Done,
        &'static [&'static str],
    );
    let cases: [Case; 24] = [
        // set group: members in the order given, each patched
        (with(&[]), front, Ok(&[]), &["front: F3 F1 F2"]),
        (with(&[]), back, Ok(&[]), &["back: L6 L5 F1"]),
        (with(&[]), set("empty", &[]), Ok(&[]), &["empty:"]),
        (
            with(&[]),
            set("twice", &[f(1), f(1)]),
            Ok(&[]),
            &["twice: F1 F1"],
        ),
        (
            with(&[front]),
            set("x", &[f(1), f(5)]),
            Err(Refusal::NotPatched),
            &["front: F3 F1 F2"],
        ),
        (
            with(&[]),
            set("x", &[l(13)]),
            Err(Refusal::NoSuchLight),
            &[],
        ),
        (with(&[]), set("x", &[l(0)]), Err(Refusal::NoSuchLight), &[]),
        // a group of the same name is replaced in its place
        (
            with(&[front, back]),
            set("front", &[f(2)]),
            Ok(&[]),
            &["front: F2", "back: L6 L5 F1"],
        ),
        // remove group
        (
            with(&[front, back]),
            remove("front"),
            Ok(&[]),
            &["back: L6 L5 F1"],
        ),
        (
            with(&[front]),
            remove("back"),
            Ok(&[]),
            &["front: F3 F1 F2"],
        ),
        // unpatching a fixture takes it, and each of its lights, out of every group
        (
            with(&[front, back]),
            PatchCommand::Unpatch { fixture: 1 },
            Ok(&["writers 32", "darken 0+1"]),
            &["front: F3 F2", "back: L6 L5"],
        ),
        (
            with(&[front, back]),
            PatchCommand::Unpatch { fixture: 4 },
            Ok(&["writers 9", "darken 3+9"]),
            &["front: F3 F1 F2", "back: F1"],
        ),
        (
            with(&[set("bar", &[l(5)])]),
            PatchCommand::Unpatch { fixture: 4 },
            Ok(&["writers 9", "darken 3+9"]),
            &["bar:"],
        ),
        // the order survives any other change of the patch
        (
            with(&[front, back]),
            PatchCommand::Move {
                fixture: 1,
                at: ChannelNumbers {
                    universe: 1,
                    address: 100,
                },
            },
            Ok(&["writers 35"]),
            &["front: F3 F1 F2", "back: L6 L5 F1"],
        ),
        (
            with(&[front, back]),
            PatchCommand::Rename {
                fixture: 3,
                name: text("PAR 9"),
            },
            Ok(&[]),
            &["front: F3 F1 F2", "back: L6 L5 F1"],
        ),
        (
            with(&[front, back]),
            par("PAR 5", 1, 10),
            Ok(&["patched 5", "writers 38"]),
            &["front: F3 F1 F2", "back: L6 L5 F1"],
        ),
        // set light takes a group: its lights in the fewest runs, lowest first, sent as one
        (
            with(&[front]),
            red("front"),
            Ok(&["colour 0+3"]),
            &["front: F3 F1 F2"],
        ),
        (
            with(&[set("odd", &[f(3), f(1)])]),
            red("odd"),
            Ok(&["colour 0+1 2+1"]),
            &["odd: F3 F1"],
        ),
        (
            with(&[set("all", &[l(6), f(4), f(3), l(12)])]),
            red("all"),
            Ok(&["colour 2+10"]),
            &["all: L6 F4 F3 L12"],
        ),
        (
            with(&[back]),
            PatchCommand::SetIntensity {
                target: Target::Group {
                    group: text("back"),
                },
                intensity: 9,
            },
            Ok(&["intensity 0+1 4+2"]),
            &["back: L6 L5 F1"],
        ),
        (
            with(&[front]),
            PatchCommand::ReleaseIntensity {
                target: Target::Group {
                    group: text("front"),
                },
            },
            Ok(&["release intensity 0+3"]),
            &["front: F3 F1 F2"],
        ),
        (
            with(&[set("empty", &[])]),
            red("empty"),
            Ok(&[]),
            &["empty:"],
        ),
        (
            with(&[front]),
            red("back"),
            Err(Refusal::NoSuchGroup),
            &["front: F3 F1 F2"],
        ),
        // a group that lost its last member is still there, and sends nothing
        (
            with(&[set("one", &[f(2)]), PatchCommand::Unpatch { fixture: 2 }]),
            red("one"),
            Ok(&[]),
            &["one:"],
        ),
    ];
    for (before, command, expected, after) in cases {
        let empty = Patch::new(Universes::new(2).unwrap(), Lights::new(64).unwrap());
        let patch = before.iter().fold(empty, |patch, command| {
            let (new, _) = patch.update(*command, &library).unwrap();
            new.unwrap_or(patch)
        });
        let updated = patch.update(command, &library);
        let effects = updated
            .as_ref()
            .map(|(_, effects)| effects.iter().map(said).collect::<Vec<_>>())
            .map_err(|refusal| *refusal);
        assert_eq!(
            effects,
            expected.map(|e| e.iter().map(ToString::to_string).collect::<Vec<_>>()),
            "{command:?} after {before:?}"
        );
        let patch = match updated {
            Ok((Some(new), _)) => new,
            Ok((None, _)) | Err(_) => patch,
        };
        assert_eq!(groups(&patch), after, "{command:?} after {before:?}");
    }
}
