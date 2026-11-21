//! A lane's recovery: whether its device sends, and whether the recovery worker watches
//! it. One state and one mark in one atomic. Table in `docs/design/output-cutover.md`
//! ("Lane recovery").

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum State {
    /// The last send was accepted, or nothing failed yet.
    Healthy,
    /// The device failed a send. It reopens itself, or the recovery worker opens another.
    Lost,
    /// No device: a lane started without one, or a transferred adapter that did not come back.
    Missing,
    /// Lost or missing, and the recovery worker is gone: nobody opens another device.
    Stranded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    /// The device failed a send (T10).
    SendFailed,
    /// The device took a send (T11).
    SendAccepted,
    /// A device took the place of the lane's device (the recovery worker reopened it).
    Replaced,
    /// A device went into the lane while it had none (T8 transfer, or a reopened missing one).
    Filled,
    /// A request to the recovery worker could not be sent: the worker ended.
    WorkerGone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Watch {
    Unwatched,
    /// The recovery worker holds a request for this lane. Health reads it.
    Watched,
}

/// The transition function, a calculation. Every pair is in the table.
pub(crate) const fn next(state: State, event: Event) -> State {
    use Event::{Filled, Replaced, SendAccepted, SendFailed, WorkerGone};
    use State::{Healthy, Lost, Missing, Stranded};
    match (state, event) {
        (Healthy, SendFailed) => Lost,
        (Lost | Stranded, SendAccepted | Replaced) => Healthy,
        // A device in a lane that had none proves itself on its first accepted send.
        (Missing, Filled) => Lost,
        (Lost | Missing, WorkerGone) => Stranded,
        (Healthy, SendAccepted | Replaced | Filled | WorkerGone)
        | (Lost, SendFailed | Filled)
        // A missing lane has no device to send with or to replace.
        | (Missing, SendFailed | SendAccepted | Replaced)
        | (Stranded, SendFailed | Filled | WorkerGone) => state,
    }
}

/// Whether the recovery worker opens a device for a lane in `state`. A device that
/// reconnects itself is left to do so; a missing one has no device to ask.
pub(crate) const fn opens(state: State, reconnects_itself: bool) -> bool {
    match state {
        State::Lost => !reconnects_itself,
        State::Missing => true,
        State::Healthy | State::Stranded => false,
    }
}

/// What a reused lane starts with at T8: the old lane's state, or `Missing` when its
/// device did not come back. A carried loss is watched.
pub(crate) const fn carry(old: State, watch: Watch, returned: bool) -> (State, Watch) {
    let state = if returned { old } else { State::Missing };
    let watch = match (state, watch) {
        (State::Lost | State::Missing | State::Stranded, _) | (State::Healthy, Watch::Watched) => {
            Watch::Watched
        }
        (State::Healthy, Watch::Unwatched) => Watch::Unwatched,
    };
    (state, watch)
}

const WATCHED: u8 = 0b100;
const STATE: u8 = 0b011;

/// A lane's state and watch mark. One atomic: the output thread and the opener never wait.
pub(crate) struct Recovery(AtomicU8);

impl Recovery {
    pub(crate) const fn new(state: State) -> Self {
        Self(AtomicU8::new(state as u8))
    }

    const fn decode(word: u8) -> (State, Watch) {
        let state = match word & STATE {
            0 => State::Healthy,
            1 => State::Lost,
            2 => State::Missing,
            _ => State::Stranded,
        };
        let watch = if word & WATCHED == 0 {
            Watch::Unwatched
        } else {
            Watch::Watched
        };
        (state, watch)
    }

    const fn encode(state: State, watch: Watch) -> u8 {
        state as u8
            | match watch {
                Watch::Unwatched => 0,
                Watch::Watched => WATCHED,
            }
    }

    pub(crate) fn get(&self) -> (State, Watch) {
        Self::decode(self.0.load(Ordering::Acquire))
    }

    pub(crate) fn apply(&self, event: Event) -> State {
        let mut word = self.0.load(Ordering::Acquire);
        loop {
            let (state, watch) = Self::decode(word);
            let after = next(state, event);
            match self.0.compare_exchange_weak(
                word,
                Self::encode(after, watch),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return after,
                Err(actual) => word = actual,
            }
        }
    }

    /// A watch never ends: the mark only goes from unwatched to watched.
    pub(crate) fn watch(&self) {
        self.0.fetch_or(WATCHED, Ordering::AcqRel);
    }

    /// T8 only, before the new lane is released: nothing else writes it yet.
    pub(crate) fn set(&self, (state, watch): (State, Watch)) {
        self.0.store(Self::encode(state, watch), Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Event::{Filled, Replaced, SendAccepted, SendFailed, WorkerGone};
    use State::{Healthy, Lost, Missing, Stranded};

    /// The table in output-cutover.md, "Lane recovery", one row per pair.
    #[test]
    fn every_state_and_event_pair_follows_the_recovery_table() {
        let table = [
            (Healthy, SendFailed, Lost),
            (Healthy, SendAccepted, Healthy),
            (Healthy, Replaced, Healthy),
            (Healthy, Filled, Healthy),
            (Healthy, WorkerGone, Healthy),
            (Lost, SendFailed, Lost),
            (Lost, SendAccepted, Healthy),
            (Lost, Replaced, Healthy),
            (Lost, Filled, Lost),
            (Lost, WorkerGone, Stranded),
            (Missing, SendFailed, Missing),
            (Missing, SendAccepted, Missing),
            (Missing, Replaced, Missing),
            (Missing, Filled, Lost),
            (Missing, WorkerGone, Stranded),
            (Stranded, SendFailed, Stranded),
            (Stranded, SendAccepted, Healthy),
            (Stranded, Replaced, Healthy),
            (Stranded, Filled, Stranded),
            (Stranded, WorkerGone, Stranded),
        ];
        for (state, event, expected) in table {
            assert_eq!(next(state, event), expected, "{state:?} + {event:?}");
            for watch in [Watch::Unwatched, Watch::Watched] {
                let cell = Recovery::new(state);
                if watch == Watch::Watched {
                    cell.watch();
                }
                assert_eq!(
                    cell.apply(event),
                    expected,
                    "{state:?} + {event:?} in the cell"
                );
                assert_eq!(
                    cell.get(),
                    (expected, watch),
                    "{state:?} + {event:?} kept the watch"
                );
            }
        }
    }

    #[test]
    fn the_worker_opens_for_a_lost_device_that_does_not_reconnect_itself_or_a_missing_one() {
        for (state, reconnects_itself, expected) in [
            (Healthy, false, false),
            (Healthy, true, false),
            (Lost, false, true),
            (Lost, true, false),
            (Missing, false, true),
            (Missing, true, true),
            (Stranded, false, false),
            (Stranded, true, false),
        ] {
            assert_eq!(
                opens(state, reconnects_itself),
                expected,
                "{state:?} {reconnects_itself}"
            );
        }
    }

    #[test]
    fn a_reused_lane_carries_the_state_and_watches_a_carried_loss() {
        use Watch::{Unwatched, Watched};
        for (old, watch, returned, expected) in [
            (Healthy, Unwatched, true, (Healthy, Unwatched)),
            (Healthy, Watched, true, (Healthy, Watched)),
            (Healthy, Unwatched, false, (Missing, Watched)),
            (Lost, Unwatched, true, (Lost, Watched)),
            (Lost, Watched, true, (Lost, Watched)),
            (Lost, Unwatched, false, (Missing, Watched)),
            (Missing, Unwatched, false, (Missing, Watched)),
            (Stranded, Unwatched, true, (Stranded, Watched)),
        ] {
            assert_eq!(
                carry(old, watch, returned),
                expected,
                "{old:?} {watch:?} {returned}"
            );
        }
    }

    #[test]
    fn a_watch_stays_whatever_comes_after() {
        let cell = Recovery::new(Healthy);
        cell.watch();
        cell.watch();
        for event in [SendFailed, Filled, WorkerGone, SendAccepted, Replaced] {
            cell.apply(event);
            assert_eq!(cell.get().1, Watch::Watched, "{event:?}");
        }
    }
}
