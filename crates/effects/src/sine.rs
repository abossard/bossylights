//! `1 + sin` of a turn, in Q16, from a table of a quarter turn. Linear between entries,
//! rounded half up, exact at every quarter turn.

/// One quarter turn in a Q32 turn.
const QUARTER_TURN: u32 = 1 << 30;
/// Turn bits under one table step: 256 steps per quarter.
const STEP_BITS: u32 = 22;
const STEP: u64 = 1 << STEP_BITS;
/// 1.0 in Q16.
pub(crate) const ONE: u32 = 1 << 16;

/// `round(65536 x sin(2π i / 1024))` for `i` 0 to 256: a quarter turn, both ends included.
const QUARTER: [u32; 257] = [
    0, 402, 804, 1206, 1608, 2010, 2412, 2814, 3216, 3617, 4019, 4420, 4821, 5222, 5623, 6023,
    6424, 6824, 7224, 7623, 8022, 8421, 8820, 9218, 9616, 10014, 10411, 10808, 11204, 11600, 11996,
    12391, 12785, 13180, 13573, 13966, 14359, 14751, 15143, 15534, 15924, 16314, 16703, 17091,
    17479, 17867, 18253, 18639, 19024, 19409, 19792, 20175, 20557, 20939, 21320, 21699, 22078,
    22457, 22834, 23210, 23586, 23961, 24335, 24708, 25080, 25451, 25821, 26190, 26558, 26925,
    27291, 27656, 28020, 28383, 28745, 29106, 29466, 29824, 30182, 30538, 30893, 31248, 31600,
    31952, 32303, 32652, 33000, 33347, 33692, 34037, 34380, 34721, 35062, 35401, 35738, 36075,
    36410, 36744, 37076, 37407, 37736, 38064, 38391, 38716, 39040, 39362, 39683, 40002, 40320,
    40636, 40951, 41264, 41576, 41886, 42194, 42501, 42806, 43110, 43412, 43713, 44011, 44308,
    44604, 44898, 45190, 45480, 45769, 46056, 46341, 46624, 46906, 47186, 47464, 47741, 48015,
    48288, 48559, 48828, 49095, 49361, 49624, 49886, 50146, 50404, 50660, 50914, 51166, 51417,
    51665, 51911, 52156, 52398, 52639, 52878, 53114, 53349, 53581, 53812, 54040, 54267, 54491,
    54714, 54934, 55152, 55368, 55582, 55794, 56004, 56212, 56418, 56621, 56823, 57022, 57219,
    57414, 57607, 57798, 57986, 58172, 58356, 58538, 58718, 58896, 59071, 59244, 59415, 59583,
    59750, 59914, 60075, 60235, 60392, 60547, 60700, 60851, 60999, 61145, 61288, 61429, 61568,
    61705, 61839, 61971, 62101, 62228, 62353, 62476, 62596, 62714, 62830, 62943, 63054, 63162,
    63268, 63372, 63473, 63572, 63668, 63763, 63854, 63944, 64031, 64115, 64197, 64277, 64354,
    64429, 64501, 64571, 64639, 64704, 64766, 64827, 64884, 64940, 64993, 65043, 65091, 65137,
    65180, 65220, 65259, 65294, 65328, 65358, 65387, 65413, 65436, 65457, 65476, 65492, 65505,
    65516, 65525, 65531, 65535, 65536,
];

/// `65536 x (1 + sin(2π turn / 2^32))`: 0 to 131072.
pub(crate) fn one_plus_sine(turn: u32) -> u32 {
    let quadrant = turn >> 30;
    let inside = turn & (QUARTER_TURN - 1);
    // The second and fourth quarters run the table backwards.
    let from_zero = if quadrant & 1 == 0 {
        inside
    } else {
        QUARTER_TURN - inside
    };
    let index = (from_zero >> STEP_BITS) as usize;
    let weight = u64::from(from_zero) & (STEP - 1);
    let low = QUARTER.get(index).copied().unwrap_or(ONE);
    let high = QUARTER.get(index + 1).copied().unwrap_or(low); // past the last entry only at weight 0
    let sine =
        (u64::from(low) * (STEP - weight) + u64::from(high) * weight + STEP / 2) >> STEP_BITS;
    // The third and fourth quarters are below zero.
    if quadrant < 2 {
        ONE + sine as u32
    } else {
        ONE - sine as u32
    }
}

#[cfg(test)]
mod tests {
    use super::{ONE, one_plus_sine};

    fn exact(turn: f64) -> f64 {
        f64::from(ONE) * (1.0 + (std::f64::consts::TAU * turn).sin())
    }

    #[test]
    fn every_entry_of_every_quarter_is_the_formula_rounded() {
        for i in 0..1024u32 {
            let expected = exact(f64::from(i) / 1024.0).round();
            assert_eq!(f64::from(one_plus_sine(i << 22)), expected, "entry {i}");
        }
    }

    #[test]
    fn exact_at_quarter_turns() {
        let cases = [(0, ONE), (1 << 30, 2 * ONE), (2 << 30, ONE), (3 << 30, 0)];
        for (turn, expected) in cases {
            assert_eq!(one_plus_sine(turn), expected, "turn {turn:#x}");
        }
    }

    #[test]
    fn linear_between_entries_rounded_half_up() {
        // Entries 8 and 9 are round(65536 sin(2π 8/1024)) = 3216 and 3617 (f64, outside this table).
        // A quarter of the way: 3216 + 401/4 = 3316.25. Half: 3416.5, up to 3417.
        let at = |entry: u32, step_part: u32| (entry << 22) + step_part;
        let cases = [
            (at(8, 1 << 20), ONE + 3316),
            (at(8, 1 << 21), ONE + 3417),
            // the mirror: the second quarter runs from entry 256 down, 512 - 8.5 = 503.5
            (at(503, 1 << 21), ONE + 3417),
            // below zero: the third quarter, 512 + 8.5
            (at(520, 1 << 21), ONE - 3417),
            (at(1015, 1 << 21), ONE - 3417),
        ];
        for (turn, expected) in cases {
            assert_eq!(one_plus_sine(turn), expected, "turn {turn:#x}");
        }
    }

    #[test]
    fn between_entries_within_2_of_the_formula() {
        // Table rounding, the straight line and its rounding add up to less than 2.
        for step in 0..4096u32 {
            let turn = step * (1 << 20) + 12_345;
            let found = f64::from(one_plus_sine(turn));
            let expected = exact(f64::from(turn) / 4_294_967_296.0);
            assert!(
                (found - expected).abs() < 2.0,
                "turn {turn:#x}: {found} for {expected}"
            );
        }
    }
}
