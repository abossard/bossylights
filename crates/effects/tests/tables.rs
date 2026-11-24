//! The hand-worked tables of `docs/design/effects.md`, section 3, and more rows worked the
//! same way: exact fractions on paper, never this code. A 4 by 2 matrix, cell `i = y x 4 + x`,
//! measure 4 unless a row says otherwise. Each entry is a cell's R, G, B bytes (`word >> 8`).

use effects::{Beats, CellKind, Effect, Kind, Place, Rgb, Size};
use std::num::NonZeroU16;

const BEAT: u64 = 1 << 32;
const RED: Rgb = rgb(65535, 0, 0);
const GREEN: Rgb = rgb(0, 65535, 0);
const BLUE: Rgb = rgb(0, 0, 65535);
const WHITE: Rgb = rgb(65535, 65535, 65535);
const P: [u8; 3] = [128, 0, 128]; // purple: the half-way word 32768
const R: [u8; 3] = [255, 0, 0];
const G: [u8; 3] = [0, 255, 0];
const B: [u8; 3] = [0, 0, 255];
const W: [u8; 3] = [255, 255, 255];
const K: [u8; 3] = [0, 0, 0]; // black

const fn rgb(red: u16, green: u16, blue: u16) -> Rgb {
    Rgb { red, green, blue }
}

fn size(width: u16, height: u16) -> Size {
    Size {
        width: NonZeroU16::new(width).unwrap(),
        height: NonZeroU16::new(height).unwrap(),
    }
}

fn bytes(colour: Rgb) -> [u8; 3] {
    [colour.red, colour.green, colour.blue].map(|word| (word >> 8) as u8)
}

/// The bytes of every cell of a 4 by 2 matrix, row by row. `None` is a gap.
fn cells(effect: &Effect, beats: u64) -> [Option<[u8; 3]>; 8] {
    let painter = effect.painter(size(4, 2), Beats(beats));
    std::array::from_fn(|i| {
        let place = Place {
            x: i as u16 % 4,
            y: i as u16 / 4,
            index: i as u32,
        };
        painter.paint(place).map(bytes)
    })
}

/// One row of four, the same in both rows of the matrix.
fn both_rows(row: [[u8; 3]; 4]) -> [Option<[u8; 3]>; 8] {
    std::array::from_fn(|i| Some(row[i % 4]))
}

fn lit(row: [[u8; 3]; 8]) -> [Option<[u8; 3]>; 8] {
    row.map(Some)
}

fn effect(kind: Kind, measure: u16, colours: &[Rgb], values: &[u16]) -> Effect {
    Effect::new(kind, measure, colours, values).unwrap()
}

#[test]
fn wave() {
    const RIGHT: u16 = 0;
    const LEFT: u16 = 1;
    const DOWN: u16 = 2;
    const UP: u16 = 3;
    let two: &[Rgb] = &[RED, BLUE];
    let four: &[Rgb] = &[RED, GREEN, BLUE, WHITE];
    // (colours, spread, direction, measure, beats, bytes). Rows 1 to 3: the design's table.
    let cases = [
        (two, 100, RIGHT, 4, 0, both_rows([R, P, B, P])),
        (two, 100, RIGHT, 4, BEAT, both_rows([P, R, P, B])),
        (two, 100, RIGHT, 4, 2 * BEAT, both_rows([B, P, R, P])),
        // u = t + x / 4 at t = 1/4: 1/4, 1/2, 3/4, 0
        (two, 100, LEFT, 4, BEAT, both_rows([P, B, P, R])),
        // u = -x / 4: 0, 3/4, 1/2, 1/4 on four colours
        (four, 100, RIGHT, 4, 0, both_rows([R, W, B, G])),
        (four, 100, LEFT, 4, 0, both_rows([R, G, B, W])),
        // u = -x / 8: 0, 7/8 (white and red), 3/4, 5/8 (blue and white)
        (
            four,
            50,
            RIGHT,
            4,
            0,
            both_rows([R, [255, 128, 128], W, [128, 128, 255]]),
        ),
        // spread 0: every cell at u = t = 1/4
        (four, 0, RIGHT, 4, BEAT, both_rows([G, G, G, G])),
        // D is the height, 2: u = -y / 4 in row 1 is 3/4, +y / 4 is 1/4
        (four, 50, DOWN, 4, 0, lit([R, R, R, R, W, W, W, W])),
        (four, 50, UP, 4, 0, lit([R, R, R, R, G, G, G, G])),
        // measure 4 wraps: beat 5 is beat 1
        (two, 100, RIGHT, 4, 5 * BEAT, both_rows([P, R, P, B])),
    ];
    for (colours, spread, direction, measure, beats, expected) in cases {
        let wave = effect(Kind::Wave, measure, colours, &[spread, direction]);
        assert_eq!(
            cells(&wave, beats),
            expected,
            "spread {spread}, direction {direction}, beats {beats:#x}"
        );
    }
}

