//! The patch, by command: project and command in, new project and effects out.

mod common;

use common::fixture;
use engine_core::{
    ChannelNumbers, EngineCommand, Lights, PatchCommand, Place, Refusal, Target, Text, Universes,
    Writer,
};
use fixture_library::Fixture;
use rig::{Patch, PatchEffect};

fn text(text: &str) -> Text {
    Text::new(text).unwrap()
}

fn at(universe: u16, address: u16) -> ChannelNumbers {
    ChannelNumbers { universe, address }
}

/// Patch `model` of the test definitions in `mode`, named `name`, at `universe`.`address`.
fn patch(name: &str, model: &str, mode: &str, universe: u16, address: u16) -> PatchCommand {
    let maker = if model.starts_with("X-240") {
        "XStatic"
    } else {
        "Generic"
    };
    PatchCommand::Patch {
        name: text(name),
        maker: text(maker),
        model: text(model),
        mode: text(mode),
        at: at(universe, address),
    }
}

fn par(name: &str, universe: u16, address: u16) -> PatchCommand {
    patch(name, "Generic RGB", "RGB", universe, address)
}

fn red(target: Target) -> PatchCommand {
    PatchCommand::SetColour {
        target,
        red: 0xffff,
        green: 0,
        blue: 0,
    }
}

const fn fixture_(fixture: u16) -> Target {
    Target::Fixture { fixture }
}

fn library() -> Vec<Fixture> {
    [
        "Generic-Generic-RGB.qxf",
        "Generic-Generic-RGBW.qxf",
        "XStatic-X-240Bar-RGB.qxf",
    ]
    .map(fixture)
    .into()
}

/// The address a writer writes first, in universe 1 or 2.
fn address(writer: &Writer) -> u16 {
    let channel = match *writer {
        Writer::Fixed { at, .. }
        | Writer::Takes {
            at: Place::Byte(at),
            ..
        } => at,
        Writer::Takes {
            at: Place::Pair { coarse, .. },
            ..
        } => coarse,
    };
    let two = Universes::new(2).unwrap();
    (1..=2)
        .flat_map(|universe| (1..=512).map(move |address| at(universe, address)))
        .find(|numbers| numbers.check(two) == Ok(channel))
        .unwrap()
        .address
}

/// An effect in words: `writers 37 38 39`, `darken 0+1`, `colour 0+1`, `patched 1`.
fn said(effect: &PatchEffect) -> String {
    let run = |run: engine_core::LightRun| format!("{}+{}", run.first().index(), run.count());
    match effect {
        PatchEffect::Patched { fixture } => format!("patched {fixture}"),
        PatchEffect::Writers(writers) => {
            let addresses: Vec<String> = writers.iter().map(|w| address(w).to_string()).collect();
            format!("writers {}", addresses.join(" "))
                .trim_end()
                .to_string()
        }
        PatchEffect::Send(EngineCommand::Darken { lights }) => format!("darken {}", run(*lights)),
        PatchEffect::Send(EngineCommand::SetColour { lights, .. }) => {
            format!("colour {}", run(*lights))
        }
        PatchEffect::Send(EngineCommand::SetIntensity { lights, .. }) => {
            format!("intensity {}", run(*lights))
        }
        PatchEffect::Send(other) => format!("{other:?}"),
    }
}

/// A patched fixture in words: `1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes [2]`.
fn row(patch: &Patch) -> Vec<String> {
    patch
        .fixtures()
        .iter()
        .map(|f| {
            format!(
                "{} {}: {}, {} at {}.{} ({}) lights {}+{} clashes {:?}",
                f.fixture,
                f.name,
                f.model,
                f.mode,
                f.universe,
                f.address,
                f.channels,
                f.first,
                f.lights,
                f.clashes
            )
        })
        .collect()
}

