//! Who keeps Blackout on (#251, `docs/design/channel-model.md`, slot 1): the latch, and
//! one hold per hand. The tests walk each table cell by cell.
//!
//! | Latch \ event | latch on | latch off |
//! |---|---|---|
//! | off | on | off |
//! | on | on | off |
//!
//! | A hand's hold \ event | `Blackout{on}` from it | `Blackout{off}` from it | it drops |
//! |---|---|---|---|
//! | none | held | none | none |
//! | held | held | none | none |
//!
//! The tick is dark while the latch is on or any hand holds. It gets `{latched, holders}`
//! at each change of it, never else.

use crate::Hand;
use engine_core::{BlackoutAsk, BlackoutHold};

/// Hands the book has room for when it is made: a press allocates nothing until the
/// 17th hand holds. Then the room grows, here in the front: Blackout is never refused for
/// it.
const ROOM: usize = 16;

/// What reaches the book: an ask of a hand, or that the hand is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Ask(BlackoutAsk),
    Drops,
}

/// What an event changes, as data, before it is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// Nothing changes: the tick is not told.
    Same,
    /// The latch moves: the tick gets this hold.
    Latch(BlackoutHold),
    /// The hand starts holding: the tick gets this hold, and the hand's let-go keeps a
    /// place in the queue from now on.
    Holds(BlackoutHold),
    /// The hand lets go: the tick gets this hold, in the place its let-go kept.
    LetsGo(BlackoutHold),
}

#[derive(Debug)]
pub(crate) struct Blackout {
    latched: bool,
    /// The hands that hold it, each once.
    holding: Vec<Hand>,
}

impl Blackout {
    /// Nobody keeps it on.
    pub(crate) fn new() -> Self {
        Self {
            latched: false,
            holding: Vec::with_capacity(ROOM),
        }
    }

    /// What the tick has, once every step taken was sent.
    pub(crate) fn hold(&self) -> BlackoutHold {
        BlackoutHold {
            latched: self.latched,
            holders: u32::try_from(self.holding.len()).unwrap_or(u32::MAX),
        }
    }

    /// What `event` from `hand` changes. Changes nothing.
    pub(crate) fn step(&self, hand: Hand, event: Event) -> Step {
        let now = self.hold();
        let held = self.holding.contains(&hand);
        match event {
            Event::Ask(BlackoutAsk::Latch { on }) if on != self.latched => {
                Step::Latch(BlackoutHold { latched: on, ..now })
            }
            Event::Ask(BlackoutAsk::Hold { on: true }) if !held => Step::Holds(BlackoutHold {
                holders: now.holders.saturating_add(1),
                ..now
            }),
            Event::Ask(BlackoutAsk::Hold { on: false }) | Event::Drops if held => {
                Step::LetsGo(BlackoutHold {
                    holders: now.holders.saturating_sub(1),
                    ..now
                })
            }
            Event::Ask(_) | Event::Drops => Step::Same,
        }
    }

