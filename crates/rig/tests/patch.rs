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
        PatchEffect::Send(EngineCommand::ReleaseColour { lights }) => {
            format!("release colour {}", run(*lights))
        }
        PatchEffect::Send(EngineCommand::ReleaseIntensity { lights }) => {
            format!("release intensity {}", run(*lights))
        }
        PatchEffect::Send(other) => format!("{other:?}"),
        PatchEffect::Runs { change, runs } => format!("{change:?} {runs:?}"),
    }
}

/// A patched fixture in words: `1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1`.
fn row(patch: &Patch) -> Vec<String> {
    patch
        .fixtures()
        .iter()
        .map(|f| {
            format!(
                "{} {}: {}, {} at {}.{} ({}) lights {}+{}",
                f.fixture,
                f.name,
                f.model,
                f.mode,
                f.universe,
                f.address,
                f.channels,
                f.first,
                f.lights
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
    let cases: [Case; 28] = [
        (
            many,
            vec![],
            par_3,
            Ok(&["patched 1", "writers 37 38 39"]),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1"],
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
            par("X", 2, 511),
            Err(Refusal::PastTheEnd { channels: 3 }),
            &[],
        ),
        (
            many,
            vec![],
            par("X", 1, 510),
            Ok(&["patched 1", "writers 510 511 512"]),
            &["1 X: Generic RGB, RGB at 1.510 (3) lights 1+1"],
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
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1",
                "2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9",
            ],
        ),
        (
            Lights::new(9).unwrap(),
            vec![par_3],
            bar,
            Err(Refusal::NoLightsLeft),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1"],
        ),
        // on its channels: refused, nothing changed (more in `patches_never_overlap`)
        (
            many,
            vec![par_3],
            par("PAR 4", 1, 39),
            Err(Refusal::Overlaps { fixture: 1 }),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1"],
        ),
        (
            many,
            vec![par_3],
            par("PAR 4", 1, 40),
            Ok(&["patched 2", "writers 37 38 39 40 41 42"]),
            &[
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1",
                "2 PAR 4: Generic RGB, RGB at 1.40 (3) lights 2+1",
            ],
        ),
        // move: a new address, the same lights
        (
            many,
            vec![par_3, par("PAR 4", 1, 40)],
            PatchCommand::Move {
                fixture: 2,
                at: at(2, 100),
            },
            Ok(&["writers 37 38 39 100 101 102"]),
            &[
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1",
                "2 PAR 4: Generic RGB, RGB at 2.100 (3) lights 2+1",
            ],
        ),
        (
            many,
            vec![par_3],
            PatchCommand::Move {
                fixture: 1,
                at: at(2, 511),
            },
            Err(Refusal::PastTheEnd { channels: 3 }),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1"],
        ),
        (
            many,
            vec![par_3],
            PatchCommand::Move {
                fixture: 2,
                at: at(1, 1),
            },
            Err(Refusal::NotPatched),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1"],
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
            &["2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9"],
        ),
        (
            many,
            vec![par_3],
            PatchCommand::Unpatch { fixture: 2 },
            Err(Refusal::NotPatched),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1"],
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
                "1 PAR 5: Generic RGB, RGB at 1.1 (3) lights 1+1",
                "2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9",
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
            &["1 Front left: Generic RGB, RGB at 1.37 (3) lights 1+1"],
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
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1",
                "2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9",
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
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1",
                "2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9",
            ],
        ),
        (
            many,
            vec![par_3, bar],
            red(Target::Light { light: 11 }),
            Err(Refusal::NoSuchLight),
            &[
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1",
                "2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9",
            ],
        ),
        (
            many,
            vec![par_3],
            red(Target::Light { light: 0 }),
            Err(Refusal::NoSuchLight),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1"],
        ),
        (
            many,
            vec![par_3],
            red(fixture_(2)),
            Err(Refusal::NotPatched),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1"],
        ),
        // a release names its lights as a set does, and changes no patch
        (
            many,
            vec![par_3, bar],
            PatchCommand::ReleaseColour {
                target: fixture_(2),
            },
            Ok(&["release colour 1+9"]),
            &[
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1",
                "2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9",
            ],
        ),
        (
            many,
            vec![par_3, bar],
            PatchCommand::ReleaseIntensity {
                target: Target::Light { light: 10 },
            },
            Ok(&["release intensity 9+1"]),
            &[
                "1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1",
                "2 Bar: X-240Bar RGB, 26 Channel at 2.1 (26) lights 2+9",
            ],
        ),
        (
            many,
            vec![par_3],
            PatchCommand::ReleaseIntensity {
                target: Target::Light { light: 2 },
            },
            Err(Refusal::NoSuchLight),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1"],
        ),
        (
            many,
            vec![par_3],
            PatchCommand::ReleaseColour {
                target: fixture_(2),
            },
            Err(Refusal::NotPatched),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1"],
        ),
    ];
    for (lights, before, command, expected, after) in cases {
        let empty = Patch::new(two, lights, common::whole());
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

fn strip(pixels: u16, universe: u16, address: u16) -> PatchCommand {
    PatchCommand::PatchStrip {
        name: text("Strip"),
        pixels: engine_core::Pixels::try_from(pixels).unwrap(),
        color: engine_core::Color::Rgb,
        at: at(universe, address),
    }
}

/// No two fixtures share a channel: a patch or move that would make two is refused, and
/// the patch stays as it was. PAR 3 is at 1.37 to 1.39.
#[test]
fn patches_never_overlap() {
    let library = library();
    let two = Universes::new(2).unwrap();
    let par_3 = par("PAR 3", 1, 37);
    let overlaps = Err(Refusal::Overlaps { fixture: 1 });
    let move_ = |fixture, universe, address| PatchCommand::Move {
        fixture,
        at: at(universe, address),
    };
    // (name, patch before, command, refused or not)
    let cases = [
        ("touching before", vec![par_3], par("X", 1, 34), Ok(())),
        ("touching after", vec![par_3], par("X", 1, 40), Ok(())),
        ("one apart", vec![par_3], par("X", 1, 41), Ok(())),
        ("same address", vec![par_3], par("X", 1, 37), overlaps),
        (
            "last channel on its first",
            vec![par_3],
            par("X", 1, 35),
            overlaps,
        ),
        (
            "first channel on its last",
            vec![par_3],
            par("X", 1, 39),
            overlaps,
        ),
        (
            "a strip spanning into it",
            vec![par_3],
            strip(3, 1, 30),
            overlaps,
        ),
        (
            "a strip spanning over it",
            vec![par_3],
            strip(5, 1, 30),
            overlaps,
        ),
        (
            "a strip ending right before it",
            vec![par_3],
            strip(2, 1, 31),
            Ok(()),
        ),
        (
            "a strip running on into the next universe",
            vec![par("PAR 3", 2, 1)],
            strip(1, 1, 511),
            overlaps,
        ),
        (
            "move onto another",
            vec![par_3, par("X", 1, 41)],
            move_(2, 1, 39),
            overlaps,
        ),
        (
            "move onto its own channels",
            vec![par_3, par("X", 1, 41)],
            move_(1, 1, 38),
            Ok(()),
        ),
        (
            "move next to another",
            vec![par_3, par("X", 1, 41)],
            move_(2, 1, 40),
            Ok(()),
        ),
        (
            "move one onto the other",
            vec![par_3, par("X", 1, 41)],
            move_(1, 1, 40),
            Err(Refusal::Overlaps { fixture: 2 }),
        ),
    ];
    for (name, before, command, expected) in cases {
        let empty = Patch::new(two, Lights::new(64).unwrap(), common::whole());
        let project = before.iter().fold(empty, |patch, command| {
            let (new, _) = patch.update(*command, &library).unwrap();
            new.unwrap_or(patch)
        });
        let updated = project.update(command, &library).map(|_| ());
        assert_eq!(updated, expected, "{name}");
    }
}
