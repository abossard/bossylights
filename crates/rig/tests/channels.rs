//! A mode at a channel becomes channel config, pairs and compounds (`rig::channels`):
//! real definitions and hand-made modes, each case a row.

mod common;

use common::mode;
use engine_core::{Levels, Light, Universes, resolve};
use fixture_library::{Byte, Direction, Does, Function, Mode, Range};
use rig::channels::{
    Change, Compound, Config, Field, Follows, PastTheEnd, Sense, Shape, Ties, Travel, Width,
    degrees, lower, rgb_to_rgbw, rgbw_to_rgb, slot_byte, to_native, word_join, word_split,
};

const SHEHDS: &str = "Shehds-LED-Spot-100W.qxf";
const MH6: &str = "Martin-Rush-MH6-Wash.qxf";
const FOGGER: &str = "Stairville-AF-180-LED-Fogger-Co2-FX.qxf";
const SPECTRAL: &str = "Showtec-Spectral-850-RGBA-Tour.qxf";
const CLB4: &str = "Stairville-CLB4-RGB-Compact-LED-Bar-4.qxf";
const ELATION: &str = "Elation-Platinum-Spot-5R.qxf";
const SMOKE: &str = "Generic-Generic-Smoke.qxf";
const CAMEO: &str = "Cameo-CLBARL10RGBA.qxf";
const SPACE: u32 = 524_288;

/// All synthetic channels have the same name and v11 function. Only their declarations differ.
fn declared(preset: Option<&str>, group: Option<&str>, byte: Byte) -> fixture_library::Channel {
    let mut channel = fixture_library::Channel {
        name: "test".into(),
        function: Function::Dimmer,
        declared: Default::default(),
        byte,
        default: None,
        direction: None,
        ranges: vec![],
    };
    channel.declared.preset = preset.map(str::to_owned);
    channel.declared.group = group.map(str::to_owned);
    channel
}

fn channel(preset: &str) -> fixture_library::Channel {
    declared(Some(preset), None, Byte::Coarse)
}

fn range(min: u8, max: u8, label: &str, does: Option<Does>) -> Range {
    Range {
        min,
        max,
        label: label.to_string(),
        does,
    }
}

/// A hand-made mode of these channels and heads.
fn hand(channels: Vec<fixture_library::Channel>, pixels: &[&[u16]]) -> Mode {
    Mode {
        name: "hand".to_string(),
        channels,
        pixels: pixels.iter().map(|head| head.to_vec()).collect(),
        layout: None,
        degrees: None,
    }
}

fn fine(preset: &str) -> fixture_library::Channel {
    declared(Some(&format!("{preset}Fine")), None, Byte::Fine)
}

fn reverse_dimmer() -> fixture_library::Channel {
    fixture_library::Channel {
        direction: Some(Direction::Down),
        ..channel("IntensityDimmer")
    }
}

/// A parsed Intensity channel, optionally followed by a separate LED dimmer.
fn intensity_mode(kind: &str, name: &str, ranges: &str, led_dimmer: bool) -> Mode {
    let (definition, entry) = if led_dimmer {
        (
            r#"<Channel Name="LED Dimmer" Preset="IntensityDimmer"/>"#,
            r#"<Channel Number="1">LED Dimmer</Channel>"#,
        )
    } else {
        ("", "")
    };
    let xml = format!(
        r#"<FixtureDefinition><Manufacturer>Test</Manufacturer><Model>Test</Model><Type>{kind}</Type>
        <Channel Name="{name}"><Group Byte="0">Intensity</Group>{ranges}</Channel>{definition}
        <Mode Name="{name}"><Channel Number="0">{name}</Channel>{entry}</Mode></FixtureDefinition>"#
    );
    fixture_library::parse("intensity.qxf", xml.as_bytes())
        .unwrap()
        .modes
        .remove(0)
}

/// A channel's config in words: "rest 252, dark 0, jump, blackout".
fn say(config: &Config) -> String {
    let change = match config.change {
        Change::Fade => "fade",
        Change::Jump => "jump",
    };
    let follows = match (
        config.follows.has(Follows::MASTER),
        config.follows.has(Follows::BLACKOUT),
    ) {
        (true, true) => "master and blackout",
        (false, true) => "blackout",
        (true, false) => "master",
        (false, false) => "none",
    };
    let mut said = format!(
        "rest {}, dark {}, {change}, {follows}",
        config.rest, config.dark
    );
    if config.ties == Ties::Highest {
        said.push_str(", highest");
    }
    let Travel { low, high, sense } = config.travel;
    if (low, high, sense) != (0, 255, Sense::Rising) {
        let (from, to) = match sense {
            Sense::Rising => (low, high),
            Sense::Falling => (high, low),
        };
        said.push_str(&format!(", runs {from} to {to}"));
    }
    said
}

fn say_field(field: &Field) -> String {
    match field {
        Field::Byte(at) => at.to_string(),
        Field::Word(pair) => format!("{}+{}", pair.coarse, pair.fine),
    }
}

