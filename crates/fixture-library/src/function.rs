//! What a channel does, from a preset, a group or a name. A calculation: strings in, a
//! closed `Function` out. Anything the list does not know stays as `Other(name)`.

use crate::model::{Byte, Function};
use alloc::string::{String, ToString};

/// A QLC+ preset name. `None` when the list does not know it.
fn from_preset(preset: &str) -> Option<(Function, Byte)> {
    let (stem, byte) = match preset.strip_suffix("Fine") {
        Some(stem) => (stem, Byte::Fine),
        None => (preset, Byte::Coarse),
    };
    let function = match stem {
        "IntensityDimmer" | "IntensityMasterDimmer" => Function::Dimmer,
        "IntensityRed" => Function::Red,
        "IntensityGreen" => Function::Green,
        "IntensityBlue" => Function::Blue,
        "IntensityWhite" => Function::White,
        "IntensityAmber" => Function::Amber,
        "IntensityUV" => Function::Uv,
        "IntensityLime" => Function::Lime,
        "IntensityCyan" => Function::Cyan,
        "IntensityMagenta" => Function::Magenta,
        "IntensityYellow" => Function::Yellow,
        "PositionPan" => Function::Pan,
        "PositionTilt" => Function::Tilt,
        "ColorMacro" | "ColorDoubleMacro" | "ColorWheel" => Function::ColorWheel,
        "ShutterOpen" | "ShutterClose" => Function::Shutter,
        "ShutterStrobeSlowFast" => Function::Strobe,
        _ if stem.starts_with("BeamZoom") => Function::Zoom,
        _ if stem.starts_with("BeamFocus") => Function::Focus,
        _ if stem.starts_with("Gobo") => Function::Gobo,
        _ if stem.starts_with("Strobe") => Function::Strobe,
        _ if stem.starts_with("Prism") => Function::Prism,
        _ if stem.starts_with("SpeedPanTilt") => Function::Speed,
        _ if stem.starts_with("Rotation") => Function::Rotation,
        _ if stem.starts_with("Reset") || stem.starts_with("Lamp") => Function::Control,
        _ => return None,
    };
    Some((function, byte))
}

/// A channel's colour or dimmer, said only by its name inside the Intensity group.
fn intensity_by_name(name: &str) -> Function {
    match name.trim().to_ascii_lowercase().as_str() {
        "red" => Function::Red,
        "green" => Function::Green,
        "blue" => Function::Blue,
        "white" => Function::White,
        "amber" => Function::Amber,
        "uv" => Function::Uv,
        _ => Function::Dimmer,
    }
}

/// The Beam group holds zoom, focus, iris and more: only its name tells which.
fn beam_by_name(name: &str) -> Option<Function> {
    let name = name.to_ascii_lowercase();
    if name.contains("focus") {
        Some(Function::Focus)
    } else if name.contains("zoom") {
        Some(Function::Zoom)
    } else {
        None
    }
}

/// What a channel does: its preset if it has one, else its group, else it is `Other`.
/// `group` is the group's name and the byte it says.
pub fn function_of(
    preset: Option<&str>,
    group: Option<(&str, Byte)>,
    name: &str,
) -> (Function, Byte) {
    let other = || Function::Other(name.to_string());
    if let Some(preset) = preset {
        return from_preset(preset)
            .unwrap_or_else(|| (Function::Other(String::from(preset)), Byte::Coarse));
    }
    let Some((group, byte)) = group else {
        return (other(), Byte::Coarse);
    };
    let function = match group {
        "Intensity" => intensity_by_name(name),
        "Pan" => Function::Pan,
        "Tilt" => Function::Tilt,
        "Colour" => Function::ColorWheel,
        "Gobo" => Function::Gobo,
        "Shutter" => Function::Shutter,
        "Speed" => Function::Speed,
        "Effect" => Function::Effect,
        "Prism" => Function::Prism,
        "Beam" => beam_by_name(name).unwrap_or_else(other),
        "Maintenance" => Function::Control,
        _ => other(),
    };
    (function, byte)
}
