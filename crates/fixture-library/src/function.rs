//! What a channel does, from a preset, a group or a name. A calculation: strings in, a
//! closed `Function` out. Anything the list does not know stays as `Other(name)`.

use crate::model::{Byte, Function};
use alloc::string::{String, ToString};

/// A QLC+ channel preset (`engine/src/qlcchannel.h`), every one of them, without its
/// `Fine` ending. `None` when the list does not know it.
fn from_preset(stem: &str) -> Option<Function> {
    let function = match stem {
        "IntensityMasterDimmer" | "IntensityDimmer" => Function::Dimmer,
        "IntensityRed" => Function::Red,
        "IntensityGreen" => Function::Green,
        "IntensityBlue" => Function::Blue,
        "IntensityCyan" => Function::Cyan,
        "IntensityMagenta" => Function::Magenta,
        "IntensityYellow" => Function::Yellow,
        "IntensityAmber" => Function::Amber,
        "IntensityWhite" => Function::White,
        "IntensityUV" => Function::Uv,
        "IntensityIndigo" => Function::Indigo,
        "IntensityLime" => Function::Lime,
        "IntensityHue" => Function::Hue,
        "IntensitySaturation" => Function::Saturation,
        "IntensityLightness" => Function::Lightness,
        "IntensityValue" => Function::Value,
        "PositionPan" => Function::Pan,
        "PositionTilt" => Function::Tilt,
        // Other on purpose: the X or Y of a laser or an effect, not where a beam points.
        // QLC+ itself gives them no pan or tilt meaning.
        "PositionXAxis" | "PositionYAxis" => return None,
        "SpeedPanSlowFast"
        | "SpeedPanFastSlow"
        | "SpeedTiltSlowFast"
        | "SpeedTiltFastSlow"
        | "SpeedPanTiltSlowFast"
        | "SpeedPanTiltFastSlow" => Function::Speed,
        "ColorMacro" | "ColorWheel" => Function::ColorWheel,
        // Other on purpose: one channel that picks a whole colour, two channels in the
        // library. Nothing says how its values map to red, green and blue.
        "ColorRGBMixer" => return None,
        "ColorCTOMixer" | "ColorCTCMixer" | "ColorCTBMixer" => Function::ColorTemperature,
        "GoboWheel" | "GoboIndex" => Function::Gobo,
        "ShutterStrobeSlowFast" | "ShutterStrobeFastSlow" => Function::Strobe,
        "ShutterIrisMinToMax" | "ShutterIrisMaxToMin" | "ShutterIris" => Function::Iris,
        "BeamFocusNearFar" | "BeamFocusFarNear" | "BeamFocus" => Function::Focus,
        "BeamZoomSmallBig" | "BeamZoomBigSmall" | "BeamZoom" => Function::Zoom,
        "PrismRotationSlowFast" | "PrismRotationFastSlow" => Function::Prism,
        "NoFunction" => Function::NoFunction,
        _ => return None,
    };
    Some(function)
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
    } else if name.contains("iris") {
        Some(Function::Iris)
    } else if name.contains("frost") {
        Some(Function::Frost)
    } else {
        None
    }
}

/// What a channel does: its preset if it has one, else its group, else it is `Other`.
/// `group` is the group's name and the byte it says. A preset ending in `Fine` is the
/// fine byte, also when the list does not know the preset.
pub fn function_of(
    preset: Option<&str>,
    group: Option<(&str, Byte)>,
    name: &str,
) -> (Function, Byte) {
    let other = || Function::Other(name.to_string());
    if let Some(preset) = preset {
        let (stem, byte) = match preset.strip_suffix("Fine") {
            Some(stem) => (stem, Byte::Fine),
            None => (preset, Byte::Coarse),
        };
        let function = from_preset(stem).unwrap_or_else(|| Function::Other(String::from(preset)));
        return (function, byte);
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
        "Nothing" => Function::NoFunction,
        _ => other(),
    };
    (function, byte)
}
