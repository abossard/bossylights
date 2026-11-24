//! The four kinds as integer math. Turns, phases and ramp weights are Q32 (2^32 is one).

use crate::Rgb;
use crate::settings::{Background, Effect, Fade, Palette, Settings, Way};
use crate::sine::one_plus_sine;
use core::num::{NonZeroU16, NonZeroU64, NonZeroUsize};

const ONE: u64 = 1 << 32;
const HALF: u64 = 1 << 31;
const PERCENT: NonZeroU64 = match NonZeroU64::new(100) {
    Some(hundred) => hundred,
    None => NonZeroU64::MIN,
};
const Q32: NonZeroU64 = match NonZeroU64::new(ONE) {
    Some(one) => one,
    None => NonZeroU64::MIN,
};
const SPARKLE_CELL: u32 = 0x9E37_79B1;
const SPARKLE_SLOT: u32 = 0x85EB_CA6B;

/// Beats since the play started, Q32 (`1 << 32` is one beat), speed already applied. Never
/// wraps at the measure: Sparkle counts its slots from the start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Beats(pub u64);

/// The matrix: largest `x` + 1, largest `y` + 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub width: NonZeroU16,
    pub height: NonZeroU16,
}

/// A cell: where it is (`y` 0 is the top row) and its index in the matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub x: u16,
    pub y: u16,
    pub index: u32,
}

/// An effect at one moment on one matrix. Made once per play per wake, then asked per cell.
#[derive(Clone, Copy, Debug)]
pub struct Painter {
    palette: Palette,
    shape: Shape,
}

#[derive(Clone, Copy, Debug)]
enum Shape {
    Wave {
        phase: u32,
        spread: u64,
        way: Way,
        /// 100 x the matrix's extent along the way.
        per: NonZeroU64,
    },
    Stripes {
        way: Way,
        stripe: NonZeroU64,
        /// One repeat, `colours x stripe` cells.
        repeat: NonZeroU64,
        /// One repeat in Q32 cells.
        whole: NonZeroU64,
        /// How far the stripes moved, in Q32 cells: less than one repeat.
        moved: u64,
    },
    Plasma {
        phase: u32,
        scale: u64,
        width: NonZeroU64,
        height: NonZeroU64,
    },
    Sparkle {
        /// The seed with the slot mixed in.
        key: u32,
        /// Lit when 100 x the low 16 bits of the hash are below it: `density x 65536`.
        below: u32,
        /// Q32, one at the start of a slot.
        brightness: u64,
        background: Background,
    },
}

impl Effect {
    /// The effect at `beats` on a matrix of `size`.
    pub fn painter(&self, size: Size, beats: Beats) -> Painter {
        let phase = (beats.0 / NonZeroU64::from(self.measure)) as u32;
        let shape = match self.settings {
            Settings::Wave { spread, way } => Shape::Wave {
                phase,
                spread: u64::from(spread),
                way,
                per: NonZeroU64::from(way.extent(size)).saturating_mul(PERCENT),
            },
            Settings::Stripes { size: stripe, way } => {
                let stripe = NonZeroU64::from(stripe);
                let repeat = stripe.saturating_mul(NonZeroU64::from(self.palette.count));
                Shape::Stripes {
                    way,
                    stripe,
                    repeat,
                    whole: repeat.saturating_mul(Q32),
                    moved: u64::from(phase) * repeat.get(),
                }
            }
            Settings::Plasma { scale } => Shape::Plasma {
                phase,
                scale: u64::from(scale),
                width: NonZeroU64::from(size.width),
                height: NonZeroU64::from(size.height),
            },
            Settings::Sparkle {
                density,
                rate,
                fade,
                seed,
                background,
            } => {
                let slots = beats.0.wrapping_mul(u64::from(rate));
                Shape::Sparkle {
                    key: u32::from(seed) ^ ((slots >> 32) as u32).wrapping_mul(SPARKLE_SLOT),
                    below: u32::from(density) << 16,
                    brightness: match fade {
                        Fade::Off => ONE,
                        Fade::On => ONE - (slots & (ONE - 1)),
                    },
                    background,
                }
            }
        };
        Painter {
            palette: self.palette,
            shape,
        }
    }
}

