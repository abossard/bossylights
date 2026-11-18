//! What a range does, when the definition says so by preset, and the value of a shutter
//! or strobe channel that lets light out.

use Direction::{Down, Up};
use Pace::{Goes, Random, Unsaid};
use fixture_library::{Channel, Direction, Does, Hertz, Pace, Rgb, Turn, parse};

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

fn strobe(pace: Pace) -> Option<Does> {
    Some(Does::Strobe(pace))
}

/// From and to, in millihertz.
const fn hz(from: u32, to: u32) -> Pace {
    Pace::Hertz(Hertz { from, to })
}

fn gobo(picture: Option<&str>, shakes: bool) -> Option<Does> {
    Some(Does::Gobo {
        picture: picture.map(str::to_string),
        shakes,
    })
}

// the capability element's attributes, what the range does
#[test]
fn a_range_does_what_its_preset_says() {
    let cases: Vec<(&str, Option<Does>)> = vec![
        (r#"Preset="ShutterOpen""#, Some(Does::Open)),
        (r#"Preset="ShutterClose""#, Some(Does::Closed)),
        (r#"Preset="StrobeSlowToFast""#, strobe(Goes(Up))),
        (r#"Preset="StrobeFastToSlow""#, strobe(Goes(Down))),
        (r#"Preset="StrobeRandom""#, strobe(Random)),
        (r#"Preset="StrobeRandomSlowToFast""#, strobe(Random)),
        (r#"Preset="StrobeRandomFastToSlow""#, strobe(Random)),
        (
            r#"Preset="StrobeFrequency" Res1="10""#,
            strobe(hz(10_000, 10_000)),
        ),
        (
            r#"Preset="StrobeFrequency" Res1="0.5""#,
            strobe(hz(500, 500)),
        ),
        (
            r#"Preset="StrobeFreqRange" Res1="1" Res2="20""#,
            strobe(hz(1_000, 20_000)),
        ),
        (
            r#"Preset="StrobeFreqRange" Res1="0" Res2="12.25""#,
            strobe(hz(0, 12_250)),
        ),
        // a frequency that cannot be read is not said
        (r#"Preset="StrobeFrequency""#, strobe(Unsaid)),
        (r#"Preset="StrobeFrequency" Res1="fast""#, strobe(Unsaid)),
        (r#"Preset="StrobeFreqRange" Res1="1""#, strobe(Unsaid)),
        (r#"Preset="StrobeFrequency" Res1="-3""#, strobe(Unsaid)),
        (r#"Preset="PulseSlowToFast""#, strobe(Goes(Up))),
        (r#"Preset="PulseFastToSlow""#, strobe(Goes(Down))),
        (
            r#"Preset="PulseFrequency" Res1="2""#,
            strobe(hz(2_000, 2_000)),
        ),
        (r#"Preset="PulseFreqRange""#, strobe(Unsaid)),
        (r#"Preset="RampUpSlowToFast""#, strobe(Goes(Up))),
        (r#"Preset="RampDownFastToSlow""#, strobe(Goes(Down))),
        (
            r#"Preset="RampUpFreqRange" Res1="1" Res2="4""#,
            strobe(hz(1_000, 4_000)),
        ),
        (r#"Preset="RampDownFrequency""#, strobe(Unsaid)),
        (r#"Preset="RotationStop""#, Some(Does::Rotation(Turn::Stop))),
        (
            r#"Preset="RotationIndexed""#,
            Some(Does::Rotation(Turn::Indexed)),
        ),
        (
            r#"Preset="RotationClockwise""#,
            Some(Does::Rotation(Turn::Clockwise(Unsaid))),
        ),
        (
            r#"Preset="RotationClockwiseSlowToFast""#,
            Some(Does::Rotation(Turn::Clockwise(Goes(Up)))),
        ),
        (
            r#"Preset="RotationClockwiseFastToSlow""#,
            Some(Does::Rotation(Turn::Clockwise(Goes(Down)))),
        ),
        (
            r#"Preset="RotationCounterClockwise""#,
            Some(Does::Rotation(Turn::CounterClockwise(Unsaid))),
        ),
        (
            r#"Preset="RotationCounterClockwiseSlowToFast""#,
            Some(Does::Rotation(Turn::CounterClockwise(Goes(Up)))),
        ),
        (
            r#"Preset="RotationCounterClockwiseFastToSlow""#,
            Some(Does::Rotation(Turn::CounterClockwise(Goes(Down)))),
        ),
        (
            r#"Preset="PrismEffectOn" Res1="6""#,
            Some(Does::Prism { faces: Some(6) }),
        ),
        (
            r#"Preset="PrismEffectOn""#,
            Some(Does::Prism { faces: None }),
        ),
        (
            r#"Preset="PrismEffectOn" Res1="many""#,
            Some(Does::Prism { faces: None }),
        ),
        (r#"Preset="PrismEffectOff""#, Some(Does::PrismOff)),
        (
            r#"Preset="GoboMacro" Res1="Others/rainbow.png""#,
            gobo(Some("Others/rainbow.png"), false),
        ),
        (r#"Preset="GoboMacro" Res1="""#, gobo(None, false)),
        (
            r#"Preset="GoboShakeMacro" Res1="SGM/gobo00145.svg""#,
            gobo(Some("SGM/gobo00145.svg"), true),
        ),
        (r#"Preset="GoboShakeMacro""#, gobo(None, true)),
        // the older way to name a gobo picture, without a preset
        (
            r#"Res="Others/gobo00016.svg""#,
            gobo(Some("Others/gobo00016.svg"), false),
        ),
        (r#"Preset="SlowToFast""#, Some(Does::Runs(Up))),
        (r#"Preset="NearToFar""#, Some(Does::Runs(Up))),
        (r#"Preset="SmallToBig""#, Some(Does::Runs(Up))),
        (r#"Preset="FastToSlow""#, Some(Does::Runs(Down))),
        (r#"Preset="FarToNear""#, Some(Does::Runs(Down))),
        (r#"Preset="BigToSmall""#, Some(Does::Runs(Down))),
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
        (r#"Preset="LampOff""#, Some(Does::LampOff)),
        (r#"Preset="ResetAll""#, Some(Does::Reset)),
        (r#"Preset="ResetPanTilt""#, Some(Does::Reset)),
        (r#"Preset="ResetIris""#, Some(Does::Reset)),
        // left as the label only, on purpose
        (r#"Preset="ColorWheelIndex""#, None),
        (r#"Preset="GenericPicture" Res1="Others/fx.png""#, None),
        (r#"Preset="SilentModeOn""#, None),
        (r#"Preset="Alias""#, None),
        (r#"Preset="SomethingNew""#, None),
        ("", None),
    ];
    for (attributes, does) in cases {
        let channel = format!(
            r#"<Channel Name="C"><Group Byte="0">Effect</Group><Capability Min="0" Max="9" {attributes}>x</Capability></Channel>"#
        );
        let found = only_channel(&channel);
        assert_eq!(
            found.ranges.first().map(|r| r.does.clone()),
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
