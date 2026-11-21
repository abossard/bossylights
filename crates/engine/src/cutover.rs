//! Moving one output between workers. The actors, ordering and observations are
//! specified in `docs/design/output-cutover.md`, T1 through T13.
//! The tick, opener and worker share one state. The serving lane number changes
//! at T9 so a displaced worker can never acquire the new lane's rights.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum State {
    Serving,
    Preparing,
    Published,
    Switched,
    Returned,
    Stopped,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Event {
    Begin,
    Wake,
    FrameBack,
    Refuse,
    Publish,
    Cutoff,
    DeviceReturned,
    SendFailed,
    AdapterAvailable,
    Stop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Effect {
    Open,
    SendOld,
    CountOld,
    FreePrepared,
    DrainOld,
    SwitchLane,
    SendNew,
    ReleaseAndAccept,
    ShowNew,
    RecordFailure,
    RecordAvailable,
    Wait,
    Stop,
    Nothing,
}

fn transition(state: State, event: Event) -> (State, Effect) {
    use State::{Preparing, Published, Returned, Serving, Stopped, Switched};
    match (state, event) {
        (_, Event::Stop) => (Stopped, Effect::Stop), // T13
        (Serving, Event::Begin) => (Preparing, Effect::Open), // T1
        (Preparing | Published | Switched | Returned, Event::Begin) => (state, Effect::Wait), // T12
        (Serving | Preparing, Event::Wake) => (state, Effect::SendOld), // T2
        (Serving | Preparing | Published | Switched | Returned, Event::FrameBack) => {
            (state, Effect::CountOld) // T3, including final collection at T9
        }
        (Preparing, Event::Refuse) => (Serving, Effect::FreePrepared), // T4
        (Preparing, Event::Publish) => (Published, Effect::DrainOld),  // T5
        (Published, Event::Cutoff) => (Switched, Effect::SwitchLane),  // T6
        // T6 after T8: the old lane returned before the cutoff carried the switch, so the
        // switch is applied now and T9 shows it (T9, step 1)
        (Returned, Event::Cutoff) => (Returned, Effect::SwitchLane),
        (Switched, Event::Wake) => (Switched, Effect::SendNew),        // T7
        (Published | Switched, Event::DeviceReturned) => (Returned, Effect::ReleaseAndAccept), // T8
        (Returned, Event::Wake) => (Serving, Effect::ShowNew),         // T9
        (Serving | Preparing | Published | Switched | Returned, Event::SendFailed) => {
            (state, Effect::RecordFailure) // T10
        }
        (Serving | Preparing | Published | Switched | Returned, Event::AdapterAvailable) => {
            (state, Effect::RecordAvailable) // T11
        }
        // For instance, a competing publisher or a cutoff can observe a transfer
        // its peer already completed. Repeating that effect would run it twice.
        _ => (state, Effect::Nothing),
    }
}

/// One atomic word lets each actor see the phase and the serving lane together.
/// The lane number advances when T9 makes the new route visible.
pub(crate) struct Machine(AtomicU64);

const PHASE_BITS: u32 = 3;
const PHASE_MASK: u64 = (1 << PHASE_BITS) - 1;

impl State {
    fn decode(word: u64) -> Self {
        match word & PHASE_MASK {
            0 => Self::Serving,
            1 => Self::Preparing,
            2 => Self::Published,
            3 => Self::Switched,
            4 => Self::Returned,
            _ => Self::Stopped,
        }
    }
}

/// A worker's rights follow from its immutable lane number and the shared state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Work {
    Send,
    Hold,
    Drain,
    Stop,
}

impl Machine {
    pub(crate) fn new() -> Self {
        Self(AtomicU64::new(State::Serving as u64))
    }

    pub(crate) fn state(&self) -> State {
        State::decode(self.0.load(Ordering::Acquire))
    }

    /// Observe the effect at the cutoff. Only the actor completing a transition
    /// commits its new state with `apply`.
    pub(crate) fn effect(&self, event: Event) -> Effect {
        transition(self.state(), event).1
    }

