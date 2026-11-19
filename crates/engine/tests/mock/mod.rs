//! A device written in a test, through the same door every output kind uses: the
//! `Device` trait and `Sender::attach`. It takes every frame, counts it and keeps the
//! last one (up to two universes) in atomics, and the first channel of each frame and how
//! many of its channels are not zero.

#![allow(dead_code, reason = "each test binary uses only part of it")]

use engine::{
    ADDRESSES, Device, Levels, Outcome, OutputNumber, Rate, RunNumbers, Sender, Universes,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Frames whose first channel is kept: the last this many.
const HISTORY: usize = 4096;

pub struct Heard {
    frames: AtomicU64,
    levels: [AtomicU8; 2 * ADDRESSES],
    /// The first channel of frame `n` (counted from 1) at `n % HISTORY`.
    history: [AtomicU8; HISTORY],
    /// The channels not zero in frame `n` at `n % HISTORY`.
    lit: [AtomicUsize; HISTORY],
}

impl Heard {
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Acquire)
    }

    /// The level of a channel in the last frame, counted from 1 within the run.
    pub fn level(&self, universe: u16, address: u16) -> u8 {
        let at = usize::from(universe - 1) * ADDRESSES + usize::from(address - 1);
        self.levels[at].load(Ordering::Relaxed)
    }

    /// The first channel of frame `n`, counted from 1, if it is one of the last kept.
    pub fn first_channel_of(&self, n: u64) -> Option<u8> {
        let kept = (1..=self.frames()).contains(&n) && self.frames() - n < HISTORY as u64;
        kept.then(|| self.history[(n % HISTORY as u64) as usize].load(Ordering::Relaxed))
    }

    /// How many channels of frame `n` are not zero, counted from 1, if it is one of the
    /// last kept.
    pub fn lit_of(&self, n: u64) -> Option<usize> {
        let kept = (1..=self.frames()).contains(&n) && self.frames() - n < HISTORY as u64;
        kept.then(|| self.lit[(n % HISTORY as u64) as usize].load(Ordering::Relaxed))
    }

    /// Waits until two more frames arrived: the one that may have been computed while
    /// the commands were sent, and the next one.
    pub fn two_more_frames(&self) {
        let before = self.frames();
        let started = Instant::now();
        while self.frames() < before + 2 {
            assert!(started.elapsed() < Duration::from_secs(2), "no frames");
            thread::sleep(Duration::from_millis(1)); // poll
        }
    }
}

/// Waits until `done` holds. A hang guard of seconds, never a rate or a duration.
pub fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(10), "{what}");
        thread::sleep(Duration::from_millis(1)); // poll
    }
}

struct Mock(Arc<Heard>);

impl Device for Mock {
    fn take(&mut self, frame: &[Levels]) -> Outcome {
        let levels = frame.iter().flat_map(|universe| universe.0.iter());
        for (cell, level) in self.0.levels.iter().zip(levels) {
            cell.store(*level, Ordering::Relaxed);
        }
        let n = self.0.frames.load(Ordering::Relaxed) + 1;
        let first = frame.first().map_or(0, |universe| universe.0[0]);
        self.0.history[(n % HISTORY as u64) as usize].store(first, Ordering::Relaxed);
        let lit = frame
            .iter()
            .flat_map(|u| u.0.iter())
            .filter(|l| **l > 0)
            .count();
        self.0.lit[(n % HISTORY as u64) as usize].store(lit, Ordering::Relaxed);
        self.0.frames.store(n, Ordering::Release);
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
        history: [const { AtomicU8::new(0) }; HISTORY],
        lit: [const { AtomicUsize::new(0) }; HISTORY],
    });
    let output = OutputNumber::try_from(output).unwrap();
    let run = run.check(universes).unwrap();
    let mock = Box::new(Mock(Arc::clone(&heard)));
    sender.attach(output, mock, run, rate).unwrap();
    heard
}
