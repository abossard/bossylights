//! One engine frame at 60 a second, on a thread with the engine's real-time policy, for a
//! number of RGB PARs (10 000 if none given), 170 to a universe from the first address.
//! Each frame every light takes the next of four values: the frame resets the patched
//! channels to their rest, one grab offers each light's words on its fields through the
//! field table, the Mixer decides each channel, Blackout goes on every other frame. Prints the p99 of how late
//! a frame was done (after it was due) and the worst, in microseconds, and the p99 of the
//! work itself in nanoseconds. The system's answer on the policy goes to stderr.
//!
//! The work follows the rig, not the size of the space: run it with 1 PAR and with 10 000.
//!
//! `cargo run -q --release -p rig --example frame_lateness -- [seconds, 60] [PARs, 10000]`

use engine_core::{
    ChannelNumbers, ChannelSet, Channels, EngineCommand, GrabChange, GrabEntry, Grabs, Levels,
    Light, LightNumber, LightRun, Lights, MAX_LIGHTS, MAX_UNIVERSES, Priority, Rgb, State,
    Universes, frame, next_frame, sleep_before, update,
};
use rig::{FieldPlan, Patch, Placed, Whole, Words};
use std::hint;
use std::num::NonZeroU16;
use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

const PERIOD: Duration = Duration::from_nanos(1_000_000_000 / 60);

fn main() -> ExitCode {
    let arg = |n: usize, none: u64| {
        std::env::args()
            .nth(n)
            .map_or(Ok(none), |s| s.parse::<u64>())
    };
    let asked = arg(1, 60).and_then(|seconds| Ok((seconds, arg(2, 10_000)?)));
    match asked.map_err(|e| e.to_string()).and_then(run) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(problem) => {
            eprintln!("frame_lateness: {problem}");
            ExitCode::FAILURE
        }
    }
}

fn run((seconds, pars): (u64, u64)) -> Result<String, String> {
    let pars = u16::try_from(pars).map_err(|e| e.to_string())?;
    let rig = patch(pars)?;
    let frames = seconds * 60;
    // made here: the space does not fit on the stack of the thread that runs the frames
    let mut channels = Box::new(Channels::NEW);
    channels.configure(&[], rig.0.configured());
    // a real-time thread: one at a time on the machine, as an engine (#319)
    let _slot = engine_slot::take().map_err(|e| e.to_string())?;
    let (thread, priority) = rt_thread::spawn("frame_lateness", move |_| {
        frames_of(&rig, &mut channels, frames)
    })
    .map_err(|e| e.to_string())?;
    eprintln!("priority: {priority:?}, {frames} frames of {pars} PARs");
    let (mut late, mut work) = thread.join().map_err(|_| "the thread panicked")?;
    late.sort_unstable();
    work.sort_unstable();
    let p99 = |sorted: &[Duration]| {
        let rank = (sorted.len() * 99).div_ceil(100).max(1) - 1;
        sorted.get(rank).copied().unwrap_or_default()
    };
    let worst = late.last().copied().unwrap_or_default().as_micros();
    Ok(format!(
        "{} {worst} {}",
        p99(&late).as_micros(),
        p99(&work).as_nanos()
    ))
}

/// `pars` RGB PARs of three channels, 170 to a universe: the field table, and the words
/// of every light in each of four values.
fn patch(pars: u16) -> Result<(FieldPlan, Vec<Words>), String> {
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
    let placed = (0..pars)
        .map(|n| {
            Some(Placed {
                fixture: NonZeroU16::new(n.checked_add(1)?)?,
                name: format!("PAR {n}"),
                maker: fixture.maker.clone(),
                model: fixture.model.clone(),
                mode: mode.clone(),
                at: ChannelNumbers {
                    universe: 1 + n / 170,
                    address: 1 + n % 170 * 3,
                },
                first: n.checked_add(1)?,
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or("fixture number")?;
    let patch = Patch::new(universes, lights, Whole(|_| NonZeroU16::MIN))
        .restore(placed)
        .map_err(|e| format!("{e:?}"))?;
    let every = LightNumber::new(0, lights)
        .and_then(|first| LightRun::new(first, pars, lights))
        .ok_or("lights")?;
    let words = (0..4u16)
        .map(|k| {
            let step = k.wrapping_mul(97);
            let value = Light {
                intensity: Some(step),
                colour: Some(Rgb {
                    red: step.wrapping_mul(3),
                    green: step.wrapping_mul(5),
                    blue: step.wrapping_mul(7),
                }),
                ..Light::DARK
            };
            patch.words(&[every], &value)
        })
        .collect();
    Ok((patch.prepare(), words))
}

/// Runs `frames` frames as the engine's tick does: sleep, spin the last millisecond, then
/// the frame's work. How late each frame was done, and how long its work took.
fn frames_of(
    (plan, words): &(FieldPlan, Vec<Words>),
    channels: &mut Channels,
    frames: u64,
) -> (Vec<Duration>, Vec<Duration>) {
    let mut raw = vec![Levels::ZERO; usize::from(MAX_UNIVERSES)];
    let mut holds = vec![ChannelSet::NONE; usize::from(MAX_UNIVERSES)];
    let mut grabs = Box::new(Grabs::NONE);
    let mut state = State::new(Priority::RealTime);
    let Some(entry) = GrabEntry::new(0) else {
        return (Vec::new(), Vec::new());
    };
    let start_grab = EngineCommand::Grab {
        entry,
        change: GrabChange::Start,
    };
    state = update(state, &mut raw, &mut holds, &mut grabs, start_grab);
    let targets = plan.targets();
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
        let blackout = EngineCommand::Blackout {
            on: frame_number % 2 == 1,
        };
        state = update(state, &mut raw, &mut holds, &mut grabs, blackout);
        let now = words.get(frame_number as usize % words.len());
        frame(
            &state,
            &grabs,
            (&raw, &holds),
            (plan.configured(), plan.followers()),
            channels,
            |offers| {
                if let Some(now) = now {
                    now.offer(targets, |field, word| offers.grab(entry, field, word));
                }
            },
        );
        hint::black_box(channels.values());
        let done = start.elapsed();
        late.push(done.saturating_sub(due));
        work.push(done.saturating_sub(woke));
        due = next_frame(due, done, PERIOD);
    }
    (late, work)
}
