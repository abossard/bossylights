use crate::model::{LeftOut, Problem, Refused, Warning};
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
            Self::HeadPastEnd {
                head,
                channel,
                channels,
            } => write!(
                f,
                "head {head} names channel {channel}, the mode has {channels}"
            ),
            Self::ActsOnPastEnd { channel, channels } => {
                write!(f, "ActsOn names channel {channel}, the mode has {channels}")
            }
            Self::NoChannel => write!(f, "the mode has no channel"),
            Self::LayoutCells { cells, heads } => {
                write!(
                    f,
                    "the layout has {cells} cells, the mode has {heads} heads"
                )
            }
            Self::NoMode => write!(f, "the file has no <Mode>"),
            Self::NoModeKept(refused) => {
                write!(f, "no mode can be used")?;
                refused
                    .iter()
                    .try_for_each(|refused| write!(f, "; {refused}"))
            }
        }
    }
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "mode \"{}\": line {}: {}",
            self.mode, self.line, self.problem
        )
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mode \"{}\": line {}: ", self.mode, self.line)?;
        match self.left_out {
            LeftOut::Layout { layout, heads } => write!(
                f,
                "the layout is {} by {}, the mode has {heads} heads: the layout is left out",
                layout.width, layout.height
            ),
        }
    }
}
