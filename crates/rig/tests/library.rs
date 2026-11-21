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

/// What a preset controls, without the way it runs (`BeamZoomSmallBig` is `BeamZoom`) and
/// with QLC+'s other names for the same control (a master dimmer is a dimmer, the X axis is pan).
fn kin(stem: &str) -> String {
    let stem = [
        "SlowFast", "FastSlow", "SmallBig", "BigSmall", "NearFar", "FarNear", "MinToMax",
        "MaxToMin",
    ]
    .into_iter()
    .find_map(|way| stem.strip_suffix(way))
    .unwrap_or(stem);
    stem.replace("Master", "")
        .replace("PositionXAxis", "PositionPan")
        .replace("PositionYAxis", "PositionTilt")
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

/// What the ranges do, by kind, in order. No range, one range that names nothing and one
/// range over the whole byte are one shape: a plain byte (Arolla's gobo rotation fine runs
/// 0 to 255 as `RotationIndexed`).
fn shape(c: &Channel) -> Vec<Option<Discriminant<Does>>> {
    match (kinds(c).as_slice(), c.ranges.as_slice()) {
        ([] | [None], _) => Vec::new(),
        (_, [r]) if r.min == 0 && r.max == 255 => Vec::new(),
        (kinds, _) => kinds.to_vec(),
    }
}

/// The pairs a mode must have, as (coarse, fine), from the definition alone. A fine joins
/// only a coarse it certainly belongs to; every other fine stays a byte of its own. The
/// definition is the truth: where it contradicts the maker's chart, this follows it.
///
/// A coarse takes at most one fine, never an index to a wheel selector, and only within
/// the same heads (a fine in no head a coarse in no head) with the same colour, and never a
/// fog output to a light dimmer (on a smoke machine or hazer, `light` says which a dimmer
/// preset is: a pump with "no fog" then "fog" declared a dimmer fine stays apart from the
/// LED dimmer). The evidence, in order, each kind choosing before the next:
/// 1. `ActsOn`: the fine refines what it names, so it joins the channel that acts on the
///    same one (a gobo rotation fine names the wheel, as its rotation does), else the named
///    channel itself; sharing what they act on stands for the group (Ayrton's rotation sits
///    in group Speed, its fine in Gobo). Several: the one right before the fine. One that
///    finds no coarse stays apart.
/// 2. Preset: a fine with a preset joins the coarse with that preset (`kin`: the way it runs
///    and QLC+'s other names do not count) before the fine, or right after it when that is
///    the only coarse of its control (Briteq BT-250-EZ: Dimmer fine, then Dimmer). One such
///    coarse: it, unless the channel right before the fine is another coarse of its group
///    (two readings). Several: the one right before the fine, unless a fine follows (a run
///    of fines after a run of coarses: Showtec Acrobat's Pan 1, Tilt, Pan 2, then their
///    fines). It chooses before any fine that only sits next to a coarse.
/// 3. Adjacent: a fine with no preset, or with no coarse of its preset left, joins the
///    channel right before it, a coarse of its group that it fits (when the fine has a
///    preset, a coarse with no preset: Zoom never takes a Focus fine), unless coarses and
///    fines come in blocks: the channel before that coarse looks the same and the channel
///    after the fine is a fine of its group (Shutter B, C, D coarse, then B, C, D fine).
fn expected_pairs(mode: &Mode, kind: &Kind, heads: &[Vec<usize>]) -> Vec<(usize, usize)> {
    let channels = &mode.channels;
    // A dimmer preset on a smoke machine or hazer that neither dims light nor emits it
    // (a declared `<Colour>` is an emitter): the fog output.
    let fog = |j: usize| {
        let c = &channels[j];
        matches!(kind, Kind::Smoke | Kind::Hazer)
            && stem(c).is_some_and(|s| kin(s) == "IntensityDimmer")
            && light(c, kind, emitter_in(mode, heads, j)) == Light::Independent
    };
    let only_index = |c: &Channel| index(c) && !wheel(c);
    let only_wheel = |c: &Channel| wheel(c) && !index(c);
    let mixes = |i: usize, j: usize| {
        let (fine, coarse) = (&channels[i], &channels[j]);
        (only_index(fine) && only_wheel(coarse)) || (only_wheel(fine) && only_index(coarse))
    };
    // A coarse this fine could take, before the index and wheel guard.
    let open = |taken: &[bool], i: usize, j: usize| {
        let coarse = &channels[j];
        coarse.byte == Byte::Coarse
            && !taken[j]
            && heads[j] == heads[i]
            && colour(coarse) == colour(&channels[i])
            && fog(j) == fog(i)
    };
    let free = |taken: &[bool], i: usize, j: usize| open(taken, i, j) && !mixes(i, j);
    let same_group = |i: usize, j: usize| {
        group(&channels[i]).is_some() && group(&channels[j]) == group(&channels[i])
    };
    // A plain fine fits any coarse, a fine with steps only the same steps.
    let steps_fit = |i: usize, j: usize| {
        let steps = shape(&channels[i]);
        steps.is_empty() || shape(&channels[j]) == steps
    };
    let fits = |taken: &[bool], i: usize, j: usize| {
        free(taken, i, j) && same_group(i, j) && steps_fit(i, j)
    };
    let fines = || {
        channels
            .iter()
            .enumerate()
            .filter(|(_, c)| c.byte == Byte::Fine)
    };
    let mut taken = vec![false; channels.len()];
    let mut pairs = Vec::new();
    for (i, fine) in fines() {
        let Some(named) = fine.declared.acts_on.map(usize::from) else {
            continue;
        };
        // Every other channel that acts on the same one, coarse or not: a fine refines one
        // of them, never the parent they act on (Elation E-Spot III's index fine).
        let siblings: Vec<_> = (0..channels.len())
            .filter(|&j| j != i && channels[j].declared.acts_on == fine.declared.acts_on)
            .collect();
        let acting = if siblings.is_empty() && named < channels.len() {
            vec![named]
        } else {
            siblings
        };
        let fitting: Vec<_> = acting
            .into_iter()
            .filter(|&j| free(&taken, i, j) && steps_fit(i, j))
            .collect();
        let partner = match fitting.as_slice() {
            &[j] => Some(j),
            _ => i.checked_sub(1).filter(|b| fitting.contains(b)),
        };
        if let Some(j) = partner {
            taken[j] = true;
            pairs.push((j, i));
        }
    }
    // 2. Preset. A fine with no coarse of its preset left waits for 3.
    let mut adjacent_only = Vec::new();
    for (i, fine) in fines().filter(|(_, c)| c.declared.acts_on.is_none()) {
        let Some(preset) = stem(fine).map(kin) else {
            adjacent_only.push(i);
            continue;
        };
        let free = |j: usize| free(&taken, i, j);
        let before = i.checked_sub(1);
        let all: Vec<_> = (0..channels.len())
            .filter(|&j| free(j) && stem(&channels[j]).map(kin).as_ref() == Some(&preset))
            .collect();
        // A coarse right after the fine only when it is the one coarse of its control.
        let same: Vec<_> = all
            .iter()
            .copied()
            .filter(|&j| j < i || (j == i + 1 && all == [j]))
            .collect();
        let fine_after = channels.get(i + 1).is_some_and(|c| c.byte == Byte::Fine);
        let partner = match same.as_slice() {
            [] => {
                adjacent_only.push(i);
                None
            }
            &[j] if before == Some(j)
                || before.is_none_or(|b| !(open(&taken, i, b) && same_group(i, b))) =>
            {
                Some(j)
            }
            [_] => None,
            _ => before.filter(|b| same.contains(b) && !fine_after),
        };
        if let Some(j) = partner {
            taken[j] = true;
            pairs.push((j, i));
        }
    }
    // 3. Adjacent.
    for i in adjacent_only {
        let fine = &channels[i];
        let free = |j: usize| free(&taken, i, j);
        let fits = |j: usize| fits(&taken, i, j);
        let alike = |j: usize, k: usize| {
            free(k)
                && group(&channels[k]) == group(&channels[j])
                && stem(&channels[k]) == stem(&channels[j])
                && shape(&channels[k]) == shape(&channels[j])
        };
        let fine_after = channels.get(i + 1).is_some_and(|c| c.byte == Byte::Fine);
        let blocks = |j: usize| {
            fine_after
                && group(&channels[i + 1]) == group(fine)
                && j.checked_sub(1).is_some_and(|k| alike(j, k))
        };
        let partner = i.checked_sub(1).filter(|&j| {
            fits(j) && !blocks(j) && (stem(fine).is_none() || stem(&channels[j]).is_none())
        });
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
/// dims emitters: it is a plain byte (a level with steps, "fog off" then "fog output", is
/// a pump) and it shares a head with an emitter, or the mode has no heads and has emitters.
/// On a dimmer pack every group-only Intensity channel is a dimmer.
fn light(c: &Channel, kind: &Kind, dims_emitters: bool) -> Light {
    if colour(c).is_some() {
        return Light::Emitter;
    }
    let fog = matches!(kind, Kind::Smoke | Kind::Hazer) && !(dims_emitters && shape(c).is_empty());
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

/// What the maker's chart says of one fine byte, and what the lowering must do with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fine {
    /// The chart's coarse, and the definition makes the pair certain.
    Joins(usize),
    /// Not joined: the chart's coarse, which the definition does not make certain (the
    /// user joins it in Patch), or `None` when the chart has it as a channel of its own.
    Apart(Option<usize>),
}
use Fine::{Apart, Joins};

/// Ground truth for every fixture the reviews named (rounds 3 to 7): written by hand from
/// the maker's DMX chart, never from the rule. Where no chart was at hand, from the channel
/// names the definition copies from it. Every fine byte of the mode is listed once.
/// Where the definition contradicts the chart (a slot moved, a fine declared coarse), the
/// entry says "definition error, follows the definition" and lists what the definition
/// makes certain: the fix is the definition (Patch, and upstream to QLC+), never a rule
/// for one fixture.
/// (file, mode, every fine byte with what the chart says of it)
type Chart = (&'static str, &'static str, &'static [(usize, Fine)]);

const CHARTS: &[Chart] = &[
    // Maker's chart, Option 16 b column: gobo rotation 1 (11) with fine 1 (27), rotation 2
    // (13) with fine 2 (28). Nothing in the definition says which fine is which rotation.
    (
        "Clay-Paky-Alpha-Spot-575-HPE.qxf",
        "Option 16b",
        &[
            (24, Joins(20)),
            (25, Joins(21)),
            (26, Apart(Some(10))),
            (27, Apart(Some(12))),
        ],
    ),
    // The gobo index is a channel of its own, declared fine (with `ActsOn` the wheel).
    (
        "Vari-Lite-VL2202-Spot.qxf",
        "Standard 16bit",
        &[(2, Joins(1)), (4, Joins(3)), (12, Apart(None))],
    ),
    (
        "Vari-Lite-VL2202-Spot.qxf",
        "Enhanced 16bit",
        &[(2, Joins(1)), (4, Joins(3)), (12, Apart(None))],
    ),
    (
        "Vari-Lite-VL2202-Spot.qxf",
        "Standard 8bit",
        &[(10, Apart(None))],
    ),
    (
        "Vari-Lite-VL2202-Spot.qxf",
        "Enhanced 8bit",
        &[(10, Apart(None))],
    ),
    // External light 1 and 2 on/off, declared fine.
    (
        "Cameo-CLMPAR3.qxf",
        "9 Channel",
        &[(7, Apart(None)), (8, Apart(None))],
    ),
    (
        "Cameo-CLMPAR3.qxf",
        "21 Channel",
        &[(19, Apart(None)), (20, Apart(None))],
    ),
    // Static gobo fine positioning is declared an index beside the static wheel selector;
    // the Iris fine is in group Shutter, its Iris in group Beam.
    (
        "Robe-ColorSpot-575-AT.qxf",
        "1 - 27-Channel",
        &[
            (1, Joins(0)),
            (3, Joins(2)),
            (7, Joins(6)),
            (9, Joins(8)),
            (11, Apart(Some(10))),
            (14, Joins(13)),
            (18, Joins(17)),
            (20, Apart(Some(19))),
            (23, Joins(22)),
            (26, Joins(25)),
        ],
    ),
    // Contest Evora-Beam15R manual v1.0, p. 16, column Ex: DMX 12 gobo rotation with its
    // fine at DMX 13. Definition error, follows the definition: DMX 13 is "Gobo indexing",
    // declared coarse (`GoboIndex`), so 11 and 12 stay two channels. The fixed gobos (13)
    // are a wheel selector; the fixed gobo index fine (14) stays apart from it.
    (
        "Contest-Evora-Beam-15R.qxf",
        "Extended",
        &[
            (1, Joins(0)),
            (3, Joins(2)),
            (7, Joins(6)),
            (9, Joins(8)),
            (14, Apart(Some(13))),
            (17, Joins(16)),
            (19, Joins(18)),
        ],
    ),
    // ADB WARP/M M5084-D v1.1, p. 34: each shutter's coarse and fine side by side (DMX
    // 11/12, 14/15, 17/18). Definition error, follows the definition: it declares Shutter
    // A, then B, C, D coarse, then B, C, D fine, then In/Out declared fine; blocks stay
    // apart (the coarse is the one the fine's name gives).
    (
        "ADB-Warp-M.qxf",
        "34 Channel",
        &[
            (2, Joins(1)),
            (4, Joins(3)),
            (8, Joins(7)),
            (9, Apart(None)),
            (13, Apart(Some(10))),
            (14, Apart(Some(11))),
            (15, Apart(Some(12))),
            (16, Apart(None)),
            (17, Apart(None)),
            (18, Apart(None)),
            (20, Joins(19)),
            (22, Joins(21)),
            (24, Joins(23)),
        ],
    ),
    // Fixed gobo indexing fine is declared an index beside the fixed gobo wheel selector.
    (
        "Elation-Platinum-Spot-5R.qxf",
        "Extended",
        &[
            (1, Joins(0)),
            (3, Joins(2)),
            (5, Joins(4)),
            (8, Joins(7)),
            (10, Apart(Some(9))),
            (13, Joins(12)),
            (15, Joins(14)),
            (18, Joins(17)),
            (20, Joins(19)),
        ],
    ),
    (
        "Shehds-120W-4in1-RGBW-LED-Wash-Moving-Head.qxf",
        "23 Channel",
        &[(1, Joins(0)), (3, Joins(2))],
    ),
    ("Stairville-AF-180-LED-Fogger-Co2-FX.qxf", "7 Channel", &[]),
    ("Botex-DSP-405.qxf", "4 Channels", &[]),
    // Ayrton's chart: each gobo index/rotation is a channel of its own (declared fine here),
    // its index fine right after it; each blade A fine right after its blade.
    // Basic: definition error, follows the definition. The chart has Prism 2 insertion at
    // DMX 31 and its rotation at 32; the definition drops the insertion and declares 31 the
    // Prism 2 rotation, 32 its fine, so they join.
    (
        "Ayrton-Domino-LT.qxf",
        "Standard",
        &[
            (1, Joins(0)),
            (3, Joins(2)),
            (23, Apart(None)),
            (25, Apart(None)),
        ],
    ),
    (
        "Ayrton-Domino-LT.qxf",
        "Basic",
        &[(21, Apart(None)), (23, Apart(None)), (31, Joins(30))],
    ),
    // Extended: definition error, follows the definition. The chart has each blade B fine
    // after its blade B (DMX 54/55, 58/59, 62/63, 66/67); the definition declares the four
    // Blade B Fine channels (54, 58, 62, 66) coarse, so they are channels of their own.
    (
        "Ayrton-Domino-LT.qxf",
        "Extended",
        &[
            (1, Joins(0)),
            (3, Joins(2)),
            (7, Joins(6)),
            (9, Joins(8)),
            (11, Joins(10)),
            (15, Joins(14)),
            (17, Joins(16)),
            (19, Joins(18)),
            (21, Joins(20)),
            (23, Joins(22)),
            (25, Joins(24)),
            (27, Joins(26)),
            (33, Apart(None)),
            (34, Apart(Some(33))),
            (36, Apart(None)),
            (37, Apart(Some(36))),
            (42, Joins(41)),
            (45, Joins(44)),
            (48, Joins(47)),
            (52, Joins(51)),
            (56, Joins(55)),
            (60, Joins(59)),
            (64, Joins(63)),
            (68, Joins(67)),
        ],
    ),
    // Auto Focus fine names its coarse: `ActsOn="12"`, `ActsOn="10"`.
    (
        "Ayrton-Mistral.qxf",
        "Standard",
        &[(1, Joins(0)), (3, Joins(2)), (13, Joins(12))],
    ),
    ("Ayrton-Mistral.qxf", "Basic", &[(11, Joins(10))]),
    (
        "Clay-Paky-Arolla-Profile-HP.qxf",
        "Standard",
        &[
            (8, Joins(7)),
            (13, Joins(12)),
            (34, Joins(33)),
            (36, Joins(35)),
        ],
    ),
    // Showtec 41377 manual: DMX 1/4 Pan #1, 2/5 Tilt, 3/6 Pan #2. Two pan coarses and two
    // pan fines: the definition does not say which is which.
    (
        "Showtec-Acrobat.qxf",
        "16 Channel",
        &[(3, Apart(Some(0))), (4, Joins(1)), (5, Apart(Some(2)))],
    ),
    // Robe Robin 800 LEDWash DMX chart v1.1: each colour's coarse and fine side by side.
    // Definition error, follows the definition, in every mode: it declares the four
    // coarses of a zone, then their four fines (38 Channel #2 from DMX 15, the other modes
    // from DMX 7), and lists each zone head one channel early, so each white fine sits in
    // the next zone's head (the last in none), away from its white coarse.
    (
        "Robe-Robin-800-LEDWash.qxf",
        "38 Channel #2",
        &[
            (1, Joins(0)),
            (3, Joins(2)),
            (7, Joins(6)),
            (11, Joins(10)),
            (18, Joins(14)),
            (19, Joins(15)),
            (20, Joins(16)),
            (21, Apart(Some(17))),
            (26, Joins(22)),
            (27, Joins(23)),
            (28, Joins(24)),
            (29, Apart(Some(25))),
            (34, Joins(30)),
            (35, Joins(31)),
            (36, Joins(32)),
            (37, Apart(Some(33))),
        ],
    ),
    // Ayrton Mistral 1.1, p. 16: the gobo rotation fine (29) after the rotation (28),
    // which sits in group Speed with the same `ActsOn` (27). Auto Focus fine names its
    // coarse (`ActsOn="15"`); the Iris fine has no Iris coarse of its preset.
    (
        "Ayrton-Mistral.qxf",
        "Extended",
        &[
            (1, Joins(0)),
            (3, Joins(2)),
            (7, Joins(6)),
            (12, Joins(11)),
            (14, Joins(13)),
            (16, Joins(15)),
            (18, Joins(17)),
            (20, Joins(19)),
            (22, Joins(21)),
            (24, Joins(23)),
            (26, Joins(25)),
            (29, Joins(28)),
            (33, Joins(32)),
            (36, Joins(35)),
        ],
    ),
    // Ayrton Rivale Profile DMX chart V115: the gobo rotation fine after the rotation, in
    // group Speed with the same `ActsOn`; Standard 21/22, Extended 32/33. The Extended Iris
    // fine is in group Shutter, its Iris in group Beam.
    (
        "Ayrton-Rivale-Profile.qxf",
        "Standard",
        &[
            (1, Joins(0)),
            (3, Joins(2)),
            (8, Joins(7)),
            (10, Joins(9)),
            (12, Joins(11)),
            (14, Joins(13)),
            (22, Joins(21)),
        ],
    ),
    (
        "Ayrton-Rivale-Profile.qxf",
        "Extended",
        &[
            (1, Joins(0)),
            (3, Joins(2)),
            (9, Joins(8)),
            (11, Joins(10)),
            (13, Joins(12)),
            (15, Joins(14)),
            (17, Joins(16)),
            (19, Joins(18)),
            (21, Joins(20)),
            (23, Joins(22)),
            (25, Joins(24)),
            (27, Joins(26)),
            (33, Joins(32)),
            (37, Apart(Some(36))),
            (40, Joins(39)),
            (43, Joins(42)),
            (47, Joins(46)),
            (49, Joins(48)),
            (51, Joins(50)),
            (53, Joins(52)),
            (55, Joins(54)),
            (57, Joins(56)),
            (59, Joins(58)),
            (61, Joins(60)),
            (63, Joins(62)),
        ],
    ),
    // Elation E Spot III DMX Traits v1, p. 1: the gobo index/rotation (8) is a coarse with
    // its fine at DMX 9. Definition error, follows the definition: it declares the index
    // `Byte="1"`, so it is a channel of its own and its fine stays apart.
    (
        "Elation-E-Spot-III.qxf",
        "Extended",
        &[
            (1, Joins(0)),
            (3, Joins(2)),
            (5, Joins(4)),
            (7, Apart(None)),
            (8, Apart(Some(7))),
            (11, Joins(10)),
            (15, Joins(14)),
        ],
    ),
];

/// The chart of a mode, by file name and mode name.
fn chart(file: &str, mode: &str) -> Option<&'static [(usize, Fine)]> {
    let file = Path::new(file).file_name()?.to_str()?;
    CHARTS
        .iter()
        .find(|(f, m, _)| *f == file && *m == mode)
        .map(|(_, _, fines)| *fines)
}

fn chart_pairs(fines: &[(usize, Fine)]) -> Vec<(usize, usize)> {
    let mut pairs: Vec<_> = fines
        .iter()
        .filter_map(|&(fine, says)| match says {
            Joins(coarse) => Some((coarse, fine)),
            Apart(_) => None,
        })
        .collect();
    pairs.sort_unstable();
    pairs
}

/// Whether an emitter shares a head with this channel, or, in no head, the mode has no
/// heads and declares an emitter.
fn emitter_in(mode: &Mode, heads: &[Vec<usize>], i: usize) -> bool {
    let channels = &mode.channels;
    if heads[i].is_empty() {
        mode.pixels.is_empty() && channels.iter().any(|c| colour(c).is_some())
    } else {
        (0..channels.len()).any(|j| {
            heads[i].iter().any(|h| heads[j].contains(h)) && colour(&channels[j]).is_some()
        })
    }
}

fn head_lists(mode: &Mode) -> Vec<Vec<usize>> {
    let mut heads = vec![Vec::new(); mode.channels.len()];
    for (head, indices) in mode.pixels.iter().enumerate() {
        for &i in indices {
            heads[usize::from(i)].push(head);
        }
    }
    heads
}

/// Check a lowering. The pairs come from the hand-written chart when the mode has one,
/// else from the rule.
fn check_mode(file: &str, mode: &Mode, kind: &Kind, result: &Lowered) -> Vec<String> {
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
    let heads = head_lists(mode);
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
    let expected =
        chart(file, &mode.name).map_or_else(|| expected_pairs(mode, kind, &heads), chart_pairs);
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

    let roles: Vec<_> = channels
        .iter()
        .enumerate()
        .map(|(i, c)| light(c, kind, emitter_in(mode, &heads, i)))
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
                Ok(Ok(result)) => check_mode(&file, mode, &fixture.kind, &result),
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
fn every_chart_lists_each_fine_once() {
    let mut wrong = Vec::new();
    for &(file, name, fines) in CHARTS {
        let mode = common::mode(file, name);
        let declared: Vec<_> = (0..mode.channels.len())
            .filter(|&i| mode.channels[i].byte == Byte::Fine)
            .collect();
        let listed: Vec<_> = fines.iter().map(|&(fine, _)| fine).collect();
        if listed != declared {
            wrong.push(format!(
                "{file} / {name}: chart lists {listed:?}, fines are {declared:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn every_charted_fine_joins_as_its_chart_says() {
    let mut wrong = Vec::new();
    for &(file, name, fines) in CHARTS {
        let mode = common::mode(file, name);
        let lowered = lower(&mode, 0, u32::try_from(mode.channels.len()).unwrap()).unwrap();
        for &(fine, says) in fines {
            let joined = lowered
                .pairs
                .iter()
                .find(|p| p.fine as usize == fine)
                .map(|p| p.coarse as usize);
            let expected = match says {
                Joins(coarse) => Some(coarse),
                Apart(_) => None,
            };
            if joined != expected {
                wrong.push(format!(
                    "{file} / {name} / {fine} {}: joined {joined:?}, chart says {says:?}",
                    mode.channels[fine].name
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_rule_agrees_with_every_chart() {
    let mut wrong = Vec::new();
    for &(file, name, fines) in CHARTS {
        let mode = common::mode(file, name);
        let kind = common::fixture(file).kind;
        let rule = expected_pairs(&mode, &kind, &head_lists(&mode));
        if rule != chart_pairs(fines) {
            wrong.push(format!(
                "{file} / {name}: rule {rule:?}, chart {:?}",
                chart_pairs(fines)
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
        let oracle = check_mode("inline.qxf", &mode, &kind_read, &lowered);
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
        (
            pairs,
            follows,
            check_mode("inline.qxf", &mode, &kind, &lowered)
        ),
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
        let before = check_mode(file, &mode, &fixture.kind, &lowered);
        lowered.pairs.retain(|p| !out.contains(&(p.coarse, p.fine)));
        lowered.pairs.extend(
            put.iter()
                .map(|&(coarse, fine)| rig::channels::Pair { coarse, fine }),
        );
        let after = check_mode(file, &mode, &fixture.kind, &lowered);
        if !before.is_empty() || after.is_empty() {
            wrong.push(format!(
                "{file} / {name} -{out:?} +{put:?}: before {before:?}, after {after:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_fog_pump_with_steps_never_follows_whatever_the_heads() {
    let led_head = "<Head><Channel>1</Channel><Channel>2</Channel></Head>";
    let shared = "<Head><Channel>0</Channel><Channel>1</Channel><Channel>2</Channel></Head>";
    let pump_head = "<Head><Channel>0</Channel></Head>";
    let layouts = [
        ("headless", String::new()),
        ("pump in the LED head", shared.to_string()),
        ("pump in no head", led_head.to_string()),
        (
            "pump in a head of its own",
            format!("{led_head}{pump_head}"),
        ),
    ];
    let mut wrong = Vec::new();
    for kind in ["Smoke", "Hazer"] {
        for (layout, heads) in &layouts {
            let (kind_read, mode) = inline(&format!(
                r#"<FixtureDefinition><Manufacturer>Test</Manufacturer><Model>Fog with LEDs</Model><Type>{kind}</Type>
                <Channel Name="Fog pump" Preset="IntensityMasterDimmer">
                    <Capability Min="0" Max="9">No fog output</Capability>
                    <Capability Min="10" Max="255">Fog output</Capability>
                </Channel>
                <Channel Name="LED dimmer" Preset="IntensityDimmer"/>
                <Channel Name="Red" Preset="IntensityRed"/>
                <Mode Name="3 channels">
                    <Channel Number="0">Fog pump</Channel><Channel Number="1">LED dimmer</Channel>
                    <Channel Number="2">Red</Channel>{heads}
                </Mode></FixtureDefinition>"#
            ));
            let lowered = lower(&mode, 0, 3).unwrap();
            let follows: Vec<_> = lowered.channels.iter().map(|c| c.follows.bits()).collect();
            let oracle = check_mode("inline.qxf", &mode, &kind_read, &lowered);
            if follows != [0, 3, 0] || !oracle.is_empty() {
                wrong.push(format!(
                    "{kind}, {layout}: follows {follows:?}, expected [0, 3, 0]; oracle {oracle:?}"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_fine_never_joins_a_coarse_of_another_preset() {
    // (coarse, its preset, fine, its preset)
    let cases = [
        ("Zoom", "BeamZoomSmallBig", "Focus fine", "BeamFocusFine"),
        ("Focus", "BeamFocusNearFar", "Zoom fine", "BeamZoomFine"),
        ("Pan", "PositionPan", "Tilt fine", "PositionTiltFine"),
    ];
    let mut wrong = Vec::new();
    for (coarse, high, fine, low) in cases {
        let (kind, mode) = inline(&format!(
            r#"<FixtureDefinition><Manufacturer>Test</Manufacturer><Model>Two</Model><Type>Moving Head</Type>
            <Channel Name="{coarse}" Preset="{high}"/>
            <Channel Name="{fine}" Preset="{low}"/>
            <Mode Name="Both"><Channel Number="0">{coarse}</Channel><Channel Number="1">{fine}</Channel></Mode>
            </FixtureDefinition>"#
        ));
        let lowered = lower(&mode, 0, 2).unwrap();
        let oracle = check_mode("inline.qxf", &mode, &kind, &lowered);
        if !lowered.pairs.is_empty() || !oracle.is_empty() {
            wrong.push(format!(
                "{fine} joined {coarse}: pairs {:?}; oracle {oracle:?}",
                lowered.pairs
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn acts_on_decides_before_any_other_evidence() {
    // (ActsOn of fine 1, ActsOn of fine 2, the pairs as (coarse, fine))
    type Case<'a> = (&'a str, &'a str, &'a [(u32, u32)]);
    // An ActsOn past the mode refuses the mode: fixture-library's `modes` test.
    let cases: [Case; 3] = [
        ("", "", &[(0, 1)]),
        // fine 2 names the dimmer, so the fine right after it stays apart
        ("", r#" ActsOn="0""#, &[(0, 2)]),
        // a fine that names a channel it may not take stays apart, even with a coarse at hand
        (r#" ActsOn="2""#, "", &[]),
    ];
    let mut wrong = Vec::new();
    for (first, second, expected) in cases {
        let (kind, mode) = inline(&format!(
            r#"<FixtureDefinition><Manufacturer>Test</Manufacturer><Model>Acts on</Model><Type>Moving Head</Type>
            <Channel Name="Dimmer" Preset="IntensityDimmer"/>
            <Channel Name="Dimmer fine" Preset="IntensityDimmerFine"/>
            <Channel Name="Other fine"><Group Byte="1">Intensity</Group></Channel>
            <Mode Name="Three">
                <Channel Number="0">Dimmer</Channel><Channel Number="1"{first}>Dimmer fine</Channel>
                <Channel Number="2"{second}>Other fine</Channel>
            </Mode></FixtureDefinition>"#
        ));
        let lowered = lower(&mode, 0, 3).unwrap();
        let pairs: Vec<_> = lowered.pairs.iter().map(|p| (p.coarse, p.fine)).collect();
        let oracle = check_mode("inline.qxf", &mode, &kind, &lowered);
        if pairs != expected || !oracle.is_empty() {
            wrong.push(format!(
                "ActsOn {first:?} {second:?}: pairs {pairs:?}, expected {expected:?}; oracle {oracle:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_coarse_after_its_fine_only_when_it_is_the_only_one() {
    // (channels as (name, preset), the pairs as (coarse, fine))
    type Case<'a> = (&'a [(&'a str, &'a str)], &'a [(u32, u32)]);
    let cases: [Case; 4] = [
        (
            &[
                ("Dimmer fine", "IntensityDimmerFine"),
                ("Dimmer", "IntensityDimmer"),
            ],
            &[(1, 0)],
        ),
        (
            &[
                ("Pan fine", "PositionPanFine"),
                ("Pan", "PositionPan"),
                ("Pan 2", "PositionPan"),
            ],
            &[],
        ),
        (
            &[
                ("Zoom fine", "BeamZoomFine"),
                ("Zoom", "BeamZoomSmallBig"),
                ("Zoom fine 2", "BeamZoomFine"),
                ("Zoom 2", "BeamZoomSmallBig"),
            ],
            &[(1, 2)],
        ),
        (
            &[
                ("Pan", "PositionPan"),
                ("Pan fine", "PositionPanFine"),
                ("Pan 2", "PositionPan"),
            ],
            &[(0, 1)],
        ),
    ];
    let mut wrong = Vec::new();
    for (channels, expected) in cases {
        let defined: String = channels
            .iter()
            .map(|(name, preset)| format!(r#"<Channel Name="{name}" Preset="{preset}"/>"#))
            .collect();
        let numbered: String = channels
            .iter()
            .enumerate()
            .map(|(i, (name, _))| format!(r#"<Channel Number="{i}">{name}</Channel>"#))
            .collect();
        let (kind, mode) = inline(&format!(
            r#"<FixtureDefinition><Manufacturer>Test</Manufacturer><Model>After</Model><Type>Moving Head</Type>
            {defined}<Mode Name="All">{numbered}</Mode></FixtureDefinition>"#
        ));
        let lowered = lower(&mode, 0, u32::try_from(channels.len()).unwrap()).unwrap();
        let pairs: Vec<_> = lowered.pairs.iter().map(|p| (p.coarse, p.fine)).collect();
        let oracle = check_mode("inline.qxf", &mode, &kind, &lowered);
        if pairs != expected || !oracle.is_empty() {
            wrong.push(format!(
                "{channels:?}: pairs {pairs:?}, expected {expected:?}; oracle {oracle:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_unique_preset_chooses_before_a_fine_that_only_sits_next_to_a_coarse() {
    // (name, its preset, the fine's preset, group of the group-only fine, its name)
    let cases = [
        ("Pan", "PositionPan", "PositionPanFine", "Pan", "Pan trim"),
        ("Focus", "BeamFocus", "BeamFocusFine", "Beam", "Iris fine"),
    ];
    let mut wrong = Vec::new();
    for (name, preset, fine_preset, group, adjacent) in cases {
        let (kind, mode) = inline(&format!(
            r#"<FixtureDefinition><Manufacturer>Test</Manufacturer><Model>Preset first</Model><Type>Moving Head</Type>
            <Channel Name="{name}" Preset="{preset}"/>
            <Channel Name="{adjacent}"><Group Byte="1">{group}</Group></Channel>
            <Channel Name="{name} fine" Preset="{fine_preset}"/>
            <Mode Name="Three">
                <Channel Number="0">{name}</Channel><Channel Number="1">{adjacent}</Channel>
                <Channel Number="2">{name} fine</Channel>
            </Mode></FixtureDefinition>"#
        ));
        let lowered = lower(&mode, 0, 3).unwrap();
        let pairs: Vec<_> = lowered.pairs.iter().map(|p| (p.coarse, p.fine)).collect();
        let oracle = check_mode("inline.qxf", &mode, &kind, &lowered);
        if pairs != [(0, 2)] || !oracle.is_empty() {
            wrong.push(format!(
                "{name}: pairs {pairs:?}, expected [(0, 2)]; oracle {oracle:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_fog_pump_declared_a_dimmer_fine_never_joins_and_never_follows() {
    let shared = "<Head><Channel>0</Channel><Channel>1</Channel><Channel>2</Channel></Head>";
    let layouts = [("headless", ""), ("one shared head", shared)];
    let mut wrong = Vec::new();
    for kind in ["Smoke", "Hazer"] {
        for (layout, heads) in layouts {
            let (kind_read, mode) = inline(&format!(
                r#"<FixtureDefinition><Manufacturer>Test</Manufacturer><Model>Fog with an LED dimmer</Model><Type>{kind}</Type>
                <Channel Name="LED dimmer" Preset="IntensityDimmer"/>
                <Channel Name="Fog pump" Preset="IntensityDimmerFine">
                    <Capability Min="0" Max="9">No fog output</Capability>
                    <Capability Min="10" Max="255">Fog output</Capability>
                </Channel>
                <Channel Name="Red" Preset="IntensityRed"/>
                <Mode Name="3 channels">
                    <Channel Number="0">LED dimmer</Channel><Channel Number="1">Fog pump</Channel>
                    <Channel Number="2">Red</Channel>{heads}
                </Mode></FixtureDefinition>"#
            ));
            let lowered = lower(&mode, 0, 3).unwrap();
            let follows: Vec<_> = lowered.channels.iter().map(|c| c.follows.bits()).collect();
            let oracle = check_mode("inline.qxf", &mode, &kind_read, &lowered);
            if follows != [3, 0, 0] || !lowered.pairs.is_empty() || !oracle.is_empty() {
                wrong.push(format!(
                    "{kind}, {layout}: follows {follows:?}, expected [3, 0, 0]; pairs {:?}, expected []; oracle {oracle:?}",
                    lowered.pairs
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_fog_switch_declared_a_shutter_follows_the_definition() {
    // Stairville SF-1500, mode Normal, DMX 1 (the pinned definition, first channel only).
    // The maker's switch turns fog output on and off; QLC+ declares it a shutter that
    // closes at 0 to 13. Definition error, follows the definition: it follows Blackout.
    let (kind, mode) = inline(
        r#"<FixtureDefinition><Manufacturer>Stairville</Manufacturer><Model>SF-1500</Model><Type>Smoke</Type>
        <Channel Name="On-Off">
            <Group Byte="0">Shutter</Group>
            <Capability Min="0" Max="13" Preset="ShutterClose">Off</Capability>
            <Capability Min="14" Max="127">Continuos CH2</Capability>
            <Capability Min="128" Max="231">Timer-Controlled CH2,3,4</Capability>
            <Capability Min="232" Max="255">Continuos to MAX</Capability>
        </Channel>
        <Mode Name="Normal"><Channel Number="0">On-Off</Channel></Mode>
        </FixtureDefinition>"#,
    );
    let lowered = lower(&mode, 0, 1).unwrap();
    assert_eq!(lowered.channels[0].follows, Follows::BLACKOUT);
    assert_eq!(
        check_mode("inline.qxf", &mode, &kind, &lowered),
        Vec::<String>::new()
    );
}
