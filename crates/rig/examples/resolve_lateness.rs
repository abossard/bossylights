//! Mixes 10 000 RGB lights on 64 universes at 60 frames a second, on a thread with the
//! engine's real-time policy. The rig gives each light its fields, off the thread. Each
//! frame one source offers a new word on every field, as a play sampled before the frame,
//! then the core's `frame` writes the channels. Prints, in microseconds: the p99 of how
//! late a frame was done (mixed, after it was due), the worst, and the p99 of the work
//! itself. The system's answer on the policy goes to stderr.
//!
//! `cargo run -q --release -p rig --example resolve_lateness -- [seconds, 60 if none]`

use engine_core::{
    ChannelNumbers, ChannelSet, Channels, Entry, Field, GRABS, Grab, Levels, LightNumber, Lights,
    MAX_LIGHTS, MAX_UNIVERSES, Offering, PLAYS, Priority, SOURCES, State, Universes, frame,
    next_frame, sleep_before,
};
use std::hint;
use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

const LIGHTS: u16 = 10_000;
const PERIOD: Duration = Duration::from_nanos(1_000_000_000 / 60);

fn main() -> ExitCode {
    let seconds = std::env::args().nth(1).map_or(Ok(60), |s| s.parse::<u64>());
    match seconds.map_err(|e| e.to_string()).and_then(run) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(problem) => {
            eprintln!("resolve_lateness: {problem}");
            ExitCode::FAILURE
        }
    }
}

fn run(seconds: u64) -> Result<String, String> {
    let fields = patch()?;
    let frames = seconds * 60;
    // made here: the thread's stack need not hold the channels
    let channels = Box::new(Channels::NEW);
    let (thread, priority) = rt_thread::spawn("resolve_lateness", move |_| {
        frames_of(&fields, channels, frames)
    })
    .map_err(|e| e.to_string())?;
    eprintln!("priority: {priority:?}, {frames} frames of {LIGHTS} lights");
    let (mut late, mut work) = thread.join().map_err(|_| "the thread panicked")?;
    late.sort_unstable();
    work.sort_unstable();
    let p99 = |sorted: &[Duration]| {
        let rank = (sorted.len() * 99).div_ceil(100).max(1) - 1;
        sorted.get(rank).copied().unwrap_or_default().as_micros()
    };
    let worst = late.last().copied().unwrap_or_default().as_micros();
    Ok(format!("{} {worst} {}", p99(&late), p99(&work)))
}

/// The fields of 10 000 RGB PARs of three channels, 170 to a universe, from address 1 of
/// universe 1, light after light.
fn patch() -> Result<Vec<Field>, String> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");
    let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
    let fixture =
        fixture_library::parse("Generic-Generic-RGB.qxf", &bytes).map_err(|e| format!("{e:?}"))?;
    let mode = fixture
        .modes
        .iter()
        .find(|m| m.name == "RGB")
        .ok_or("no RGB mode")?;
    let universes = Universes::new(MAX_UNIVERSES).ok_or("universes")?;
    let lights = Lights::new(MAX_LIGHTS).ok_or("lights")?;
    let mut fields = Vec::new();
    for n in 0..LIGHTS {
        let at = ChannelNumbers {
            universe: 1 + n / 170,
            address: 1 + n % 170 * 3,
        };
        let first = LightNumber::new(n, lights).ok_or("light number")?;
        let rigged = rig::compile(mode, at, first, universes, lights)
            .map_err(|e| format!("light {n}: {e:?}"))?;
        for target in rigged.targets {
            fields.extend(target.fields);
        }
    }
    Ok(fields)
}

/// Runs `frames` frames as the engine's tick does: sleep, spin the last millisecond, then
/// the frame's work. How late each frame was done, and how long its work took.
fn frames_of(
    fields: &[Field],
    mut channels: Box<Channels>,
    frames: u64,
) -> (Vec<Duration>, Vec<Duration>) {
    let state = State::new(Priority::RealTime);
    let raw = vec![Levels::ZERO; usize::from(MAX_UNIVERSES)];
    let holds = vec![ChannelSet::NONE; usize::from(MAX_UNIVERSES)];
    // the one source: the first grab entry, after the plays
    let mut grabs: Vec<Option<Grab>> = vec![None; GRABS];
    grabs[0] = Some(Grab {
        order: 1,
        kept: false,
    });
    let mut sources: Vec<Option<Entry>> = vec![None; SOURCES];
    let mut values: Vec<Option<u16>> = vec![None; fields.len()];
    let capacity = usize::try_from(frames).unwrap_or(0);
    let (mut late, mut work) = (Vec::with_capacity(capacity), Vec::with_capacity(capacity));
    let start = Instant::now();
    let mut due = PERIOD;
    for frame_number in 0..frames {
        thread::sleep(sleep_before(due, start.elapsed()));
        while start.elapsed() < due {
            hint::spin_loop();
        }
        let woke = start.elapsed();
        let step = frame_number as u16;
        for (n, value) in (0..).zip(values.iter_mut()) {
            *value = Some(step.wrapping_mul(97).wrapping_add(n));
        }
        let offering = |entry: usize| {
            if entry == PLAYS {
                Offering::Sampled {
                    fields,
                    values: &values,
                }
            } else {
                Offering::NOTHING
            }
        };
        frame(
            &state,
            (&raw, &holds),
            &grabs,
            &mut sources,
            offering,
            &mut channels,
        );
        hint::black_box(channels.values());
        let done = start.elapsed();
        late.push(done.saturating_sub(due));
        work.push(done.saturating_sub(woke));
        due = next_frame(due, done, PERIOD);
    }
    (late, work)
}
