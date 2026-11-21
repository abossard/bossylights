//! Check the channel model against the declarations in every fixture and head.
//!
//! An independent oracle: it never asks the lowering's own pairing key. It reads only
//! what a definition declares (preset, group, `<Colour>`, capability ranges, heads, the
//! fixture's type) and checks every pair and every follows byte against it.
mod common;

use fixture_library::{Byte, Channel, Does, Function, Kind, Mode, parse};
use rig::channels::{Follows, Lowered, lower};
use std::mem::{Discriminant, discriminant};
use std::path::{Path, PathBuf};
use std::{env, fs, io};

/// QXF files of the pinned QLC+ library. A smaller `QLCPLUS_FIXTURES` corpus is refused.
const PINNED: usize = 1782;

fn qxf_files(folder: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            qxf_files(&path, files)?;
        } else if path.extension().is_some_and(|e| e == "qxf") {
            files.push(path);
        }
    }
    Ok(())
}

fn stem(c: &Channel) -> Option<&str> {
    c.declared
        .preset
        .as_deref()
        .map(|p| p.strip_suffix("Fine").unwrap_or(p))
}

/// A preset without the way it runs (`BeamZoomSmallBig` is `BeamZoom`).
fn without_way(stem: &str) -> &str {
    [
        "SlowFast", "FastSlow", "SmallBig", "BigSmall", "NearFar", "FarNear", "MinToMax",
        "MaxToMin",
    ]
    .into_iter()
    .find_map(|way| stem.strip_suffix(way))
    .unwrap_or(stem)
}

/// The QLC+ group: the `<Group>` text, or the group a preset belongs to.
fn group(c: &Channel) -> Option<&str> {
    if let Some(group) = &c.declared.group {
        return Some(group);
    }
    let preset = stem(c)?;
    match preset {
        "PositionXAxis" => Some("Pan"),
        "PositionYAxis" => Some("Tilt"),
        "NoFunction" => Some("Nothing"),
        _ => [
            ("Intensity", "Intensity"),
            ("PositionPan", "Pan"),
            ("PositionTilt", "Tilt"),
            ("Speed", "Speed"),
            ("Color", "Colour"),
            ("Gobo", "Gobo"),
            ("Shutter", "Shutter"),
            ("Beam", "Beam"),
            ("Prism", "Prism"),
        ]
        .into_iter()
        .find_map(|(prefix, group)| preset.starts_with(prefix).then_some(group)),
    }
}

/// The colour a channel emits or subtracts: its `<Colour>`, else its Intensity colour preset.
fn colour(c: &Channel) -> Option<Function> {
    use Function::{Amber, Blue, Cyan, Green, Indigo, Lime, Magenta, Red, Uv, White, Yellow};
    if let Some(colour) = &c.declared.colour {
        return Some(colour.clone());
    }
    Some(match stem(c)? {
        "IntensityRed" => Red,
        "IntensityGreen" => Green,
        "IntensityBlue" => Blue,
        "IntensityWhite" => White,
        "IntensityAmber" => Amber,
        "IntensityUV" => Uv,
        "IntensityLime" => Lime,
        "IntensityIndigo" => Indigo,
        "IntensityCyan" => Cyan,
        "IntensityMagenta" => Magenta,
        "IntensityYellow" => Yellow,
        _ => return None,
    })
}

/// What each range does, by kind, in order. A range with no capability preset is `None`.
fn kinds(c: &Channel) -> Vec<Option<Discriminant<Does>>> {
    c.ranges
        .iter()
        .map(|r| r.does.as_ref().map(discriminant))
        .collect()
}

/// A wheel selector: a range picks a gobo, a colour or a prism. A preset name alone
/// does not say it (Briteq BTX Saturn calls its rotation fine `GoboWheelFine`).
fn wheel(c: &Channel) -> bool {
    c.ranges.iter().any(|r| {
        matches!(
            r.does,
            Some(
                Does::Gobo { .. }
                    | Does::Color(_)
                    | Does::Colors(..)
                    | Does::Prism { .. }
                    | Does::PrismOff
            )
        )
    })
}

