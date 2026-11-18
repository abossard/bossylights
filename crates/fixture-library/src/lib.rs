//! Fixture definitions as a model. QLC+ `.qxf` bytes go in, a [`Fixture`] comes out:
//! maker, model, type, and per mode the channels in order, each with what it does and
//! its ranges. Pixel layouts of bars and panels are part of it.
//!
//! This is a calculation crate: it reads no files and no clock. The caller reads the
//! bytes (see `examples/read_folder.rs`) and passes them to [`parse`].
//!
//! ```text
//! bytes + file name --parse--> Fixture | Error { file, line, problem }
//! [Fixture] + words --search--> the matching fixtures, best first
//! ```
//!
//! What a channel does is a closed [`Function`]. A capability the list does not know is
//! kept as `Function::Other` with its original name, never dropped.
//!
//! What the Rig needs to make light come out:
//!
//! | Need | In the model |
//! |---|---|
//! | what a channel does, coarse or fine | [`Channel::function`], [`Channel::byte`]: every QLC+ channel preset |
//! | its value when nothing is said | [`Channel::default`], when the definition gives one |
//! | what a range does | [`Range::does`]: open, closed, strobe, a colour, lamp on, reset |
//! | the value of a shutter or strobe that lets light out | [`Channel::open`], one rule |
//! | pixels | [`Mode::pixels`]: each head's channels, all in the mode |
//!
//! Left out for now: pan and tilt ranges in degrees (QLC+ `Physical/Focus` `PanMax`,
//! `TiltMax`), lens, bulb and dimensions.

#![no_std]
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

extern crate alloc;

mod error;
mod function;
mod model;
mod open;
mod read;
mod search;

pub use error::{Error, Problem};
pub use model::{Byte, Channel, Does, Fixture, Function, Kind, Layout, Mode, Range, Rgb};
pub use read::parse;
pub use search::search;