#[test]
fn a_patch_command_makes_a_new_patch_and_says_what_follows() {
    let library = library();
    let two = Universes::new(2).unwrap();
    let many = Lights::new(64).unwrap();
    let par_3 = par("PAR 3", 1, 37);
    let bar = patch("Bar", "X-240Bar RGB", "26 Channel", 2, 1);
    // (lights of the engine, patch before, command, effects or why not, patch after)
    type Done = Result<&'static [&'static str], Refusal>;
    type Case = (
        Lights,
        Vec<PatchCommand>,
        PatchCommand,
        Done,
        &'static [&'static str],
    );
    let cases: [Case; 24] = [
        (
            many,
            vec![],
            par_3,
            Ok(&["patched 1", "writers 37 38 39"]),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []"],
        ),
        (
            many,
            vec![],
            patch("X", "Generic RGBX", "RGB", 1, 1),
            Err(Refusal::NoSuchFixture),
            &[],
        ),
        (
            many,
            vec![],
            patch("X", "Generic RGB", "RGBX", 1, 1),
            Err(Refusal::NoSuchMode),
            &[],
        ),
        (
            many,
            vec![],
            par("X", 1, 511),
            Err(Refusal::PastTheEnd { channels: 3 }),
            &[],
        ),
        (
            many,
            vec![],
            par("X", 1, 510),
            Ok(&["patched 1", "writers 510 511 512"]),
            &["1 X: Generic RGB, RGB at 1.510 (3) lights 1+1 clashes []"],
        ),
        (
            many,
            vec![],
            par("X", 3, 1),
            Err(Refusal::NoSuchUniverse { universes: 2 }),
            &[],
        ),
        (
            many,
            vec![],
            par("X", 1, 0),
            Err(Refusal::NoSuchAddress),
            &[],
        ),
        // a bar of 8 pixels is 9 lights: the bar, then each pixel
        (
            many,
            vec![par_3],
            bar,
            Ok(&[
                "patched 2",
                "writers 37 38 39 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26",
            ]),
            &[
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []",
                "2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9 clashes []",
            ],
        ),
        (
            Lights::new(9).unwrap(),
            vec![par_3],
            bar,
            Err(Refusal::NoLightsLeft),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []"],
        ),
        // the same channels: allowed, the patch shows the clash on both
        (
            many,
            vec![par_3],
            par("PAR 4", 1, 39),
            Ok(&["patched 2", "writers 37 38 39 39 40 41"]),
            &[
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes [2]",
                "2 PAR 4: Generic RGB, RGB at 1.39 (3) lights 2+1 clashes [1]",
            ],
        ),
        (
            many,
            vec![par_3],
            par("PAR 4", 1, 40),
            Ok(&["patched 2", "writers 37 38 39 40 41 42"]),
            &[
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []",
                "2 PAR 4: Generic RGB, RGB at 1.40 (3) lights 2+1 clashes []",
            ],
        ),
        // move: a new address, the same lights
        (
            many,
            vec![par_3, par("PAR 4", 1, 39)],
            PatchCommand::Move {
                fixture: 2,
                at: at(2, 100),
            },
            Ok(&["writers 37 38 39 100 101 102"]),
            &[
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []",
                "2 PAR 4: Generic RGB, RGB at 2.100 (3) lights 2+1 clashes []",
            ],
        ),
        (
            many,
            vec![par_3],
            PatchCommand::Move {
                fixture: 1,
                at: at(1, 511),
            },
            Err(Refusal::PastTheEnd { channels: 3 }),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []"],
        ),
        (
            many,
            vec![par_3],
            PatchCommand::Move {
                fixture: 2,
                at: at(1, 1),
            },
            Err(Refusal::NotPatched),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []"],
        ),
        // unpatch: its writers and its held values go, the others keep their lights
        (
            many,
            vec![par_3, bar],
            PatchCommand::Unpatch { fixture: 1 },
            Ok(&[
                "writers 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26",
                "darken 0+1",
            ]),
            &["2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9 clashes []"],
        ),
        (
            many,
            vec![par_3],
            PatchCommand::Unpatch { fixture: 2 },
            Err(Refusal::NotPatched),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []"],
        ),
        // a free number and free lights are taken again, the lowest first
        (
            many,
            vec![par_3, bar, PatchCommand::Unpatch { fixture: 1 }],
            par("PAR 5", 1, 1),
            Ok(&[
                "patched 1",
                "writers 1 2 3 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26",
            ]),
            &[
                "1 PAR 5: Generic RGB, RGB at 1.1 (3) lights 1+1 clashes []",
                "2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9 clashes []",
            ],
        ),
        (
            many,
            vec![par_3],
            PatchCommand::Rename {
                fixture: 1,
                name: text("Front left"),
            },
            Ok(&[]),
            &["1 Front left: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []"],
        ),
        (
            many,
            vec![],
            PatchCommand::Rename {
                fixture: 1,
                name: text("X"),
            },
            Err(Refusal::NotPatched),
            &[],
        ),
        // light commands: checked against the patch, sent to the tick thread
        (
            many,
            vec![par_3, bar],
            red(fixture_(2)),
            Ok(&["colour 1+9"]),
            &[
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []",
                "2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9 clashes []",
            ],
        ),
        (
            many,
            vec![par_3, bar],
            PatchCommand::SetIntensity {
                target: Target::Light { light: 10 },
                intensity: 7,
            },
            Ok(&["intensity 9+1"]),
            &[
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []",
                "2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9 clashes []",
            ],
        ),
        (
            many,
            vec![par_3, bar],
            red(Target::Light { light: 11 }),
            Err(Refusal::NoSuchLight),
            &[
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []",
                "2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9 clashes []",
            ],
        ),
        (
            many,
            vec![par_3],
            red(Target::Light { light: 0 }),
            Err(Refusal::NoSuchLight),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []"],
        ),
        (
            many,
            vec![par_3],
            red(fixture_(2)),
            Err(Refusal::NotPatched),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1 clashes []"],
        ),
    ];
    for (lights, before, command, expected, after) in cases {
        let empty = Patch::new(two, lights);
        let project = before.iter().fold(empty, |patch, command| {
            let (new, _) = patch.update(*command, &library).unwrap();
            new.unwrap_or(patch)
        });
        let updated = project.update(command, &library);
        let effects = updated
            .as_ref()
            .map(|(_, effects)| effects.iter().map(said).collect::<Vec<_>>())
            .map_err(|refusal| *refusal);
        assert_eq!(
            effects,
            expected.map(|e| e.iter().map(ToString::to_string).collect::<Vec<_>>()),
            "{command:?} after {before:?}"
        );
        let project = match updated {
            Ok((Some(new), _)) => new,
            Ok((None, _)) | Err(_) => project,
        };
        assert_eq!(row(&project), after, "{command:?} after {before:?}");
    }
}
