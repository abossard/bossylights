//! What a channel does, from the three ways a definition says it: a preset, a group, a name.

use Byte::{Coarse, Fine};
use Function::{
    Amber, Blue, ColorTemperature, ColorWheel, Control, Cyan, Dimmer, Effect, Focus, Frost, Gobo,
    Green, Hue, Indigo, Iris, Lightness, Lime, Magenta, NoFunction, Other, Pan, Prism, Red,
    Saturation, Shutter, Speed, Strobe, Tilt, Uv, Value, White, Yellow, Zoom,
};
use fixture_library::{Byte, Channel, Direction, Function, parse};

fn definition(channel: &str) -> Vec<u8> {
    let name = channel
        .split("Name=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("a name");
    format!(
        r#"<?xml version="1.0"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Acme</Manufacturer><Model>Test</Model><Type>Other</Type>
 {channel}
 <Mode Name="M"><Channel Number="0">{name}</Channel></Mode>
</FixtureDefinition>"#
    )
    .into_bytes()
}

fn only_channel(channel: &str) -> Channel {
    let fixture = parse("t.qxf", &definition(channel)).expect("a definition reads");
    fixture
        .modes
        .into_iter()
        .next()
        .and_then(|m| m.channels.into_iter().next())
        .expect("one channel")
}

fn other(name: &str) -> Function {
    Other(name.to_string())
}

// the QLC+ channel presets (engine/src/qlcchannel.h), every one: what the channel does, its byte
#[test]
fn every_qlc_channel_preset_has_a_function_and_keeps_its_byte() {
    let cases: Vec<(&str, Function, Byte)> = vec![
        ("IntensityMasterDimmer", Dimmer, Coarse),
        ("IntensityMasterDimmerFine", Dimmer, Fine),
        ("IntensityDimmer", Dimmer, Coarse),
        ("IntensityDimmerFine", Dimmer, Fine),
        ("IntensityRed", Red, Coarse),
        ("IntensityRedFine", Red, Fine),
        ("IntensityGreen", Green, Coarse),
        ("IntensityGreenFine", Green, Fine),
        ("IntensityBlue", Blue, Coarse),
        ("IntensityBlueFine", Blue, Fine),
        ("IntensityCyan", Cyan, Coarse),
        ("IntensityCyanFine", Cyan, Fine),
        ("IntensityMagenta", Magenta, Coarse),
        ("IntensityMagentaFine", Magenta, Fine),
        ("IntensityYellow", Yellow, Coarse),
        ("IntensityYellowFine", Yellow, Fine),
        ("IntensityAmber", Amber, Coarse),
        ("IntensityAmberFine", Amber, Fine),
        ("IntensityWhite", White, Coarse),
        ("IntensityWhiteFine", White, Fine),
        ("IntensityUV", Uv, Coarse),
        ("IntensityUVFine", Uv, Fine),
        ("IntensityIndigo", Indigo, Coarse),
        ("IntensityIndigoFine", Indigo, Fine),
        ("IntensityLime", Lime, Coarse),
        ("IntensityLimeFine", Lime, Fine),
        ("IntensityHue", Hue, Coarse),
        ("IntensityHueFine", Hue, Fine),
        ("IntensitySaturation", Saturation, Coarse),
        ("IntensitySaturationFine", Saturation, Fine),
        ("IntensityLightness", Lightness, Coarse),
        ("IntensityLightnessFine", Lightness, Fine),
        ("IntensityValue", Value, Coarse),
        ("IntensityValueFine", Value, Fine),
        ("PositionPan", Pan, Coarse),
        ("PositionPanFine", Pan, Fine),
        ("PositionTilt", Tilt, Coarse),
        ("PositionTiltFine", Tilt, Fine),
        ("ColorWheel", ColorWheel, Coarse),
        ("ColorWheelFine", ColorWheel, Fine),
        ("GoboWheel", Gobo, Coarse),
        ("GoboWheelFine", Gobo, Fine),
        ("GoboIndex", Gobo, Coarse),
        ("GoboIndexFine", Gobo, Fine),
        ("ShutterIrisFine", Iris, Fine),
        ("BeamFocusFine", Focus, Fine),
        ("BeamZoomFine", Zoom, Fine),
        ("PositionXAxis", other("PositionXAxis"), Coarse),
        ("PositionYAxis", other("PositionYAxis"), Coarse),
        ("SpeedPanSlowFast", Speed, Coarse),
        ("SpeedPanFastSlow", Speed, Coarse),
        ("SpeedTiltSlowFast", Speed, Coarse),
        ("SpeedTiltFastSlow", Speed, Coarse),
        ("SpeedPanTiltSlowFast", Speed, Coarse),
        ("SpeedPanTiltFastSlow", Speed, Coarse),
        ("ColorMacro", ColorWheel, Coarse),
        ("ColorRGBMixer", other("ColorRGBMixer"), Coarse),
        ("ColorCTOMixer", ColorTemperature, Coarse),
        ("ColorCTCMixer", ColorTemperature, Coarse),
        ("ColorCTBMixer", ColorTemperature, Coarse),
        ("ShutterStrobeSlowFast", Strobe, Coarse),
        ("ShutterStrobeFastSlow", Strobe, Coarse),
        ("ShutterIrisMinToMax", Iris, Coarse),
        ("ShutterIrisMaxToMin", Iris, Coarse),
        ("BeamFocusNearFar", Focus, Coarse),
        ("BeamFocusFarNear", Focus, Coarse),
        ("BeamZoomSmallBig", Zoom, Coarse),
        ("BeamZoomBigSmall", Zoom, Coarse),
        ("PrismRotationSlowFast", Prism, Coarse),
        ("PrismRotationFastSlow", Prism, Coarse),
        ("NoFunction", NoFunction, Coarse),
        // not a QLC+ preset: kept under its own name, and its byte
        ("SomethingNew", other("SomethingNew"), Coarse),
        ("SomethingNewFine", other("SomethingNewFine"), Fine),
        // capability presets, not channel presets
        ("RotationClockwise", other("RotationClockwise"), Coarse),
        ("ResetAll", other("ResetAll"), Coarse),
        ("ShutterOpen", other("ShutterOpen"), Coarse),
    ];
    for (preset, function, byte) in cases {
        let channel = format!(r#"<Channel Name="C" Preset="{preset}"/>"#);
        let found = only_channel(&channel);
        assert_eq!((&found.function, found.byte), (&function, byte), "{preset}");
    }
}

// a channel without a preset: its group and name say what it does
#[test]
fn a_channel_does_what_its_group_or_name_says() {
    let cases: Vec<(&str, Function, Byte)> = vec![
        (
            r#"<Channel Name="C"><Group Byte="0">Intensity</Group></Channel>"#,
            Dimmer,
            Coarse,
        ),
        (
            r#"<Channel Name="Red"><Group Byte="0">Intensity</Group></Channel>"#,
            Red,
            Coarse,
        ),
        (
            r#"<Channel Name="Green"><Group Byte="0">Intensity</Group></Channel>"#,
            Green,
            Coarse,
        ),
        (
            r#"<Channel Name="Blue"><Group Byte="0">Intensity</Group></Channel>"#,
            Blue,
            Coarse,
        ),
        (
            r#"<Channel Name="White"><Group Byte="0">Intensity</Group></Channel>"#,
            White,
            Coarse,
        ),
        (
            r#"<Channel Name="Amber"><Group Byte="0">Intensity</Group></Channel>"#,
            Amber,
            Coarse,
        ),
        (
            r#"<Channel Name="UV"><Group Byte="0">Intensity</Group></Channel>"#,
            Uv,
            Coarse,
        ),
        (
            r#"<Channel Name="Dimmer fine"><Group Byte="1">Intensity</Group></Channel>"#,
            Dimmer,
            Fine,
        ),
        // fog, haze or smoke in the Intensity group is no dimmer
        (
            r#"<Channel Name="Amount of Fog"><Group Byte="0">Intensity</Group></Channel>"#,
            other("Amount of Fog"),
            Coarse,
        ),
        (
            r#"<Channel Name="Haze"><Group Byte="0">Intensity</Group></Channel>"#,
            other("Haze"),
            Coarse,
        ),
        (
            r#"<Channel Name="Smoke-output"><Group Byte="0">Intensity</Group></Channel>"#,
            other("Smoke-output"),
            Coarse,
        ),
        (
            r#"<Channel Name="Foggy dimmer"><Group Byte="0">Intensity</Group></Channel>"#,
            Dimmer,
            Coarse,
        ),
        (
            r#"<Channel Name="C"><Group Byte="0">Pan</Group></Channel>"#,
            Pan,
            Coarse,
        ),
        (
            r#"<Channel Name="C"><Group Byte="1">Pan</Group></Channel>"#,
            Pan,
            Fine,
        ),
        (
            r#"<Channel Name="C"><Group Byte="1">Tilt</Group></Channel>"#,
            Tilt,
            Fine,
        ),
        (
            r#"<Channel Name="C"><Group Byte="0">Colour</Group></Channel>"#,
            ColorWheel,
            Coarse,
        ),
        (
            r#"<Channel Name="C"><Group Byte="0">Gobo</Group></Channel>"#,
            Gobo,
            Coarse,
        ),
        (
            r#"<Channel Name="C"><Group Byte="0">Shutter</Group></Channel>"#,
            Shutter,
            Coarse,
        ),
        (
            r#"<Channel Name="C"><Group Byte="0">Speed</Group></Channel>"#,
            Speed,
            Coarse,
        ),
        (
            r#"<Channel Name="C"><Group Byte="0">Effect</Group></Channel>"#,
            Effect,
            Coarse,
        ),
        (
            r#"<Channel Name="C"><Group Byte="0">Prism</Group></Channel>"#,
            Prism,
            Coarse,
        ),
        (
            r#"<Channel Name="C"><Group Byte="0">Maintenance</Group></Channel>"#,
            Control,
            Coarse,
        ),
        (
            r#"<Channel Name="Nothing here"><Group Byte="0">Nothing</Group></Channel>"#,
            NoFunction,
            Coarse,
        ),
        (
            r#"<Channel Name="Iris"><Group Byte="0">Beam</Group></Channel>"#,
            Iris,
            Coarse,
        ),
        (
            r#"<Channel Name="Iris fine"><Group Byte="1">Beam</Group></Channel>"#,
            Iris,
            Fine,
        ),
        (
            r#"<Channel Name="Frost"><Group Byte="0">Beam</Group></Channel>"#,
            Frost,
            Coarse,
        ),
        (
            r#"<Channel Name="Zoom"><Group Byte="0">Beam</Group></Channel>"#,
            Zoom,
            Coarse,
        ),
        (
            r#"<Channel Name="Focus"><Group Byte="0">Beam</Group></Channel>"#,
            Focus,
            Coarse,
        ),
        (
            r#"<Channel Name="Beam angle"><Group Byte="1">Beam</Group></Channel>"#,
            other("Beam angle"),
            Fine,
        ),
        (
            r#"<Channel Name="Odd"><Group Byte="0">Sparkle</Group></Channel>"#,
            other("Odd"),
            Coarse,
        ),
        (r#"<Channel Name="C"/>"#, other("C"), Coarse),
    ];
    for (channel, function, byte) in cases {
        let found = only_channel(channel);
        assert_eq!(
            (&found.function, found.byte),
            (&function, byte),
            "{channel}"
        );
    }
}

// the channel element, its default value
#[test]
fn a_channel_carries_the_default_its_definition_gives() {
    let cases: [(&str, Option<u8>); 5] = [
        (r#"<Channel Name="C" Preset="IntensityRed"/>"#, None),
        (
            r#"<Channel Name="C" Default="128" Preset="PositionPan"/>"#,
            Some(128),
        ),
        (
            r#"<Channel Name="C" Default="0"><Group Byte="0">Shutter</Group></Channel>"#,
            Some(0),
        ),
        (
            r#"<Channel Name="C" Default="255"><Group Byte="0">Effect</Group></Channel>"#,
            Some(255),
        ),
        (
            r#"<Channel Name="C" Default=" 32 " Preset="NoFunction"/>"#,
            Some(32),
        ),
    ];
    for (channel, default) in cases {
        assert_eq!(only_channel(channel).default, default, "{channel}");
    }
}

// Real files name a channel with a space at its end, in one place and not the other.
#[test]
fn a_mode_finds_a_channel_whose_name_has_spaces_at_the_end() {
    let cases = [
        ("Strobe ", "Strobe "),
        ("Strobe ", "Strobe"),
        ("Strobe", "Strobe "),
    ];
    for (defined, used) in cases {
        let source = format!(
            r#"<FixtureDefinition><Manufacturer>A</Manufacturer><Model>M</Model><Type>Other</Type>
 <Channel Name="{defined}" Preset="ShutterStrobeSlowFast"/>
 <Mode Name="M"><Channel Number="0">{used}</Channel></Mode></FixtureDefinition>"#
        );
        let fixture = parse("t.qxf", source.as_bytes()).expect("the name is found");
        let channel = fixture
            .modes
            .first()
            .and_then(|m| m.channels.first())
            .expect("one channel");
        assert_eq!(channel.function, Strobe, "{defined:?} / {used:?}");
    }
}

// the channel element, which way its values run
#[test]
fn a_channel_preset_says_which_way_its_values_run() {
    use fixture_library::Direction::{Down, Up};
    let cases: Vec<(&str, Option<Direction>)> = vec![
        (r#"Preset="SpeedPanSlowFast""#, Some(Up)),
        (r#"Preset="SpeedPanFastSlow""#, Some(Down)),
        (r#"Preset="SpeedTiltSlowFast""#, Some(Up)),
        (r#"Preset="SpeedTiltFastSlow""#, Some(Down)),
        (r#"Preset="SpeedPanTiltSlowFast""#, Some(Up)),
        (r#"Preset="SpeedPanTiltFastSlow""#, Some(Down)),
        (r#"Preset="ShutterStrobeSlowFast""#, Some(Up)),
        (r#"Preset="ShutterStrobeFastSlow""#, Some(Down)),
        (r#"Preset="ShutterIrisMinToMax""#, Some(Up)),
        (r#"Preset="ShutterIrisMaxToMin""#, Some(Down)),
        (r#"Preset="BeamFocusNearFar""#, Some(Up)),
        (r#"Preset="BeamFocusFarNear""#, Some(Down)),
        (r#"Preset="BeamZoomSmallBig""#, Some(Up)),
        (r#"Preset="BeamZoomBigSmall""#, Some(Down)),
        (r#"Preset="PrismRotationSlowFast""#, Some(Up)),
        (r#"Preset="PrismRotationFastSlow""#, Some(Down)),
        // a fine byte follows its coarse byte
        (r#"Preset="BeamZoomFine""#, None),
        (r#"Preset="ShutterIrisFine""#, None),
        (r#"Preset="BeamFocusFine""#, None),
        // a preset without a direction, or no preset
        (r#"Preset="IntensityRed""#, None),
        (r#"Preset="PositionPan""#, None),
        (r#"Preset="ColorWheel""#, None),
        (r#"Preset="SomethingSlowFast""#, None),
        ("", None),
    ];
    for (attributes, direction) in cases {
        let channel = format!(r#"<Channel Name="C" {attributes}/>"#);
        assert_eq!(only_channel(&channel).direction, direction, "{attributes}");
    }
}
