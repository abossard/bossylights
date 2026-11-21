//! Follow the level and beat of a stream of samples on the caller's clock.
//!
//! ```mermaid
//! flowchart LR
//!   B["Timed samples"] --> H["Energy per hop<br/>round(rate / 100) frames"]
//!   H --> O["Onset"] --> T["Last eight gaps<br/>tempo and confidence"]
//!   T --> S["Signal"] --> BT["Beat"]
//!   H --> R["Reading every two hops<br/>level, signal, confidence and tempo"]
//!   S --> R
//! ```
//!
//! The signal starts at None. Onset columns give the confidence in percent.
//!
//! | From | Onset < 60 | Onset 60 to 79 | Onset ≥ 80 | Quiet < 3 s | Quiet ≥ 3 s | Gap |
//! |------|------------|----------------|------------|-------------|-------------|-----|
//! | None   | Weak | Weak   | Weak   | None   | None | None |
//! | Weak   | Weak | Weak   | Locked | Weak   | Lost | Lost |
//! | Locked | Weak | Locked | Locked | Locked | Lost | Lost |
//! | Lost   | Weak | Weak   | Weak   | Lost   | Lost | Lost |

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod estimate;
mod tracker;
pub use tracker::Tracker;

/// A sample rate in Hz and the number of interleaved channels in each frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    rate: u32,
    channels: u16,
}

impl Format {
    pub fn new(rate: u32, channels: u16) -> Option<Self> {
        ((8_000..=192_000).contains(&rate) && (1..=32).contains(&channels))
            .then_some(Self { rate, channels })
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Samples<'a> {
    F32(&'a [f32]),
    I16(&'a [i16]),
    U16(&'a [u16]),
}

/// The observed beats and their total span in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tempo {
    pub span: u64,
    pub beats: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    None,
    Weak,
    Locked,
    Lost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Onset { confidence: u8 },
    Quiet { since_onset: u64 },
    Gap,
}

impl Signal {
    pub const fn next(self, step: Step) -> Self {
        match (self, step) {
            (Self::None | Self::Lost, Step::Onset { .. }) => Self::Weak,
            (Self::Weak, Step::Onset { confidence: 0..80 }) => Self::Weak,
            (Self::Weak, Step::Onset { confidence: 80.. }) => Self::Locked,
            (Self::Locked, Step::Onset { confidence: 0..60 }) => Self::Weak,
            (Self::Locked, Step::Onset { confidence: 60.. }) => Self::Locked,
            (Self::None | Self::Lost, Step::Quiet { .. } | Step::Gap) => self,
            (Self::Weak | Self::Locked, Step::Quiet { since_onset })
                if since_onset < 3_000_000_000 =>
            {
                self
            }
            (Self::Weak | Self::Locked, Step::Quiet { .. } | Step::Gap) => Self::Lost,
        }
    }
}

/// Times are on the caller's clock, in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Onset {
        at: u64,
        confidence: u8,
    },
    Beat {
        at: u64,
        tempo: Tempo,
    },
    Reading {
        at: u64,
        level: u8,
        signal: Signal,
        confidence: u8,
        tempo: Option<Tempo>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartialFrame;
