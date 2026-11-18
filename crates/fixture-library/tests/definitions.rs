//! Real QLC+ definitions, read through the public door: bytes in, model out.
//! The files are in `tests/definitions/`, with their licence beside them.

use Byte::{Coarse, Fine};
use Function::{Blue, Dimmer, Effect, Focus, Green, Pan, Red, Shutter, Tilt, White, Zoom};
use fixture_library::{Byte, Function, Kind, Layout, parse, search};

fn read(file: &str) -> fixture_library::Fixture {
    let path = format!("{}/tests/definitions/{file}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).expect("the test file is in the repository");
    parse(file, &bytes).expect("a real definition reads")
}

type Plan = Vec<(Function, Byte)>;
type Ranges = Vec<(u8, u8, &'static str)>;

fn plan(mode: &fixture_library::Mode) -> Plan {
    mode.channels
        .iter()
        .map(|c| (c.function.clone(), c.byte))
        .collect()
}

// file, maker, model, kind, mode names
#[test]
fn a_definition_says_who_made_what_in_which_modes() {
    let cases: [(&str, &str, &str, Kind, &[&str]); 6] = [
        (
            "Showtec-Par-64-100W-COB-UV.qxf",
            "Showtec",
            "Par 64 100W COB UV",
            Kind::Dimmer,
            &["8 Bit"],
        ),
        (
            "Generic-Generic-RGB.qxf",
            "Generic",
            "Generic RGB",
            Kind::ColorChanger,
            &["RGB", "GRB", "BGR", "RGB Dimmer", "Dimmer RGB"],
        ),
        (
            "Generic-Generic-RGBW.qxf",
            "Generic",
            "Generic RGBW",
            Kind::ColorChanger,
            &[
                "RGBW",
                "WRGB",
                "RGBW Dimmer",
                "WRGB Dimmer",
                "Dimmer RGBW",
                "Dimmer WRGB",
            ],
        ),
        (
            "Elation-EVC-MH.qxf",
            "Elation",
            "EVC MH",
            Kind::MovingHead,
            &["12-CH"],
        ),
        (
            "XStatic-X-240Bar-RGB.qxf",
            "XStatic",
            "X-240Bar RGB",
            Kind::LedBarPixels,
            &[
                "2 Channel",
                "3 Channel",
                "4 Channel",
                "7 Channel",
                "14 Channel",
                "26 Channel",
            ],
        ),
        (
            "JB-Systems-Lounge-Laser-DMX.qxf",
            "JB Systems",
            "Lounge Laser DMX",
            Kind::Laser,
            &["1-Channel", "7-Channel"],
        ),
    ];
    for (file, maker, model, kind, modes) in cases {
        let fixture = read(file);
        assert_eq!(fixture.maker, maker, "{file}");
        assert_eq!(fixture.model, model, "{file}");
        assert_eq!(fixture.kind, kind, "{file}");
        let names: Vec<&str> = fixture.modes.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, modes, "{file}");
    }
}

// file, mode, the channels in order
#[test]
fn a_mode_lists_its_channels_in_order_with_what_each_does() {
    let cases: [(&str, &str, Plan); 8] = [
        (
            "Showtec-Par-64-100W-COB-UV.qxf",
            "8 Bit",
            vec![(Dimmer, Coarse)],
        ),
        (
            "Generic-Generic-RGB.qxf",
            "BGR",
            vec![(Blue, Coarse), (Green, Coarse), (Red, Coarse)],
        ),
        (
            "Generic-Generic-RGB.qxf",
            "Dimmer RGB",
            vec![
                (Dimmer, Coarse),
                (Red, Coarse),
                (Green, Coarse),
                (Blue, Coarse),
            ],
        ),
        (
            "Generic-Generic-RGBW.qxf",
            "WRGB Dimmer",
            vec![
                (White, Coarse),
                (Red, Coarse),
                (Green, Coarse),
                (Blue, Coarse),
                (Dimmer, Coarse),
            ],
        ),
        (
            "Elation-EVC-MH.qxf",
            "12-CH",
            vec![
                (Pan, Coarse),
                (Pan, Fine),
                (Tilt, Coarse),
                (Tilt, Fine),
                (Function::Speed, Coarse),
                (Function::ColorWheel, Coarse),
                (White, Coarse),
                (White, Coarse),
                (Zoom, Coarse),
                (Focus, Coarse),
                (Focus, Fine),
                (Function::Control, Coarse),
            ],
        ),
        (
            "JB-Systems-Lounge-Laser-DMX.qxf",
            "7-Channel",
            vec![
                (Effect, Coarse),
                (Shutter, Coarse),
                (Shutter, Coarse),
                (Effect, Coarse),
                (Effect, Coarse),
                (Dimmer, Coarse),
                (Shutter, Coarse),
            ],
        ),
        (
            "JB-Systems-Lounge-Laser-DMX.qxf",
            "1-Channel",
            vec![(Effect, Coarse)],
        ),
        (
            "XStatic-X-240Bar-RGB.qxf",
            "4 Channel",
            vec![
                (Dimmer, Coarse),
                (Red, Coarse),
                (Green, Coarse),
                (Blue, Coarse),
            ],
        ),
    ];
    for (file, mode, expected) in cases {
        let fixture = read(file);
        let found = fixture
            .modes
            .iter()
            .find(|m| m.name == mode)
            .expect("the mode is there");
        assert_eq!(plan(found), expected, "{file} {mode}");
    }
}

#[test]
fn a_pixel_bar_has_its_pixels_and_layout() {
    let fixture = read("XStatic-X-240Bar-RGB.qxf");
    let mode = fixture
        .modes
        .iter()
        .find(|m| m.name == "26 Channel")
        .expect("the mode is there");
    assert_eq!(mode.pixels.len(), 8);
    assert_eq!(mode.pixels.first(), Some(&vec![1, 2, 3]));
    assert_eq!(mode.pixels.get(2), Some(&vec![8, 9, 7]));
    assert_eq!(mode.pixels.get(7), Some(&vec![23, 24, 22]));
    let four = fixture
        .modes
        .iter()
        .find(|m| m.name == "14 Channel")
        .expect("the mode is there");
    assert_eq!(four.pixels.len(), 4);
    assert_eq!(
        mode.layout,
        Some(Layout {
            width: 8,
            height: 1
        })
    );
    let plain = read("Generic-Generic-RGB.qxf");
    assert!(
        plain
            .modes
            .iter()
            .all(|m| m.pixels.is_empty() && m.layout.is_none())
    );
}

// file, channel name, its ranges as (min, max, label)
#[test]
fn a_channel_keeps_its_ranges() {
    let cases: [(&str, &str, &str, Ranges); 3] = [
        (
            "Elation-EVC-MH.qxf",
            "12-CH",
            "Function",
            vec![
                (0, 69, "No function"),
                (70, 79, "Enable Blackout while moving"),
                (80, 89, "Disable Blackout while moving"),
                (90, 199, "No function"),
                (200, 209, "Reset All"),
                (210, 255, "No function"),
            ],
        ),
        (
            "Elation-EVC-MH.qxf",
            "12-CH",
            "Focus Coarse",
            vec![(0, 0, "Auto Focus"), (1, 255, "Near - Far")],
        ),
        (
            "JB-Systems-Lounge-Laser-DMX.qxf",
            "7-Channel",
            "LED Strobe",
            vec![
                (0, 4, "Blackout"),
                (5, 127, "LED on"),
                (128, 254, "Frequency from fast to slow"),
                (255, 255, "Sound controlled"),
            ],
        ),
    ];
    for (file, mode, channel, expected) in cases {
        let fixture = read(file);
        let found = fixture
            .modes
            .iter()
            .find(|m| m.name == mode)
            .expect("the mode is there");
        let channel = found
            .channels
            .iter()
            .find(|c| c.name == channel)
            .expect("the channel is there");
        let ranges: Vec<(u8, u8, &str)> = channel
            .ranges
            .iter()
            .map(|r| (r.min, r.max, r.label.as_str()))
            .collect();
        assert_eq!(ranges, expected, "{file} {}", channel.name);
    }
}

#[test]
fn a_colour_channel_without_ranges_has_none() {
    let fixture = read("Generic-Generic-RGB.qxf");
    assert!(
        fixture
            .modes
            .iter()
            .flat_map(|m| &m.channels)
            .all(|c| c.ranges.is_empty())
    );
}

// query, the "maker model" of what is found, best first
#[test]
fn search_finds_by_maker_and_model_ignoring_case_and_small_typos() {
    let library: Vec<_> = [
        "Showtec-Par-64-100W-COB-UV.qxf",
        "Generic-Generic-RGB.qxf",
        "Generic-Generic-RGBW.qxf",
        "Elation-EVC-MH.qxf",
        "XStatic-X-240Bar-RGB.qxf",
        "JB-Systems-Lounge-Laser-DMX.qxf",
    ]
    .map(read)
    .into();
    let cases: [(&str, &[&str]); 11] = [
        ("elation", &["Elation EVC MH"]),
        ("ELATION evc", &["Elation EVC MH"]),
        ("evc mh", &["Elation EVC MH"]),
        ("elatoin", &["Elation EVC MH"]),
        ("laser", &["JB Systems Lounge Laser DMX"]),
        ("lounge lazer", &["JB Systems Lounge Laser DMX"]),
        (
            "generic rgb",
            &["Generic Generic RGB", "Generic Generic RGBW"],
        ),
        (
            "generic rgbw",
            &["Generic Generic RGBW", "Generic Generic RGB"],
        ),
        ("x-240", &["XStatic X-240Bar RGB"]),
        ("zzzz", &[]),
        ("", &[]),
    ];
    for (query, expected) in cases {
        let found: Vec<String> = search(&library, query)
            .iter()
            .map(|f| format!("{} {}", f.maker, f.model))
            .collect();
        assert_eq!(found, expected, "{query:?}");
    }
}
