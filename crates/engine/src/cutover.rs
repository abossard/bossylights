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

    #[test]
    fn a_cutover_follows_every_row_of_the_written_machine() {
        let cases = [
            ("T1", S::Serving, E::Begin, S::Preparing, F::Open),
            ("T2", S::Preparing, E::Wake, S::Preparing, F::SendOld),
            ("T3", S::Preparing, E::FrameBack, S::Preparing, F::CountOld),
            ("T3", S::Published, E::FrameBack, S::Published, F::CountOld),
            ("T3", S::Switched, E::FrameBack, S::Switched, F::CountOld),
            ("T4", S::Preparing, E::Refuse, S::Serving, F::FreePrepared),
            ("T5", S::Preparing, E::Publish, S::Published, F::DrainOld),
            ("T6", S::Published, E::Cutoff, S::Switched, F::SwitchLane),
            ("T7", S::Switched, E::Wake, S::Switched, F::SendNew),
            (
                "T8",
                S::Published,
                E::DeviceReturned,
                S::Returned,
                F::ReleaseAndAccept,
            ),
            (
                "T8",
                S::Switched,
                E::DeviceReturned,
                S::Returned,
                F::ReleaseAndAccept,
            ),
            ("T9", S::Returned, E::Wake, S::Serving, F::ShowNew),
            (
                "T10",
                S::Published,
                E::SendFailed,
                S::Published,
                F::RecordFailure,
            ),
            (
                "T10",
                S::Switched,
                E::SendFailed,
                S::Switched,
                F::RecordFailure,
            ),
            (
                "T11",
                S::Serving,
                E::AdapterAvailable,
                S::Serving,
                F::RecordAvailable,
            ),
        ];
        for (row, state, event, next, effect) in cases {
            assert_eq!(transition(state, event), (next, effect), "{row} {state:?}");
        }
        for state in [S::Preparing, S::Published, S::Switched, S::Returned] {
            assert_eq!(
                transition(state, E::Begin),
                (state, F::Wait),
                "T12 {state:?}"
            );
        }
        for state in [
            S::Serving,
            S::Preparing,
            S::Published,
            S::Switched,
            S::Returned,
        ] {
            assert_eq!(
                transition(state, E::Stop),
                (S::Stopped, F::Stop),
                "T13 {state:?}"
            );
        }
    }
}
