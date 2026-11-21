//! The show file: a patch written and read back is the same patch; a file that fails a
//! check of the commands is not read; one change of a fixture is one line.

use engine_core::{
    Brightness, ChannelNumbers, Color, Light, Lights, ManualEnd, Member, Members, MixRules,
    OtherEnd, PatchCommand, Pixels, Position, Property, Refusal, Rgb, Saved, Target, Text,
    Universes, Unopened, Wheel,
};
use fixture_library::{Fixture, parse};
use looks::Looks;
use rig::{Patch, Whole};
use std::path::Path;

fn text(text: &str) -> Text {
    Text::new(text).unwrap()
}

const fn at(universe: u16, address: u16) -> ChannelNumbers {
    ChannelNumbers { universe, address }
}

fn par(name: &str, address: u16) -> PatchCommand {
    PatchCommand::Patch {
        name: text(name),
        maker: text("Generic"),
        model: text("Generic RGB"),
        mode: text("RGB"),
        at: at(1, address),
    }
}

/// A bar of 8 pixels at 2.1: 9 lights, the bar first.
fn bar() -> PatchCommand {
    PatchCommand::Patch {
        name: text("Bar"),
        maker: text("XStatic"),
        model: text("X-240Bar RGB"),
        mode: text("26 Channel"),
        at: at(2, 1),
    }
}

fn strip(pixels: u16) -> PatchCommand {
    PatchCommand::PatchStrip {
        name: text("Strip"),
        pixels: Pixels::try_from(pixels).unwrap(),
        color: Color::Rgbw,
        at: at(2, 100),
    }
}

fn group(name: &str, members: &[Member]) -> PatchCommand {
    PatchCommand::SetGroup {
        name: text(name),
        members: Members::new(members).unwrap(),
    }
}

const fn f(fixture: u16) -> Member {
    Member::Fixture { fixture }
}

const fn l(light: u16) -> Member {
    Member::Light { light }
}

fn definition(file: &str) -> Fixture {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixture-library/tests/definitions");
    parse(file, &std::fs::read(folder.join(file)).unwrap()).unwrap()
}

fn library() -> Vec<Fixture> {
    ["Generic-Generic-RGB.qxf", "XStatic-X-240Bar-RGB.qxf"]
        .map(definition)
        .into()
}

/// The generic PAR, its RGB mode with another default on red: the library changed.
fn changed_library() -> Vec<Fixture> {
    let mut par = definition("Generic-Generic-RGB.qxf");
    for mode in &mut par.modes {
        mode.channels[0].default = Some(9);
    }
    vec![par]
}

fn empty() -> Patch {
    // no output here: what an output sends is never asked
    let whole = Whole(|_| std::num::NonZeroU16::MIN);
    Patch::new(Universes::new(2).unwrap(), Lights::new(64).unwrap(), whole)
}

#[test]
fn v5_show_with_many_sequences_still_opens() {
    let empty = Patch::new(
        Universes::new(18).unwrap(),
        Lights::MOST,
        Whole(|_| std::num::NonZeroU16::MIN),
    );
    let patch = empty
        .update(
            PatchCommand::PatchStrip {
                name: text("wall"),
                pixels: Pixels::try_from(2972).unwrap(),
                color: Color::Rgb,
                at: at(1, 1),
            },
            &[],
        )
        .unwrap()
        .0
        .unwrap();
    let mut project: serde_json::Value =
        serde_json::from_str(&show_file::write(&patch, &Looks::new()).unwrap()).unwrap();
    project["version"] = 5.into();
    let mut rows = vec![serde_json::json!({
        "look": 1, "name": "red",
        "step": {
            "target": {"Fixture": {"fixture": 1}},
            "values": {"colour": {"red": 65535, "green": 0, "blue": 0}}
        }
    })];
    rows.extend((2..=403).map(|look| {
        serde_json::json!({
            "look": look, "name": format!("cue {look}"),
            "sequence": {"items": [{"look": 1}]}
        })
    }));
    project["looks"] = serde_json::Value::Array(rows);

    let (_, looks) = show_file::read(&project.to_string(), &empty).unwrap();
    assert_eq!(looks.all().len(), 403);
}

/// A command and the library it is sent with.
type Step = (PatchCommand, Vec<Fixture>);

/// `commands` from an empty patch, each with its library; every one is taken.
fn patched(commands: Vec<Step>) -> Patch {
    commands
        .into_iter()
        .fold(empty(), |patch, (command, library)| {
            match patch.update(command, &library) {
                Ok((Some(next), _)) => next,
                Ok((None, _)) => patch,
                Err(reason) => panic!("{command:?}: {reason:?}"),
            }
        })
}

