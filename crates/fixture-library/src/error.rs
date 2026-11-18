use crate::model::Layout;
use alloc::string::String;
use core::fmt;

/// A definition that could not be read: which file, where in it, and what is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub file: String,
    /// The line in the file, counted from 1, when the problem has one.
    pub line: Option<u32>,
    pub problem: Problem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    NotText,
    NotXml(String),
    /// A DOCTYPE with declarations (`[ ... ]`): entities could grow the file when read.
    Declarations,
    TooLarge {
        bytes: usize,
        most: usize,
    },
    WrongRoot(String),
    Missing(&'static str),
    BadNumber {
        what: &'static str,
        text: String,
    },
    UnknownChannel(String),
    DuplicateNumber(u16),
    SkippedNumber(u16),
    BackwardsRange {
        min: u8,
        max: u8,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}: line {line}: {}", self.file, self.problem),
            None => write!(f, "{}: {}", self.file, self.problem),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotText => write!(f, "the file is not text"),
            Self::NotXml(why) => write!(f, "not valid xml: {why}"),
            Self::Declarations => write!(
                f,
                "the DOCTYPE has declarations, which a definition does not need"
            ),
            Self::TooLarge { bytes, most } => write!(f, "too large: {bytes} bytes, at most {most}"),
            Self::WrongRoot(found) => write!(f, "expected FixtureDefinition, found {found}"),
            Self::Missing(what) => write!(f, "<{what}> is missing"),
            Self::BadNumber { what, text } => {
                write!(f, "{what}=\"{text}\" is not a number in range")
            }
            Self::UnknownChannel(name) => {
                write!(f, "the mode names channel \"{name}\", which is not defined")
            }
            Self::DuplicateNumber(n) => write!(f, "channel number {n} is used twice in a mode"),
            Self::SkippedNumber(n) => write!(f, "channel number {n} skips a number in a mode"),
            Self::BackwardsRange { min, max } => {
                write!(f, "the range {min} to {max} runs backwards")
            }
        }
    }
}

/// Something a definition gets wrong that the rest can be read without. It is left out of
/// the model, and said here: which mode, where in the file, what and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub mode: String,
    /// The line in the file, counted from 1.
    pub line: u32,
    pub left_out: LeftOut,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeftOut {
    /// A head that names a channel the mode does not have. `head` counts the mode's heads
    /// from 0, in the file's order; `channels` is how many the mode has.
    Head {
        head: usize,
        channel: u16,
        channels: usize,
    },
    /// A layout without exactly one cell for each head the mode keeps.
    Layout { layout: Layout, heads: usize },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mode \"{}\": line {}: ", self.mode, self.line)?;
        match self.left_out {
            LeftOut::Head {
                head,
                channel,
                channels,
            } => write!(
                f,
                "head {head} names channel {channel}, the mode has {channels}: the head is left out"
            ),
            LeftOut::Layout { layout, heads } => write!(
                f,
                "the layout is {} by {}, the mode has {heads} heads: the layout is left out",
                layout.width, layout.height
            ),
        }
    }
}
