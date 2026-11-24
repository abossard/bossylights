//! The optional wait-free timing stream used by the scorecard.

use crate::{Clock, Outcome, OutputNumber};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub enum Timing {
    Applied {
        number: u64,
        at: Duration,
    },
    Due {
        output: OutputNumber,
        due: Duration,
        applied: u64,
    },
    Finished {
        output: OutputNumber,
        due: Duration,
        at: Duration,
        outcome: Outcome,
    },
}

pub struct Timings {
    clock: Clock,
    events: rtrb::Consumer<Timing>,
    lost: Arc<AtomicBool>,
}

impl Timings {
    pub fn now(&self) -> Duration {
        self.clock.now()
    }

    pub fn discard(&mut self) {
        while self.events.pop().is_ok() {}
        self.lost.store(false, Ordering::Release);
    }

    pub fn pop(&mut self) -> io::Result<Option<Timing>> {
        if self.lost.load(Ordering::Acquire) {
            return Err(io::Error::other("scorecard timing reader fell behind"));
        }
        Ok(self.events.pop().ok())
    }
}

pub(crate) struct TimingWriter {
    events: rtrb::Producer<Timing>,
    lost: Arc<AtomicBool>,
}

impl TimingWriter {
    pub(crate) fn new(clock: Clock) -> (Self, Timings) {
        let (events, reader) = rtrb::RingBuffer::new(8192);
        let lost = Arc::new(AtomicBool::new(false));
        (
            Self {
                events,
                lost: Arc::clone(&lost),
            },
            Timings {
                events: reader,
                lost,
                clock,
            },
        )
    }

    pub(crate) fn write(&mut self, event: Timing) {
        if self.events.push(event).is_err() {
            self.lost.store(true, Ordering::Release);
        }
    }
}
