//! The four effects (Wave, Stripes, Plasma, Sparkle) as pure math, and a colour lowered to
//! a cell's channels. Spec: `docs/design/effects.md`, section 3.
//!
//! ```mermaid
//! flowchart LR
//!   E["Effect<br/>kind, measure, colours,<br/>settings in range"] -- "painter: once per play per wake" --> P["Painter<br/>phase, slot, settings"]
//!   P -- "paint: per cell" --> C["Rgb words,<br/>or a gap"]
//!   C -- "lower: per cell kind" --> L["1 to 4 lane words"]
//! ```
//!
//! No `std`, no `alloc`, no panic: every divisor is non-zero by its type, every product fits
//! in 64 bits, turns wrap. Nothing is kept between calls: the same effect, size, beats and
//! place always paint the same colour, Sparkle included.

#![cfg_attr(not(test), no_std)] // the sine test compares with `f64::sin`
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod lower;
mod paint;
mod settings;
mod sine;

pub use lower::{CellKind, Lanes};
pub use paint::{Beats, Painter, Place, Size};
pub use settings::{Effect, Kind, MEASURE, Refused, Setting, Unit};

/// A colour as three 16-bit words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
}