fn lib(command: PatchCommand) -> Step {
    (command, library())
}

#[test]
fn v6_keeps_the_old_channel_schema_for_tagged_emitters() {
    let fixture = definition("Cameo-CLBARL10RGBA.qxf");
    let patch = patched(vec![(
        PatchCommand::Patch {
            name: text("Bar"),
            maker: text(&fixture.maker),
            model: text(&fixture.model),
            mode: text("19-Channel"),
            at: at(1, 37),
        },
        vec![fixture],
    )]);
    let saved = write(&patch).unwrap();
    let show: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(show["version"], 6);
    let mode = show["modes"].as_object().unwrap().values().next().unwrap();
    let channels = mode["mode"]["channels"].as_array().unwrap();
    assert_eq!(channels.len(), 19);
    for channel in channels {
        for field in ["colour", "declared"] {
            assert!(
                channel.get(field).is_none(),
                "{field} on {}",
                channel["name"]
            );
        }
    }
    assert_eq!(write(&read(&saved, &empty()).unwrap()).unwrap(), saved);
}

const V6_DIMMER: &str = r#"{
        "version": 6,
        "patch": [{"fixture":1,"name":"Dimmer","mode":"Generic / Dimmer / 1ch","at":{"universe":1,"address":1},"first":1}],
        "groups": [], "looks": [],
        "rules": {"brightness":"Latest","manual_end":"UntilRelease","other_end":"Reveal"},
        "modes": {"Generic / Dimmer / 1ch": {"maker":"Generic","model":"Dimmer","mode":{
            "name":"1ch","channels":[{"name":"Dimmer","function":"Dimmer","byte":"Coarse","default":null,"direction":null,"ranges":[]}],
            "pixels":[],"layout":null,"degrees":null
        }}}
    }"#;

#[test]
fn a_v6_channel_written_without_declarations_still_opens() {
    let restored = read(V6_DIMMER, &empty()).unwrap();
    let channel = &restored.mode(1).unwrap().channels[0];
    assert_eq!(channel.function, fixture_library::Function::Dimmer);
    assert_eq!(
        channel.role(),
        fixture_library::Function::Other(String::new())
    );
    let original: serde_json::Value = serde_json::from_str(V6_DIMMER).unwrap();
    let written: serde_json::Value = serde_json::from_str(&write(&restored).unwrap()).unwrap();
    assert_eq!(written, original);
}

#[test]
fn v6_rejects_channel_fields_outside_the_v6_schema() {
    for (field, value) in [
        ("colour", serde_json::json!("Red")),
        ("declared", serde_json::json!({})),
        (
            "declared",
            serde_json::json!({
                "preset": "IntensityRed", "group": null, "colour": "Red", "smoke": false
            }),
        ),
    ] {
        let mut show: serde_json::Value = serde_json::from_str(V6_DIMMER).unwrap();
        show["modes"]["Generic / Dimmer / 1ch"]["mode"]["channels"][0][field] = value;
        assert!(
            matches!(
                read(&show.to_string(), &empty()),
                Err(Unopened::Broken { .. })
            ),
            "v6 accepted channel field {field}"
        );
    }
}

/// V6 keeps the mode and all of its v11 writers; declarations first persist in v7.
fn assert_v6_patch(read: &Patch, original: &Patch) {
    assert_eq!(read.writers(), original.writers());
    assert_eq!(read.groups(), original.groups());
    assert_eq!(read.fixtures(), original.fixtures());
    for fixture in original.fixtures() {
        let number = fixture.fixture;
        let mut mode = original.mode(number).unwrap().clone();
        for channel in &mut mode.channels {
            channel.declared = Default::default();
        }
        assert_eq!(read.mode(number), Some(&mode));
        assert_eq!(
            read.definition_text(number),
            original.definition_text(number)
        );
    }
}