/// A compound in words, skipping the individual bytes and words.
fn say_compound(compound: &Compound) -> Option<String> {
    let fields = |fields: &[Field]| fields.iter().map(say_field).collect::<Vec<_>>().join(" ");
    let shape = match &compound.shape {
        Shape::Byte(_) | Shape::Word(_) => return None,
        Shape::Xy { pan, tilt, degrees } => {
            let degrees = degrees.map_or(String::new(), |d| format!(", {} x {}", d.pan, d.tilt));
            format!("xy {} {}{degrees}", say_field(pan), say_field(tilt))
        }
        Shape::Rgb(f) => format!("rgb {}", fields(f)),
        Shape::Rgbw(f) => format!("rgbw {}", fields(f)),
        Shape::Rgba(f) => format!("rgba {}", fields(f)),
        Shape::Cmy(f) => format!("cmy {}", fields(f)),
        Shape::Slot { at, slots } => format!("slot {at}: {} slots", slots.len()),
        Shape::Components(parts) => format!(
            "components {}",
            parts
                .iter()
                .map(|(function, field)| format!("{function:?} {}", say_field(field)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    Some(match compound.head {
        Some(head) => format!("head {head}: {shape}"),
        None => shape,
    })
}

#[test]
fn a_mode_takes_its_channels_from_the_first_on() {
    // (channels of the mode, first, channels of the engine, last channel or refused)
    let cases: [(usize, u32, u32, Result<u32, PastTheEnd>); 7] = [
        // a 100-channel mode addresses byte 100: its last channel is 99 from 0
        (100, 0, SPACE, Ok(99)),
        // over the end of a universe: one flat space, no stride
        (100, 470, SPACE, Ok(569)),
        (100, 412, 512, Ok(511)),
        (100, 413, 512, Err(PastTheEnd { channels: 100 })),
        (1, SPACE - 1, SPACE, Ok(SPACE - 1)),
        (1, SPACE, SPACE, Err(PastTheEnd { channels: 1 })),
        (2, u32::MAX, u32::MAX, Err(PastTheEnd { channels: 2 })),
    ];
    for (count, first, space, expected) in cases {
        let mode = hand((0..count).map(|_| channel("Thing")).collect(), &[]);
        let lowered = lower(&mode, first, space);
        let last = lowered.map(|lowered| {
            let ats: Vec<u32> = lowered.channels.iter().map(|c| c.at).collect();
            assert_eq!(ats, (first..first + count as u32).collect::<Vec<_>>());
            ats[count - 1]
        });
        assert_eq!(last, expected, "{count} channels at {first} of {space}");
    }
}

#[test]
fn each_channel_gets_rules_from_its_declared_role() {
    // (mode, channel of the mode, its config in words)
    let clay = mode("Clay-Paky-Alpha-Spot-575-HPE.qxf", "Option 16b");
    let shehds = mode(SHEHDS, "14 Channel");
    let mh6 = mode(MH6, "Standard");
    let fogger = mode(FOGGER, "7 Channel");
    let spectral = mode(SPECTRAL, "8 channel");
    let clb4 = mode(CLB4, "14 Channel");
    let rgb = hand(
        vec![
            channel("IntensityRed"),
            channel("IntensityGreen"),
            channel("IntensityBlue"),
        ],
        &[],
    );
    let cmy = hand(
        vec![
            channel("IntensityCyan"),
            channel("IntensityMagenta"),
            channel("IntensityYellow"),
            channel("IntensityDimmer"),
            channel("IntensityCyan"),
        ],
        &[&[0, 1, 2], &[3, 4]],
    );
    let reverse = hand(vec![reverse_dimmer(), fine("IntensityDimmer")], &[]);
    let no_closed = hand(
        vec![fixture_library::Channel {
            ranges: vec![
                range(0, 9, "Strobe off", None),
                range(10, 255, "Strobe", None),
            ],
            ..declared(None, Some("Shutter"), Byte::Coarse)
        }],
        &[],
    );
    // two heads, a dimmer on the first only
    let two_heads = hand(
        vec![
            channel("IntensityDimmer"),
            channel("IntensityRed"),
            channel("IntensityGreen"),
            channel("IntensityBlue"),
            channel("IntensityRed"),
            channel("IntensityGreen"),
            channel("IntensityBlue"),
        ],
        &[&[0, 1, 2, 3], &[4, 5, 6]],
    );
    let lone_fine = hand(
        vec![fixture_library::Channel {
            default: Some(7),
            ..fine("IntensityDimmer")
        }],
        &[],
    );
    let lone_cyan = hand(vec![fine("IntensityCyan")], &[]);
    let pan = hand(
        vec![
            fixture_library::Channel {
                default: Some(128),
                ..channel("PositionPan")
            },
            fixture_library::Channel {
                default: Some(19),
                ..fine("PositionPan")
            },
        ],
        &[],
    );
    let cases = [
        // Iris is declared only as a Beam channel.
        (&clay, 7, "rest 0, dark 0, jump, none"),
        (&shehds, 0, "rest 0, dark 0, fade, master and blackout"),
        // open shutter: rest where light comes out, dark at the first byte of Closed
        (&shehds, 1, "rest 252, dark 0, jump, blackout"),
        // the pinned moving head: pan at 2, its fine at 11, both one fader
        (&shehds, 2, "rest 0, dark 0, fade, none"),
        (&shehds, 11, "rest 0, dark 0, fade, none"),
        (&shehds, 5, "rest 0, dark 0, jump, none"),
        (&shehds, 10, "rest 0, dark 0, fade, none, runs 255 to 0"),
        (&shehds, 13, "rest 0, dark 0, jump, none"),
        (&mh6, 0, "rest 8, dark 0, jump, blackout, runs 181 to 140"),
        (&mh6, 1, "rest 0, dark 0, fade, master and blackout"),
        (&mh6, 2, "rest 0, dark 0, fade, master and blackout"),
        // a dimmer carries the emitters
        (&mh6, 3, "rest 0, dark 0, fade, none"),
        (&mh6, 6, "rest 0, dark 0, fade, none"),
        // inverse zoom: big to small
        (&mh6, 8, "rest 0, dark 0, fade, none, runs 255 to 0"),
        // Smoke leaves its dimmer independent. The emitters beside it follow both.
        (&fogger, 0, "rest 0, dark 0, jump, none"),
        (&fogger, 1, "rest 0, dark 0, jump, none"),
        (&fogger, 4, "rest 0, dark 0, fade, master and blackout"),
        // a shutter with no Closed range does not follow Blackout
        (&spectral, 6, "rest 0, dark 0, jump, none"),
        // the macro wheel lets the mix through at the midpoint of its 0..13 bypass
        (&spectral, 5, "rest 6, dark 0, jump, none"),
        (&pan, 0, "rest 128, dark 0, fade, none"),
        (&pan, 1, "rest 19, dark 0, fade, none"),
        (&no_closed, 0, "rest 0, dark 0, jump, none"),
        // emitters with no dimmer follow both
        (&rgb, 0, "rest 0, dark 0, fade, master and blackout"),
        // subtractive: dark is full, the dimmer of the second head carries its cyan
        (&cmy, 0, "rest 0, dark 255, fade, master and blackout"),
        (&cmy, 4, "rest 0, dark 255, fade, none"),
        // a reverse dimmer is dark at the top, its fine byte too
        (
            &reverse,
            0,
            "rest 0, dark 255, fade, master and blackout, runs 255 to 0",
        ),
        (
            &reverse,
            1,
            "rest 0, dark 255, fade, master and blackout, runs 255 to 0",
        ),
        (&two_heads, 1, "rest 0, dark 0, fade, none"),
        (&two_heads, 4, "rest 0, dark 0, fade, master and blackout"),
        // a fine byte with no coarse byte is a channel of its own
        (&lone_fine, 0, "rest 7, dark 0, fade, none"),
        (&lone_cyan, 0, "rest 0, dark 255, fade, master and blackout"),
    ];
    for (mode, index, expected) in cases {
        let lowered = lower(mode, 100, SPACE).unwrap();
        let config = &lowered.channels[index];
        assert_eq!(config.at, 100 + index as u32);
        assert_eq!(say(config), expected, "{} channel {index}", mode.name);
    }
    // the shared dimmer of a bar carries the emitters of its heads: master scales once
    let lowered = lower(&clb4, 0, SPACE).unwrap();
    let said: Vec<String> = lowered.channels.iter().map(say).collect();
    let mut expected = vec!["rest 0, dark 0, fade, none".to_string(); 12];
    expected.push("rest 0, dark 0, jump, none".into());
    expected.push("rest 0, dark 0, fade, master and blackout".into());
    assert_eq!(said, expected);
}

#[test]
fn a_fine_byte_pairs_by_adjacency_then_declared_key_then_group() {
    // (mode, first, pairs as coarse and fine)
    let shehds = mode(SHEHDS, "14 Channel");
    let mh6 = mode(MH6, "Standard");
    let rgb = mode("Generic-Generic-RGB.qxf", "RGB");
    let clay = mode("Clay-Paky-Alpha-Spot-575-HPE.qxf", "Option Extn");
    let clay_16 = mode("Clay-Paky-Alpha-Spot-575-HPE.qxf", "Option 16b");
    let focus_spot = mode("American-DJ-Focus-Spot-4Z.qxf", "22 Channel");
    let elation = mode(ELATION, "Extended");
    let trailing = hand(
        vec![
            channel("PositionPan"),
            channel("PositionTilt"),
            channel("IntensityDimmer"),
            fine("PositionPan"),
            fine("PositionTilt"),
        ],
        &[],
    );
    let xml = br#"<FixtureDefinition><Manufacturer>Test</Manufacturer><Model>Gobo</Model><Type>Other</Type>
        <Channel Name="A" Preset="GoboWheel"/>
        <Channel Name="B"><Group Byte="1">Gobo</Group></Channel>
        <Mode Name="Mixed"><Channel Number="0">A</Channel><Channel Number="1">B</Channel></Mode>
        </FixtureDefinition>"#;
    let mixed = fixture_library::parse("gobo.qxf", xml)
        .unwrap()
        .modes
        .remove(0);
    let twice = hand(
        vec![
            channel("PositionPan"),
            channel("PositionPan"),
            fine("PositionPan"),
            fine("PositionPan"),
            fine("PositionPan"),
        ],
        &[],
    );
    let heads = hand(
        vec![
            channel("IntensityDimmer"),
            channel("IntensityDimmer"),
            fine("IntensityDimmer"),
            fine("IntensityDimmer"),
        ],
        &[&[0, 3], &[1, 2]],
    );
    let before = hand(
        vec![
            fine("PositionPan"),
            channel("PositionPan"),
            channel("PositionPan"),
        ],
        &[],
    );
    let between = hand(
        vec![
            channel("PositionPan"),
            channel("PositionTilt"),
            fine("PositionPan"),
            channel("PositionPan"),
        ],
        &[],
    );
    type Case<'a> = (&'a Mode, u32, &'a [(u32, u32)]);
    let cases: &[Case] = &[
        // not adjacent: pan at 2 and its fine at 11
        (&shehds, 0, &[(2, 11), (3, 12)]),
        (&mh6, 600, &[(601, 602), (609, 610), (611, 612)]),
        (&rgb, 0, &[]),
        // the first fine is adjacent, the next takes the remaining coarse, the third stays alone
        (&twice, 0, &[(0, 3), (1, 2)]),
        (&before, 0, &[(1, 0)]),
        // The earlier coarse byte wins even when the following one is closer.
        (&between, 0, &[(0, 2)]),
        // a fine byte joins within its head
        (&heads, 0, &[(0, 3), (1, 2)]),
        (&trailing, 0, &[(0, 3), (1, 4)]),
        (&mixed, 0, &[(0, 1)]),
        (&clay, 0, &[(10, 11), (13, 14), (22, 23), (24, 25)]),
        (&clay_16, 0, &[(10, 26), (12, 27), (20, 24), (21, 25)]),
        (
            &focus_spot,
            0,
            &[(0, 1), (2, 3), (8, 9), (11, 12), (14, 15), (17, 18)],
        ),
        (
            &elation,
            0,
            &[
                (0, 1),
                (2, 3),
                (4, 5),
                (7, 8),
                (9, 10),
                (12, 13),
                (14, 15),
                (17, 18),
                (19, 20),
            ],
        ),
    ];
    let mut wrong = Vec::new();
    for &(mode, first, expected) in cases {
        let lowered = lower(mode, first, SPACE).unwrap();
        let pairs: Vec<_> = lowered.pairs.iter().map(|p| (p.coarse, p.fine)).collect();
        if pairs != expected {
            wrong.push((&mode.name, pairs, expected));
        }
    }
    assert!(wrong.is_empty(), "(mode, actual, expected): {wrong:?}");
}

#[test]
fn fine_pair_candidates_use_the_declared_group_and_all_the_heads() {
    let group = |name, byte| declared(None, Some(name), byte);
    let pan = || channel("PositionPan");
    let pan_fine = || fine("PositionPan");
    type Case<'a> = (&'a str, Mode, &'a [(u32, u32)]);
    let cases: &[Case<'_>] = &[
        (
            "no declarations",
            hand(
                vec![
                    declared(None, None, Byte::Coarse),
                    declared(None, None, Byte::Fine),
                ],
                &[],
            ),
            &[],
        ),
        (
            "both presets in Intensity",
            hand(vec![channel("IntensityRed"), fine("IntensityGreen")], &[]),
            &[],
        ),
        (
            "unknown presets",
            hand(vec![channel("Unknown"), fine("Another")], &[]),
            &[],
        ),
        (
            "unknown preset with its fine",
            hand(vec![channel("Unknown"), fine("Unknown")], &[]),
            &[(0, 1)],
        ),
        (
            "explicit group overrides the preset group",
            hand(
                vec![
                    declared(Some("PositionPan"), Some("Tilt"), Byte::Coarse),
                    group("Tilt", Byte::Fine),
                ],
                &[],
            ),
            &[(0, 1)],
        ),
        (
            "loose adjacent before strict",
            hand(vec![pan(), group("Pan", Byte::Coarse), pan_fine()], &[]),
            &[(1, 2)],
        ),
        (
            "earlier fine claims a later coarse before its adjacent fine",
            hand(vec![pan_fine(), pan(), pan_fine()], &[]),
            &[(1, 0)],
        ),
        (
            "two heads share the coarse",
            hand(vec![pan(), pan_fine()], &[&[0], &[0, 1]]),
            &[(0, 1)],
        ),
        (
            "two heads list the fine",
            hand(vec![pan(), pan(), pan_fine()], &[&[0, 2], &[2], &[1]]),
            &[(0, 2)],
        ),
        (
            "fine outside every head",
            hand(vec![pan(), pan_fine()], &[&[0]]),
            &[(0, 1)],
        ),
        (
            "coarse outside the fine's head",
            hand(vec![pan(), pan_fine()], &[&[1]]),
            &[],
        ),
    ];
    for &(case, ref mode, expected) in cases {
        let actual: Vec<_> = lower(mode, 0, SPACE)
            .unwrap()
            .pairs
            .iter()
            .map(|p| (p.coarse, p.fine))
            .collect();
        assert_eq!(actual, expected, "{case}");
    }
}

#[test]
fn loose_candidates_use_the_qlc_group_of_either_preset() {
    let cases = [
        ("IntensityRed", "Intensity"),
        ("PositionPan", "Pan"),
        ("PositionXAxis", "Pan"),
        ("PositionTilt", "Tilt"),
        ("PositionYAxis", "Tilt"),
        ("SpeedPanFastSlow", "Speed"),
        ("ColorWheel", "Colour"),
        ("GoboIndex", "Gobo"),
        ("ShutterIris", "Shutter"),
        ("BeamFocus", "Beam"),
        ("PrismRotationSlowFast", "Prism"),
        ("NoFunction", "Nothing"),
    ];
    for (preset, group) in cases {
        for channels in [
            vec![channel(preset), declared(None, Some(group), Byte::Fine)],
            vec![declared(None, Some(group), Byte::Coarse), fine(preset)],
        ] {
            let lowered = lower(&hand(channels, &[]), 0, SPACE).unwrap();
            assert_eq!(
                lowered.pairs,
                [rig::channels::Pair { coarse: 0, fine: 1 }],
                "{preset} / {group}"
            );
        }
    }
}

#[test]
fn strict_candidates_precede_loose_candidates_and_non_slots_precede_slots() {
    let rgb = fixture_library::Rgb {
        red: 255,
        green: 0,
        blue: 0,
    };
    for does in [
        Does::Gobo {
            picture: None,
            shakes: false,
        },
        Does::Color(rgb),
        Does::Colors(rgb, rgb),
        Does::Prism { faces: None },
        Does::PrismOff,
    ] {
        let slot = |mut c: fixture_library::Channel| {
            c.ranges.push(range(3, 8, "A", Some(does.clone())));
            c
        };
        let strict = || channel("GoboWheel");
        let loose = || declared(None, Some("Gobo"), Byte::Coarse);
        let gap = || channel("PositionPan");
        let end = || fine("GoboIndex");
        let cases = [
            (
                "strict non-slot",
                vec![slot(strict()), strict(), strict(), gap(), end()],
                1,
            ),
            (
                "strict slot before loose non-slot",
                vec![loose(), slot(strict()), gap(), end()],
                1,
            ),
            (
                "first strict slot",
                vec![slot(strict()), slot(strict()), gap(), end()],
                0,
            ),
            (
                "loose non-slot",
                vec![slot(loose()), loose(), loose(), gap(), end()],
                1,
            ),
            (
                "first loose slot",
                vec![slot(loose()), slot(loose()), gap(), end()],
                0,
            ),
            (
                "adjacent loose slot",
                vec![strict(), slot(loose()), end()],
                1,
            ),
            (
                "a group-only fine: the group-only coarse is strict, the preset loose",
                vec![
                    strict(),
                    loose(),
                    gap(),
                    declared(None, Some("Gobo"), Byte::Fine),
                ],
                1,
            ),
        ];
        for (case, channels, coarse) in cases {
            let fine = channels.len() as u32 - 1;
            let lowered = lower(&hand(channels, &[]), 0, SPACE).unwrap();
            assert_eq!(
                lowered.pairs,
                [rig::channels::Pair { coarse, fine }],
                "{case} / {does:?}"
            );
        }
    }
}

#[test]
fn compounds_come_from_metadata() {
    // (mode, compounds past the lone bytes and words, in order)
    let shehds = mode(SHEHDS, "14 Channel");
    let mh6 = mode(MH6, "Standard");
    let clb4 = mode(CLB4, "14 Channel");
    let spectral = mode(SPECTRAL, "8 channel");
    let rgbw = mode("Generic-Generic-RGBW.qxf", "Dimmer RGBW");
    let six = hand(
        vec![
            channel("IntensityRed"),
            channel("IntensityGreen"),
            channel("IntensityBlue"),
            channel("IntensityWhite"),
            channel("IntensityAmber"),
            channel("IntensityUV"),
        ],
        &[],
    );
    let cmy = hand(
        vec![
            channel("IntensityYellow"),
            channel("IntensityMagenta"),
            channel("IntensityCyan"),
        ],
        &[],
    );
    let one = hand(vec![channel("IntensityRed")], &[]);
    let cameo = mode(CAMEO, "19-Channel");
    let cases: &[(&Mode, &[&str])] = &[
        (
            &shehds,
            &[
                "xy 2+11 3+12, 540 x 200",
                "slot 1: 3 slots",
                "slot 5: 17 slots",
                "slot 6: 18 slots",
                "slot 7: 16 slots",
                "slot 8: 3 slots",
                "slot 9: 3 slots",
                "slot 13: 1 slots",
            ],
        ),
        (
            &mh6,
            &[
                "xy 9+10 11+12, 540 x 270",
                "rgbw 3 4 5 6",
                "slot 0: 10 slots",
                "slot 7: 9 slots",
                "slot 13: 19 slots",
            ],
        ),
        // one compound per head
        (
            &clb4,
            &[
                "head 0: rgb 0 1 2",
                "head 1: rgb 3 4 5",
                "head 2: rgb 6 7 8",
                "head 3: rgb 9 10 11",
            ],
        ),
        (
            &spectral,
            &[
                "rgba 1 2 3 4",
                "slot 5: 39 slots",
                "slot 6: 2 slots",
                "slot 7: 1 slots",
            ],
        ),
        (&rgbw, &["rgbw 1 2 3 4"]),
        // named in their order, past the listed shapes
        (
            &six,
            &["components Red 0, Green 1, Blue 2, White 3, Amber 4, Uv 5"],
        ),
        // in the order of the shape, not of the mode
        (&cmy, &["cmy 2 1 0"]),
        (&one, &[]),
        (
            &cameo,
            &[
                "slot 0: 31 slots",
                "slot 1: 1 slots",
                "slot 2: 1 slots",
                "head 0: rgba 3 4 5 6",
                "head 1: rgba 7 8 9 10",
                "head 2: rgba 11 12 13 14",
                "head 3: rgba 15 16 17 18",
            ],
        ),
    ];
    let mut wrong = Vec::new();
    for &(mode, expected) in cases {
        let lowered = lower(mode, 0, SPACE).unwrap();
        let said: Vec<String> = lowered.compounds.iter().filter_map(say_compound).collect();
        if said != expected {
            wrong.push((&mode.name, said, expected));
        }
    }
    assert!(wrong.is_empty(), "(mode, actual, expected): {wrong:?}");
}

#[test]
fn v11_colour_bar_writers_match_the_mode_without_colour_tags() {
    let mode = mode(CAMEO, "19-Channel");
    let mut untagged = mode.clone();
    for channel in &mut untagged.channels {
        channel.declared.colour = None;
    }
    let writers = |mode: &Mode| {
        rig::compile(
            mode,
            common::at(1),
            common::light(0),
            Universes::ONE,
            common::lights(),
        )
        .unwrap()
        .writers
    };
    assert_eq!(writers(&mode), writers(&untagged));
}

#[test]
fn hue_saturation_lightness_and_value_make_a_compound_in_channel_order() {
    let hsv = mode("Chauvet-Ovation-CYC-1-FC.qxf", "HSV");
    let mixed = hand(
        vec![
            channel("IntensityLightness"),
            channel("IntensityBlue"),
            channel("IntensityHue"),
            fine("IntensityHue"),
            channel("IntensityRed"),
            channel("IntensityHue"),
            channel("IntensityGreen"),
            channel("IntensityValue"),
            channel("IntensitySaturation"),
        ],
        &[&[8, 7, 6, 5, 4, 3, 2, 1, 0]],
    );
    let separate = hand(
        vec![channel("IntensityHue"), channel("IntensityValue")],
        &[&[0], &[1]],
    );
    let two = hand(
        vec![channel("IntensityValue"), channel("IntensityHue")],
        &[&[1, 0]],
    );
    let cases: &[(&Mode, &[&str])] = &[
        (&hsv, &["components Hue 40, Saturation 41, Value 42"]),
        (
            &mixed,
            &[
                "head 0: rgb 44 46 41",
                "head 0: components Lightness 40, Hue 42+43, Value 47, Saturation 48",
            ],
        ),
        (&separate, &[]),
        (&two, &["head 0: components Value 40, Hue 41"]),
    ];
    for &(mode, expected) in cases {
        let lowered = lower(mode, 40, SPACE).unwrap();
        let said: Vec<_> = lowered.compounds.iter().filter_map(say_compound).collect();
        assert_eq!(said, expected, "{}", mode.name);
    }
}

#[test]
fn every_channel_has_a_byte_and_each_paired_coarse_has_its_word_next() {
    let modes = [
        mode(SHEHDS, "14 Channel"),
        mode(MH6, "Standard"),
        mode(CLB4, "14 Channel"),
        mode(FOGGER, "7 Channel"),
    ];
    for mode in &modes {
        let lowered = lower(mode, 0, SPACE).unwrap();
        let mut shapes = lowered.compounds.iter().map(|c| &c.shape);
        for config in &lowered.channels {
            assert_eq!(
                shapes.next(),
                Some(&Shape::Byte(config.at)),
                "{} byte {}",
                mode.name,
                config.at
            );
            if let Some(pair) = lowered.pairs.iter().find(|p| p.coarse == config.at) {
                assert_eq!(
                    shapes.next(),
                    Some(&Shape::Word(*pair)),
                    "{} word at {}",
                    mode.name,
                    config.at
                );
            }
        }
        assert!(shapes.all(|s| !matches!(s, Shape::Byte(_) | Shape::Word(_))));
    }
}

#[test]
fn a_wheel_slot_sits_at_the_lower_midpoint_of_its_range() {
    // (range, its byte)
    let cases = [
        ((20, 29), 24),
        ((0, 4), 2),
        ((5, 9), 7),
        ((0, 255), 127),
        ((255, 255), 255),
    ];
    for ((min, max), expected) in cases {
        assert_eq!(
            slot_byte(&range(min, max, "", None)),
            expected,
            "{min} to {max}"
        );
    }
    let cases = [
        (SHEHDS, "14 Channel", 5, 0, ("Open", 2)),
        (SHEHDS, "14 Channel", 5, 1, ("Red", 7)),
        (SHEHDS, "14 Channel", 5, 2, ("Green", 12)),
        (ELATION, "Standard", 4, 1, ("Red", 20)),
        (ELATION, "Extended", 4, 1, ("Red", 20)),
    ];
    for (file, name, channel, index, expected) in cases {
        let lowered = lower(&mode(file, name), 0, SPACE).unwrap();
        let slot = lowered.compounds.iter().find_map(|compound| {
            if let Shape::Slot { at, slots } = &compound.shape
                && *at == channel
            {
                slots.get(index)
            } else {
                None
            }
        });
        assert_eq!(
            slot.map(|s| (s.label.as_str(), s.byte)),
            Some(expected),
            "{file} {name} channel {channel} slot {index}"
        );
    }
}

#[test]
fn a_word_splits_into_coarse_and_fine_and_joins_back() {
    // (word, coarse, fine)
    let cases = [
        (0, 0, 0),
        (0x1234, 0x12, 0x34),
        (0x00FF, 0, 255),
        (u16::MAX, 255, 255),
    ];
    for (word, coarse, fine) in cases {
        assert_eq!(word_split(word), (coarse, fine), "{word}");
        assert_eq!(word_join(coarse, fine), word, "{word}");
    }
}

#[test]
fn a_normalised_value_becomes_a_native_byte_or_word() {
    let full = Travel {
        low: 0,
        high: 255,
        sense: Sense::Rising,
    };
    let inverse = Travel {
        sense: Sense::Falling,
        ..full
    };
    let part = Travel {
        low: 10,
        high: 20,
        sense: Sense::Rising,
    };
    let wash = mode("Martin-MAC-2000-Wash.qxf", "8-bit with Color Wheel 2");
    let speed = lower(&wash, 0, SPACE).unwrap().channels[17];
    assert_eq!(speed.change, Change::Jump);
    // (value 0 to 65535, travel, width, native)
    let cases = [
        (0, speed.travel, Width::Byte, 236),
        (u16::MAX, speed.travel, Width::Byte, 3),
        (0, full, Width::Byte, 0),
        (u16::MAX, full, Width::Byte, 255),
        (32_768, full, Width::Byte, 128),
        (u16::MAX, full, Width::Word, u16::MAX),
        (0x8000, full, Width::Word, 0x8000),
        // inverse zoom: the smallest is the top byte
        (0, inverse, Width::Byte, 255),
        (u16::MAX, inverse, Width::Byte, 0),
        (0, inverse, Width::Word, u16::MAX),
        (0, part, Width::Byte, 10),
        (32_768, part, Width::Byte, 15),
        (u16::MAX, part, Width::Byte, 20),
        (0, part, Width::Word, 0x0A00),
        (u16::MAX, part, Width::Word, 0x14FF),
    ];
    for (value, travel, width, expected) in cases {
        assert_eq!(
            to_native(value, travel, width),
            expected,
            "{value} over {travel:?} as {width:?}"
        );
    }
}

#[test]
fn white_is_the_part_red_green_and_blue_share() {
    // (red, green, blue), (red, green, blue, white)
    let cases = [
        ([0, 0, 0], [0, 0, 0, 0]),
        ([65_535, 65_535, 65_535], [0, 0, 0, 65_535]),
        ([65_535, 32_768, 0], [65_535, 32_768, 0, 0]),
        ([40_000, 30_000, 20_000], [20_000, 10_000, 0, 20_000]),
    ];
    for (rgb, rgbw) in cases {
        assert_eq!(rgb_to_rgbw(rgb), rgbw, "{rgb:?}");
        assert_eq!(rgbw_to_rgb(rgbw), rgb, "{rgbw:?}");
    }
    // more than full saturates
    assert_eq!(rgbw_to_rgb([65_535, 1, 0, 10]), [65_535, 11, 10]);
}

#[test]
fn degrees_become_a_word() {
    // (degrees, of the full range, word)
    let cases = [
        (0, 540, Some(0)),
        (540, 540, Some(u16::MAX)),
        (270, 540, Some(32_768)),
        (90, 270, Some(21_845)),
        (541, 540, None),
        (0, 0, None),
    ];
    for (at, of, expected) in cases {
        assert_eq!(degrees(at, of), expected, "{at} of {of}");
    }
}

#[test]
fn dark_is_a_word_split_across_the_pair() {
    let dimmer = |low, high, sense, closed: Option<u8>| {
        let mut ranges = vec![range(low, high, "Dimmer", Some(Does::Runs(sense)))];
        if let Some(byte) = closed {
            ranges.push(range(byte, byte, "Closed", Some(Does::Closed)));
        }
        fixture_library::Channel {
            ranges,
            ..channel("IntensityDimmer")
        }
    };
    let cases = [
        (dimmer(20, 200, Direction::Down, Some(240)), [240, 0]),
        (dimmer(0, 254, Direction::Down, Some(255)), [255, 0]),
        (dimmer(0, 200, Direction::Down, None), [200, 255]),
        (dimmer(20, 200, Direction::Up, Some(240)), [0, 0]),
        (channel("IntensityCyan"), [255, 255]),
    ];
    for (coarse, [high, low]) in cases {
        let fine = fine(coarse.declared.preset.as_deref().unwrap());
        let mut mode = hand(vec![coarse], &[]);
        let single = lower(&mode, 0, SPACE).unwrap().channels[0];
        assert_eq!((single.dark, single.follows.bits()), (high, 3));

        mode.channels.push(fine);
        let lowered = lower(&mode, 0, SPACE).unwrap();
        let bytes = [0, 1].map(|i| (lowered.channels[i].dark, lowered.channels[i].follows.bits()));
        assert_eq!(bytes, [(high, 3), (low, 3)]);
    }
}

#[test]
fn an_emitter_follows_modifiers_only_when_no_dimmer_carries_it() {
    let (dimmer, red) = ("IntensityDimmer", "IntensityRed");
    let cases = [
        // One dimmer in two heads, another head without it.
        (
            Vec::from([dimmer, red, red, red].map(channel)),
            vec![&[0, 1][..], &[0, 2], &[3]],
            vec![3, 0, 0, 3],
        ),
        // A shared dimmer, plus a head with a dimmer of its own.
        (
            Vec::from([dimmer, red, red, dimmer, red].map(channel)),
            vec![&[2], &[3, 4]],
            vec![3, 0, 0, 3, 0],
        ),
        // The second head also lists the first head's emitter. The last emitter has no head.
        (
            Vec::from([dimmer, red, red, red].map(channel)),
            vec![&[0, 1], &[1, 2]],
            vec![3, 0, 3, 3],
        ),
        // An unpaired fine dimmer cannot carry an emitter, shared or in a head.
        (vec![fine(dimmer), channel(red)], vec![], vec![0, 3]),
        (vec![fine(dimmer), channel(red)], vec![&[0, 1]], vec![0, 3]),
        // An unpaired emitter is a single byte. A dimmer in another head cannot carry it.
        (
            vec![channel(dimmer), fine(red)],
            vec![&[0], &[1]],
            vec![3, 3],
        ),
        (vec![channel(dimmer), fine(red)], vec![&[0, 1]], vec![3, 0]),
    ];
    for (channels, heads, expected) in cases {
        let mode = hand(channels, &heads);
        let lowered = lower(&mode, 0, SPACE).unwrap();
        let follows: Vec<_> = lowered.channels.iter().map(|c| c.follows.bits()).collect();
        assert_eq!(follows, expected, "{heads:?}");
    }
}

#[test]
fn fog_fan_aux_and_dimming_curves_jump_independently() {
    let cases = [
        (intensity_mode("Smoke", "Fog Output", "", false), 0),
        (intensity_mode("Fan", "Fan Speed", "", false), 0),
        (intensity_mode("Fan", "Fan speed", "", false), 0),
        (intensity_mode("Other", "Auxiliary", "", false), 0),
        (mode(SPECTRAL, "8 channel"), 7),
    ];
    let actual = cases.each_ref().map(|(mode, index)| {
        let lowered = lower(mode, 0, SPACE).unwrap();
        say(&lowered.channels[*index])
    });
    assert_eq!(actual, ["rest 0, dark 0, jump, none"; 5]);
}

#[test]
fn undeclared_intensity_and_smoke_dimmers_jump_independently() {
    let cases = [
        (
            "American DJ Mega Hex Par",
            intensity_mode(
                "Color Changer",
                "Dimmer curves",
                r#"<Capability Min="0" Max="20">Standard</Capability>
                   <Capability Min="21" Max="40">Stage</Capability>"#,
                false,
            ),
            "Intensity",
        ),
        (
            "Martin JEM ZR44",
            intensity_mode(
                "Smoke",
                "Intensity",
                r#"<Capability Min="0" Max="12">No fog</Capability>
                   <Capability Min="13" Max="255">Fog at variable density, (13=minimum, 255=maximum)</Capability>"#,
                false,
            ),
            "Intensity",
        ),
        (
            "Stairville M-Fog",
            intensity_mode(
                "Smoke",
                "Volume",
                r#"<Capability Min="0" Max="10">No Fog</Capability>
                   <Capability Min="11" Max="11">Min. Fog</Capability>
                   <Capability Min="12" Max="255">Min-Max</Capability>"#,
                false,
            ),
            "Intensity",
        ),
        (
            "Generic Smoke",
            mode(SMOKE, "Amount"),
            "IntensityMasterDimmer",
        ),
        (
            "Botex DSP-405",
            mode("Botex-DSP-405.qxf", "4 Channels"),
            "Intensity",
        ),
    ];
    let mut wrong = Vec::new();
    for (fixture, mode, label) in cases {
        let lowered = lower(&mode, 0, SPACE).unwrap();
        for (channel, config) in mode.channels.iter().zip(lowered.channels) {
            let actual = (
                channel.function.clone(),
                channel.role(),
                config.change,
                config.follows,
            );
            let expected = (
                Function::Dimmer,
                Function::Other(label.into()),
                Change::Jump,
                Follows::NONE,
            );
            if actual != expected {
                wrong.push((fixture, channel.name.clone(), actual));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

#[test]
fn the_auxiliary_head_dimmer_carries_its_emitters() {
    let mode = mode(
        "Shehds-120W-4in1-RGBW-LED-Wash-Moving-Head.qxf",
        "23 Channel",
    );
    let lowered = lower(&mode, 0, SPACE).unwrap();
    assert_eq!(
        [16, 18, 19, 20].map(|i| lowered.channels[i].follows.bits()),
        [3, 0, 0, 0]
    );
}

#[test]
fn only_the_colour_wheel_rests_at_its_bypass() {
    let xml = br##"<FixtureDefinition><Manufacturer>Test</Manufacturer><Model>Fan and wheel</Model><Type>Other</Type>
        <Channel Name="Red" Preset="IntensityRed"/>
        <Channel Name="Fan Speed" Default="73"><Group Byte="0">Intensity</Group>
            <Capability Min="0" Max="9">Off</Capability>
            <Capability Min="10" Max="255">Run</Capability></Channel>
        <Channel Name="Wheel" Default="73"><Group Byte="0">Colour</Group>
            <Capability Min="0" Max="9">Open</Capability>
            <Capability Min="10" Max="255" Preset="ColorMacro" Res1="#0000ff">Blue</Capability></Channel>
        <Mode Name="All"><Channel Number="0">Red</Channel><Channel Number="1">Fan Speed</Channel>
            <Channel Number="2">Wheel</Channel></Mode></FixtureDefinition>"##;
    let mode = fixture_library::parse("fan-wheel.qxf", xml)
        .unwrap()
        .modes
        .remove(0);
    let lowered = lower(&mode, 0, SPACE).unwrap();
    assert_eq!([0, 1, 2].map(|i| lowered.channels[i].rest), [0, 73, 4]);
}

#[test]
fn tagged_emitters_pair_and_mix_beside_a_fog_channel() {
    let xml = br##"<FixtureDefinition><Manufacturer>Test</Manufacturer><Model>Fog and colour</Model><Type>Smoke</Type>
        <Channel Name="Emitter"><Group Byte="0">Intensity</Group><Colour>Red</Colour></Channel>
        <Channel Name="Fog"><Group Byte="0">Intensity</Group></Channel>
        <Channel Name="Fine"><Group Byte="1">Intensity</Group><Colour>Red</Colour></Channel>
        <Channel Name="Wheel" Default="47"><Group Byte="0">Colour</Group>
            <Capability Min="0" Max="19" Preset="ColorMacro" Res1="#0000ff">Blue</Capability>
            <Capability Min="20" Max="29">Open</Capability>
        </Channel>
        <Mode Name="All"><Channel Number="0">Emitter</Channel><Channel Number="1">Fine</Channel>
            <Channel Number="2">Fog</Channel><Channel Number="3">Wheel</Channel></Mode></FixtureDefinition>"##;
    let emitter_first = fixture_library::parse("fog-colour.qxf", xml)
        .unwrap()
        .modes
        .remove(0);
    let mut fog_before_fine = emitter_first.clone();
    fog_before_fine.channels.swap(1, 2);
    let light = "rest 0, dark 0, fade, master and blackout";
    let independent = "rest 0, dark 0, jump, none";
    let bypass = "rest 24, dark 0, jump, none";
    for (mode, coarse, fine, expected) in [
        (emitter_first, 0, 1, [light, light, independent, bypass]),
        (
            fog_before_fine,
            1,
            2,
            [light, independent, independent, bypass],
        ),
    ] {
        let lowered = lower(&mode, 0, SPACE).unwrap();
        let configs: Vec<_> = lowered.channels.iter().map(say).collect();
        assert_eq!(configs, expected);
        assert_eq!(lowered.pairs, [rig::channels::Pair { coarse, fine }]);
    }
}

#[test]
fn v11_intensity_still_reaches_the_fog_channels() {
    let cases = [
        (
            intensity_mode("Hazer", "Haze", "", false),
            91 * 257,
            vec![91],
        ),
        (mode(SMOKE, "Amount"), 91 * 257, vec![91]),
        (
            intensity_mode("Smoke", "Amount of Fog", "", true),
            u16::MAX,
            vec![255, 255],
        ),
    ];
    let actual = cases.each_ref().map(|(mode, intensity, _)| {
        let compiled = rig::compile(
            mode,
            common::at(1),
            common::light(0),
            Universes::ONE,
            common::lights(),
        )
        .unwrap();
        let light = Light {
            intensity: Some(*intensity),
            ..Light::DARK
        };
        let mut frame = [Levels::ZERO];
        resolve(&[light], &compiled.writers, &mut frame);
        frame[0].0[..mode.channels.len()].to_vec()
    });
    assert_eq!(actual, cases.map(|(_, _, expected)| expected));
}

#[test]
fn follows_records_each_modifier_in_its_slot() {
    let lowered = lower(&hand(vec![channel("IntensityDimmer")], &[]), 0, SPACE).unwrap();
    let cases = [
        (Follows::NONE, 0),
        (Follows::MASTER, 1),
        (Follows::BLACKOUT, 2),
        (lowered.channels[0].follows, 3),
    ];
    for (follows, expected) in cases {
        assert_eq!(follows.bits(), expected);
    }
}

#[test]
fn ties_are_latest_wins_by_default() {
    let lowered = lower(&mode(MH6, "Standard"), 0, SPACE).unwrap();
    assert!(lowered.channels.iter().all(|c| c.ties == Ties::Latest));
    assert!(Follows::NONE.has(Follows::NONE));
    assert!(!Follows::NONE.has(Follows::MASTER));
}
