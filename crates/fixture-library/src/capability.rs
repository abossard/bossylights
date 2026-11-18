//! What a range does, from its QLC+ capability preset (`engine/src/qlccapability.h`) and
//! the values beside it (`Res1`, `Res2`, a legacy `Res`). A calculation: strings in, a
//! closed `Does` out. A value that cannot be read is not said, never guessed.

use crate::model::{Direction, Does, Hertz, Pace, Rgb, Turn};
use alloc::string::ToString;

/// The attributes of a capability that say what it does.
pub struct Said<'a> {
    pub preset: Option<&'a str>,
    pub first: Option<&'a str>,
    pub second: Option<&'a str>,
    /// The legacy picture attribute, from before presets.
    pub picture: Option<&'a str>,
}

pub fn does(said: &Said) -> Option<Does> {
    let Some(preset) = said.preset else {
        // A relative picture path is a gobo, as QLC+ reads it. An absolute one is not.
        return said
            .picture
            .filter(|path| !path.starts_with('/'))
            .map(|path| gobo(Some(path), false));
    };
    let flash = ["Strobe", "Pulse", "RampUp", "RampDown"]
        .iter()
        .any(|start| preset.starts_with(start));
    match preset {
        "ShutterOpen" => Some(Does::Open),
        "ShutterClose" => Some(Does::Closed),
        "LampOn" => Some(Does::LampOn),
        "LampOff" => Some(Does::LampOff),
        "ColorMacro" => Some(Does::Color(rgb(said.first?)?)),
        "ColorDoubleMacro" => Some(Does::Colors(rgb(said.first?)?, rgb(said.second?)?)),
        "PrismEffectOn" => Some(Does::Prism {
            faces: said.first.and_then(|f| f.trim().parse().ok()),
        }),
        "PrismEffectOff" => Some(Does::PrismOff),
        "GoboMacro" => Some(gobo(said.first, false)),
        "GoboShakeMacro" => Some(gobo(said.first, true)),
        "SlowToFast" | "NearToFar" | "SmallToBig" => Some(Does::Runs(Direction::Up)),
        "FastToSlow" | "FarToNear" | "BigToSmall" => Some(Does::Runs(Direction::Down)),
        "RotationStop" => Some(Does::Rotation(Turn::Stop)),
        "RotationIndexed" => Some(Does::Rotation(Turn::Indexed)),
        _ if flash => Some(Does::Strobe(pace(preset, said))),
        _ => {
            if let Some(rest) = preset.strip_prefix("RotationCounterClockwise") {
                Some(Does::Rotation(Turn::CounterClockwise(pace(rest, said))))
            } else if let Some(rest) = preset.strip_prefix("RotationClockwise") {
                Some(Does::Rotation(Turn::Clockwise(pace(rest, said))))
            } else if preset.starts_with("Reset") {
                Some(Does::Reset)
            } else {
                None
            }
        }
    }
}

fn gobo(picture: Option<&str>, shakes: bool) -> Does {
    Does::Gobo {
        picture: picture
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(ToString::to_string),
        shakes,
    }
}

/// The pace a preset's name ends in: `SlowToFast`, `FastToSlow`, `Random...`, a
/// `Frequency` (`Res1` in hertz) or a `FreqRange` (`Res1` to `Res2`).
fn pace(preset: &str, said: &Said) -> Pace {
    let hertz = |from: Option<&str>, to: Option<&str>| {
        let (from, to) = (millihertz(from?)?, millihertz(to?)?);
        Some(Pace::Hertz(Hertz { from, to }))
    };
    let found = if preset.contains("Random") {
        Some(Pace::Random)
    } else if preset.ends_with("SlowToFast") {
        Some(Pace::Goes(Direction::Up))
    } else if preset.ends_with("FastToSlow") {
        Some(Pace::Goes(Direction::Down))
    } else if preset.ends_with("Frequency") {
        hertz(said.first, said.first)
    } else if preset.ends_with("FreqRange") {
        hertz(said.first, said.second)
    } else {
        None
    };
    found.unwrap_or(Pace::Unsaid)
}

/// Hertz as QLC+ writes them (`25`, `0.5`, `12.25`) in millihertz. Digits after the
/// third decimal are dropped.
fn millihertz(text: &str) -> Option<u32> {
    let (whole, part) = text.trim().split_once('.').unwrap_or((text.trim(), ""));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || !(part.is_empty() || digits(part)) {
        return None;
    }
    let whole: u32 = whole.parse().ok()?;
    let thousandths = part
        .bytes()
        .chain(core::iter::repeat(b'0'))
        .take(3)
        .fold(0, |sum, b| sum * 10 + u32::from(b - b'0'));
    whole.checked_mul(1000)?.checked_add(thousandths)
}

/// `#rrggbb`, as every colour in the QLC+ library is written.
fn rgb(text: &str) -> Option<Rgb> {
    let hex = text.trim().strip_prefix('#')?;
    let byte = |at: usize| {
        hex.get(at..at + 2)
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
    };
    (hex.len() == 6).then_some(())?;
    Some(Rgb {
        red: byte(0)?,
        green: byte(2)?,
        blue: byte(4)?,
    })
}
