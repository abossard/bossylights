//! Check the channel model against the declarations in every fixture and head.
mod common;

use fixture_library::{Byte, Channel, Does, Function, Kind, Mode, parse};
use rig::channels::{Follows, Lowered, lower};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::{env, fs, io};

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

/// Presets with the same function share a key. Every other stem is its own key.
fn preset_key(stem: &str) -> &str {
    match stem {
        "IntensityMasterDimmer" => "IntensityDimmer",
        "ColorMacro" => "ColorWheel",
        "ColorCTOMixer" | "ColorCTBMixer" => "ColorCTCMixer",
        "GoboIndex" => "GoboWheel",
        "SpeedPanFastSlow"
        | "SpeedTiltSlowFast"
        | "SpeedTiltFastSlow"
        | "SpeedPanTiltSlowFast"
        | "SpeedPanTiltFastSlow" => "SpeedPanSlowFast",
        "ShutterStrobeFastSlow" => "ShutterStrobeSlowFast",
        "ShutterIrisMinToMax" | "ShutterIrisMaxToMin" => "ShutterIris",
        "BeamFocusNearFar" | "BeamFocusFarNear" => "BeamFocus",
        "BeamZoomSmallBig" | "BeamZoomBigSmall" => "BeamZoom",
        "PrismRotationFastSlow" => "PrismRotationSlowFast",
        _ => stem,
    }
}

fn stem(c: &Channel) -> Option<&str> {
    c.declared
        .preset
        .as_deref()
        .map(|p| p.strip_suffix("Fine").unwrap_or(p))
}

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

/// Some(false) for a strict match, Some(true) for a loose match (second choice).
fn loose_match(fine: &Channel, coarse: &Channel) -> Option<bool> {
    match (stem(fine), stem(coarse)) {
        (Some(f), Some(c)) => (preset_key(f) == preset_key(c)).then_some(false),
        (None, None) => (group(fine)? == group(coarse)?).then_some(false),
        _ => (group(fine)? == group(coarse)?).then_some(true),
    }
}

fn is_slot(c: &Channel) -> bool {
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Light {
    Dimmer,
    Emitter,
    Shutter,
    Independent,
}

fn light(c: &Channel, kind: &Kind) -> Light {
    use Function::{Amber, Blue, Cyan, Green, Indigo, Lime, Magenta, Red, Uv, White, Yellow};
    if matches!(
        c.declared.colour,
        Some(Red | Green | Blue | White | Amber | Uv | Lime | Indigo | Cyan | Magenta | Yellow)
    ) {
        return Light::Emitter;
    }
    match stem(c) {
        Some("IntensityDimmer" | "IntensityMasterDimmer")
            if !matches!(kind, Kind::Smoke | Kind::Hazer) =>
        {
            Light::Dimmer
        }
        Some(
            "IntensityRed" | "IntensityGreen" | "IntensityBlue" | "IntensityWhite"
            | "IntensityAmber" | "IntensityUV" | "IntensityLime" | "IntensityIndigo"
            | "IntensityCyan" | "IntensityMagenta" | "IntensityYellow",
        ) => Light::Emitter,
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
    let mut owner = vec![None; channels.len()];
    for pair in &result.pairs {
        let fine = pair.fine as usize;
        let coarse = pair.coarse as usize;
        if channels.get(fine).is_none_or(|c| c.byte != Byte::Fine) {
            report(fine, format!("pair with {coarse} needs a fine byte here"));
        } else if channels.get(coarse).is_none_or(|c| c.byte != Byte::Coarse) {
            report(coarse, format!("pair with {fine} needs a coarse byte here"));
        } else if let Some(previous) = owner[fine].replace(coarse) {
            report(fine, format!("joined both {previous} and {coarse}"));
        }
    }

    // Rank all free candidates by the table. Replay the actual pairs in fine order
    // so a wrong choice also reports the remaining options for the next fine.
    let mut used = BTreeSet::new();
    for (i, fine) in channels
        .iter()
        .enumerate()
        .filter(|(_, c)| c.byte == Byte::Fine)
    {
        let best = channels
            .iter()
            .enumerate()
            .filter_map(|(j, c)| {
                if c.byte == Byte::Fine
                    || used.contains(&j)
                    || !(heads[i].is_empty() || share_head(i, j))
                {
                    return None;
                }
                Some((
                    i.checked_sub(1) != Some(j),
                    loose_match(fine, c)?,
                    is_slot(c),
                    j,
                ))
            })
            .min()
            .map(|(_, _, _, j)| j);
        if owner[i] != best {
            report(
                i,
                format!(
                    "joined {:?}, expected {best:?} from the free candidates",
                    owner[i]
                ),
            );
        }
        if let Some(j) = owner[i]
            && !used.insert(j)
        {
            report(i, format!("coarse {j} already has a fine"));
        }
    }

    let roles: Vec<_> = channels.iter().map(|c| light(c, kind)).collect();
    let dimmers: Vec<_> = channels
        .iter()
        .enumerate()
        .filter_map(|(i, c)| (c.byte == Byte::Coarse && roles[i] == Light::Dimmer).then_some(i))
        .collect();
    let both = Follows::MASTER.bits() | Follows::BLACKOUT.bits();
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
                    "follows {}, expected {expected} (controlling channel {controlling:?})",
                    config.follows.bits()
                ),
            );
        }
        if non_light_name(c) && roles[i] == Light::Independent && config.follows != Follows::NONE {
            report(
                i,
                "non-light name and no declared light, yet follows a modifier".into(),
            );
        }
        if roles[i] == Light::Emitter && config.follows == Follows::NONE && !carried(i) {
            report(
                i,
                "emitter follows nothing and has no dimmer to carry it".into(),
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
        let before = paths.len();
        for maker in fs::read_dir(&root)? {
            let maker = maker?;
            if maker.file_type()?.is_dir() {
                qxf_files(&maker.path(), &mut paths)?;
            }
        }
        assert!(paths.len() > before, "no QXF files under {root:?}/*/");
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
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    Ok(())
}
