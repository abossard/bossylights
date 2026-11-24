//! What a channel declares travels with it through serde (a show file keeps it), and a
//! channel that declares nothing leaves the key out.

use fixture_library::{Channel, parse};
use serde_json::{Value, json};

/// The first channel of a one-channel definition of type `kind`.
fn channel(channel: &str, kind: &str) -> Channel {
    let definition = format!(
        r#"<?xml version="1.0"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Acme</Manufacturer><Model>Test</Model><Type>{kind}</Type>
 {channel}
 <Mode Name="One"><Channel Number="0">C</Channel></Mode>
</FixtureDefinition>"#
    );
    let fixture = parse("test.qxf", definition.as_bytes()).unwrap();
    fixture.modes[0].channels[0].clone()
}

#[test]
fn a_channel_keeps_what_it_declares_through_serde() {
    // (case, channel, fixture type, its "declared" in JSON, absent when it declares nothing)
    let cases = [
        (
            "a preset",
            r#"<Channel Name="C" Preset="IntensityRed"/>"#,
            "Color Changer",
            json!({"preset": "IntensityRed"}),
        ),
        (
            "a group and a colour",
            r#"<Channel Name="C"><Group Byte="0">Intensity</Group><Colour>Red</Colour></Channel>"#,
            "Color Changer",
            json!({"group": "Intensity", "colour": "Red"}),
        ),
        (
            "an Intensity group on a dimmer pack: it dims light",
            r#"<Channel Name="C"><Group Byte="0">Intensity</Group></Channel>"#,
            "Dimmer",
            json!({"group": "Intensity", "dimming": "DimmerPack"}),
        ),
        (
            "nothing declared",
            r#"<Channel Name="C"/>"#,
            "Color Changer",
            Value::Null,
        ),
    ];
    for (case, xml, kind, declared) in cases {
        let channel = channel(xml, kind);
        let written = serde_json::to_value(&channel).unwrap();
        assert_eq!(
            written.get("declared").cloned().unwrap_or(Value::Null),
            declared,
            "{case}"
        );
        let read: Channel = serde_json::from_value(written).unwrap();
        assert_eq!(read, channel, "{case}");
    }
}