#[test]
fn a_patch_written_and_read_back_is_the_same_patch() {
    let cases: Vec<(&str, Vec<Step>)> = vec![
        ("empty", vec![]),
        ("one PAR", vec![lib(par("PAR 1", 37))]),
        (
            "three PARs, the first unpatched: numbers 2 and 3, lights 2 and 3",
            vec![
                lib(par("PAR 1", 1)),
                lib(par("PAR 2", 4)),
                lib(par("PAR 3", 7)),
                lib(PatchCommand::Unpatch { fixture: 1 }),
            ],
        ),
        (
            "unpatched, then a bar: number 1, lights from 4",
            vec![
                lib(par("PAR 1", 1)),
                lib(par("PAR 2", 4)),
                lib(PatchCommand::Unpatch { fixture: 1 }),
                lib(bar()),
            ],
        ),
        (
            "groups in their order, of fixtures and lights",
            vec![
                lib(par("PAR 1", 1)),
                lib(par("PAR 2", 4)),
                lib(bar()),
                lib(group("front", &[f(2), f(1)])),
                lib(group("pixels", &[l(5), l(4), f(3)])),
                lib(group("empty", &[])),
            ],
        ),
        ("a strip, no definition", vec![lib(strip(30))]),
        (
            "one fixture in two versions of its mode",
            vec![
                lib(par("PAR 1", 1)),
                (par("PAR 2", 4), changed_library()),
                lib(par("PAR 3", 7)),
            ],
        ),
    ];
    for (case, commands) in cases {
        let patch = patched(commands);
        let file = write(&patch).unwrap();
        let read = read(&file, &empty()).unwrap_or_else(|why| panic!("{case}: {why:?}\n{file}"));
        assert_v6_patch(&read, &patch);
        assert_eq!(write(&read).unwrap(), file, "{case}: written again");
    }
}

