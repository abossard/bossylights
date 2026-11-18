//! What a channel does, from the three ways a definition says it: a preset, a group, a name.

use Byte::{Coarse, Fine};
use Function::{
    Amber, Blue, ColorWheel, Control, Dimmer, Effect, Focus, Gobo, Green, Lime, Other, Pan, Prism,
    Red, Rotation, Shutter, Speed, Strobe, Tilt, Uv, White, Zoom,
};
use fixture_library::{Byte, Function, parse};

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

fn other(name: &str) -> Function {
    Other(name.to_string())
}

// the channel element, what it does
#[test]
fn a_channel_does_what_its_preset_group_or_name_says() {
    let cases: Vec<(&str, Function, Byte)> = vec![
        (
            r#"<Channel Name="C" Preset="IntensityDimmer"/>"#,
            Dimmer,
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="IntensityMasterDimmer"/>"#,
            Dimmer,
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="IntensityMasterDimmerFine"/>"#,
            Dimmer,
            Fine,
        ),
        (r#"<Channel Name="C" Preset="IntensityRed"/>"#, Red, Coarse),
        (
            r#"<Channel Name="C" Preset="IntensityGreen"/>"#,
            Green,
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="IntensityBlue"/>"#,
            Blue,
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="IntensityRedFine"/>"#,
            Red,
            Fine,
        ),
        (
            r#"<Channel Name="C" Preset="IntensityWhite"/>"#,
            White,
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="IntensityAmber"/>"#,
            Amber,
            Coarse,
        ),
        (r#"<Channel Name="C" Preset="IntensityUV"/>"#, Uv, Coarse),
        (
            r#"<Channel Name="C" Preset="IntensityLime"/>"#,
            Lime,
            Coarse,
        ),
        (r#"<Channel Name="C" Preset="PositionPan"/>"#, Pan, Coarse),
        (r#"<Channel Name="C" Preset="PositionPanFine"/>"#, Pan, Fine),
        (r#"<Channel Name="C" Preset="PositionTilt"/>"#, Tilt, Coarse),
        (
            r#"<Channel Name="C" Preset="PositionTiltFine"/>"#,
            Tilt,
            Fine,
        ),
        (
            r#"<Channel Name="C" Preset="BeamZoomSmallBig"/>"#,
            Zoom,
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="BeamFocusNearFar"/>"#,
            Focus,
            Coarse,
        ),
        (r#"<Channel Name="C" Preset="GoboMacro"/>"#, Gobo, Coarse),
        (r#"<Channel Name="C" Preset="GoboIndexFine"/>"#, Gobo, Fine),
        (
            r#"<Channel Name="C" Preset="ColorMacro"/>"#,
            ColorWheel,
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="ColorWheelFine"/>"#,
            ColorWheel,
            Fine,
        ),
        (
            r#"<Channel Name="C" Preset="ShutterStrobeSlowFast"/>"#,
            Strobe,
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="StrobeSlowToFast"/>"#,
            Strobe,
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="ShutterOpen"/>"#,
            Shutter,
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="PrismEffectOn"/>"#,
            Prism,
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="SpeedPanTiltFastSlow"/>"#,
            Speed,
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="RotationClockwise"/>"#,
            Rotation,
            Coarse,
        ),
        (r#"<Channel Name="C" Preset="ResetAll"/>"#, Control, Coarse),
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
            r#"<Channel Name="Dimmer fine"><Group Byte="1">Intensity</Group></Channel>"#,
            Dimmer,
            Fine,
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
        // never dropped: what is not in the list stays, under its own name
        (
            r#"<Channel Name="C" Preset="PulseSlowToFast"/>"#,
            other("PulseSlowToFast"),
            Coarse,
        ),
        (
            r#"<Channel Name="C" Preset="SomethingNew"/>"#,
            other("SomethingNew"),
            Coarse,
        ),
        (
            r#"<Channel Name="Iris"><Group Byte="0">Beam</Group></Channel>"#,
            other("Iris"),
            Coarse,
        ),
        (
            r#"<Channel Name="Odd"><Group Byte="0">Sparkle</Group></Channel>"#,
            other("Odd"),
            Coarse,
        ),
        (
            r#"<Channel Name="Nothing here"><Group Byte="0">Nothing</Group></Channel>"#,
            other("Nothing here"),
            Coarse,
        ),
        (r#"<Channel Name="C"/>"#, other("C"), Coarse),
    ];
    for (channel, function, byte) in cases {
        let fixture = parse("t.qxf", &definition(channel)).expect("a definition reads");
        let found = fixture
            .modes
            .first()
            .and_then(|m| m.channels.first())
            .expect("one channel");
        assert_eq!(
            (&found.function, found.byte),
            (&function, byte),
            "{channel}"
        );
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