/// An index: its preset is an index, or a range turns or indexes.
fn index(c: &Channel) -> bool {
    stem(c).is_some_and(|s| s.ends_with("Index"))
        || c.ranges
            .iter()
            .any(|r| matches!(r.does, Some(Does::Rotation(_))))
}

/// What the ranges do, by kind, in order. No range and one range that names nothing
/// are one shape: a plain byte.
fn shape(c: &Channel) -> Vec<Option<Discriminant<Does>>> {
    match kinds(c).as_slice() {
        [] | [None] => Vec::new(),
        kinds => kinds.to_vec(),
    }
}

/// The pairs a mode must have, as (coarse, fine), from the definition alone. A fine joins
/// only a coarse it certainly belongs to; every other fine stays a byte of its own.
///
/// Both join only within the same heads (a fine in no head joins a coarse in no head),
/// with the same colour, a coarse no earlier fine took, and never an index to a wheel
/// selector. Then:
/// - a fine with a preset joins the coarse right before it with that preset (a coarse
///   preset names the way it runs, its fine does not: `BeamZoomSmallBig`, `BeamZoomFine`),
///   unless the channel before that coarse looks the same; else the one coarse with that
///   preset, unless the channel right before the fine is another coarse of its group.
///   Two such coarses and neither right before: no pair. None: as a fine with no preset,
///   in the group of its preset;
/// - a fine with no preset joins the channel right before it when that is a coarse of
///   the same group that it fits (a plain fine fits any coarse, a fine with steps only
///   the same steps), and the channel before that coarse does not look the same (same
///   group, preset and shape: Shutter B, C, D coarse, then B, C, D fine).
fn expected_pairs(mode: &Mode, heads: &[Vec<usize>]) -> Vec<(usize, usize)> {
    let channels = &mode.channels;
    let only_index = |c: &Channel| index(c) && !wheel(c);
    let only_wheel = |c: &Channel| wheel(c) && !index(c);
    let mut taken = vec![false; channels.len()];
    let mut pairs = Vec::new();
    for (i, fine) in channels.iter().enumerate() {
        if fine.byte != Byte::Fine {
            continue;
        }
        let free = |j: usize| {
            let coarse = &channels[j];
            coarse.byte == Byte::Coarse
                && !taken[j]
                && heads[j] == heads[i]
                && colour(coarse) == colour(fine)
        };
        let same_group = |j: usize| group(fine).is_some() && group(&channels[j]) == group(fine);
        let mixes = |j: usize| {
            let coarse = &channels[j];
            (only_index(fine) && only_wheel(coarse)) || (only_wheel(fine) && only_index(coarse))
        };
        let before = i.checked_sub(1);
        let steps = shape(fine);
        let fits = |j: usize| {
            free(j)
                && same_group(j)
                && !mixes(j)
                && (steps.is_empty() || shape(&channels[j]) == steps)
        };
        let alike = |j: usize, k: usize| {
            free(k)
                && group(&channels[k]) == group(&channels[j])
                && stem(&channels[k]) == stem(&channels[j])
                && shape(&channels[k]) == shape(&channels[j])
        };
        let unlike_before = |j: usize| j.checked_sub(1).is_none_or(|k| !alike(j, k));
        let adjacent = || before.filter(|&j| fits(j) && unlike_before(j));
        let partner = match stem(fine).map(without_way) {
            None => adjacent(),
            Some(preset) => {
                let same: Vec<_> = (0..channels.len())
                    .filter(|&j| {
                        free(j) && !mixes(j) && stem(&channels[j]).map(without_way) == Some(preset)
                    })
                    .collect();
                match same.as_slice() {
                    [] => adjacent(),
                    _ if before.is_some_and(|b| same.contains(&b) && unlike_before(b)) => before,
                    &[j] if before.is_none_or(|b| !(free(b) && same_group(b))) => Some(j),
                    _ => None,
                }
            }
        };
        if let Some(j) = partner {
            taken[j] = true;
            pairs.push((j, i));
        }
    }
    pairs.sort_unstable();
    pairs
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Light {
    Dimmer,
    Emitter,
    Shutter,
    Independent,
}

/// What a channel does to the light, from its declarations and the fixture's type.
/// On a smoke machine or hazer a dimmer preset is the fog output, unless it certainly
/// dims emitters: it shares a head with one, or the mode has no heads and has emitters.
/// On a dimmer pack every group-only Intensity channel is a dimmer.
fn light(c: &Channel, kind: &Kind, dims_emitters: bool) -> Light {
    if colour(c).is_some() {
        return Light::Emitter;
    }
    let fog = matches!(kind, Kind::Smoke | Kind::Hazer) && !dims_emitters;
    match stem(c) {
        Some("IntensityDimmer" | "IntensityMasterDimmer") if !fog => Light::Dimmer,
        None if c.declared.group.as_deref() == Some("Intensity") && *kind == Kind::Dimmer => {
            Light::Dimmer
        }
        None if group(c) == Some("Shutter")
            && c.ranges.iter().any(|r| r.does == Some(Does::Closed)) =>
        {
            Light::Shutter
        }
        _ => Light::Independent,
    }
}

fn non_light_name(c: &Channel) -> bool {
    c.name
        .to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| {
            matches!(
                word,
                "fog"
                    | "haze"
                    | "hazer"
                    | "smoke"
                    | "fan"
                    | "pump"
                    | "curve"
                    | "curves"
                    | "sensitivity"
            )
        })
}

