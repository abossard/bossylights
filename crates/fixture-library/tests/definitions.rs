//! Real QLC+ definitions, read through the public door: bytes in, model out.
//! The files are in `tests/definitions/`, with their licence beside them.

use Byte::{Coarse, Fine};
use Function::{Blue, Dimmer, Effect, Focus, Green, Pan, Red, Shutter, Tilt, White, Zoom};
use fixture_library::{Byte, Does, Function, Kind, Layout, Rgb, parse, search};

fn read(file: &str) -> fixture_library::Fixture {
    let path = format!("{}/tests/definitions/{file}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).expect("the test file is in the repository");
    parse(file, &bytes).expect("a real definition reads")
}

type Plan = Vec<(Function, Byte)>;
type Ranges = Vec<(u8, u8, &'static str)>;
/// A channel's name and the value that lets light out.
type OpenAt = Vec<(&'static str, u8)>;

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

fn channel(file: &str, mode: &str, name: &str) -> fixture_library::Channel {
    read(file)
        .modes
        .into_iter()
        .find(|m| m.name == mode)
        .and_then(|m| m.channels.into_iter().find(|c| c.name == name))
        .expect("the channel is there")
}

// file, mode, channel, what its first ranges do
#[test]
fn a_range_says_what_it_does_when_its_preset_says() {
    let red = Rgb {
        red: 255,
        green: 0,
        blue: 0,
    };
    let yellow = Rgb {
        red: 255,
        green: 255,
        blue: 0,
    };
    let green = Rgb {
        red: 0,
        green: 255,
        blue: 0,
    };
    let cases: [(&str, &str, &str, Vec<Option<Does>>); 4] = [
        (
            "JB-Systems-Lounge-Laser-DMX.qxf",
            "7-Channel",
            "LED Strobe",
            vec![
                Some(Does::Closed),
                Some(Does::Open),
                Some(Does::Strobe),
                None,
            ],
        ),
        (
            "XStatic-X-240Bar-RGB.qxf",
            "2 Channel",
            "Function",
            vec![None, Some(Does::Color(red)), Some(Does::Color(yellow))],
        ),
        (
            "XStatic-X-240Bar-RGB.qxf",
            "7 Channel",
            "Function",
            vec![None, Some(Does::Color(red))],
        ),
        (
            "Elation-EVC-MH.qxf",
            "12-CH",
            "Function",
            vec![None, None, None, None, Some(Does::Reset), None],
        ),
    ];
    for (file, mode, name, expected) in cases {
        let found = channel(file, mode, name);
        let does: Vec<Option<Does>> = found
            .ranges
            .iter()
            .take(expected.len())
            .map(|r| r.does)
            .collect();
        assert_eq!(does, expected, "{file} {mode} {name}");
    }
    let split = channel("XStatic-X-240Bar-RGB.qxf", "7 Channel", "Function");
    assert_eq!(
        split.ranges.iter().find(|r| r.min == 216).map(|r| r.does),
        Some(Some(Does::Colors(red, green)))
    );
}

// file, every mode's shutter and strobe channels with the value that lets light out
#[test]
fn a_mode_says_which_value_lets_its_light_out() {
    let cases: [(&str, Vec<(&str, OpenAt)>); 6] = [
        ("Showtec-Par-64-100W-COB-UV.qxf", vec![("8 Bit", vec![])]),
        ("Generic-Generic-RGB.qxf", vec![("Dimmer RGB", vec![])]),
        ("Generic-Generic-RGBW.qxf", vec![("RGBW Dimmer", vec![])]),
        ("Elation-EVC-MH.qxf", vec![("12-CH", vec![])]),
        (
            "XStatic-X-240Bar-RGB.qxf",
            vec![
                ("2 Channel", vec![]),
                ("7 Channel", vec![("Flash", 0)]),
                ("26 Channel", vec![("Flash", 0)]),
            ],
        ),
        (
            "JB-Systems-Lounge-Laser-DMX.qxf",
            vec![
                ("1-Channel", vec![]),
                (
                    "7-Channel",
                    vec![
                        ("Red Laser Strobe", 5),
                        ("Green Laser Strobe", 5),
                        ("LED Strobe", 5),
                    ],
                ),
            ],
        ),
    ];
    for (file, modes) in cases {
        let fixture = read(file);
        for (mode, expected) in modes {
            let found = fixture
                .modes
                .iter()
                .find(|m| m.name == mode)
                .expect("the mode is there");
            let open: Vec<(&str, u8)> = found
                .channels
                .iter()
                .filter_map(|c| c.open().map(|v| (c.name.as_str(), v)))
                .collect();
            assert_eq!(open, expected, "{file} {mode}");
        }
        // none of the six gives a default
        assert!(
            fixture
                .modes
                .iter()
                .flat_map(|m| &m.channels)
                .all(|c| c.default.is_none()),
            "{file}"
        );
    }
}

// heads in the mode, the layout under the mode or the fixture, the layout kept
#[test]
fn a_layout_is_kept_only_when_it_has_a_cell_for_every_head() {
    let layout =
        |w: u16, h: u16| format!(r#"<Physical><Layout Width="{w}" Height="{h}"/></Physical>"#);
    let cases: Vec<(usize, String, String, Option<Layout>)> = vec![
        (
            2,
            layout(2, 1),
            String::new(),
            Some(Layout {
                width: 2,
                height: 1,
            }),
        ),
        (
            2,
            String::new(),
            layout(1, 2),
            Some(Layout {
                width: 1,
                height: 2,
            }),
        ),
        (2, layout(3, 1), String::new(), None),
        (2, String::new(), layout(1, 1), None),
        (4, layout(2, 1), String::new(), None),
        // no heads: the layout stays as the definition gives it
        (
            0,
            String::new(),
            layout(1, 1),
            Some(Layout {
                width: 1,
                height: 1,
            }),
        ),
    ];
    for (heads, in_mode, in_fixture, expected) in cases {
        let channels: String = (0..4)
            .map(|n| format!(r#"<Channel Number="{n}">C</Channel>"#))
            .collect();
        let heads: String = (0..heads)
            .map(|n| format!("<Head><Channel>{}</Channel></Head>", n % 4))
            .collect();
        let source = format!(
            r#"<FixtureDefinition><Manufacturer>A</Manufacturer><Model>M</Model><Type>Other</Type>
 <Channel Name="C" Preset="IntensityRed"/>{in_fixture}
 <Mode Name="M">{in_mode}{channels}{heads}</Mode></FixtureDefinition>"#
        );
        let fixture = parse("t.qxf", source.as_bytes()).expect("a definition reads");
        let mode = fixture.modes.first().expect("one mode");
        assert_eq!(mode.layout, expected, "{source}");
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
        "Cameo-CLBARL10RGBA.qxf",
        "Lanta-Fireball-P64s.qxf",
        "American-DJ-Ultra-Bar-6.qxf",
    ]
    .map(read)
    .into();
    let cases: [(&str, &[&str]); 15] = [
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
        ("clbar10rgba", &["Cameo LED Colour Bar RGBA (CLBAR10RGBA)"]),
        (
            "(clbar10rgba)",
            &["Cameo LED Colour Bar RGBA (CLBAR10RGBA)"],
        ),
        ("clbar", &["Cameo LED Colour Bar RGBA (CLBAR10RGBA)"]),
        (
            "par",
            &[
                "Showtec Par 64 100W COB UV",
                "Lanta Fireball P64s Slimline LED Parcan",
                "American DJ Ultra Bar 6",
                "Cameo LED Colour Bar RGBA (CLBAR10RGBA)",
            ],
        ),
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
