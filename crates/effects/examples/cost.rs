//! What an effect costs per cell: 10,922 `Rgbw` cells per kind (the most one look may have),
//! 10,000 wakes at 60 a second and 120 bpm, on a thread with the engine's real-time policy.
//! Each wake runs the four kinds one after another: make the painter, paint every cell,
//! lower it to its four lane words. The offers into the mixer are not in it. Prints per kind
//! the p50 and p99 of a wake in nanoseconds per cell, and the p99 and worst of a wake in
//! microseconds. The system's answer on the policy goes to stderr. Takes about three minutes.
//!
//! `cargo run -q --release -p effects --example cost`

use effects::{Beats, CellKind, Effect, Kind, Place, Rgb, Size};
use std::hint::black_box;
use std::num::NonZeroU16;
use std::thread;
use std::time::{Duration, Instant};

const CELLS: u16 = 10_922;
const WIDTH: u16 = 128;
const WAKES: usize = 10_000;
const PERIOD: Duration = Duration::from_nanos(1_000_000_000 / 60);
/// 120 bpm at 60 wakes a second: 1/30 of a beat per wake, in Q32 beats.
const BEATS_PER_WAKE: u64 = (1 << 32) / 30;

fn main() {
    let (thread, priority) = rt_thread::spawn("effects_cost", |_| measure()).unwrap();
    eprintln!("priority: {priority:?}");
    thread.join().unwrap();
}

fn measure() {
    let colours = [
        Rgb {
            red: 65535,
            green: 0,
            blue: 0,
        },
        Rgb {
            red: 0,
            green: 65535,
            blue: 0,
        },
        Rgb {
            red: 0,
            green: 0,
            blue: 65535,
        },
        Rgb {
            red: 65535,
            green: 65535,
            blue: 65535,
        },
    ];
    let size = Size {
        width: NonZeroU16::new(WIDTH).unwrap(),
        height: NonZeroU16::new(CELLS.div_ceil(WIDTH)).unwrap(),
    };
    let places: Vec<Place> = (0..CELLS)
        .map(|i| Place {
            x: i % WIDTH,
            y: i / WIDTH,
            index: u32::from(i),
        })
        .collect();
    let mut lanes = vec![0u16; places.len() * 4];
    let effects = Kind::ALL.map(|kind| Effect::new(kind, 4, &colours, &[]).unwrap());
    let mut work = Kind::ALL.map(|_| Vec::with_capacity(WAKES));
    let first = Instant::now();
    for wake in 0..WAKES {
        thread::sleep((first + PERIOD * wake as u32).saturating_duration_since(Instant::now()));
        let beats = Beats(wake as u64 * BEATS_PER_WAKE);
        for (effect, work) in effects.iter().zip(&mut work) {
            let start = Instant::now();
            let painter = effect.painter(size, black_box(beats));
            for (place, out) in places.iter().zip(lanes.chunks_exact_mut(4)) {
                let colour = painter.paint(*place).unwrap_or_default();
                out.copy_from_slice(CellKind::Rgbw.lower(colour).as_slice());
            }
            black_box(&mut lanes);
            work.push(start.elapsed());
        }
    }
    println!("kind     ns per cell p50  p99   wake µs p99  worst");
    for (kind, mut work) in Kind::ALL.into_iter().zip(work) {
        work.sort_unstable();
        let at = |percent: usize| work[(work.len() * percent).div_ceil(100).max(1) - 1];
        let per_cell = |d: Duration| d.as_nanos() as f64 / f64::from(CELLS);
        println!(
            "{:<8} {:>16.1} {:>5.1} {:>14} {:>6}",
            format!("{kind:?}"),
            per_cell(at(50)),
            per_cell(at(99)),
            at(99).as_micros(),
            work[work.len() - 1].as_micros(),
        );
    }
}
