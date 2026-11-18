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
    /// A head names a channel number the mode does not have.
    UnknownHeadChannel(u16),
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
            Self::UnknownHeadChannel(n) => {
                write!(f, "a head names channel {n}, which the mode does not have")
            }
            Self::DuplicateNumber(n) => write!(f, "channel number {n} is used twice in a mode"),
            Self::SkippedNumber(n) => write!(f, "channel number {n} skips a number in a mode"),
            Self::BackwardsRange { min, max } => {
                write!(f, "the range {min} to {max} runs backwards")
            }
        }
    }
}