    /// Takes `step`, as [`Blackout::step`] made it for `hand`.
    pub(crate) fn take(&mut self, hand: Hand, step: Step) {
        match step {
            Step::Same => {}
            Step::Latch(hold) => self.latched = hold.latched,
            Step::Holds(_) => self.holding.push(hand),
            Step::LetsGo(_) => self.holding.retain(|holder| *holder != hand),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use BlackoutAsk::{Hold, Latch};

    const A: Hand = Hand(1);
    const B: Hand = Hand(2);

    const fn hold(latched: bool, holders: u32) -> BlackoutHold {
        BlackoutHold { latched, holders }
    }

    /// The book after `events`, each taken as its step says.
    fn after(events: &[(Hand, Event)]) -> Blackout {
        let mut book = Blackout::new();
        for (hand, event) in events {
            book.take(*hand, book.step(*hand, *event));
        }
        book
    }

    #[test]
    fn the_latch_follows_its_table() {
        let latch = |on| Event::Ask(Latch { on });
        // (latch before, event, step, latch after)
        let cases = [
            (false, latch(true), Step::Latch(hold(true, 0)), true),
            (false, latch(false), Step::Same, false),
            (true, latch(true), Step::Same, true),
            (true, latch(false), Step::Latch(hold(false, 0)), false),
        ];
        for (before, event, step, latched) in cases {
            let mut book = after(&[(B, latch(before))]);
            assert_eq!(book.step(A, event), step, "{before} + {event:?}");
            book.take(A, step);
            assert_eq!(book.hold(), hold(latched, 0), "{before} + {event:?}");
        }
    }

    #[test]
    fn one_hands_hold_follows_its_table() {
        let blackout = |on| Event::Ask(Hold { on });
        // (A held before, event from A, step, A holds after); B holds throughout
        let cases = [
            (false, blackout(true), Step::Holds(hold(false, 2)), true),
            (false, blackout(false), Step::Same, false),
            (false, Event::Drops, Step::Same, false),
            (true, blackout(true), Step::Same, true),
            (true, blackout(false), Step::LetsGo(hold(false, 1)), false),
            (true, Event::Drops, Step::LetsGo(hold(false, 1)), false),
        ];
        for (before, event, step, holds) in cases {
            let mut book = after(&[(B, blackout(true)), (A, blackout(before))]);
            assert_eq!(book.step(A, event), step, "{before} + {event:?}");
            book.take(A, step);
            assert_eq!(book.holding.contains(&A), holds, "{before} + {event:?}");
            assert!(book.holding.contains(&B), "B's hold is its own");
        }
    }

    #[test]
    fn the_tick_is_dark_while_latched_or_any_hand_holds() {
        let (on, off) = (
            Event::Ask(Hold { on: true }),
            Event::Ask(Hold { on: false }),
        );
        let latch = |on| Event::Ask(Latch { on });
        // (case, [(hand, event, what the tick is told, dark after)])
        type Steps<'a> = &'a [(Hand, Event, Option<BlackoutHold>, bool)];
        let cases: [(&str, Steps); 3] = [
            (
                "A on, B on, A off, B off",
                &[
                    (A, on, Some(hold(false, 1)), true),
                    (B, on, Some(hold(false, 2)), true),
                    (A, off, Some(hold(false, 1)), true),
                    (B, off, Some(hold(false, 0)), false),
                ],
            ),
            (
                "A on, latch on, A drops, latch off",
                &[
                    (A, on, Some(hold(false, 1)), true),
                    (B, latch(true), Some(hold(true, 1)), true),
                    (A, Event::Drops, Some(hold(true, 0)), true),
                    (B, latch(false), Some(hold(false, 0)), false),
                ],
            ),
            (
                "A on twice, A off once",
                &[
                    (A, on, Some(hold(false, 1)), true),
                    (A, on, None, true),
                    (A, off, Some(hold(false, 0)), false),
                ],
            ),
        ];
        for (case, steps) in cases {
            let mut book = Blackout::new();
            for (n, (hand, event, told, dark)) in steps.iter().enumerate() {
                let step = book.step(*hand, *event);
                let sent = match step {
                    Step::Same => None,
                    Step::Latch(hold) | Step::Holds(hold) | Step::LetsGo(hold) => Some(hold),
                };
                book.take(*hand, step);
                assert_eq!(sent, *told, "{case}: told at step {n}");
                assert_eq!(book.hold().dark(), *dark, "{case}: dark at step {n}");
                if let Some(sent) = sent {
                    assert_eq!(sent, book.hold(), "{case}: the tick gets the book's hold");
                }
            }
        }
    }

    /// The 17th hand finds the room full: the book grows, its hold counts all 17.
    #[test]
    fn a_17th_hand_holds_like_the_first() {
        let on = Event::Ask(Hold { on: true });
        let hands: Vec<(Hand, Event)> = (1..=17).map(|n| (Hand(n), on)).collect();
        let book = after(&hands);
        assert_eq!(book.hold(), hold(false, 17));
        assert_eq!(
            book.step(Hand(17), Event::Drops),
            Step::LetsGo(hold(false, 16))
        );
    }
}
