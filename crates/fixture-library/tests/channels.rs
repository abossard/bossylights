//! What a channel does, from the three ways a definition says it: a preset, a group, a name.

use Byte::{Coarse, Fine};
use Function::{
    Amber, Blue, ColorTemperature, ColorWheel, Control, Cyan, Dimmer, Effect, Focus, Frost, Gobo,
    Green, Hue, Indigo, Iris, Lightness, Lime, Magenta, NoFunction, Other, Pan, Prism, Red,
    Saturation, Shutter, Speed, Strobe, Tilt, Uv, Value, White, Yellow, Zoom,
};
use fixture_library::{Byte, Channel, Direction, Function, Problem, parse};

fn definition(channel: &str, kind: &str) -> Vec<u8> {
    let name = channel
        .split("Name=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("a name");
    format!(
        r#"<?xml version="1.0"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Acme</Manufacturer><Model>Test</Model><Type>{kind}</Type>
 {channel}
 <Mode Name="M"><Channel Number="0">{name}</Channel></Mode>
</FixtureDefinition>"#
    )
    .into_bytes()
}

fn only_channel(channel: &str) -> Channel {
    channel_of_kind(channel, "Other")
}

fn channel_of_kind(channel: &str, kind: &str) -> Channel {
    let fixture = parse("t.qxf", &definition(channel, kind)).expect("a definition reads");
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
        assert_eq!(found.role(), function, "{preset}");
        assert_eq!(found.declared.preset.as_deref(), Some(preset));
        assert_eq!(
            found.declared.preset_stem(),
            Some(preset.strip_suffix("Fine").unwrap_or(preset)),
            "{preset}"
        );
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
        // v11 uses Dimmer for an unrecognised Intensity name.
        (
            r#"<Channel Name="Amount of Fog"><Group Byte="0">Intensity</Group></Channel>"#,
            Dimmer,
            Coarse,
        ),
        (
            r#"<Channel Name="Haze"><Group Byte="0">Intensity</Group></Channel>"#,
            Dimmer,
            Coarse,
        ),
        (
            r#"<Channel Name="Smoke-output"><Group Byte="0">Intensity</Group></Channel>"#,
            Dimmer,
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

#[test]
fn roles_without_a_preset_come_from_the_group() {
    let cases = [
        ("Amount of Fog", "Intensity", other("Intensity")),
        ("Haze", "Intensity", other("Intensity")),
        ("Hazer level", "Intensity", other("Intensity")),
        ("Smoke-output", "Intensity", other("Intensity")),
        ("FAN speed", "Intensity", other("Intensity")),
        ("Aux", "Intensity", other("Intensity")),
        ("Auxiliary", "Intensity", other("Intensity")),
        ("Dimming curve", "Intensity", other("Intensity")),
        ("Foggy dimmer", "Intensity", other("Intensity")),
        ("Dimmer", "Intensity", other("Intensity")),
        ("Focus", "Beam", other("Beam")),
        ("Odd", "Sparkle", other("Sparkle")),
        ("C", "Pan", Pan),
        ("C", "Tilt", Tilt),
        ("C", "Colour", ColorWheel),
        ("C", "Gobo", Gobo),
        ("C", "Shutter", Shutter),
        ("C", "Speed", Speed),
        ("Fog", "Effect", Effect),
        ("C", "Prism", Prism),
        ("C", "Maintenance", Control),
        ("C", "Nothing", NoFunction),
    ];
    let actual = cases.each_ref().map(|(name, group, _)| {
        let channel = format!(r#"<Channel Name="{name}"><Group>{group}</Group></Channel>"#);
        let channel = only_channel(&channel);
        assert_eq!(channel.declared.group.as_deref(), Some(*group));
        assert_eq!(channel.declared.preset_stem(), None);
        channel.role()
    });
    assert_eq!(actual, cases.map(|(_, _, expected)| expected));

    let mut undeclared = only_channel(r#"<Channel Name="Dimmer"/>"#);
    undeclared.function = Dimmer;
    assert_eq!(undeclared.role(), other(""));
}

#[test]
fn roles_with_a_preset_respect_declared_colour_and_fixture_type() {
    let cases = [
        ("Fog", "IntensityRed", "Other", "", Red),
        (
            "Pump",
            "IntensityDimmer",
            "Smoke",
            "",
            other("IntensityDimmer"),
        ),
        (
            "Pump",
            "IntensityMasterDimmerFine",
            "Hazer",
            "",
            other("IntensityMasterDimmerFine"),
        ),
        ("Pump", "IntensityDimmerFine", "Other", "", Dimmer),
        (
            "Auxiliary Light Dimmer",
            "IntensityMasterDimmer",
            "Moving Head",
            "",
            Dimmer,
        ),
        ("Fan", "IntensityBlueFine", "Smoke", "", Blue),
        (
            "C",
            "UnknownFine",
            "Other",
            "<Group>Effect</Group>",
            other("UnknownFine"),
        ),
        (
            "C",
            "IntensityDimmer",
            "Smoke",
            "<Colour>White</Colour>",
            White,
        ),
    ];
    let actual = cases.each_ref().map(|(name, preset, kind, children, _)| {
        let channel = format!(r#"<Channel Name="{name}" Preset="{preset}">{children}</Channel>"#);
        let channel = channel_of_kind(&channel, kind);
        channel.role()
    });
    assert_eq!(actual, cases.map(|(_, _, _, _, expected)| expected));
}

#[test]
fn dimmer_roles_come_from_the_channels_in_each_mode() {
    for (kind, pump, bank) in [
        ("Smoke", other("IntensityDimmer"), other("Intensity")),
        ("Hazer", other("IntensityDimmer"), other("Intensity")),
        ("Dimmer", Dimmer, Dimmer),
        ("Other", Dimmer, other("Intensity")),
    ] {
        let xml = format!(
            r#"<FixtureDefinition><Manufacturer>A</Manufacturer><Model>M</Model><Type>{kind}</Type>
            <Channel Name="Level" Preset="IntensityDimmer"/>
            <Channel Name="Ch 1"><Group>Intensity</Group></Channel>
            <Channel Name="Lamp"><Group>Intensity</Group><Colour>Amber</Colour></Channel>
            <Channel Name="Fine" Preset="IntensityRedFine"/>
            <Channel Name="Aux"><Group>Effect</Group></Channel>
            <Mode Name="Unlit"><Channel Number="0">Level</Channel><Channel Number="1">Ch 1</Channel><Channel Number="2">Aux</Channel></Mode>
            <Mode Name="Lamp"><Channel Number="0">Level</Channel><Channel Number="1">Lamp</Channel></Mode>
            <Mode Name="Fine"><Channel Number="0">Level</Channel><Channel Number="1">Fine</Channel></Mode>
            <Mode Name="Heads">
                <Channel Number="0">Level</Channel><Channel Number="1">Level</Channel>
                <Channel Number="2">Lamp</Channel><Channel Number="3">Level</Channel>
                <Head><Channel>1</Channel><Channel>2</Channel></Head>
                <Head><Channel>0</Channel><Channel>1</Channel></Head>
            </Mode>
            </FixtureDefinition>"#
        );
        let fixture = parse("modes.qxf", xml.as_bytes()).unwrap();
        let roles: Vec<Vec<_>> = fixture
            .modes
            .iter()
            .map(|mode| mode.channels.iter().map(Channel::role).collect())
            .collect();
        assert_eq!(
            roles,
            [
                vec![pump.clone(), bank, Effect],
                vec![Dimmer, Amber],
                vec![Dimmer, Red],
                vec![pump.clone(), Dimmer, Amber, pump]
            ],
            "{kind}"
        );
    }
}

// ActsOn as written, what the mode reads: the channel it names, or why the mode is refused
#[test]
fn an_acts_on_names_a_channel_of_its_mode_or_the_mode_is_refused() {
    let unreadable = |text: &str| {
        Err(Problem::BadNumber {
            what: "ActsOn",
            text: text.into(),
        })
    };
    for (target, expected) in [
        (" 0 ", Ok(Some(0))),
        (
            "65535",
            Err(Problem::ActsOnPastEnd {
                channel: u16::MAX,
                channels: 1,
            }),
        ),
        ("65536", unreadable("65536")),
        ("-1", unreadable("-1")),
        ("Pump", unreadable("Pump")),
        ("", unreadable("")),
    ] {
        let xml = format!(
            r#"<FixtureDefinition><Manufacturer>A</Manufacturer><Model>M</Model><Type>Other</Type>
            <Channel Name="Level" Preset="IntensityDimmer"/>
            <Mode Name="One"><Channel Number="0" ActsOn="{target}">Level</Channel></Mode>
            </FixtureDefinition>"#
        );
        let read = match parse("number.qxf", xml.as_bytes()) {
            Ok(fixture) => Ok(fixture.modes[0].channels[0].declared.acts_on),
            Err(error) => {
                let Problem::NoModeKept(refused) = error.problem else {
                    panic!("{target}: {error}");
                };
                Err(refused[0].problem.clone())
            }
        };
        assert_eq!(read, expected, "{target}");
    }
}

#[test]
fn a_fog_fixture_only_dims_its_emitters_with_a_plain_level() {
    let cases = [
        ("", Dimmer),
        (r#"<Capability Min="3" Max="32">Level</Capability>"#, Dimmer),
        (r#"<Capability Min="0" Max="255" Preset="LampOn"/>"#, Dimmer),
        (
            r#"<Capability Min="1" Max="255" Preset="LampOn"/>"#,
            other("IntensityDimmer"),
        ),
        (
            r#"<Capability Min="0" Max="254" Preset="LampOn"/>"#,
            other("IntensityDimmer"),
        ),
        (
            r#"<Capability Min="0" Max="9">Off</Capability><Capability Min="10" Max="255">Output</Capability>"#,
            other("IntensityDimmer"),
        ),
    ];
    for kind in ["Smoke", "Hazer"] {
        for (ranges, expected) in &cases {
            let xml = format!(
                r#"<FixtureDefinition><Manufacturer>A</Manufacturer><Model>M</Model><Type>{kind}</Type>
                <Channel Name="Level" Preset="IntensityDimmer">{ranges}</Channel>
                <Channel Name="Red" Preset="IntensityRed"/>
                <Mode Name="Shared"><Channel Number="0">Level</Channel><Channel Number="1">Red</Channel>
                    <Head><Channel>0</Channel><Channel>1</Channel></Head></Mode>
                <Mode Name="Headless"><Channel Number="0">Level</Channel><Channel Number="1">Red</Channel></Mode>
                </FixtureDefinition>"#
            );
            for mode in parse("fog.qxf", xml.as_bytes()).unwrap().modes {
                assert_eq!(
                    &mode.channels[0].role(),
                    expected,
                    "{kind} {}, {ranges}",
                    mode.name
                );
            }
        }
    }
}

#[test]
fn a_declared_colour_sets_the_role_and_keeps_the_v11_function() {
    for (name, colour) in [
        ("Red", Some(Red)),
        ("Green", Some(Green)),
        ("Blue", Some(Blue)),
        ("White", Some(White)),
        ("Amber", Some(Amber)),
        ("UV", Some(Uv)),
        ("Cyan", Some(Cyan)),
        ("Magenta", Some(Magenta)),
        ("Yellow", Some(Yellow)),
        ("Lime", Some(Lime)),
        ("Indigo", Some(Indigo)),
        ("red", None),
        ("Unknown", None),
    ] {
        for (group, function, fallback) in [
            ("Intensity", Dimmer, other("Intensity")),
            ("Effect", Effect, Effect),
        ] {
            let channel = only_channel(&format!(
                r#"<Channel Name="Fan"><Group>{group}</Group><Colour>{name}</Colour></Channel>"#
            ));
            let expected_role = colour.clone().unwrap_or(fallback);
            assert_eq!(channel.declared.emitter_colour(), colour);
            assert_eq!(
                (channel.role(), &channel.declared.colour, channel.function),
                (expected_role, &colour, function),
                "{name} in {group}"
            );
        }
    }

    for (tag, expected) in [("", Red), ("<Colour>Blue</Colour>", Blue)] {
        let channel = only_channel(&format!(
            r#"<Channel Name="Lamp" Preset="IntensityRedFine">{tag}</Channel>"#
        ));
        assert_eq!(channel.declared.emitter_colour(), Some(expected));
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

// `ActsOn` belongs to the mode: the same channel acts on another channel in each mode
#[test]
fn acts_on_is_read_per_mode() {
    let xml = r#"<?xml version="1.0"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Acme</Manufacturer><Model>Test</Model><Type>Moving Head</Type>
 <Channel Name="Focus" Preset="BeamFocusNearFar"/>
 <Channel Name="Auto Focus"><Group Byte="0">Beam</Group></Channel>
 <Channel Name="Auto Focus fine" Preset="BeamFocusFine"/>
 <Mode Name="Named">
  <Channel Number="0">Focus</Channel><Channel Number="1">Auto Focus</Channel>
  <Channel Number="2" ActsOn="1">Auto Focus fine</Channel>
 </Mode>
 <Mode Name="Moved">
  <Channel Number="0">Auto Focus</Channel><Channel Number="1" ActsOn="0">Auto Focus fine</Channel>
  <Channel Number="2">Focus</Channel>
 </Mode>
 <Mode Name="None">
  <Channel Number="0">Focus</Channel><Channel Number="1">Auto Focus fine</Channel>
 </Mode>
</FixtureDefinition>"#;
    let fixture = parse("acts-on.qxf", xml.as_bytes()).unwrap();
    let acts_on: Vec<Vec<Option<u16>>> = fixture
        .modes
        .iter()
        .map(|mode| mode.channels.iter().map(|c| c.declared.acts_on).collect())
        .collect();
    assert_eq!(
        acts_on,
        [
            vec![None, None, Some(1)],
            vec![None, Some(0), None],
            vec![None, None]
        ]
    );
}
