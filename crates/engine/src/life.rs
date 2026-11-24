//! The engine's life and the tick's wake phase: two small machines every thread of
//! the engine reads. Tables in `docs/engine.md` ("Engine life").

use std::sync::atomic::{AtomicU8, Ordering};

/// Where the engine is in its life. Ordered: a life never goes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(crate) enum Life {
    /// The tick wakes, every lane sends.
    Running,
    /// `stop_outputs`: the tick still wakes for a final report, no lane sends, no
    /// output switch is published.
    OutputsStopped,
    /// The engine was dropped: every thread ends.
    Ended,
    /// The tick thread unwound: no picture comes back again.
    TickFailed,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum LifeEvent {
    StopOutputs,
    Drop,
    TickPanicked,
}

/// The transition function, a calculation. Every pair is in the table.
pub(crate) const fn next(life: Life, event: LifeEvent) -> Life {
    use Life::{Ended, OutputsStopped, Running, TickFailed};
    match (life, event) {
        (Running, LifeEvent::StopOutputs) => OutputsStopped,
        (Running | OutputsStopped, LifeEvent::Drop) => Ended,
        (Running | OutputsStopped | Ended, LifeEvent::TickPanicked) => TickFailed,
        (OutputsStopped | Ended | TickFailed, LifeEvent::StopOutputs)
        | (Ended | TickFailed, LifeEvent::Drop)
        | (TickFailed, LifeEvent::TickPanicked) => life,
    }
}

impl Life {
    const fn decode(word: u8) -> Self {
        match word {
            0 => Self::Running,
            1 => Self::OutputsStopped,
            2 => Self::Ended,
            _ => Self::TickFailed,
        }
    }

    /// Lanes send and switches are published only while running.
    pub(crate) fn sends(self) -> bool {
        self == Self::Running
    }

    /// The threads around the tick end; a wait for the tick ends.
    pub(crate) fn over(self) -> bool {
        self >= Self::Ended
    }
}

/// The engine's life, shared by every thread. One atomic: a load never waits.
pub(crate) struct EngineLife(AtomicU8);

impl EngineLife {
    pub(crate) const fn new() -> Self {
        Self(AtomicU8::new(Life::Running as u8))
    }

    pub(crate) fn get(&self) -> Life {
        Life::decode(self.0.load(Ordering::Acquire))
    }

    pub(crate) fn apply(&self, event: LifeEvent) -> Life {
        let mut word = self.0.load(Ordering::Acquire);
        loop {
            let next = next(Life::decode(word), event);
            match self.0.compare_exchange_weak(
                word,
                next as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return next,
                Err(actual) => word = actual,
            }
        }
    }
}

/// Sets [`Life::TickFailed`] when the tick thread unwinds. Lives on the tick thread.
pub(crate) struct TickGuard<'a>(pub(crate) &'a EngineLife);

impl Drop for TickGuard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.apply(LifeEvent::TickPanicked);
        }
    }
}

/// What [`crate::Engine::fresh`] says once the tick thread is gone: no picture comes
/// back again. The engine's process should end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickFailed;

impl std::fmt::Display for TickFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the tick thread failed: no frame is computed any more")
    }
}

impl std::error::Error for TickFailed {}

/// The tick's wake phase, read from the wake count's parity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// No cutoff open: every frame of the last wake was handed.
    Between,
    /// A wake took its commands; its due frames are not all handed yet.
    Handing,
}

/// The tick adds one at the cutoff and one when every due frame is handed.
pub(crate) const fn phase(wake: u64) -> Phase {
    if wake.is_multiple_of(2) {
        Phase::Between
    } else {
        Phase::Handing
    }
}

#[cfg(test)]
mod tests {
    use super::{EngineLife, Life, LifeEvent, Phase, next, phase};

    #[test]
    fn every_life_and_event_has_one_next_life() {
        use Life::{Ended, OutputsStopped, Running, TickFailed};
        use LifeEvent::{Drop, StopOutputs, TickPanicked};
        // docs/engine.md, "Engine life": one row per pair
        let table = [
            (Running, StopOutputs, OutputsStopped),
            (Running, Drop, Ended),
            (Running, TickPanicked, TickFailed),
            (OutputsStopped, StopOutputs, OutputsStopped),
            (OutputsStopped, Drop, Ended),
            (OutputsStopped, TickPanicked, TickFailed),
            (Ended, StopOutputs, Ended),
            (Ended, Drop, Ended),
            (Ended, TickPanicked, TickFailed),
            (TickFailed, StopOutputs, TickFailed),
            (TickFailed, Drop, TickFailed),
            (TickFailed, TickPanicked, TickFailed),
        ];
        for (life, event, expected) in table {
            assert_eq!(next(life, event), expected, "{life:?} + {event:?}");
            assert!(next(life, event) >= life, "{life:?} + {event:?} went back");
            let cell = EngineLife::new();
            for step in [StopOutputs, Drop, TickPanicked] {
                if cell.get() == life {
                    break;
                }
                if next(cell.get(), step) <= life {
                    cell.apply(step);
                }
            }
            assert_eq!(cell.get(), life);
            assert_eq!(
                cell.apply(event),
                expected,
                "{life:?} + {event:?} in the cell"
            );
        }
    }

    #[test]
    fn only_a_running_engine_sends_and_only_an_ended_one_is_over() {
        use Life::{Ended, OutputsStopped, Running, TickFailed};
        for (life, sends, over) in [
            (Running, true, false),
            (OutputsStopped, false, false),
            (Ended, false, true),
            (TickFailed, false, true),
        ] {
            assert_eq!((life.sends(), life.over()), (sends, over), "{life:?}");
        }
    }

    #[test]
    fn an_odd_wake_count_is_a_wake_handing_its_frames() {
        for (wake, expected) in [
            (0, Phase::Between),
            (1, Phase::Handing),
            (2, Phase::Between),
            (u64::MAX, Phase::Handing),
        ] {
            assert_eq!(phase(wake), expected, "{wake}");
        }
    }
}
