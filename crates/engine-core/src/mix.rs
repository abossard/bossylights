//! Who wins: the one place that decides between two values for one channel
//! (`docs/design/who-wins.md`). Nothing else compares or overrides a level.
//!
//! | Rank | | Wins over |
//! |---|---|---|
//! | 1 | blackout | everything |
//! | 2 | a raw channel held on the DMX screen | the light that writes that channel, until released |
//! | 3 | a held value of a light | its default |
//!
//! Two lights never reach one channel (decision 48): the patch refuses two fixtures on
//! one channel, and the Rig makes a channel two heads name the fixture's own, one writer.
//! Who wins between lights is decided per light property, before the Rig.

/// What wants a channel this frame, and how strongly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// What the definition gives a channel no light value writes (a shutter held open),
    /// or 0 where nothing writes.
    Default(u8),
    /// The held values of a light, through its writer. A part not held is `Default`: its
    /// writer writes the channel's default from the definition.
    Light(u8),
    /// A raw channel held on the DMX screen.
    Raw(u8),
    /// Blackout: 0.
    Blackout,
}

impl Want {
    /// The byte that goes out.
    pub const fn level(self) -> u8 {
        match self {
            Self::Default(level) | Self::Light(level) | Self::Raw(level) => level,
            Self::Blackout => 0,
        }
    }
}

/// The winner of `first` and `then`, which came after it. The table of who wins, one
/// arm per row.
pub const fn mix(first: Want, then: Want) -> Want {
    use Want::{Blackout, Default, Light, Raw};
    match (first, then) {
        // 1: blackout wins over everything
        (Blackout, Default(_) | Light(_) | Raw(_) | Blackout)
        | (Default(_) | Light(_) | Raw(_), Blackout) => Blackout,
        // 2: a raw channel held wins over the light that writes that channel
        (Default(_) | Light(_), Raw(level)) | (Raw(level), Default(_) | Light(_)) => Raw(level),
        // 3: a held value of a light wins over its default
        (Default(_), Light(level)) | (Light(level), Default(_)) => Light(level),
        // the same rank: the later wins. Two lights never meet here (decision 48)
        (Default(_), Default(_)) | (Light(_), Light(_)) | (Raw(_), Raw(_)) => then,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Want::{Blackout, Default, Light, Raw};

    #[test]
    fn who_wins_row_by_row() {
        // (row, first, then, winner, level out)
        let cases = [
            (
                "1 blackout over a raw channel",
                Raw(9),
                Blackout,
                Blackout,
                0,
            ),
            (
                "1 blackout over a raw channel",
                Blackout,
                Raw(9),
                Blackout,
                0,
            ),
            ("1 blackout over a light", Light(255), Blackout, Blackout, 0),
            ("1 blackout over a light", Blackout, Light(255), Blackout, 0),
            (
                "1 blackout over a default",
                Default(255),
                Blackout,
                Blackout,
                0,
            ),
            (
                "1 blackout over a default",
                Blackout,
                Default(255),
                Blackout,
                0,
            ),
            ("1 blackout and blackout", Blackout, Blackout, Blackout, 0),
            ("2 raw over a light", Light(255), Raw(9), Raw(9), 9),
            ("2 raw over a light", Raw(9), Light(255), Raw(9), 9),
            ("2 raw at 0 over a light", Light(255), Raw(0), Raw(0), 0),
            ("2 raw over a default", Default(255), Raw(9), Raw(9), 9),
            ("2 raw over a default", Raw(9), Default(255), Raw(9), 9),
            ("2 raw over nothing", Default(0), Raw(9), Raw(9), 9),
            (
                "3 a light over its default",
                Default(255),
                Light(0),
                Light(0),
                0,
            ),
            (
                "3 a light over its default",
                Light(7),
                Default(255),
                Light(7),
                7,
            ),
            ("3 a light over nothing", Default(0), Light(7), Light(7), 7),
        ];
        for (row, first, then, winner, level) in cases {
            assert_eq!(mix(first, then), winner, "{row}: {first:?} then {then:?}");
            assert_eq!(mix(first, then).level(), level, "{row}");
        }
    }
}
