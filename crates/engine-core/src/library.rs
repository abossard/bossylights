//! A text a client sends: a search, a maker, a model, a name. The library's answers are
//! in `crates/wire`.

use core::fmt;
use serde::de::{Deserializer, Error, Visitor};
use serde::{Deserialize, Serialize};

/// A text a client sends: a search, a maker, a model. At most [`Text::MOST`] bytes of
/// UTF-8, kept in place so a request needs no heap.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Text {
    len: u8,
    bytes: [u8; Text::MOST],
}

impl Text {
    /// Longer than any maker, model or mode name of the QLC+ library (66 at most).
    pub const MOST: usize = 128;

    /// `None` when `text` is longer than [`Text::MOST`] bytes.
    pub fn new(text: &str) -> Option<Self> {
        let mut bytes = [0; Self::MOST];
        bytes
            .get_mut(..text.len())?
            .copy_from_slice(text.as_bytes());
        Some(Self {
            len: u8::try_from(text.len()).ok()?,
            bytes,
        })
    }

    pub fn as_str(&self) -> &str {
        let bytes = self.bytes.get(..usize::from(self.len)).unwrap_or_default();
        core::str::from_utf8(bytes).unwrap_or_default() // made from a `&str`: always UTF-8
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl Serialize for Text {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Short;
        impl Visitor<'_> for Short {
            type Value = Text;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a string of at most {} bytes", Text::MOST)
            }
            fn visit_str<E: Error>(self, text: &str) -> Result<Text, E> {
                Text::new(text).ok_or_else(|| E::invalid_length(text.len(), &self))
            }
        }
        deserializer.deserialize_str(Short)
    }
}

#[cfg(test)]
mod tests {
    use super::Text;

    #[test]
    fn a_text_keeps_up_to_most_bytes() {
        let most = "a".repeat(Text::MOST);
        let over = "a".repeat(Text::MOST + 1);
        let cases: [(&str, Option<&str>); 5] = [
            ("", Some("")),
            ("rgbw", Some("rgbw")),
            ("BL63 10\" LED Bar", Some("BL63 10\" LED Bar")),
            (&most, Some(&most)),
            (&over, None),
        ];
        for (text, expected) in cases {
            assert_eq!(
                Text::new(text).as_ref().map(Text::as_str),
                expected,
                "{text}"
            );
        }
    }
}