#[test]
fn the_phase_is_floored_to_a_q32_turn() {
    let three: &[Rgb] = &[RED, GREEN, BLUE];
    // Spread 0 on three colours: p = 3t. Measure 3, beat 1: t = floor(2^32 / 3), so
    // p = 1 - 2^-32 and the word is 65535 x (1 - 2^-32) rounded half up: all green.
    // Beat 1/2: t = floor(2^31 / 3) = 715827882, w = 2^31 - 2. Red: 2^47 + 131070 over 2^32
    // is 32768; green: 2^47 - 131070 is 32767. Exact fractions would give 128, 128.
    let cases = [
        (0, R),
        (BEAT, G),
        (2 * BEAT, B),
        (3 * BEAT, R),
        (4 * BEAT, G),
        (BEAT / 2, [128, 127, 0]),
    ];
    let wave = effect(Kind::Wave, 3, three, &[0, 0]);
    for (beats, expected) in cases {
        assert_eq!(
            cells(&wave, beats),
            both_rows([expected; 4]),
            "beats {beats:#x}"
        );
    }
}

#[test]
fn stripes() {
    const RIGHT: u16 = 0;
    const LEFT: u16 = 1;
    const DOWN: u16 = 2;
    const UP: u16 = 3;
    let two: &[Rgb] = &[RED, BLUE];
    let three: &[Rgb] = &[RED, GREEN, BLUE];
    // (colours, size, direction, beats, bytes). Rows 1 to 3: the design's table.
    // band = floor((e - t x k x size) / size) mod k
    let cases = [
        (two, 2, RIGHT, 0, both_rows([R, R, B, B])),
        (two, 2, RIGHT, BEAT, both_rows([B, R, R, B])),
        (two, 2, RIGHT, 2 * BEAT, both_rows([B, B, R, R])),
        // shift 1/2 cell: floor(-1/4), floor(1/4), floor(3/4), floor(5/4)
        (two, 2, RIGHT, BEAT / 2, both_rows([B, R, R, B])),
        // e = -x: floor(0), floor(-1/2), floor(-1), floor(-3/2)
        (two, 2, LEFT, 0, both_rows([R, B, B, R])),
        // shift 1: floor(-1/2), floor(-1), floor(-3/2), floor(-2)
        (two, 2, LEFT, BEAT, both_rows([B, B, R, R])),
        // size 1, a repeat of 2 cells, so x passes it. Shift 1/2: floor(-x - 1/2)
        (two, 1, LEFT, BEAT, both_rows([B, R, B, R])),
        // size 3, shift 3 at t = 1/2: floor(-1), floor(-2/3), floor(-1/3), floor(0)
        (two, 3, RIGHT, 0, both_rows([R, R, R, B])),
        (two, 3, RIGHT, 2 * BEAT, both_rows([B, B, B, R])),
        // e = y, size 1, three colours: row 0 band 0, row 1 band 1
        (three, 1, DOWN, 0, lit([R, R, R, R, G, G, G, G])),
        // e = -y: row 1 band -1 mod 3 = 2
        (three, 1, UP, 0, lit([R, R, R, R, B, B, B, B])),
        // shift t x 3 = 3/4: floor(-3/4) = -1, floor(1/4) = 0
        (three, 1, DOWN, BEAT, lit([B, B, B, B, R, R, R, R])),
    ];
    for (colours, size, direction, beats, expected) in cases {
        let stripes = effect(Kind::Stripes, 4, colours, &[size, direction]);
        assert_eq!(
            cells(&stripes, beats),
            expected,
            "size {size}, direction {direction}, beats {beats:#x}"
        );
    }
}

#[test]
fn plasma() {
    let four: &[Rgb] = &[RED, GREEN, BLUE, WHITE];
    let seven_eighths = [255, 128, 128]; // white and red
    let eighth = [128, 128, 0]; // u = 1/8: red and green
    let three_eighths = [0, 128, 128]; // green and blue
    let five_eighths = [128, 128, 255]; // blue and white
    // (scale, beats, bytes). Rows 1 to 3: the design's table.
    let cases = [
        (
            1,
            0,
            [
                B,
                seven_eighths,
                B,
                eighth,
                B,
                three_eighths,
                B,
                five_eighths,
            ],
        ),
        (
            1,
            BEAT,
            [
                B,
                three_eighths,
                G,
                three_eighths,
                W,
                five_eighths,
                B,
                five_eighths,
            ],
        ),
        (
            1,
            2 * BEAT,
            [
                B,
                eighth,
                B,
                seven_eighths,
                B,
                five_eighths,
                B,
                three_eighths,
            ],
        ),
        // s = 2 at t = 1/4: a1 = x/2 + 1/4, a2 = y - 1/4, a3 = x/2 + y + 1/4, a4 = x/2 - y - 1/4.
        // x 0: S = 1, 0, 1, 0, u = 1/2. x 1: S = 0, 0, 0, 1, u = 1/4.
        (2, BEAT, [B, G, B, G, B, G, B, G]),
    ];
    for (scale, beats, expected) in cases {
        let plasma = effect(Kind::Plasma, 4, four, &[scale]);
        assert_eq!(
            cells(&plasma, beats),
            lit(expected),
            "scale {scale}, beats {beats:#x}"
        );
    }
}

