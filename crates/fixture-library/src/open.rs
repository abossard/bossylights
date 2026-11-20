//! The value of a shutter or strobe channel that lets light out. A calculation on the
//! channel's ranges: no light is dark because a shutter was left at 0 by accident.

use crate::model::{Channel, Does, Function, Range};
use alloc::string::String;
use alloc::vec::Vec;

impl Channel {
    /// The value that lets light out, for a shutter or a strobe channel. `None` for any
    /// other channel. One rule, first match wins:
    ///
    /// | # | When | Value |
    /// |---|---|---|
    /// | 1 | a range is marked open (`ShutterOpen`) | the lowest value of the first such range |
    /// | 2 | a value, from 0 up, that no range covers, or whose range is not marked closed or a strobe and has no dark label | that value |
    /// | 3 | every value is closed, a strobe or dark | the channel's default, else 0 |
    ///
    /// A dark label says blackout, black out, close, closed, dark, or a lamp, LED, light or
    /// laser off, and does not say open. A bare "off" is a strobe that is off: light.
    #[must_use]
    pub fn open(&self) -> Option<u8> {
        if !matches!(self.function, Function::Shutter | Function::Strobe) {
            return None;
        }
        let marked = self.ranges.iter().find(|r| r.does == Some(Does::Open));
        let free = || {
            (0..=u8::MAX).find(|value| {
                self.ranges
                    .iter()
                    .find(|r| (r.min..=r.max).contains(value))
                    .is_none_or(lets_light_out)
            })
        };
        Some(
            marked
                .map(|r| r.min)
                .or_else(free)
                .unwrap_or(self.default.unwrap_or(0)),
        )
    }

    /// The lowest value of the first range marked closed (`ShutterClose`): where no light
    /// comes out. `None` when no range is marked closed.
    #[must_use]
    pub fn closed(&self) -> Option<u8> {
        self.ranges
            .iter()
            .find(|r| r.does == Some(Does::Closed))
            .map(|r| r.min)
    }
}

fn lets_light_out(range: &Range) -> bool {
    !matches!(range.does, Some(Does::Closed | Does::Strobe(_))) && !dark(&range.label)
}

fn dark(label: &str) -> bool {
    let label = String::from(label).to_ascii_lowercase();
    let words: Vec<&str> = label
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let says = |word: &str| words.contains(&word);
    let off = words.windows(2).any(|pair| match pair {
        [before, "off"] => matches!(
            *before,
            "lamp" | "lamps" | "led" | "leds" | "light" | "lights" | "laser"
        ),
        [before, after] => *before == "black" && *after == "out",
        _ => false,
    });
    !says("open")
        && (off
            || ["blackout", "close", "closed", "dark"]
                .into_iter()
                .any(says))
}