    pub(crate) fn apply(&self, event: Event) -> Effect {
        let mut word = self.0.load(Ordering::Acquire);
        loop {
            let state = State::decode(word);
            let (next, effect) = transition(state, event);
            if state == next {
                return effect;
            }
            let serving = (word >> PHASE_BITS) + u64::from(effect == Effect::ShowNew);
            let new = (serving << PHASE_BITS) | next as u64;
            match self
                .0
                .compare_exchange(word, new, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return effect,
                Err(actual) => word = actual,
            }
        }
    }

    /// T13. Its effect is the same from every state (the lanes stop by the engine's
    /// life), so there is nothing to return.
    pub(crate) fn stop(&self) {
        self.apply(Event::Stop);
    }

    pub(super) fn waiting_for_visibility(&self, lane: u64) -> bool {
        lane > self.0.load(Ordering::Acquire) >> PHASE_BITS
    }

    pub(super) fn work(&self, lane: u64) -> Work {
        let word = self.0.load(Ordering::Acquire);
        let state = State::decode(word);
        let serving = word >> PHASE_BITS;
        match (state, lane.cmp(&serving)) {
            (State::Stopped, _) | (_, std::cmp::Ordering::Less) => Work::Stop,
            (State::Returned, std::cmp::Ordering::Greater) => Work::Send,
            (_, std::cmp::Ordering::Greater) => Work::Hold,
            (State::Serving | State::Preparing, _) => Work::Send,
            (State::Published | State::Switched, _) => Work::Drain,
            (State::Returned, _) => Work::Stop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Effect as F, Event as E, State as S, transition};

    const STATES: [S; 6] = [
        S::Serving,
        S::Preparing,
        S::Published,
        S::Switched,
        S::Returned,
        S::Stopped,
    ];
    const EVENTS: [E; 10] = [
        E::Begin,
        E::Wake,
        E::FrameBack,
        E::Refuse,
        E::Publish,
        E::Cutoff,
        E::DeviceReturned,
        E::SendFailed,
        E::AdapterAvailable,
        E::Stop,
    ];

    /// The written row for a pair; `None` is "any other pair": same state, nothing.
    fn row(state: S, event: E) -> Option<(&'static str, S, F)> {
        let live = state != S::Stopped;
        Some(match event {
            E::Stop => ("T13", S::Stopped, F::Stop),
            E::Begin if state == S::Serving => ("T1", S::Preparing, F::Open),
            E::Begin if live => ("T12", state, F::Wait),
            E::Wake if matches!(state, S::Serving | S::Preparing) => ("T2", state, F::SendOld),
            E::FrameBack if live => ("T3", state, F::CountOld),
            E::Refuse if state == S::Preparing => ("T4", S::Serving, F::FreePrepared),
            E::Publish if state == S::Preparing => ("T5", S::Published, F::DrainOld),
            E::Cutoff if state == S::Published => ("T6", S::Switched, F::SwitchLane),
            E::Cutoff if state == S::Returned => ("T6 after T8", S::Returned, F::SwitchLane),
            E::Wake if state == S::Switched => ("T7", S::Switched, F::SendNew),
            E::DeviceReturned if matches!(state, S::Published | S::Switched) => {
                ("T8", S::Returned, F::ReleaseAndAccept)
            }
            E::Wake if state == S::Returned => ("T9", S::Serving, F::ShowNew),
            E::SendFailed if live => ("T10", state, F::RecordFailure),
            E::AdapterAvailable if live => ("T11", state, F::RecordAvailable),
            E::Begin
            | E::Wake
            | E::FrameBack
            | E::Refuse
            | E::Publish
            | E::Cutoff
            | E::DeviceReturned
            | E::SendFailed
            | E::AdapterAvailable => return None,
        })
    }

    #[test]
    fn a_cutover_follows_every_pair_of_the_written_machine() {
        let mut written = 0;
        for state in STATES {
            for event in EVENTS {
                let (name, next, effect) =
                    row(state, event).unwrap_or(("other", state, F::Nothing));
                written += usize::from(name != "other");
                assert_eq!(
                    transition(state, event),
                    (next, effect),
                    "{name} {state:?} {event:?}"
                );
            }
        }
        assert_eq!(written, 36, "rows of the doc's table, pair by pair");
    }
}