#[test]
fn plasma_on_four_rows() {
    // 4 by 4, scale 1: y / H runs 0, 1/4, 1/2, 3/4, so the sign of y shows. In brackets: u.
    let four: &[Rgb] = &[RED, GREEN, BLUE, WHITE];
    let plasma = effect(Kind::Plasma, 4, four, &[1]);
    let seven_eighths = [255, 128, 128];
    let five_eighths = [128, 128, 255];
    let three_eighths = [0, 128, 128];
    let eighth = [128, 128, 0];
    // (beats, rows)
    let cases = [
        (
            0,
            [
                [B, seven_eighths, B, eighth],        // 1/2, 7/8, 1/2, 1/8
                [five_eighths, W, five_eighths, B],   // 5/8, 3/4, 5/8, 1/2
                [B, three_eighths, B, five_eighths],  // 1/2, 3/8, 1/2, 5/8
                [three_eighths, B, three_eighths, G], // 3/8, 1/2, 3/8, 1/4
            ],
        ),
        (
            BEAT,
            [
                [B, three_eighths, G, three_eighths], // 1/2, 3/8, 1/4, 3/8
                [five_eighths, G, three_eighths, W],  // 5/8, 1/4, 3/8, 3/4
                [W, five_eighths, B, five_eighths],   // 3/4, 5/8, 1/2, 5/8
                [five_eighths, W, three_eighths, G],  // 5/8, 3/4, 3/8, 1/4
            ],
        ),
    ];
    for (beats, rows) in cases {
        let painter = plasma.painter(size(4, 4), Beats(beats));
        for (y, row) in (0u16..).zip(rows) {
            for (x, expected) in (0u16..).zip(row) {
                let place = Place {
                    x,
                    y,
                    index: u32::from(y * 4 + x),
                };
                assert_eq!(
                    painter.paint(place).map(bytes),
                    Some(expected),
                    "beats {beats:#x}, x {x}, y {y}"
                );
            }
        }
    }
}

#[test]
fn the_largest_inputs_paint_without_a_panic() {
    // Every value at its top: beats u64::MAX, a 65535 by 65535 matrix, the cell at (65534,
    // 65534) with index u32::MAX, measure 32. Worked with the integer rules of effects.md
    // section 3 in Python, outside this code.
    let four: &[Rgb] = &[RED, GREEN, BLUE, WHITE];
    let two: &[Rgb] = &[RED, BLUE];
    // (kind, colours, values, bytes)
    type Case<'a> = (Kind, &'a [Rgb], &'a [u16], [u8; 3]);
    let cases: [Case; 4] = [
        (Kind::Wave, four, &[100, 1], R),
        (Kind::Stripes, four, &[64, 1], R),
        (Kind::Plasma, four, &[8], B),
        (Kind::Sparkle, two, &[100, 8, 0, 65535, 0], R),
    ];
    let place = Place {
        x: 65534,
        y: 65534,
        index: u32::MAX,
    };
    for (kind, colours, values, expected) in cases {
        let painter =
            effect(kind, 32, colours, values).painter(size(65535, 65535), Beats(u64::MAX));
        assert_eq!(painter.paint(place).map(bytes), Some(expected), "{kind:?}");
    }
}

