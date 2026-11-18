//! A device written in a test, through the same door every output kind uses: the
//! `Device` trait and `Sender::attach`. It takes every frame, counts it and keeps the
//! last one (up to two universes) in atomics.

#![allow(dead_code, reason = "each test binary uses only part of it")]

use engine::{
    ADDRESSES, Device, Levels, Outcome, OutputNumber, Rate, RunNumbers, Sender, Universes,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub struct Heard {
    frames: AtomicU64,
    levels: [AtomicU8; 2 * ADDRESSES],
}

impl Heard {
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// The level of a channel in the last frame, counted from 1 within the run.
    pub fn level(&self, universe: u16, address: u16) -> u8 {
        let at = usize::from(universe - 1) * ADDRESSES + usize::from(address - 1);
        self.levels[at].load(Ordering::Relaxed)
    }

    /// Waits until two more frames arrived: the one that may have been computed while
    /// the commands were sent, and the next one.
    pub fn two_more_frames(&self) {
        let before = self.frames();
        let started = Instant::now();
        while self.frames() < before + 2 {
            assert!(started.elapsed() < Duration::from_secs(2), "no frames");
            thread::sleep(Duration::from_millis(1));
        }
    }
}

struct Mock(Arc<Heard>);

impl Device for Mock {
    fn take(&mut self, frame: &[Levels]) -> Outcome {
        let levels = frame.iter().flat_map(|universe| universe.0.iter());
        for (cell, level) in self.0.levels.iter().zip(levels) {
            cell.store(*level, Ordering::Relaxed);
        }
        self.0.frames.fetch_add(1, Ordering::Relaxed);
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
    let heard = Arc::new(Heard {
        frames: AtomicU64::new(0),
        levels: [const { AtomicU8::new(0) }; 2 * ADDRESSES],
    });
    let output = OutputNumber::try_from(output).unwrap();
    let run = run.check(universes).unwrap();
    let mock = Box::new(Mock(Arc::clone(&heard)));
    sender.attach(output, mock, run, rate).unwrap();
    heard
}
