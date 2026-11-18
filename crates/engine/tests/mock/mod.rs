//! A device written in a test, through the same door DDP uses: the `Device` trait and
//! `Sender::attach`. It takes every frame, counts it and keeps channel 1 of its run.

use engine::{Device, Levels, Outcome, OutputNumber, Rate, RunNumbers, Sender, Universes};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

#[derive(Default)]
pub struct Heard {
    pub frames: AtomicU64,
    pub first: AtomicU8,
}

pub struct Mock(Arc<Heard>);

impl Device for Mock {
    fn take(&mut self, frame: &[Levels]) -> Outcome {
        self.0.frames.fetch_add(1, Ordering::Relaxed);
        if let Some(universe) = frame.first() {
            self.0.first.store(universe.0[0], Ordering::Relaxed);
        }
        Outcome::Accepted
    }
}

/// Attaches a mock as `output` for the run, and returns what it hears.
pub fn attach(
    sender: &mut Sender,
    universes: Universes,
    output: u8,
    run: RunNumbers,
    rate: Rate,
) -> Arc<Heard> {
    let heard = Arc::new(Heard::default());
    let output = OutputNumber::try_from(output).unwrap();
    let run = run.check(universes).unwrap();
    let mock = Box::new(Mock(Arc::clone(&heard)));
    sender.attach(output, mock, run, rate).unwrap();
    heard
}
