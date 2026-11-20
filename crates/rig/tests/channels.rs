//! A mode at a channel becomes channel config, pairs and compounds (`rig::channels`):
//! real definitions and hand-made modes, each case a row.

mod common;

use common::mode;
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
const SPACE: u32 = 524_288;

/// A channel that does `function`, coarse, with nothing else said.
fn channel(function: Function) -> fixture_library::Channel {
    fixture_library::Channel {
        name: format!("{function:?}"),
        function,
        byte: Byte::Coarse,
        default: None,
        direction: None,
        ranges: vec![],
    }
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

fn fine(function: Function) -> fixture_library::Channel {
    fixture_library::Channel {
        byte: Byte::Fine,
        ..channel(function)
    }
}

fn reverse_dimmer() -> fixture_library::Channel {
    fixture_library::Channel {
        direction: Some(Direction::Down),
        ..channel(Function::Dimmer)
    }
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

/// A compound in words, `None` for a lone byte or word: every channel has one.
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
        let mode = hand(
            (0..count)
                .map(|_| channel(Function::Other("Thing".into())))
                .collect(),
            &[],
        );
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
fn each_channel_gets_rules_from_its_function() {
    // (mode, channel of the mode, its config in words)
    let shehds = mode(SHEHDS, "14 Channel");
    let mh6 = mode(MH6, "Standard");
    let fogger = mode(FOGGER, "7 Channel");
    let spectral = mode(SPECTRAL, "8 channel");
    let clb4 = mode(CLB4, "14 Channel");
    let rgb = hand(
        vec![
            channel(Function::Red),
            channel(Function::Green),
            channel(Function::Blue),
        ],
        &[],
    );
    let cmy = hand(
        vec![
            channel(Function::Cyan),
            channel(Function::Magenta),
            channel(Function::Yellow),
            channel(Function::Dimmer),
            channel(Function::Cyan),
        ],
        &[&[0, 1, 2], &[3, 4]],
    );
    let reverse = hand(vec![reverse_dimmer(), fine(Function::Dimmer)], &[]);
    let no_closed = hand(
        vec![fixture_library::Channel {
            ranges: vec![
                range(0, 9, "Strobe off", None),
                range(10, 255, "Strobe", None),
            ],
            ..channel(Function::Shutter)
        }],
        &[],
    );
    // two heads, a dimmer on the first only
    let two_heads = hand(
        vec![
            channel(Function::Dimmer),
            channel(Function::Red),
            channel(Function::Green),
            channel(Function::Blue),
            channel(Function::Red),
            channel(Function::Green),
            channel(Function::Blue),
        ],
        &[&[0, 1, 2, 3], &[4, 5, 6]],
    );
    let lone_fine = hand(
        vec![fixture_library::Channel {
            default: Some(7),
            ..fine(Function::Dimmer)
        }],
        &[],
    );
    let cases: [(&Mode, usize, &str); 26] = [
        (&shehds, 0, "rest 0, dark 0, fade, master and blackout"),
        // open shutter: rest where light comes out, dark at the first byte of Closed
        (&shehds, 1, "rest 252, dark 0, jump, blackout"),
        // the pinned moving head: pan at 2, its fine at 11, both one fader
        (&shehds, 2, "rest 0, dark 0, fade, none"),
        (&shehds, 11, "rest 0, dark 0, fade, none"),
        (&shehds, 5, "rest 0, dark 0, jump, none"),
        (&shehds, 10, "rest 0, dark 0, fade, none, runs 255 to 0"),
        (&shehds, 13, "rest 0, dark 0, jump, none"),
        (&mh6, 0, "rest 8, dark 0, jump, blackout"),
        (&mh6, 1, "rest 0, dark 0, fade, master and blackout"),
        (&mh6, 2, "rest 0, dark 0, fade, master and blackout"),
        // a dimmer carries the emitters
        (&mh6, 3, "rest 0, dark 0, fade, none"),
        (&mh6, 6, "rest 0, dark 0, fade, none"),
        // inverse zoom: big to small
        (&mh6, 8, "rest 0, dark 0, fade, none, runs 255 to 0"),
        // fog stays independent, the LEDs beside it follow
        (&fogger, 0, "rest 0, dark 0, jump, none"),
        (&fogger, 1, "rest 0, dark 0, fade, master and blackout"),
        (&fogger, 4, "rest 0, dark 0, fade, none"),
        // a shutter with no Closed range does not follow Blackout
        (&spectral, 6, "rest 0, dark 0, jump, none"),
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
fn a_fine_byte_joins_the_first_coarse_byte_that_does_the_same() {
    // (mode, first, pairs as coarse and fine)
    let shehds = mode(SHEHDS, "14 Channel");
    let mh6 = mode(MH6, "Standard");
    let rgb = mode("Generic-Generic-RGB.qxf", "RGB");
    let twice = hand(
        vec![
            channel(Function::Pan),
            channel(Function::Pan),
            fine(Function::Pan),
            fine(Function::Pan),
            fine(Function::Pan),
        ],
        &[],
    );
    let heads = hand(
        vec![
            channel(Function::Dimmer),
            channel(Function::Dimmer),
            fine(Function::Dimmer),
            fine(Function::Dimmer),
        ],
        &[&[0, 3], &[1, 2]],
    );
    type Case<'a> = (&'a Mode, u32, &'a [(u32, u32)]);
    let cases: [Case; 5] = [
        // not adjacent: pan at 2 and its fine at 11
        (&shehds, 0, &[(2, 11), (3, 12)]),
        (&mh6, 600, &[(601, 602), (609, 610), (611, 612)]),
        (&rgb, 0, &[]),
        // each fine takes the next coarse with none yet; a third stays alone
        (&twice, 0, &[(0, 2), (1, 3)]),
        // a fine byte joins within its head
        (&heads, 0, &[(0, 3), (1, 2)]),
    ];
    for (mode, first, expected) in cases {
        let lowered = lower(mode, first, SPACE).unwrap();
        let pairs: Vec<(u32, u32)> = lowered.pairs.iter().map(|p| (p.coarse, p.fine)).collect();
        assert_eq!(pairs, expected, "{}", mode.name);
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
            channel(Function::Red),
            channel(Function::Green),
            channel(Function::Blue),
            channel(Function::White),
            channel(Function::Amber),
            channel(Function::Uv),
        ],
        &[],
    );
    let cmy = hand(
        vec![
            channel(Function::Yellow),
            channel(Function::Magenta),
            channel(Function::Cyan),
        ],
        &[],
    );
    let one = hand(vec![channel(Function::Red)], &[]);
    let cases: [(&Mode, &[&str]); 8] = [
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
            &["rgba 1 2 3 4", "slot 5: 39 slots", "slot 6: 2 slots"],
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
    ];
    for (mode, expected) in cases {
        let lowered = lower(mode, 0, SPACE).unwrap();
        let said: Vec<String> = lowered.compounds.iter().filter_map(say_compound).collect();
        assert_eq!(said, expected, "{}", mode.name);
    }
}

#[test]
fn every_channel_is_one_lone_byte_or_one_word() {
    let modes = [
        mode(SHEHDS, "14 Channel"),
        mode(MH6, "Standard"),
        mode(CLB4, "14 Channel"),
        mode(FOGGER, "7 Channel"),
    ];
    for mode in &modes {
        let lowered = lower(mode, 40, SPACE).unwrap();
        let mut covered: Vec<u32> = lowered
            .compounds
            .iter()
            .flat_map(|compound| match compound.shape {
                Shape::Byte(at) => vec![at],
                Shape::Word(pair) => vec![pair.coarse, pair.fine],
                Shape::Xy { .. }
                | Shape::Rgb(_)
                | Shape::Rgbw(_)
                | Shape::Rgba(_)
                | Shape::Cmy(_)
                | Shape::Slot { .. }
                | Shape::Components(_) => vec![],
            })
            .collect();
        covered.sort_unstable();
        let all: Vec<u32> = lowered.channels.iter().map(|c| c.at).collect();
        assert_eq!(covered, all, "{}", mode.name);
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
    let lowered = lower(&mode(SHEHDS, "14 Channel"), 0, SPACE).unwrap();
    let wheel = lowered
        .compounds
        .iter()
        .find_map(|compound| {
            if let Shape::Slot { at: 5, slots } = &compound.shape {
                Some(slots)
            } else {
                None
            }
        })
        .unwrap();
    let first: Vec<(&str, u8)> = wheel
        .iter()
        .take(3)
        .map(|slot| (slot.label.as_str(), slot.byte))
        .collect();
    assert_eq!(first, [("Open", 2), ("Red", 7), ("Green", 12)]);
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
    // (value 0 to 65535, travel, width, native)
    let cases = [
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
fn ties_are_latest_wins_by_default() {
    let lowered = lower(&mode(MH6, "Standard"), 0, SPACE).unwrap();
    assert!(lowered.channels.iter().all(|c| c.ties == Ties::Latest));
    assert!(Follows::NONE.has(Follows::NONE));
    assert!(!Follows::NONE.has(Follows::MASTER));
}