fn check_mode(mode: &Mode, kind: &Kind, result: &Lowered) -> Vec<String> {
    let channels = &mode.channels;
    let mut errors = Vec::new();
    if result.channels.len() != channels.len() {
        errors.push(format!(
            "expected {} configs, got {}",
            channels.len(),
            result.channels.len()
        ));
    }
    let mut report = |i: usize, why: String| {
        let name = channels
            .get(i)
            .map_or("(outside mode)", |c| c.name.as_str());
        errors.push(format!("{i} {name}: {why}"));
    };
    let mut heads = vec![Vec::new(); channels.len()];
    for (head, indices) in mode.pixels.iter().enumerate() {
        for &i in indices {
            heads[usize::from(i)].push(head);
        }
    }
    let share_head = |i: usize, j: usize| heads[i].iter().any(|h| heads[j].contains(h));

    let mut lowered: Vec<_> = result
        .pairs
        .iter()
        .map(|p| (p.coarse as usize, p.fine as usize))
        .collect();
    lowered.sort_unstable();
    for twice in lowered.windows(2).filter(|w| w[0] == w[1]) {
        report(twice[0].1, format!("joined {} twice", twice[0].0));
    }
    let expected = expected_pairs(mode, &heads);
    let name = |i: usize| {
        channels
            .get(i)
            .map_or("(outside mode)", |c| c.name.as_str())
    };
    for &(coarse, fine) in &lowered {
        if !expected.contains(&(coarse, fine)) {
            report(
                fine,
                format!("joined {coarse} {}: not certain", name(coarse)),
            );
        }
    }
    for &(coarse, fine) in &expected {
        if !lowered.contains(&(coarse, fine)) {
            report(fine, format!("not joined to {coarse} {}", name(coarse)));
        }
    }
    let mut owner = vec![None; channels.len()];
    for &(coarse, fine) in &expected {
        owner[fine] = Some(coarse);
    }

    let emitter_in = |i: usize| {
        if heads[i].is_empty() {
            mode.pixels.is_empty() && channels.iter().any(|c| colour(c).is_some())
        } else {
            (0..channels.len()).any(|j| share_head(i, j) && colour(&channels[j]).is_some())
        }
    };
    let roles: Vec<_> = channels
        .iter()
        .enumerate()
        .map(|(i, c)| light(c, kind, emitter_in(i)))
        .collect();
    let dimmers: Vec<_> = channels
        .iter()
        .enumerate()
        .filter_map(|(i, c)| (c.byte == Byte::Coarse && roles[i] == Light::Dimmer).then_some(i))
        .collect();
    let both = Follows::MASTER.bits() | Follows::BLACKOUT.bits();
    // A dimmer carries the emitters of its own heads. A dimmer in no head is shared.
    let carried = |i| {
        dimmers
            .iter()
            .any(|&d| heads[d].is_empty() || share_head(i, d))
    };
    let expected_mask = |i: usize| match roles[i] {
        Light::Dimmer => both,
        Light::Emitter if carried(i) => 0,
        Light::Emitter => both,
        Light::Shutter => Follows::BLACKOUT.bits(),
        Light::Independent => 0,
    };
    for (i, (c, config)) in channels.iter().zip(&result.channels).enumerate() {
        // The coarse supplies a word's policy. An orphan emitter is a light byte of its own.
        let controlling =
            if c.byte == Byte::Coarse || (owner[i].is_none() && roles[i] == Light::Emitter) {
                Some(i)
            } else {
                owner[i]
            };
        let expected = controlling.map_or(0, expected_mask);
        if config.follows.bits() != expected {
            report(
                i,
                format!(
                    "follows {}, expected {expected} ({:?}, controlling channel {controlling:?})",
                    config.follows.bits(),
                    roles[i]
                ),
            );
        }
        if non_light_name(c) && roles[i] == Light::Independent && config.follows != Follows::NONE {
            report(
                i,
                "non-light name and no declared light, yet follows a modifier".into(),
            );
        }
    }
    errors
}

