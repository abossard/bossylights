//! The v11 function of a channel and its declared channel-model role.
//! Both use the QLC+ preset table. Only v11 interprets a channel name.

use crate::model::{Byte, Channel, Declared, Dimming, Direction, Function};
use alloc::string::{String, ToString};

impl Channel {
    /// The declared colour, else preset, else group. A Smoke or Hazer dimmer preset
    /// controls light only on a plain byte sharing a head with an emitter, or on a
    /// plain byte in a headless mode with an emitter. On a dimmer pack the Intensity group dims.
    #[must_use]
    pub fn role(&self) -> Function {
        let declared = &self.declared;
        if let Some(colour) = &declared.colour {
            return colour.clone();
        }
        if let Some(preset) = &declared.preset {
            return match from_preset(preset_stem(preset)) {
                Some(Function::Dimmer) if declared.dimming == Dimming::Fog => {
                    Function::Other(preset.clone())
                }
                Some(function) => function,
                None => Function::Other(preset.clone()),
            };
        }
        match declared.group.as_deref().unwrap_or_default() {
            "Intensity" if declared.dimming == Dimming::DimmerPack => Function::Dimmer,
            "Pan" => Function::Pan,
            "Tilt" => Function::Tilt,
            "Colour" => Function::ColorWheel,
            "Gobo" => Function::Gobo,
            "Shutter" => Function::Shutter,
            "Speed" => Function::Speed,
            "Effect" => Function::Effect,
            "Prism" => Function::Prism,
            "Maintenance" => Function::Control,
            "Nothing" => Function::NoFunction,
            group => Function::Other(group.into()),
        }
    }

    /// A dimmer preset that runs fog, not light: on Smoke and Hazer, see `role`.
    #[must_use]
    pub fn is_fog_dimmer(&self) -> bool {
        self.declared.pairing_family().as_deref() == Some("IntensityDimmer")
            && matches!(self.role(), Function::Other(_))
    }
}

impl Declared {
    /// The channel preset without its optional `Fine` suffix.
    #[must_use]
    pub fn preset_stem(&self) -> Option<&str> {
        self.preset.as_deref().map(preset_stem)
    }

    /// The control a fine byte's preset names, to find its coarse: the preset without its
    /// `Fine`, the way it runs (`SlowFast`, ...) and `Master`, with QLC+'s X and Y axis
    /// aliases taken as pan and tilt. Not the `Function`: an X axis does not point a beam
    /// (`role` says Other), yet its fine joins it as a pan fine joins pan.
    #[must_use]
    pub fn pairing_family(&self) -> Option<String> {
        let stem = self.preset_stem()?;
        let stem = [
            "SlowFast", "FastSlow", "SmallBig", "BigSmall", "NearFar", "FarNear", "MinToMax",
            "MaxToMin",
        ]
        .into_iter()
        .find_map(|way| stem.strip_suffix(way))
        .unwrap_or(stem);
        Some(
            stem.replace("Master", "")
                .replace("PositionXAxis", "PositionPan")
                .replace("PositionYAxis", "PositionTilt"),
        )
    }

    /// The QLC+ group of the channel: its `<Group>`, else the group its preset belongs to.
    #[must_use]
    pub fn pairing_group(&self) -> Option<&str> {
        self.group
            .as_deref()
            .or_else(|| self.preset_stem().and_then(preset_group))
    }

    /// A preset for the index (position) of a wheel, not its slots.
    #[must_use]
    pub fn is_index_preset(&self) -> bool {
        self.preset_stem().is_some_and(|s| s.ends_with("Index"))
    }

    /// The emitted or subtractive colour, from `<Colour>` first, then the Intensity preset.
    #[must_use]
    pub fn emitter_colour(&self) -> Option<Function> {
        self.colour
            .clone()
            .or_else(|| preset_colour(self.preset_stem()?))
    }
}

fn preset_stem(preset: &str) -> &str {
    preset.strip_suffix("Fine").unwrap_or(preset)
}

/// The QLC+ group of a preset, for a channel without an explicit group.
fn preset_group(stem: &str) -> Option<&'static str> {
    match stem {
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
        .find_map(|(prefix, group)| stem.starts_with(prefix).then_some(group)),
    }
}

fn preset_colour(stem: &str) -> Option<Function> {
    stem.strip_prefix("Intensity").and_then(colour_of)
}

/// The colour named by a QLC+ `<Colour>` child or an `Intensity` colour preset.
pub(crate) fn colour_of(name: &str) -> Option<Function> {
    let colour = match name {
        "Red" => Function::Red,
        "Green" => Function::Green,
        "Blue" => Function::Blue,
        "Cyan" => Function::Cyan,
        "Magenta" => Function::Magenta,
        "Yellow" => Function::Yellow,
        "Amber" => Function::Amber,
        "White" => Function::White,
        "UV" => Function::Uv,
        "Indigo" => Function::Indigo,
        "Lime" => Function::Lime,
        _ => return None,
    };
    Some(colour)
}

/// A QLC+ channel preset (`engine/src/qlcchannel.h`), every one of them, without its
/// `Fine` ending. `None` when the list does not know it.
fn from_preset(stem: &str) -> Option<Function> {
    let function = match stem {
        "IntensityMasterDimmer" | "IntensityDimmer" => Function::Dimmer,
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
        _ => return preset_colour(stem),
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

/// Which way a channel preset's values run. Only the presets that say it, and never a
/// fine byte: it runs as its coarse byte.
pub fn direction_of(preset: &str) -> Option<Direction> {
    match preset {
        "SpeedPanSlowFast"
        | "SpeedTiltSlowFast"
        | "SpeedPanTiltSlowFast"
        | "ShutterStrobeSlowFast"
        | "ShutterIrisMinToMax"
        | "BeamFocusNearFar"
        | "BeamZoomSmallBig"
        | "PrismRotationSlowFast" => Some(Direction::Up),
        "SpeedPanFastSlow"
        | "SpeedTiltFastSlow"
        | "SpeedPanTiltFastSlow"
        | "ShutterStrobeFastSlow"
        | "ShutterIrisMaxToMin"
        | "BeamFocusFarNear"
        | "BeamZoomBigSmall"
        | "PrismRotationFastSlow" => Some(Direction::Down),
        _ => None,
    }
}
