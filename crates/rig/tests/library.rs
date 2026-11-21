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

/// The preset without the way it runs: a zoom from small to big and one from big to
/// small are one function. A gobo index and a gobo wheel are not.
fn family(stem: &str) -> String {
    let mut family = stem.replace("MasterDimmer", "Dimmer");
    for (from, to) in [("CTOMixer", "CTCMixer"), ("CTBMixer", "CTCMixer")] {
        family = family.replace(from, to);
    }
    for way in [
        "SlowFast", "FastSlow", "SmallBig", "BigSmall", "NearFar", "FarNear", "MinToMax",
        "MaxToMin",
    ] {
        if let Some(rest) = family.strip_suffix(way) {
            return rest.to_string();
        }
    }
    family
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

/// Why `fine` cannot be the second byte of `coarse`, from the definition alone.
fn incompatible(fine: &Channel, coarse: &Channel) -> Option<String> {
    if colour(fine) != colour(coarse) {
        return Some(format!(
            "colour {:?} joins colour {:?}",
            colour(fine),
            colour(coarse)
        ));
    }
    match (stem(fine), stem(coarse)) {
        (Some(f), Some(c)) if family(f) != family(c) => {
            return Some(format!("preset {f} joins preset {c}"));
        }
        (Some(_), Some(_)) => {}
        _ if group(fine).is_none() || group(fine) != group(coarse) => {
            return Some(format!(
                "group {:?} joins group {:?}",
                group(fine),
                group(coarse)
            ));
        }
        _ => {}
    }
    let (f, c) = (kinds(fine), kinds(coarse));
    // A fine with steps of its own has the coarse's steps. A plain fine names nothing
    // the coarse does not, unless the coarse names nothing at all (Martin smartMAC).
    let shaped = if f.len() > 1 {
        f == c
    } else {
        c.iter().all(Option::is_none) || f.iter().flatten().all(|kind| c.contains(&Some(*kind)))
    };
    if !shaped {
        return Some(format!(
            "{} ranges {:?} join {} ranges {:?}",
            f.len(),
            fine.ranges.iter().map(|r| &r.label).collect::<Vec<_>>(),
            c.len(),
            coarse.ranges.iter().map(|r| &r.label).collect::<Vec<_>>()
        ));
    }
    let only_index = |c: &Channel| index(c) && !wheel(c);
    let only_wheel = |c: &Channel| wheel(c) && !index(c);
    if (only_index(fine) && only_wheel(coarse)) || (only_wheel(fine) && only_index(coarse)) {
        return Some("an index joins a wheel selector".into());
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Light {
    Dimmer,
    Emitter,
    Shutter,
    Independent,
}

/// What a channel does to the light, from its declarations and the fixture's type.
/// On a smoke machine or hazer a dimmer preset is the fog output, unless the mode has
/// emitters for it to dim. On a dimmer pack every group-only Intensity channel is a dimmer.
fn light(c: &Channel, kind: &Kind, emitters: bool) -> Light {
    if colour(c).is_some() {
        return Light::Emitter;
    }
    let fog = matches!(kind, Kind::Smoke | Kind::Hazer) && !emitters;
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
    // A fine in a head joins a coarse in exactly its heads.
    let can_join = |fine: usize, coarse: usize| -> Option<String> {
        if channels[coarse].byte != Byte::Coarse {
            return Some("not a coarse byte".into());
        }
        if !heads[fine].is_empty() && heads[fine] != heads[coarse] {
            return Some(format!(
                "heads {:?} join heads {:?}",
                heads[fine], heads[coarse]
            ));
        }
        incompatible(&channels[fine], &channels[coarse])
    };

    let mut owner = vec![None; channels.len()];
    let mut taken = vec![None; channels.len()];
    for pair in &result.pairs {
        let fine = pair.fine as usize;
        let coarse = pair.coarse as usize;
        if channels.get(fine).is_none_or(|c| c.byte != Byte::Fine) {
            report(fine, format!("pair with {coarse} needs a fine byte here"));
        } else if channels.get(coarse).is_none_or(|c| c.byte != Byte::Coarse) {
            report(coarse, format!("pair with {fine} needs a coarse byte here"));
        } else if let Some(previous) = owner[fine].replace(coarse) {
            report(fine, format!("joined both {previous} and {coarse}"));
        } else if let Some(other) = taken[coarse].replace(fine) {
            report(coarse, format!("coarse of both {other} and {fine}"));
        } else if let Some(why) = can_join(fine, coarse) {
            report(
                fine,
                format!("joined {coarse} {}: {why}", channels[coarse].name),
            );
        }
    }
    for (i, _) in channels
        .iter()
        .enumerate()
        .filter(|(_, c)| c.byte == Byte::Fine)
    {
        // The coarse byte right before a fine is its partner, unless an earlier fine has it.
        if let Some(j) = i.checked_sub(1)
            && can_join(i, j).is_none()
            && taken[j].is_none_or(|other| other > i)
            && owner[i] != Some(j)
        {
            report(
                i,
                format!("not joined to {j} {}, right before it", channels[j].name),
            );
        }
        // A fine left alone has no free coarse it could join.
        if owner[i].is_none()
            && let Some(j) =
                (0..channels.len()).find(|&j| taken[j].is_none() && can_join(i, j).is_none())
        {
            report(
                i,
                format!("left alone, but {j} {} is free", channels[j].name),
            );
        }
    }

    let emitters = channels.iter().any(|c| colour(c).is_some());
    let roles: Vec<_> = channels.iter().map(|c| light(c, kind, emitters)).collect();
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