#[test]
fn every_mode_obeys_the_channel_rules() -> io::Result<()> {
    let mut paths = Vec::new();
    for folder in common::folders() {
        qxf_files(&folder, &mut paths)?;
    }
    if let Some(root) = env::var_os("QLCPLUS_FIXTURES") {
        let mut pinned = Vec::new();
        for maker in fs::read_dir(&root)? {
            let maker = maker?;
            if maker.file_type()?.is_dir() {
                qxf_files(&maker.path(), &mut pinned)?;
            }
        }
        assert!(
            pinned.len() >= PINNED,
            "{} QXF files under {root:?}/*/, the pinned library has {PINNED}",
            pinned.len()
        );
        paths.extend(pinned);
    }

    paths.sort();
    let mut errors = Vec::new();
    for path in paths {
        let file = path.display().to_string();
        let fixture = match parse(&file, &fs::read(&path)?) {
            Ok(fixture) => fixture,
            Err(error) => {
                errors.push(error.to_string());
                continue;
            }
        };
        for mode in &fixture.modes {
            let space = u32::try_from(mode.channels.len()).unwrap();
            let problems = match std::panic::catch_unwind(|| lower(mode, 0, space)) {
                Ok(Ok(result)) => check_mode(mode, &fixture.kind, &result),
                Ok(Err(error)) => vec![format!("lower refused a mode that fits: {error:?}")],
                Err(_) => vec!["lower panicked".into()],
            };
            errors.extend(
                problems
                    .into_iter()
                    .map(|why| format!("{file} / {} / {why}", mode.name)),
            );
        }
    }
    assert!(
        errors.is_empty(),
        "{} problems:\n{}",
        errors.len(),
        errors.join("\n")
    );
    Ok(())
}