#[test]
fn a_file_that_fails_a_check_of_the_commands_is_not_read() {
    let two = patched(vec![lib(par("PAR 1", 1)), lib(par("PAR 2", 4))]);
    let file = write(&two).unwrap();
    let row1 = r#"{"fixture":1,"name":"PAR 1","mode":"Generic / Generic RGB / RGB","at":{"universe":1,"address":1},"first":1}"#;
    let row2 = r#"{"fixture":2,"name":"PAR 2","mode":"Generic / Generic RGB / RGB","at":{"universe":1,"address":4},"first":2}"#;
    assert!(file.contains(row1) && file.contains(row2), "{file}");
    let long = "x".repeat(Text::MOST + 1);
    let version = format!("\"version\": {}", show_file::VERSION);
    let newer = format!("\"version\": {}", show_file::VERSION + 1);
    let versioned = format!("{version},\n");
    let refused = |reason| Unopened::Refused { reason };
    // (case, what is replaced, by what, why the file is not read)
    let cases = [
        (
            "a newer version",
            version.as_str(),
            newer.as_str(),
            Unopened::Newer {
                version: show_file::VERSION + 1,
            },
        ),
        (
            "version 0",
            version.as_str(),
            r#""version": 0"#,
            Unopened::Broken {
                line: 2,
                column: 14,
            },
        ),
        (
            "no version",
            versioned.as_str(),
            "",
            Unopened::Broken {
                line: 47,
                column: 1,
            },
        ),
        (
            "not JSON",
            "\"groups\": []",
            "\"groups\": [",
            Unopened::Broken {
                line: 7,
                column: 14,
            },
        ),
        (
            "a key it does not know",
            "\"groups\": []",
            "\"groups\": [], \"outputs\": []",
            Unopened::Broken {
                line: 7,
                column: 25,
            },
        ),
        (
            "fixture 0",
            r#""fixture":2"#,
            r#""fixture":0"#,
            Unopened::Broken {
                line: 5,
                column: 16,
            },
        ),
        (
            "a name too long",
            r#""name":"PAR 2""#,
            &format!(r#""name":"{long}""#),
            Unopened::Broken {
                line: 5,
                column: 155,
            },
        ),
        (
            "no such mode",
            r#""fixture":2,"name":"PAR 2","mode":"Generic"#,
            r#""fixture":2,"name":"PAR 2","mode":"Other"#,
            refused(Refusal::NoSuchMode),
        ),
        (
            "no such universe",
            r#""universe":1,"address":4"#,
            r#""universe":3,"address":4"#,
            refused(Refusal::NoSuchUniverse { universes: 2 }),
        ),
        (
            "past the end",
            r#""universe":1,"address":4"#,
            r#""universe":2,"address":511"#,
            refused(Refusal::PastTheEnd { channels: 3 }),
        ),
        (
            "light 0",
            r#""first":2"#,
            r#""first":0"#,
            refused(Refusal::NoLightsLeft),
        ),
        (
            "past the last light",
            r#""first":2"#,
            r#""first":65"#,
            refused(Refusal::NoLightsLeft),
        ),
        (
            "on the channels of another",
            r#""universe":1,"address":4"#,
            r#""universe":1,"address":3"#,
            refused(Refusal::Overlaps { fixture: 1 }),
        ),
        (
            "the number of another",
            r#""fixture":2"#,
            r#""fixture":1"#,
            Unopened::Taken { fixture: 1 },
        ),
        (
            "a light of another",
            r#""first":2"#,
            r#""first":1"#,
            Unopened::Taken { fixture: 2 },
        ),
        (
            "a group of a fixture not patched",
            "\"groups\": []",
            r#""groups": [{"name":"front","members":[{"Fixture":{"fixture":9}}]}]"#,
            refused(Refusal::NotPatched),
        ),
        (
            "a group of a light no fixture has",
            "\"groups\": []",
            r#""groups": [{"name":"front","members":[{"Light":{"light":3}}]}]"#,
            refused(Refusal::NoSuchLight),
        ),
    ];
    let wrong: Vec<String> = cases
        .into_iter()
        .filter_map(|(case, from, to, why)| {
            assert!(file.contains(from), "{case}: {from}");
            let read = read(&file.replacen(from, to, 1), &empty());
            (read != Err(why)).then(|| format!("{case}: {read:?}, wanted {why:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_change_of_one_fixture_is_one_line_of_the_file() {
    let before = patched(vec![
        lib(par("PAR 1", 1)),
        lib(par("PAR 2", 4)),
        lib(par("PAR 3", 7)),
        lib(group("front", &[f(3), f(1)])),
    ]);
    let cases = [
        (
            "move",
            PatchCommand::Move {
                fixture: 2,
                at: at(1, 100),
            },
        ),
        (
            "rename",
            PatchCommand::Rename {
                fixture: 2,
                name: text("Back"),
            },
        ),
        ("group", group("front", &[f(3), f(1), f(2)])),
    ];
    let old = write(&before).unwrap();
    for (case, command) in cases {
        let (after, _) = before.update(command, &[]).unwrap();
        let new = write(&after.unwrap()).unwrap();
        let changed = old.lines().zip(new.lines()).filter(|(a, b)| a != b).count();
        assert_eq!(old.lines().count(), new.lines().count(), "{case}\n{new}");
        assert_eq!(changed, 1, "{case}\n{new}");
    }
}

/// One PAR whose red channel's name is `extra` bytes longer: a file of any length.
fn par_with_a_longer_name(extra: usize) -> Patch {
    let mut library = library();
    for mode in &mut library[0].modes {
        mode.channels[0].name.push_str(&"x".repeat(extra));
    }
    patched(vec![(par("PAR 1", 1), library)])
}

#[test]
fn a_file_of_the_most_bytes_is_written_and_read_and_one_byte_more_is_neither() {
    let most = show_file::MOST_BYTES;
    let short = write(&par_with_a_longer_name(0)).unwrap().len();
    let at_most = par_with_a_longer_name(most - short);
    let file = write(&at_most).unwrap();
    assert_eq!(file.len(), most);
    let too_big = Unopened::TooBig {
        most: u32::try_from(most).unwrap(),
    };
    // (case, bytes over the most, written, read)
    let cases = [
        ("at the most", 0, Some(most), Ok(())),
        ("one byte over", 1, None, Err(too_big)),
    ];
    for (case, over, written, read) in cases {
        let patch = par_with_a_longer_name(most - short + over);
        assert_eq!(
            write(&patch).map(|text| text.len()),
            written,
            "{case}: written"
        );
        // the file at the most, and spaces after it
        let text = format!("{file}{}", " ".repeat(over));
        let opened = crate::read(&text, &empty()).map(|read| assert_v6_patch(&read, &at_most));
        assert_eq!(opened, read, "{case}: read");
    }
}

#[test]
fn a_patch_kept_in_its_file_refuses_the_command_that_would_take_the_file_over_the_most() {
    // the patch leaves the room every look may take
    let most = show_file::LIMIT.most;
    let short = write(&par_with_a_longer_name(0)).unwrap().len();
    let none = write(&empty()).unwrap().len();
    // the file at the most, with the PAR named "PAR 1"
    let full = par_with_a_longer_name(most - short).kept_in(show_file::LIMIT);
    let rename = |name: &str| PatchCommand::Rename {
        fixture: 1,
        name: text(name),
    };
    let refused = Err(Refusal::ShowFileFull {
        most: u32::try_from(most).unwrap(),
    });
    // (case, command, the file's bytes after it, or why refused)
    let cases = [
        ("renamed as long: at the most", rename("PAR 2"), Ok(most)),
        ("renamed one byte longer", rename("PAR 12"), refused),
        ("renamed one byte shorter", rename("PAR"), Ok(most - 2)),
        ("a group more", group("front", &[f(1)]), refused),
        ("a fixture more", par("PAR 2", 4), refused),
        ("unpatched", PatchCommand::Unpatch { fixture: 1 }, Ok(none)),
    ];
    for (case, command, bytes) in cases {
        let after = full.update(command, &library());
        let file = after.map(|(patch, _)| {
            let patch = patch.unwrap();
            let file = write(&patch).unwrap();
            let read = read(&file, &empty()).unwrap();
            assert_v6_patch(&read.kept_in(show_file::LIMIT), &patch);
            file.len()
        });
        assert_eq!(file, bytes, "{case}");
    }
}

#[test]
fn a_file_under_the_most_whose_patch_is_written_back_over_it_is_not_read() {
    let most = show_file::MOST_BYTES;
    let short = write(&par_with_a_longer_name(0)).unwrap().len();
    let full = write(&par_with_a_longer_name(most - short)).unwrap();
    // one byte more in the name, two spaces less before "patch": the file is under the
    // most, its patch is written one byte over it
    let packed = full
        .replacen("xx", "xxx", 1)
        .replacen("\n  \"patch\"", "\n\"patch\"", 1);
    assert_eq!(packed.len(), most - 1);
    let too_big = Unopened::TooBig {
        most: u32::try_from(most).unwrap(),
    };
    assert_eq!(read(&packed, &empty()), Err(too_big));
}

static ENCODED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The show file's limit, counting each definition it encodes.
const COUNTING: rig::Limit = rig::Limit {
    encode: |maker, model, mode| {
        ENCODED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        (show_file::LIMIT.encode)(maker, model, mode)
    },
    ..show_file::LIMIT
};

#[test]
fn a_definition_is_encoded_once_when_patched_and_no_change_after_it_encodes_a_mode() {
    use std::sync::atomic::Ordering::SeqCst;
    let library = library();
    let step = |patch: &Patch, command| {
        let (next, _) = patch.update(command, &library).unwrap();
        next.unwrap_or_else(|| patch.clone())
    };
    let kept = step(&empty().kept_in(COUNTING), par("PAR 1", 1));
    assert_eq!(ENCODED.load(SeqCst), 1, "patched: encoded once");
    // (case, command): none encodes, and the file is the file of a patch that was not kept
    let cases = [
        (
            "renamed",
            PatchCommand::Rename {
                fixture: 1,
                name: text("PAR 2"),
            },
        ),
        (
            "moved",
            PatchCommand::Move {
                fixture: 1,
                at: at(1, 20),
            },
        ),
        (
            "a group",
            PatchCommand::SetGroup {
                name: text("front"),
                members: Members::new(&[Member::Fixture { fixture: 1 }]).unwrap(),
            },
        ),
    ];
    let mut patch = kept;
    for (case, command) in cases {
        patch = step(&patch, command);
        assert_eq!(ENCODED.load(SeqCst), 1, "{case}: encoded nothing");
    }
    let plain = write(&patch).unwrap();
    let same = ["PAR 2", "front"].iter().all(|n| plain.contains(n));
    assert!(same, "the file has the changes");
    assert!(show_file::LIMIT.most >= plain.len());
}

/// The text of `patch` with no looks.
fn write(patch: &Patch) -> Option<String> {
    show_file::write(patch, &Looks::new())
}

/// The patch of `text`; its looks are tested in `looks.rs`.
fn read(text: &str, empty: &Patch) -> Result<Patch, Unopened> {
    show_file::read(text, empty).map(|(patch, _)| patch)
}

fn saved(look: u16, target: Target, values: Light) -> Saved {
    Saved {
        look,
        name: text("red"),
        step: engine_core::Step { target, values },
    }
}

const RED: Light = Light {
    colour: Some(Rgb {
        red: u16::MAX,
        green: 0,
        blue: 0,
    }),
    ..Light::DARK
};

#[test]
fn looks_written_and_read_back_are_the_same_looks() {
    let patch = patched(vec![lib(par("PAR 1", 1))]);
    let fixture = Target::Fixture { fixture: 1 };
    let zero = Light {
        intensity: Some(0),
        colour: Some(Rgb {
            red: 0,
            green: 0,
            blue: 0,
        }),
        ..Light::DARK
    };
    // (case, looks)
    let cases = [
        ("none", vec![]),
        ("red", vec![saved(1, fixture, RED)]),
        ("zero and black", vec![saved(7, fixture, zero)]),
        (
            "a rest",
            vec![saved(2, Target::Light { light: 1 }, Light::DARK)],
        ),
        ("two", vec![saved(9, fixture, RED), saved(1, fixture, zero)]),
    ];
    for (case, all) in cases {
        let looks = all
            .iter()
            .try_fold(Looks::new(), |looks, saved| looks.put(*saved, &patch))
            .unwrap();
        let file = show_file::write(&patch, &looks).unwrap();
        let (_, read) = show_file::read(&file, &empty()).unwrap();
        assert_eq!(read, looks, "{case}");
    }
}

/// `file`, as written now, as version 1 wrote it: no looks and no rules.
fn as_version_1(file: &str) -> String {
    let mut v1 = file.to_string();
    for (line, to) in [
        (
            format!("  \"version\": {},\n", show_file::VERSION),
            "  \"version\": 1,\n",
        ),
        ("  \"looks\": [],\n".to_string(), ""),
        (
            format!(
                "  \"rules\": {},\n",
                r#"{"brightness":"Latest","manual_end":"UntilRelease","other_end":"Reveal"}"#
            ),
            "",
        ),
    ] {
        assert!(v1.contains(&line), "{line} in {file}");
        v1 = v1.replacen(&line, to, 1);
    }
    v1
}

#[test]
fn a_file_of_version_1_has_no_looks() {
    let patch = patched(vec![lib(par("PAR 1", 1))]);
    let v1 = as_version_1(&show_file::write(&patch, &Looks::new()).unwrap());
    let (read, looks) = show_file::read(&v1, &empty()).unwrap();
    assert_v6_patch(&read, &patch);
    assert_eq!(looks, Looks::new());
}

#[test]
fn sequence_rows_first_open_in_version_5() {
    let patch = patched(vec![lib(par("PAR", 1))]);
    let mut file: serde_json::Value = serde_json::from_str(&write(&patch).unwrap()).unwrap();
    file["looks"] = serde_json::json!([
        {"look": 1, "name": "red", "step": {"target": {"Fixture": {"fixture": 1}}, "values": RED}},
        {"look": 2, "name": "sequence", "sequence": {"items": [{"look": 1}]}}
    ]);
    for (version, opens) in [(4, false), (5, true)] {
        file["version"] = version.into();
        let opened = show_file::read(&file.to_string(), &empty());
        if opens {
            assert!(opened.unwrap().1.plan(2).is_ok(), "v{version}");
        } else {
            assert!(
                matches!(opened, Err(Unopened::Broken { .. })),
                "v{version}: {opened:?}"
            );
        }
    }
}

#[test]
fn a_look_the_patch_refuses_is_not_read() {
    let patch = patched(vec![lib(par("PAR 1", 1))]);
    let looks = Looks::new()
        .put(saved(1, Target::Fixture { fixture: 1 }, RED), &patch)
        .unwrap();
    let file = show_file::write(&patch, &looks).unwrap();
    let refused = |reason| Err(Unopened::Refused { reason });
    let row = r#"{"look":1,"name":"red","step":{"target":{"Fixture":{"fixture":1}}"#;
    assert!(file.contains(row), "{file}");
    let whole = file.lines().find(|line| line.contains(row)).unwrap().trim();
    // (case, the look's line becomes, why the file is not read)
    let cases = [
        (
            "a fixture not patched",
            row.replace("\"fixture\":1}", "\"fixture\":2}"),
            refused(Refusal::NotPatched),
        ),
        (
            "a group there is none of",
            row.replace(
                r#"{"Fixture":{"fixture":1}}"#,
                r#"{"Group":{"group":"front"}}"#,
            ),
            refused(Refusal::NoSuchGroup),
        ),
        (
            "two of one number",
            format!("{whole},\n    {row}"),
            refused(Refusal::LookInUse { look: 1 }),
        ),
    ];
    for (case, to, why) in cases {
        let read = show_file::read(&file.replacen(row, &to, 1), &empty());
        assert_eq!(read.map(|_| ()), why, "{case}");
    }
}

#[test]
fn a_file_one_byte_over_the_most_is_not_read_in_any_version() {
    let most = show_file::MOST_BYTES;
    let too_big = Err(Unopened::TooBig {
        most: u32::try_from(most).unwrap(),
    });
    let short = write(&par_with_a_longer_name(0)).unwrap().len();
    let at_most = write(&par_with_a_longer_name(most - short)).unwrap();
    assert_eq!(at_most.len(), most);
    assert!(read(&at_most, &empty()).is_ok());
    // one byte longer: as it is, and as version 1, which is shorter on disk but is
    // written back as the version now
    let over = at_most.replacen("\"name\": \"Red", "\"name\": \"Redx", 1);
    assert_eq!(over.len(), most + 1);
    let v1 = as_version_1(&over);
    assert!(v1.len() <= most, "{}", v1.len());
    assert_eq!(read(&over, &empty()), too_big);
    assert_eq!(read(&v1, &empty()), too_big);
}

#[test]
fn the_longest_look_fits_its_room() {
    let escaped = "\u{1}".repeat(Text::MOST);
    let longest = Saved {
        look: u16::MAX,
        name: text(&escaped),
        step: engine_core::Step {
            target: Target::Group {
                group: text(&escaped),
            },
            values: Light {
                intensity: Some(u16::MAX),
                colour: Some(Rgb {
                    red: u16::MAX,
                    green: u16::MAX,
                    blue: u16::MAX,
                }),
                position: Some(Position {
                    pan: u16::MAX,
                    tilt: u16::MAX,
                }),
                zoom: Some(u16::MAX),
                gobos: [Some(u8::MAX); Wheel::COUNT],
            },
        },
    };
    let line = serde_json::to_string(&longest).unwrap();
    assert!(line.len() <= show_file::LOOK_MOST, "{}", line.len());
}

#[test]
fn the_mixer_settings_are_saved_and_a_file_without_them_has_the_defaults() {
    let changed = MixRules {
        brightness: Brightness::Highest,
        manual_end: ManualEnd::UntilStop,
        other_end: OtherEnd::Stay,
    };
    // (settings saved, the version the file says, whether it has the rules, read back)
    let cases = [
        (MixRules::default(), 3, true, MixRules::default()),
        (changed, 3, true, changed),
        (changed, 2, false, MixRules::default()),
        (changed, 1, false, MixRules::default()),
    ];
    for (rules, version, kept, back) in cases {
        let mut file = show_file::write(&empty(), &Looks::new().with_rules(rules)).unwrap();
        if !kept {
            let start = file.find("  \"rules\"").unwrap();
            let end = start + file[start..].find('\n').unwrap() + 1;
            file.replace_range(start..end, "");
        }
        let now = format!("\"version\": {}", show_file::VERSION);
        let file = file.replace(&now, &format!("\"version\": {version}"));
        let (_, looks) = show_file::read(&file, &empty()).unwrap();
        assert_eq!(looks.rules(), back, "{rules:?} in version {version}");
    }
}

/// The Shehds spot (gobo wheels 1 and 2) at 1.1, a PAR (no wheel) at 1.50, and the Shehds
/// with two more copies of its first wheel, wheels 3 and 4, at 1.100.
fn wheels() -> Patch {
    let shehds = definition("Shehds-LED-Spot-100W.qxf");
    let mut four = shehds.clone();
    four.model = "LED Spot 100W, four wheels".to_string();
    for mode in &mut four.modes {
        let first = mode.channels[6].clone();
        mode.channels.extend([first.clone(), first]);
    }
    let spot = |name, model: &str, address| {
        let command = PatchCommand::Patch {
            name: text(name),
            maker: text("Shehds"),
            model: text(model),
            mode: text("14 Channel"),
            at: at(1, address),
        };
        (command, vec![shehds.clone(), four.clone()])
    };
    patched(vec![
        spot("Spot", "LED Spot 100W", 1),
        lib(par("PAR", 50)),
        spot("Four", "LED Spot 100W, four wheels", 100),
    ])
}

/// A look of version 3 holds one gobo, a wheel and a slot: read and checked as version 3
/// read and checked it (the reader of 5ae0048 ran each text), as the slot of its wheel
/// (#179). A wheel no light shows now is refused where a light has a gobo wheel.
#[test]
fn a_gobo_of_version_3_is_read_as_version_3_read_it() {
    let patch = wheels();
    let gobos = |gobos| Light {
        gobos,
        ..Light::DARK
    };
    let written = |fixture| {
        let look = saved(1, Target::Fixture { fixture }, Light::DARK);
        let looks = Looks::new().put(look, &patch).unwrap();
        show_file::write(&patch, &looks).unwrap()
    };
    let now = format!("\"version\": {}", show_file::VERSION);
    let none = r#""gobos":[null,null,null]"#;
    let values =
        r#"{"intensity":null,"colour":null,"position":null,"zoom":null,"gobos":[null,null,null]}"#;
    let refused = |light, wheel| {
        Err(Unopened::Refused {
            reason: Refusal::Unsupported {
                light,
                property: Property::Gobo(Wheel::new(wheel).unwrap()),
            },
        })
    };
    let broken = |line, column| Err(Unopened::Broken { line, column });
    // (case, fixture, the text of version 4, the text of version 3 in its place, read back)
    type Case<'a> = (&'a str, u16, &'a str, &'a str, Result<Light, Unopened>);
    let cases: [Case; 19] = [
        (
            "wheel 1",
            1,
            none,
            r#""gobo":{"wheel":1,"slot":3}"#,
            Ok(gobos([Some(3), None, None])),
        ),
        (
            "wheel 2",
            1,
            none,
            r#""gobo":{"wheel":2,"slot":7}"#,
            Ok(gobos([None, Some(7), None])),
        ),
        ("no gobo", 1, none, r#""gobo":null"#, Ok(Light::DARK)),
        ("no key", 1, &format!(",{none}"), "", Ok(Light::DARK)),
        // a light with a gobo wheel refuses a gobo it cannot show
        (
            "no wheel 3 on the spot",
            1,
            none,
            r#""gobo":{"wheel":3,"slot":1}"#,
            refused(1, 3),
        ),
        (
            "no slot 40",
            1,
            none,
            r#""gobo":{"wheel":1,"slot":40}"#,
            refused(1, 1),
        ),
        (
            "slot 0",
            1,
            none,
            r#""gobo":{"wheel":1,"slot":0}"#,
            refused(1, 1),
        ),
        // no light shows a wheel past 3 now: named as wheel 3
        (
            "a fifth wheel on the spot",
            1,
            none,
            r#""gobo":{"wheel":5,"slot":2}"#,
            refused(1, 3),
        ),
        (
            "wheel 4 of four",
            3,
            none,
            r#""gobo":{"wheel":4,"slot":1}"#,
            refused(3, 3),
        ),
        (
            "wheel 3 of four",
            3,
            none,
            r#""gobo":{"wheel":3,"slot":3}"#,
            Ok(gobos([None, None, Some(3)])),
        ),
        // a light without a gobo wheel showed nothing: a gobo no wheel holds is left out
        (
            "a fifth wheel on the PAR",
            2,
            none,
            r#""gobo":{"wheel":5,"slot":2}"#,
            Ok(Light::DARK),
        ),
        (
            "slot 0 on the PAR",
            2,
            none,
            r#""gobo":{"wheel":2,"slot":0}"#,
            Ok(Light::DARK),
        ),
        (
            "wheel 2 on the PAR",
            2,
            none,
            r#""gobo":{"wheel":2,"slot":7}"#,
            Ok(gobos([None, Some(7), None])),
        ),
        // every shape version 3 read
        (
            "values in order",
            1,
            values,
            r#"[null,null,null,null,{"wheel":1,"slot":3}]"#,
            Ok(gobos([Some(3), None, None])),
        ),
        (
            "a gobo in order",
            1,
            none,
            r#""gobo":[2,7]"#,
            Ok(gobos([None, Some(7), None])),
        ),
        // and what it did not
        (
            "a slot twice",
            1,
            none,
            r#""gobo":{"wheel":2,"slot":7,"slot":1}"#,
            broken(10, 172),
        ),
        (
            "gobos",
            1,
            none,
            r#""gobos":[null,7,null]"#,
            broken(10, 146),
        ),
        (
            "gobo and gobos",
            1,
            none,
            r#""gobo":null,"gobos":[null,7,null]"#,
            broken(10, 158),
        ),
        (
            "not a gobo",
            1,
            none,
            r#""gobo":{"wheel":1}"#,
            broken(10, 157),
        ),
    ];
    for (case, fixture, from, to, back) in cases {
        let file = written(fixture);
        assert!(file.contains(from), "{file}");
        let v3 = file.replace(&now, "\"version\": 3").replace(from, to);
        let read = show_file::read(&v3, &empty()).map(|(_, looks)| looks);
        let expected = back.map(|light| {
            let look = saved(1, Target::Fixture { fixture }, light);
            Looks::new().put(look, &patch).unwrap()
        });
        assert_eq!(read, expected, "{case}");
    }
}

/// A file of version 3 the reader of version 3 did not read is not read, broken where the
/// text is: the looks twice, and a broken look after a look with a gobo.
#[test]
fn a_file_of_version_3_broken_is_broken_where_the_text_is() {
    let patch = wheels();
    let fixture = Target::Fixture { fixture: 1 };
    let looks = [1, 2].into_iter().fold(Looks::new(), |looks, look| {
        looks
            .put(saved(look, fixture, Light::DARK), &patch)
            .unwrap()
    });
    let file = show_file::write(&patch, &looks).unwrap();
    let now = format!("\"version\": {}", show_file::VERSION);
    let v3 = file.replace(&now, "\"version\": 3");
    let none = r#""gobos":[null,null,null]"#;
    let gobo = r#""gobo":{"wheel":1,"slot":3}"#;
    // (case, version 3, where it is broken)
    let cases = [
        (
            "the looks twice",
            v3.replace(none, r#""gobo":null"#)
                .replace("\"rules\": ", "\"looks\": [],\n  \"rules\": "),
            (13, 9),
        ),
        (
            "look 2 broken after a gobo in look 1",
            v3.replacen(none, gobo, 1)
                .replacen(none, r#""gobo":{"wheel":1,"slot":"3"}"#, 1),
            (11, 167),
        ),
    ];
    for (case, text, (line, column)) in cases {
        assert_eq!(
            show_file::read(&text, &empty()),
            Err(Unopened::Broken { line, column }),
            "{case}"
        );
    }
}
