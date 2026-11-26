//! The patch, by command: project and command in, new project and effects out.

mod common;

use common::fixture;
use engine_core::{
    ChannelNumbers, Lights, PatchCommand, Property, Refusal, Rgb, RunNumbers, Target, Text,
    Universes, Value,
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
    } else if model.starts_with("LED Spot") {
        "Shehds"
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
    PatchCommand::Grab {
        target,
        value: Value::Colour(Rgb {
            red: 0xffff,
            green: 0,
            blue: 0,
        }),
    }
}

fn black(target: Target) -> PatchCommand {
    PatchCommand::Grab {
        target,
        value: Value::Colour(Rgb::BLACK),
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
        "Shehds-LED-Spot-100W.qxf",
    ]
    .map(fixture)
    .into()
}

/// An effect in words: `fields`, `gone 0+1`, `colour 0+1`, `release colour 0+1`,
/// `patched 1`.
fn said(effect: &PatchEffect) -> String {
    let run = |run: engine_core::LightRun| format!("{}+{}", run.first().index(), run.count());
    let runs =
        |runs: &[engine_core::LightRun]| runs.iter().map(|r| run(*r)).collect::<Vec<_>>().join(" ");
    match effect {
        PatchEffect::Patched { fixture } => format!("patched {fixture}"),
        PatchEffect::Fields(_) => "fields".to_string(),
        PatchEffect::Gone { lights } => format!("gone {}", run(*lights)),
        PatchEffect::Grab { value, runs: of } => {
            let property = format!("{:?}", value.property()).to_lowercase();
            format!("{property} {}", runs(of))
        }
        PatchEffect::Release { property, runs: of } => format!(
            "release {} {}",
            format!("{property:?}").to_lowercase(),
            runs(of)
        ),
        PatchEffect::Send(other) => format!("{other:?}"),
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
    let spot = patch("Spot", "LED Spot 100W", "14 Channel", 1, 1);
    // (lights of the engine, patch before, command, effects or why not, patch after)
    type Done = Result<&'static [&'static str], Refusal>;
    type Case = (
        Lights,
        Vec<PatchCommand>,
        PatchCommand,
        Done,
        &'static [&'static str],
    );
    let cases: [Case; 30] = [
        (
            many,
            vec![],
            par_3,
            Ok(&["patched 1", "fields"]),
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
            Ok(&["patched 1", "fields"]),
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
            Ok(&["patched 2", "fields"]),
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
            Ok(&["patched 2", "fields"]),
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
            Ok(&["fields"]),
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
            Ok(&["fields", "gone 0+1"]),
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
            Ok(&["patched 1", "fields"]),
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
            PatchCommand::Grab {
                target: Target::Light { light: 10 },
                value: Value::Intensity(7),
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
        // a colour wheel without black: refused as a step would be
        (
            many,
            vec![spot],
            black(fixture_(1)),
            Err(Refusal::Unsupported {
                light: 1,
                property: Property::Colour,
            }),
            &["1 Spot: LED Spot 100W, 14 Channel at 1.1 (14) lights 1+1"],
        ),
        (
            many,
            vec![spot],
            red(fixture_(1)),
            Ok(&["colour 0+1"]),
            &["1 Spot: LED Spot 100W, 14 Channel at 1.1 (14) lights 1+1"],
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
            PatchCommand::Release {
                target: fixture_(2),
                property: Property::Colour,
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
            PatchCommand::Release {
                target: Target::Light { light: 10 },
                property: Property::Intensity,
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
            PatchCommand::Release {
                target: Target::Light { light: 2 },
                property: Property::Intensity,
            },
            Err(Refusal::NoSuchLight),
            &["1 PAR 3: Generic RGB, RGB at 1.37 (3) lights 1+1"],
        ),
        (
            many,
            vec![par_3],
            PatchCommand::Release {
                target: fixture_(2),
                property: Property::Colour,
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

/// Where an output's stream has fixtures: the lowest and highest patched channel, counted
/// from 1 in the stream of a run. A PAR takes 3 channels, a strip pixel 3.
#[test]
fn a_run_has_its_lowest_and_highest_patched_channel() {
    let library = library();
    let three = Universes::new(3).unwrap();
    let run = |first, count| RunNumbers { first, count }.check(three).unwrap();
    // (name, patch, run, lowest and highest)
    let cases = [
        ("nothing patched", vec![], run(1, 1), None),
        (
            "slots 1 to 50 from two fixtures",
            vec![strip(1, 1, 48), strip(1, 1, 1)],
            run(1, 1),
            Some(1..=50),
        ),
        (
            "only in another universe",
            vec![par("A", 2, 1)],
            run(1, 1),
            None,
        ),
        (
            "a strip running on from the universe before",
            vec![strip(2, 1, 511)],
            run(2, 1),
            Some(1..=4),
        ),
        (
            "a strip running on into the next universe",
            vec![strip(2, 1, 511)],
            run(1, 1),
            Some(511..=512),
        ),
        (
            "a run of two universes",
            vec![par("A", 1, 510), par("B", 2, 1)],
            run(1, 2),
            Some(510..=515),
        ),
        (
            "a fixture past the run",
            vec![par("A", 1, 1), par("B", 3, 1)],
            run(1, 2),
            Some(1..=3),
        ),
        (
            "the last channel",
            vec![par("A", 1, 510)],
            run(1, 1),
            Some(510..=512),
        ),
    ];
    for (name, commands, run, expected) in cases {
        let empty = Patch::new(three, Lights::new(64).unwrap(), common::whole());
        let patch = commands.iter().fold(empty, |patch, command| {
            let (new, _) = patch.update(*command, &library).unwrap();
            new.unwrap()
        });
        assert_eq!(patch.patched_in(run), expected, "{name}");
    }
}

/// The prepared plan carries every patched channel as its file says, and its frame shows
/// each at its rest. Ayrton Mistral, Standard: channel 6 is Shutter / Strobe, closed at 0
/// to 10 and open at 11 to 20, so it rests open at 11.
#[test]
fn a_prepared_patch_frames_each_channel_at_its_files_rest() {
    use engine_core::{ChannelSet, Channels, Grabs, Levels, Priority, State, frame};
    let mistral = vec![fixture("Ayrton-Mistral.qxf")];
    let empty = Patch::new(Universes::ONE, Lights::new(16).unwrap(), common::whole());
    let (patched, _) = empty
        .update(
            PatchCommand::Patch {
                name: text("Spot"),
                maker: text("Ayrton"),
                model: text("Mistral"),
                mode: text("Standard"),
                at: at(1, 1),
            },
            &mistral,
        )
        .unwrap();
    let plan = patched.unwrap().prepare();
    let mut channels = Box::new(Channels::NEW);
    channels.configure(&[], plan.configured());
    let state = State::new(Priority::RealTime);
    frame(
        &state,
        &Grabs::NONE,
        (&[Levels::ZERO], &[ChannelSet::NONE]),
        (plan.configured(), plan.followers()),
        &mut channels,
        |_| {},
    );
    assert_eq!(channels.values()[0].0[5], 11);
}

/// The prepared plan lists every channel that follows a modifier, in channel order, a
/// pair as one word, with the file's dark (#251). Martin Rush MH6 Wash, Standard, at 1:1:
/// 0 Strobe (a Closed range: Blackout), 1 and 2 Dimmer and its fine (Master and
/// Blackout), 3 to 6 R G B W (the dimmer carries them), then colour presets, zoom, pan,
/// tilt, control. A Generic RGB PAR at 1:101: R G B follow both, no dimmer carries them.
/// The PAR is patched first, so the index is put in channel order; then it moves to
/// 1:201 and its followers move with it.
#[test]
fn a_prepared_patch_lists_its_followers_a_pair_as_one_word() {
    use engine_core::{Field, Follows, Pair};
    let library = vec![
        fixture("Martin-Rush-MH6-Wash.qxf"),
        fixture("Generic-Generic-RGB.qxf"),
    ];
    let mh6 = PatchCommand::Patch {
        name: text("Wash"),
        maker: text("Martin"),
        model: text("Rush MH6 Wash"),
        mode: text("Standard"),
        at: at(1, 1),
    };
    let mut patched = Patch::new(Universes::ONE, Lights::new(16).unwrap(), common::whole());
    for command in [par("PAR", 1, 101), mh6] {
        let (next, _) = patched.update(command, &library).unwrap();
        patched = next.unwrap();
    }
    let wash = [Field::Byte(0), Field::Word(Pair { coarse: 1, fine: 2 })];
    let plan = patched.prepare();
    let mut expected = wash.to_vec();
    expected.extend([100, 101, 102].map(Field::Byte));
    assert_eq!(plan.followers(), expected, "patched");
    // (channel, what it follows): the file's rules reach the tick
    let both = Follows::MASTER.with(Follows::BLACKOUT);
    let cases = [
        (0, Follows::BLACKOUT),
        (1, both),
        (2, both),
        (3, Follows::NONE),
        (9, Follows::NONE),
        (100, both),
    ];
    for (at, follows) in cases {
        let config = plan.configured().iter().find(|c| c.at == at).unwrap();
        assert_eq!(config.follows, follows, "channel {at}");
    }
    let moved = PatchCommand::Move {
        fixture: 1,
        at: at(1, 201),
    };
    let (moved, _) = patched.update(moved, &library).unwrap();
    let mut expected = wash.to_vec();
    expected.extend([200, 201, 202].map(Field::Byte));
    assert_eq!(moved.unwrap().prepare().followers(), expected, "moved");
}

#[test]
fn a_step_binds_its_values_to_the_lights_of_its_target() {
    use engine_core::{Levels, Light, Step};
    let library = library();
    let mut patched = Patch::new(Universes::ONE, Lights::new(16).unwrap(), common::whole());
    for command in [par("PAR 1", 1, 1), par("PAR 2", 1, 4)] {
        patched = patched.update(command, &library).unwrap().0.unwrap();
    }
    let red = Light {
        intensity: Some(0xffff),
        colour: Some(Rgb {
            red: 0xffff,
            green: 0,
            blue: 0,
        }),
        ..Light::DARK
    };
    // Generic-Generic-RGB.qxf, mode RGB: red, green, blue, one byte each. (target, lights
    // it names, the first six bytes)
    let cases = [
        (fixture_(2), 1, [0, 0, 0, 255, 0, 0]),
        (Target::Light { light: 1 }, 1, [255, 0, 0, 0, 0, 0]),
    ];
    for (target, named, bytes) in cases {
        let words = patched
            .bind_step(&Step {
                target,
                values: red,
            })
            .unwrap();
        let mut out = [Levels::ZERO];
        words.offer(patched.prepare().targets(), |field, word| {
            common::offer(&mut out, field, word);
        });
        assert_eq!(words.lights(), named, "{target:?}: lights");
        assert_eq!(out[0].0[..6], bytes, "{target:?}: bytes");
    }
}