#[test]
fn a_fine_joins_only_a_coarse_it_certainly_belongs_to() {
    // (file, mode, fine, the coarse it joins or none)
    let robe = "Robe-ColorSpot-575-AT.qxf";
    let contest = "Contest-Evora-Beam-15R.qxf";
    let adb = "ADB-Warp-M.qxf";
    let domino = "Ayrton-Domino-LT.qxf";
    let clay = "Clay-Paky-Alpha-Spot-575-HPE.qxf";
    let elation = "Elation-Platinum-Spot-5R.qxf";
    let cases: &[(&str, &str, u32, Option<u32>)] = &[
        // ColorWheelFine and no coarse with that preset: the wheel right before it
        (robe, "1 - 27-Channel", 7, Some(6)),
        // an index fine never joins the static wheel selector right before it
        (robe, "1 - 27-Channel", 11, None),
        (robe, "1 - 27-Channel", 14, Some(13)),
        (robe, "1 - 27-Channel", 18, Some(17)),
        // ShutterIrisFine after an Iris in the Beam group: no coarse of its group before it
        (robe, "1 - 27-Channel", 20, None),
        (robe, "1 - 27-Channel", 23, Some(22)),
        (robe, "1 - 27-Channel", 26, Some(25)),
        (contest, "Extended", 7, Some(6)),
        (contest, "Extended", 9, Some(8)),
        // the one GoboIndex coarse is 12, but the fixed gobo wheel at 13 is right before
        (contest, "Extended", 14, None),
        (adb, "34 Channel", 2, Some(1)),
        (adb, "34 Channel", 8, Some(7)),
        // In/Out declared fine, right after a fine
        (adb, "34 Channel", 9, None),
        // Shutter B, C, D coarse, then B, C, D fine: the coarse before D looks the same
        (adb, "34 Channel", 13, None),
        (adb, "34 Channel", 14, None),
        (adb, "34 Channel", 15, None),
        (adb, "34 Channel", 16, None),
        (adb, "34 Channel", 20, Some(19)),
        (adb, "34 Channel", 22, Some(21)),
        (adb, "34 Channel", 24, Some(23)),
        // BeamZoomSmallBig and BeamZoomFine are the same preset
        (domino, "Extended", 9, Some(8)),
        (domino, "Extended", 15, Some(14)),
        (domino, "Extended", 33, None),
        (domino, "Extended", 34, None),
        (domino, "Extended", 37, None),
        (domino, "Extended", 42, Some(41)),
        // Frost 2 and Blade 1A look the same
        (domino, "Extended", 52, None),
        (domino, "Basic", 31, None),
        (clay, "Option 16b", 24, Some(20)),
        (clay, "Option 16b", 25, Some(21)),
        (clay, "Option 16b", 26, None),
        (clay, "Option 16b", 27, None),
        (elation, "Extended", 8, Some(7)),
        (elation, "Extended", 10, None),
        (elation, "Extended", 13, Some(12)),
    ];
    let mut wrong = Vec::new();
    for &(file, name, fine, expected) in cases {
        let mode = common::mode(file, name);
        let lowered = lower(&mode, 0, u32::try_from(mode.channels.len()).unwrap()).unwrap();
        let joined = lowered
            .pairs
            .iter()
            .find(|p| p.fine == fine)
            .map(|p| p.coarse);
        if joined != expected {
            wrong.push(format!(
                "{file} / {name} / {fine}: joined {joined:?}, expected {expected:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

fn inline(xml: &str) -> (Kind, Mode) {
    let mut fixture = parse("inline.qxf", xml.as_bytes()).unwrap();
    (fixture.kind, fixture.modes.remove(0))
}

#[test]
fn a_fog_pump_never_follows_master_or_blackout() {
    // (type, the pump in a head of its own, follows of pump, LED dimmer, red, green, blue)
    let cases = [
        ("Smoke", true, [0, 3, 0, 0, 0]),
        ("Hazer", true, [0, 3, 0, 0, 0]),
        ("Smoke", false, [0, 3, 0, 0, 0]),
        ("Hazer", false, [0, 3, 0, 0, 0]),
    ];
    let mut wrong = Vec::new();
    for (kind, own_head, expected) in cases {
        let pump_head = if own_head {
            "<Head><Channel>0</Channel></Head>"
        } else {
            ""
        };
        let (kind_read, mode) = inline(&format!(
            r#"<FixtureDefinition><Manufacturer>Test</Manufacturer><Model>Fog and LED</Model><Type>{kind}</Type>
            <Channel Name="Pump" Preset="IntensityMasterDimmer"/>
            <Channel Name="LED dimmer" Preset="IntensityDimmer"/>
            <Channel Name="Red" Preset="IntensityRed"/>
            <Channel Name="Green" Preset="IntensityGreen"/>
            <Channel Name="Blue" Preset="IntensityBlue"/>
            <Mode Name="Both">
                <Channel Number="0">Pump</Channel><Channel Number="1">LED dimmer</Channel>
                <Channel Number="2">Red</Channel><Channel Number="3">Green</Channel>
                <Channel Number="4">Blue</Channel>
                {pump_head}
                <Head><Channel>1</Channel><Channel>2</Channel><Channel>3</Channel><Channel>4</Channel></Head>
            </Mode></FixtureDefinition>"#
        ));
        let lowered = lower(&mode, 0, 5).unwrap();
        let follows: Vec<_> = lowered.channels.iter().map(|c| c.follows.bits()).collect();
        let oracle = check_mode(&mode, &kind_read, &lowered);
        if follows != expected || !oracle.is_empty() {
            wrong.push(format!(
                "{kind}, own head {own_head}: follows {follows:?}, expected {expected:?}; oracle {oracle:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_fine_in_no_head_joins_a_coarse_in_no_head() {
    let (kind, mode) = inline(
        r#"<FixtureDefinition><Manufacturer>Test</Manufacturer><Model>Shared red</Model><Type>Other</Type>
        <Channel Name="Shared red" Preset="IntensityRed"/>
        <Channel Name="Head dimmer" Preset="IntensityDimmer"/>
        <Channel Name="Head red" Preset="IntensityRed"/>
        <Channel Name="Shared red fine" Preset="IntensityRedFine"/>
        <Mode Name="Both">
            <Channel Number="0">Shared red</Channel><Channel Number="1">Head dimmer</Channel>
            <Channel Number="2">Head red</Channel><Channel Number="3">Shared red fine</Channel>
            <Head><Channel>1</Channel><Channel>2</Channel></Head>
        </Mode></FixtureDefinition>"#,
    );
    let lowered = lower(&mode, 0, 4).unwrap();
    let pairs: Vec<_> = lowered.pairs.iter().map(|p| (p.coarse, p.fine)).collect();
    let follows: Vec<_> = lowered.channels.iter().map(|c| c.follows.bits()).collect();
    assert_eq!(
        (pairs, follows, check_mode(&mode, &kind, &lowered)),
        (vec![(0, 3)], vec![3, 3, 0, 3], Vec::<String>::new())
    );
}

#[test]
fn the_oracle_refuses_a_changed_pair() {
    // (file, mode, pairs taken out, pairs put in)
    type Case<'a> = (&'a str, &'a str, &'a [(u32, u32)], &'a [(u32, u32)]);
    let clay = "Clay-Paky-Alpha-Spot-575-HPE.qxf";
    let cases: &[Case] = &[
        (clay, "Option 16b", &[], &[(10, 26)]),
        (clay, "Option 16b", &[], &[(10, 27), (12, 26)]),
        (
            clay,
            "Option 16b",
            &[(20, 24), (21, 25)],
            &[(20, 25), (21, 24)],
        ),
        (clay, "Option 16b", &[(20, 24)], &[]),
        (
            "Robe-ColorSpot-575-AT.qxf",
            "1 - 27-Channel",
            &[],
            &[(24, 20)],
        ),
        ("Contest-Evora-Beam-15R.qxf", "Extended", &[], &[(12, 14)]),
        ("ADB-Warp-M.qxf", "34 Channel", &[], &[(12, 13)]),
        ("Ayrton-Domino-LT.qxf", "Extended", &[], &[(38, 34)]),
    ];
    let mut wrong = Vec::new();
    for &(file, name, out, put) in cases {
        let fixture = common::fixture(file);
        let mode = common::mode(file, name);
        let mut lowered = lower(&mode, 0, u32::try_from(mode.channels.len()).unwrap()).unwrap();
        let before = check_mode(&mode, &fixture.kind, &lowered);
        lowered.pairs.retain(|p| !out.contains(&(p.coarse, p.fine)));
        lowered.pairs.extend(
            put.iter()
                .map(|&(coarse, fine)| rig::channels::Pair { coarse, fine }),
        );
        let after = check_mode(&mode, &fixture.kind, &lowered);
        if !before.is_empty() || after.is_empty() {
            wrong.push(format!(
                "{file} / {name} -{out:?} +{put:?}: before {before:?}, after {after:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
