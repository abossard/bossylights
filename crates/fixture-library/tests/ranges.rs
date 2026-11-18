//! What a range does, when the definition says so by preset, and the value of a shutter
//! or strobe channel that lets light out.

use fixture_library::{Channel, Does, Rgb, parse};

fn definition(channel: &str) -> Vec<u8> {
    format!(
        r#"<?xml version="1.0"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Acme</Manufacturer><Model>Test</Model><Type>Other</Type>
 {channel}
 <Mode Name="M"><Channel Number="0">C</Channel></Mode>
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

const fn rgb(red: u8, green: u8, blue: u8) -> Rgb {
    Rgb { red, green, blue }
}

// the capability element's attributes, what the range does
#[test]
fn a_range_does_what_its_preset_says() {
    let cases: Vec<(&str, Option<Does>)> = vec![
        (r#"Preset="ShutterOpen""#, Some(Does::Open)),
        (r#"Preset="ShutterClose""#, Some(Does::Closed)),
        (r#"Preset="StrobeSlowToFast""#, Some(Does::Strobe)),
        (r#"Preset="StrobeFastToSlow""#, Some(Does::Strobe)),
        (r#"Preset="StrobeRandom""#, Some(Does::Strobe)),
        (r#"Preset="StrobeRandomSlowToFast""#, Some(Does::Strobe)),
        (r#"Preset="StrobeRandomFastToSlow""#, Some(Does::Strobe)),
        (r#"Preset="StrobeFrequency" Res1="10""#, Some(Does::Strobe)),
        (
            r#"Preset="StrobeFreqRange" Res1="1" Res2="20""#,
            Some(Does::Strobe),
        ),
        (r#"Preset="PulseSlowToFast""#, Some(Does::Strobe)),
        (r#"Preset="PulseFreqRange""#, Some(Does::Strobe)),
        (r#"Preset="RampUpSlowToFast""#, Some(Does::Strobe)),
        (r#"Preset="RampDownFrequency""#, Some(Does::Strobe)),
        (
            r##"Preset="ColorMacro" Res1="#ff8000""##,
            Some(Does::Color(rgb(255, 128, 0))),
        ),
        (
            r##"Preset="ColorMacro" Res1="#00FFaa""##,
            Some(Does::Color(rgb(0, 255, 170))),
        ),
        (
            r##"Preset="ColorDoubleMacro" Res1="#ff0000" Res2="#00ff00""##,
            Some(Does::Colors(rgb(255, 0, 0), rgb(0, 255, 0))),
        ),
        // a colour preset without a colour, or with one that is not #rrggbb: nothing said
        (r#"Preset="ColorMacro""#, None),
        (r#"Preset="ColorMacro" Res1="red""#, None),
        (r##"Preset="ColorDoubleMacro" Res1="#ff0000""##, None),
        (r#"Preset="LampOn""#, Some(Does::LampOn)),
        (r#"Preset="ResetAll""#, Some(Does::Reset)),
        (r#"Preset="ResetPanTilt""#, Some(Does::Reset)),
        (r#"Preset="ResetIris""#, Some(Does::Reset)),
        // what the Rig and the colour wheel slice do not use
        (r#"Preset="LampOff""#, None),
        (r#"Preset="GoboMacro" Res1="Others/rainbow.png""#, None),
        (r#"Preset="RotationStop""#, None),
        (r#"Preset="SomethingNew""#, None),
        ("", None),
    ];
    for (attributes, does) in cases {
        let channel = format!(
            r#"<Channel Name="C"><Group Byte="0">Effect</Group><Capability Min="0" Max="9" {attributes}>x</Capability></Channel>"#
        );
        let found = only_channel(&channel);
        assert_eq!(
            found.ranges.first().map(|r| r.does),
            Some(does),
            "{attributes}"
        );
    }
}

// a shutter or strobe channel, the value that lets light out
#[test]
fn a_shutter_or_strobe_channel_has_a_value_that_lets_light_out() {
    let shutter = |ranges: &str| {
        format!(r#"<Channel Name="C"><Group Byte="0">Shutter</Group>{ranges}</Channel>"#)
    };
    let range = |min: u8, max: u8, preset: &str, label: &str| {
        format!(r#"<Capability Min="{min}" Max="{max}" {preset}>{label}</Capability>"#)
    };
    let cases: Vec<(&str, String, Option<u8>)> = vec![
        // 1. a range says open
        (
            "marked open, after closed",
            shutter(
                &[
                    range(0, 4, r#"Preset="ShutterClose""#, "Blackout"),
                    range(5, 127, r#"Preset="ShutterOpen""#, "On"),
                ]
                .concat(),
            ),
            Some(5),
        ),
        (
            "marked open wins over an earlier free range",
            shutter(
                &[
                    range(0, 9, "", "No function"),
                    range(200, 255, r#"Preset="ShutterOpen""#, "Open"),
                ]
                .concat(),
            ),
            Some(200),
        ),
        // 2. the first value that is not closed, a strobe or labelled dark
        ("no ranges", shutter(""), Some(0)),
        (
            "no function, then strobe",
            shutter(
                &[
                    range(0, 7, "", "No function"),
                    range(8, 255, r#"Preset="StrobeSlowToFast""#, "Strobe"),
                ]
                .concat(),
            ),
            Some(0),
        ),
        (
            "LED off, then LED on",
            shutter(&[range(0, 31, "", "LED Off"), range(32, 63, "", "LED On")].concat()),
            Some(32),
        ),
        (
            "blackout by label, then a gap",
            shutter(
                &[
                    range(0, 6, "", "Blackout"),
                    range(20, 255, r#"Preset="StrobeSlowToFast""#, "Strobe"),
                ]
                .concat(),
            ),
            Some(7),
        ),
        (
            "closed, pulse, then plain",
            shutter(
                &[
                    range(0, 9, r#"Preset="ShutterClose""#, "Closed"),
                    range(10, 99, r#"Preset="PulseSlowToFast""#, "Pulse"),
                    range(100, 255, "", "Strobe off"),
                ]
                .concat(),
            ),
            Some(100),
        ),
        (
            "a label that says open and close",
            shutter(&range(0, 255, "", "Open to Close")),
            Some(0),
        ),
        (
            "off means no strobe",
            shutter(
                &[
                    range(0, 8, "", "Off"),
                    range(9, 255, r#"Preset="StrobeSlowToFast""#, "Strobe"),
                ]
                .concat(),
            ),
            Some(0),
        ),
        // 3. nothing lets light out steadily: the default, else 0
        (
            "closed, then strobe up to 255",
            shutter(
                &[
                    range(0, 15, r#"Preset="ShutterClose""#, "Black Out"),
                    range(16, 255, r#"Preset="StrobeSlowToFast""#, "Strobe"),
                ]
                .concat(),
            ),
            Some(0),
        ),
        (
            "the same with a default",
            format!(
                r#"<Channel Name="C" Default="16" Preset="ShutterStrobeSlowFast">{}{}</Channel>"#,
                range(0, 15, r#"Preset="ShutterClose""#, "Black Out"),
                range(16, 255, r#"Preset="StrobeSlowToFast""#, "Strobe")
            ),
            Some(16),
        ),
        // a strobe channel by preset is the same
        (
            "strobe preset, no ranges",
            r#"<Channel Name="C" Preset="ShutterStrobeSlowFast"/>"#.to_string(),
            Some(0),
        ),
        // not a shutter or strobe channel: no such value
        (
            "a dimmer",
            r#"<Channel Name="C" Preset="IntensityDimmer"/>"#.to_string(),
            None,
        ),
        (
            "an iris",
            r#"<Channel Name="C" Preset="ShutterIrisMinToMax"/>"#.to_string(),
            None,
        ),
    ];
    for (name, channel, open) in cases {
        assert_eq!(only_channel(&channel).open(), open, "{name}");
    }
}