impl Painter {
    /// The colour of one cell, or `None`: a gap, a lower source shows through.
    pub fn paint(&self, place: Place) -> Option<Rgb> {
        match self.shape {
            Shape::Wave {
                phase,
                spread,
                way,
                per,
            } => {
                let shift = ((spread * u64::from(way.along(place))) << 32) / per;
                Some(self.palette.ramp(way.travel(phase, shift as u32)))
            }
            Shape::Stripes {
                way,
                stripe,
                repeat,
                whole,
                moved,
            } => {
                let at = u64::from(way.along(place)) % repeat;
                // Going back, `e = -x`: `repeat - at`, a whole repeat at 0, taken off by `% whole`.
                let cells = if way.forward() { at } else { repeat.get() - at };
                let band = ((((cells << 32) + whole.get() - moved) % whole) >> 32) / stripe;
                Some(self.palette.get(band as usize))
            }
            Shape::Plasma {
                phase,
                scale,
                width,
                height,
            } => {
                let x = ((scale * u64::from(place.x)) << 32) / width;
                let y = ((scale * u64::from(place.y)) << 32) / height;
                let (x, y) = (x as u32, y as u32);
                let sum = one_plus_sine(x.wrapping_add(phase))
                    + one_plus_sine(y.wrapping_sub(phase))
                    + one_plus_sine(x.wrapping_add(y).wrapping_add(phase))
                    + one_plus_sine(x.wrapping_sub(y).wrapping_sub(phase));
                // u = sum / (8 x 65536) of a turn: in Q32, sum x 2^13.
                Some(self.palette.ramp((u64::from(sum) << 13) as u32))
            }
            Shape::Sparkle {
                key,
                below,
                brightness,
                background,
            } => {
                let hash = fmix32(key ^ place.index.wrapping_mul(SPARKLE_CELL));
                if 100 * (hash & 0xFFFF) < below {
                    let colour = self.palette.get((hash >> 16) as usize);
                    Some(scale(colour, brightness))
                } else {
                    match background {
                        Background::Black => Some(Rgb::default()),
                        Background::Through => None,
                    }
                }
            }
        }
    }
}

impl Palette {
    /// Colour `i`, counted around the palette.
    fn get(&self, i: usize) -> Rgb {
        let at = i % NonZeroUsize::from(self.count);
        self.colours.get(at).copied().unwrap_or_default()
    }

    /// The cyclic ramp at `u`, a Q32 turn: colour `i` to colour `i + 1`.
    fn ramp(&self, u: u32) -> Rgb {
        let position = u64::from(u) * u64::from(self.count.get());
        let i = (position >> 32) as usize;
        let weight = position & (ONE - 1);
        let (from, to) = (self.get(i), self.get(i + 1));
        let mix = |a: u16, b: u16| {
            ((u64::from(a) * (ONE - weight) + u64::from(b) * weight + HALF) >> 32) as u16
        };
        Rgb {
            red: mix(from.red, to.red),
            green: mix(from.green, to.green),
            blue: mix(from.blue, to.blue),
        }
    }
}

/// `colour x brightness`, brightness a Q32 fraction up to one, rounded half up.
fn scale(colour: Rgb, brightness: u64) -> Rgb {
    let times = |word: u16| ((u64::from(word) * brightness + HALF) >> 32) as u16;
    Rgb {
        red: times(colour.red),
        green: times(colour.green),
        blue: times(colour.blue),
    }
}

/// MurmurHash3's finaliser (public domain).
const fn fmix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^ (h >> 16)
}

impl Way {
    /// Right and Down travel toward larger coordinates.
    const fn forward(self) -> bool {
        matches!(self, Self::Right | Self::Down)
    }

    const fn along(self, place: Place) -> u16 {
        match self {
            Self::Right | Self::Left => place.x,
            Self::Down | Self::Up => place.y,
        }
    }

    const fn extent(self, size: Size) -> NonZeroU16 {
        match self {
            Self::Right | Self::Left => size.width,
            Self::Down | Self::Up => size.height,
        }
    }

    /// The phase less the shift when travelling forward, plus it when travelling back.
    const fn travel(self, phase: u32, shift: u32) -> u32 {
        if self.forward() {
            phase.wrapping_sub(shift)
        } else {
            phase.wrapping_add(shift)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SPARKLE_CELL, SPARKLE_SLOT, fmix32};

    #[test]
    fn the_sparkle_hash_of_the_design() {
        // Seed 7, cells 0 to 7, slots 0 and 2: the `h` table of effects.md section 3.
        let cases: [(u32, [u32; 8]); 2] = [
            (
                0,
                [
                    0x18c9_aec4,
                    0xf6b7_bbec,
                    0xad04_f4bc,
                    0x6420_408b,
                    0xd13a_a416,
                    0xe5d1_0be5,
                    0x8589_6276,
                    0x16cd_0396,
                ],
            ),
            (
                2,
                [
                    0x1b23_fe5c,
                    0x6538_05ba,
                    0x13e0_f44c,
                    0xd751_7cce,
                    0x61d1_57f3,
                    0xb03b_4bfd,
                    0xdcc9_c64c,
                    0x2ba9_97cb,
                ],
            ),
        ];
        for (slot, hashes) in cases {
            for (cell, expected) in (0u32..).zip(hashes) {
                let key = 7 ^ cell.wrapping_mul(SPARKLE_CELL) ^ slot.wrapping_mul(SPARKLE_SLOT);
                assert_eq!(fmix32(key), expected, "slot {slot}, cell {cell}");
            }
        }
    }
}
