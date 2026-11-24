//! The wait-free, one-way handoff from the tick to a snapshot reader.

use crate::life::{EngineLife, Life, TickFailed};
use crate::snapshot::{Publication, Snapshot};
use std::thread;
use std::time::Duration;

const BOXES: usize = 2;

/// The tick thread's end of the hand-over.
pub(crate) struct Hand {
    asks: rtrb::Consumer<Box<Snapshot>>,
    pictures: rtrb::Producer<Box<Snapshot>>,
}

/// A box the reader asked to be filled, taken as a wake begins.
pub(crate) struct Asked(Box<Snapshot>);

impl Hand {
    pub(crate) fn asked(&mut self) -> Option<Asked> {
        self.asks.pop().ok().map(Asked)
    }

    pub(crate) fn fill(&mut self, Asked(mut picture): Asked, from: Publication<'_>) {
        picture.publish(from);
        // The queue has room for every box: this push cannot fail, so no box is freed here.
        let _ = self.pictures.push(picture);
    }
}

/// The reader's end of the hand-over: the newest picture and the box that asks for
/// the next one.
pub(crate) struct Reader {
    asks: rtrb::Producer<Box<Snapshot>>,
    pictures: rtrb::Consumer<Box<Snapshot>>,
    latest: Box<Snapshot>,
    spare: Option<Box<Snapshot>>,
}

pub(crate) struct Queues {
    asks: rtrb::Producer<Box<Snapshot>>,
    pictures: rtrb::Consumer<Box<Snapshot>>,
}

pub(crate) fn hand_over() -> (Hand, Queues) {
    let (asks, asked) = rtrb::RingBuffer::new(BOXES);
    let (filled, pictures) = rtrb::RingBuffer::new(BOXES);
    (
        Hand {
            asks: asked,
            pictures: filled,
        },
        Queues { asks, pictures },
    )
}

impl Reader {
    pub(crate) fn new(queues: Queues, first: Snapshot) -> Self {
        Self {
            asks: queues.asks,
            pictures: queues.pictures,
            spare: Some(Box::new(first.clone())),
            latest: Box::new(first),
        }
    }

    pub(crate) fn latest(&mut self) -> &Snapshot {
        self.take_back();
        self.ask();
        &self.latest
    }

    pub(crate) fn fresh(&mut self, life: &EngineLife) -> Result<&Snapshot, TickFailed> {
        self.wait_for_the_spare(life)?;
        self.ask();
        self.wait_for_the_spare(life)?;
        Ok(&self.latest)
    }

    fn wait_for_the_spare(&mut self, life: &EngineLife) -> Result<(), TickFailed> {
        while self.spare.is_none() {
            if !self.take_back() {
                if life.get() == Life::TickFailed {
                    return Err(TickFailed);
                }
                thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    }

    fn take_back(&mut self) -> bool {
        let Ok(picture) = self.pictures.pop() else {
            return false;
        };
        self.spare = Some(std::mem::replace(&mut self.latest, picture));
        true
    }

    fn ask(&mut self) {
        if let Some(spare) = self.spare.take()
            && let Err(rtrb::PushError::Full(spare)) = self.asks.push(spare)
        {
            self.spare = Some(spare);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Reader, hand_over};
    use crate::life::{EngineLife, LifeEvent, TickFailed, TickGuard};
    use crate::{Priority, Snapshot, Universes};
    use std::time::{Duration, Instant};

    #[test]
    #[expect(clippy::panic, reason = "the tick thread this test stands for unwinds")]
    fn a_fresh_picture_ends_when_the_tick_thread_unwound() {
        let (_hand, queues) = hand_over();
        let mut reader = Reader::new(queues, Snapshot::new(Universes::ONE, Priority::RealTime));
        let life = EngineLife::new();
        let unwound = std::thread::scope(|threads| {
            threads
                .spawn(|| {
                    let _unwinds = TickGuard(&life);
                    panic!("a tick that fails");
                })
                .join()
        });
        assert!(unwound.is_err());
        let started = Instant::now();
        assert_eq!(reader.fresh(&life).err(), Some(TickFailed));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_tick_that_ends_without_unwinding_is_no_failure() {
        let life = EngineLife::new();
        life.apply(LifeEvent::Drop);
        drop(TickGuard(&life));
        assert_eq!(life.get(), crate::life::Life::Ended);
    }
}
