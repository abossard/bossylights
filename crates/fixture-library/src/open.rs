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
    !matches!(range.does, Some(Does::Closed | Does::Strobe(_))) && !range.says_dark()
}

/// The words of a range label that say no light comes out.
const DARK: [&str; 4] = ["blackout", "close", "closed", "dark"];
/// The words of a range label that name what makes the light.
const EMITTERS: [&str; 7] = ["lamp", "lamps", "led", "leds", "light", "lights", "laser"];

impl Range {
    /// The label's words in lower case: runs of letters and digits, split at all else.
    #[must_use]
    pub fn words(&self) -> Vec<String> {
        self.label
            .to_ascii_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect()
    }

    /// The label says no light comes out: blackout, black out, close, closed, dark, or an
    /// emitter (lamp, LED, light, laser) off, and it does not say open. A bare "off" is a
    /// strobe that is off: light.
    #[must_use]
    pub fn says_dark(&self) -> bool {
        let words = self.words();
        let says = |word: &str| words.iter().any(|w| w == word);
        let off = words.windows(2).any(|pair| match pair {
            [before, after] if after == "off" => EMITTERS.contains(&before.as_str()),
            [before, after] => before == "black" && after == "out",
            _ => false,
        });
        !says("open") && (off || DARK.into_iter().any(says))
    }

    /// The label names black, a blackout or the LEDs ("All LEDs off"): it is about the
    /// light going dark, not about a colour macro being off.
    #[must_use]
    pub fn names_black_or_leds(&self) -> bool {
        self.words()
            .iter()
            .any(|w| matches!(w.as_str(), "black" | "blackout" | "led" | "leds"))
    }
}
