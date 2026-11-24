//! A colour lowered to the channels of a cell, by the cell's kind.

use crate::Rgb;

/// What a cell's channels are, in lane order. `Level` is a dimmer with no colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    Rgb,
    Rgbw,
    Cmy,
    Level,
}

/// One word per lane, in the kind's lane order. A byte lane takes the high byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lanes {
    words: [u16; 4],
    count: usize,
}

impl Lanes {
    pub fn as_slice(&self) -> &[u16] {
        self.words.get(..self.count).unwrap_or_default()
    }
}

impl CellKind {
    /// `Rgbw` takes the shared white out of the colour; `Cmy` is what is left of white;
    /// `Level` is the brightest component.
    pub fn lower(self, colour: Rgb) -> Lanes {
        let Rgb { red, green, blue } = colour;
        let (words, count) = match self {
            Self::Rgb => ([red, green, blue, 0], 3),
            Self::Rgbw => {
                let white = red.min(green).min(blue);
                ([red - white, green - white, blue - white, white], 4)
            }
            Self::Cmy => ([u16::MAX - red, u16::MAX - green, u16::MAX - blue, 0], 3),
            Self::Level => ([red.max(green).max(blue), 0, 0, 0], 1),
        };
        Lanes { words, count }
    }
}
