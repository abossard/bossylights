//! A falling dimmer (`BigToSmall`, dark at 255) written from a light's intensity (#355,
//! finding RIG-01 of #349): full intensity is 0, none is 255.

mod common;

use common::{at, light, lights};
use engine_core::{ADDRESSES, Field, Light, Universes};
use fixture_library::{Mode, parse};
use rig::compile;

fn first_mode(name: &str, qxf: &[u8]) -> Mode {
    parse(name, qxf).unwrap().modes.into_iter().next().unwrap()
}

fn byte_written(mode: &Mode, light_value: &Light, channel_index: u32) -> u8 {
    let rigged = compile(mode, at(1), light(0), Universes::ONE, lights()).unwrap();
    let offered = rigged.fields[0]
        .offers(light_value)
        .find_map(|(field, word)| (field == Field::Byte(channel_index)).then_some(word))
        .expect("writer offered on channel");
    (offered >> 8) as u8
}

#[test]
fn a_falling_dimmer_is_bright_at_0_and_dark_at_255() {
    let inverted_dimmer_qxf = br#"<?xml version="1.0" encoding="UTF-8"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Test</Manufacturer>
 <Model>Inverted Dimmer</Model>
 <Type>Dimmer</Type>
 <Channel Name="Dimmer" Default="255">
  <Group Byte="0">Intensity</Group>
  <Capability Min="0" Max="255" Preset="BigToSmall">Dimmer 100% to 0%</Capability>
 </Channel>
 <Mode Name="1ch">
  <Channel Number="0">Dimmer</Channel>
 </Mode>
</FixtureDefinition>"#;

    let mode = first_mode("inverted-dimmer.qxf", inverted_dimmer_qxf);
    let space = u32::try_from(ADDRESSES).unwrap();
    let lowered = rig::channels::lower(&mode, 0, space).unwrap();
    assert_eq!(
        lowered.channels[0].dark, 255,
        "dark of an inverted dimmer is 255"
    );

    let full = Light {
        intensity: Some(u16::MAX),
        ..Light::DARK
    };
    let zero = Light {
        intensity: Some(0),
        ..Light::DARK
    };

    assert_eq!(
        byte_written(&mode, &full, 0),
        0,
        "full intensity (u16::MAX) on a falling dimmer (dark = 255) must write DMX 0, not dark 255"
    );
    assert_eq!(
        byte_written(&mode, &zero, 0),
        255,
        "zero intensity (0) on a falling dimmer (dark = 255) must write dark DMX 255, not full-bright 0"
    );
}

/// A range of a channel: min, max, preset, label.
type Range = (u8, u8, &'static str, &'static str);

/// A rising dimmer with these ranges.
fn rising_dimmer(ranges: &[Range]) -> Mode {
    let capabilities: String = ranges
        .iter()
        .map(|(min, max, preset, label)| {
            format!(
                r#"  <Capability Min="{min}" Max="{max}" Preset="{preset}">{label}</Capability>
"#
            )
        })
        .collect();
    let qxf = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Test</Manufacturer>
 <Model>Dimmer With Ranges</Model>
 <Type>Dimmer</Type>
 <Channel Name="Dimmer">
  <Group Byte="0">Intensity</Group>
{capabilities} </Channel>
 <Mode Name="1ch">
  <Channel Number="0">Dimmer</Channel>
 </Mode>
</FixtureDefinition>"#
    );
    first_mode("dimmer-with-ranges.qxf", qxf.as_bytes())
}

/// A dimmer runs over its first running range only: full brightness is the top of the
/// run, never the strobe above it, and none is the run's 0% (#355, planner and checker).
/// The bytes are the files' labels.
#[test]
fn a_dimmer_runs_over_its_first_running_range() {
    let run = (0, 200, "SmallToBig", "Dimmer 0% to 100%");
    let strobe = (201, 255, "StrobeSlowToFast", "Strobe slow to fast");
    let closed = (0, 7, "ShutterClose", "Closed");
    let middle = (8, 199, "SmallToBig", "Dimmer 0% to 100%");
    let strobe_above_middle = (200, 255, "StrobeSlowToFast", "Strobe slow to fast");
    let first_run = (0, 127, "SmallToBig", "Dimmer 0% to 100%");
    let second_run = (128, 255, "SlowToFast", "Fade slow to fast");
    let cases: [(&str, &[Range], u16, u8); 4] = [
        (
            "full, strobe above: the top of the run",
            &[run, strobe],
            u16::MAX,
            200,
        ),
        (
            "full, closed below and strobe above: the top of the run",
            &[closed, middle, strobe_above_middle],
            u16::MAX,
            199,
        ),
        (
            "none, closed below and strobe above: the run's 0%",
            &[closed, middle, strobe_above_middle],
            0,
            8,
        ),
        (
            "full, two runs: the first",
            &[first_run, second_run],
            u16::MAX,
            127,
        ),
    ];
    for (name, ranges, intensity, expected) in cases {
        let value = Light {
            intensity: Some(intensity),
            ..Light::DARK
        };
        assert_eq!(
            byte_written(&rising_dimmer(ranges), &value, 0),
            expected,
            "{name}"
        );
    }
}