#[test]
fn sparkle() {
    const OFF: u16 = 0;
    const ON: u16 = 1;
    const BLACK: u16 = 0;
    const THROUGH: u16 = 1;
    let two: &[Rgb] = &[RED, BLUE];
    let red_half = [128, 0, 0];
    let blue_half = [0, 0, 128];
    // Lit and colour from the design's h table: lit when the low 16 bits are under 32768,
    // colour by the high 16 bits mod 2.
    let slot_0 = lit([K, K, K, R, K, B, B, B]);
    let slot_0_half = lit([K, K, K, red_half, K, blue_half, blue_half, blue_half]);
    let slot_2 = lit([K, R, K, B, B, B, K, K]);
    let slot_2_half = lit([K, red_half, K, blue_half, blue_half, blue_half, K, K]);
    // (measure, density, rate, fade, background, beats, bytes). Rows 1 to 3: the design's table.
    let cases = [
        (7, 4, 50, 1, ON, BLACK, 0, slot_0),
        (7, 4, 50, 1, ON, BLACK, BEAT / 2, slot_0_half),
        (7, 4, 50, 1, ON, BLACK, 2 * BEAT, slot_2),
        // fade Off: full in the whole slot
        (7, 4, 50, 1, OFF, BLACK, BEAT / 2, slot_0),
        // Through: no offer where unlit
        (
            7,
            4,
            50,
            1,
            ON,
            THROUGH,
            0,
            [None, None, None, Some(R), None, Some(B), Some(B), Some(B)],
        ),
        // density 100: every cell lit, colour by the high bits of h
        (7, 4, 100, 1, ON, BLACK, 0, lit([B, B, R, R, R, B, B, B])),
        // rate 2: beat 5/4 is slot 2 at f = 1/2
        (7, 4, 50, 2, ON, BLACK, 5 * BEAT / 4, slot_2_half),
        // the slot counts beats from the start, never the phase (call 9): measure 1, beat 2
        (7, 1, 50, 1, ON, BLACK, 2 * BEAT, slot_2),
        (7, 1, 50, 1, ON, BLACK, 5 * BEAT / 2, slot_2_half),
        // On the threshold, `100 x low < density x 65536` exactly. Seed 3767, cell 7: low 16
        // bits 0x1999 = 6553, lit at density 10 (6553 < 6553.6). Seed 6270, cell 5: 0x8000 =
        // 32768, dark at density 50. The h values from a MurmurHash3 fmix32 written from its spec.
        (3767, 4, 10, 1, OFF, BLACK, 0, lit([B, K, K, K, K, B, K, R])),
        (6270, 4, 50, 1, OFF, BLACK, 0, lit([K, R, K, B, R, K, K, R])),
    ];
    for (seed, measure, density, rate, fade, background, beats, expected) in cases {
        let sparkle = effect(
            Kind::Sparkle,
            measure,
            two,
            &[density, rate, fade, seed, background],
        );
        assert_eq!(
            cells(&sparkle, beats),
            expected,
            "seed {seed}, measure {measure}, density {density}, rate {rate}, fade {fade}, background {background}, beats {beats:#x}"
        );
    }
}

#[test]
fn lowering() {
    // (colour words, bytes for Rgb, Rgbw, Cmy, Level). Rows 1 to 3: the design's table.
    let cases: [(Rgb, [&[u8]; 4]); 5] = [
        (
            rgb(65535, 0, 0),
            [&[255, 0, 0], &[255, 0, 0, 0], &[0, 255, 255], &[255]],
        ),
        (
            rgb(32768, 0, 32768),
            [&[128, 0, 128], &[128, 0, 128, 0], &[127, 255, 127], &[128]],
        ),
        (
            rgb(65535, 65535, 65535),
            [&[255, 255, 255], &[0, 0, 0, 255], &[0, 0, 0], &[255]],
        ),
        // white 32768 shared: 65535 - 32768 = 32767
        (
            rgb(65535, 65535, 32768),
            [&[255, 255, 128], &[127, 127, 0, 128], &[0, 0, 127], &[255]],
        ),
        (
            rgb(0, 32768, 65535),
            [&[0, 128, 255], &[0, 128, 255, 0], &[255, 127, 0], &[255]],
        ),
    ];
    let kinds = [
        CellKind::Rgb,
        CellKind::Rgbw,
        CellKind::Cmy,
        CellKind::Level,
    ];
    for (colour, expected) in cases {
        for (kind, bytes) in kinds.into_iter().zip(expected) {
            let lanes: Vec<u8> = kind
                .lower(colour)
                .as_slice()
                .iter()
                .map(|word| (word >> 8) as u8)
                .collect();
            assert_eq!(lanes, bytes, "{colour:?} on {kind:?}");
        }
    }
}

#[test]
fn brightness_is_baked_into_the_colour_before_lowering() {
    // The design's last lowering row: red at b = 1/2, a Sparkle cell half-way through its slot.
    let sparkle = effect(Kind::Sparkle, 4, &[RED, BLUE], &[50, 1, 1, 7, 0]);
    let painter = sparkle.painter(size(4, 2), Beats(BEAT / 2));
    let cell_3 = painter
        .paint(Place {
            x: 3,
            y: 0,
            index: 3,
        })
        .unwrap();
    let cases: [(CellKind, &[u8]); 4] = [
        (CellKind::Rgb, &[128, 0, 0]),
        (CellKind::Rgbw, &[128, 0, 0, 0]),
        (CellKind::Cmy, &[127, 255, 255]),
        (CellKind::Level, &[128]),
    ];
    for (kind, expected) in cases {
        let lanes: Vec<u8> = kind
            .lower(cell_3)
            .as_slice()
            .iter()
            .map(|word| (word >> 8) as u8)
            .collect();
        assert_eq!(lanes, expected, "{kind:?}");
    }
}
