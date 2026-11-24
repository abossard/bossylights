//! Runs the engine with DDP outputs that send to a receiver on this machine, at 60 and 44
//! a second in turn. Prints, in microseconds, the worst p99 lateness of the outputs, the
//! worst lateness of the outputs, then the intervals off by more than 2 ms on all of them
//! together. Each output's numbers go to stderr. A reader takes a snapshot on every frame,
//! as the server does. The scorecard reads it. An optional second argument overrides the run length in seconds.
//!
//! | Run | Outputs, universes each | For | Command |
//! |---|---|---|---|
//! | normal | 2, 1 | 60 s | `cargo run -q --release -p engine-server --example lateness` |
//! | at the limits the API allows | 16, 64 | 30 s | `cargo run -q --release -p engine-server --example lateness -- limits` |
//! | 10 000 lights, each a Generic RGB, on 59 of 64 universes | 1, 64 | 60 s | `cargo run -q --release -p engine-server --example lateness -- lights` |
//! | 10 000 pixels, one generic RGB strip over 59 of 64 universes | 1, 64 | 60 s | `cargo run -q --release -p engine-server --example lateness -- pixels` |

use engine::{
    Begun, ChannelNumbers, Color, Command, Engine, Fixture, MAX_OUTPUTS, MAX_UNIVERSES, OutputKind,
    OutputNumber, Rate, Refusal, RunNumbers, Sender, Target, Text, Universes,
};
use engine_core::Pixels;
use std::net::{SocketAddr, UdpSocket};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[path = "../src/report_line.rs"]
mod report_line;

/// How many outputs, how many universes each, for how long, with what patched.
struct Setup {
    outputs: u8,
    universes: u16,
    run: Duration,
    lights: Lights,
}

/// What is patched, each light lit in a colour of its own.
#[derive(Clone, Copy)]
enum Lights {
    /// This many Generic RGB, 170 to a universe.
    Pars(u16),
    /// One generic RGB strip of this many pixels from universe 1, address 1.
    Strip(u16),
}

const NORMAL: Setup = Setup {
    outputs: 2,
    universes: 1,
    run: Duration::from_secs(60),
    lights: Lights::Pars(0),
};

const LIMITS: Setup = Setup {
    outputs: MAX_OUTPUTS as u8,
    universes: MAX_UNIVERSES,
    run: Duration::from_secs(30),
    lights: Lights::Pars(0),
};

const LIGHTS: Setup = Setup {
    outputs: 1,
    universes: MAX_UNIVERSES,
    run: Duration::from_secs(60),
    lights: Lights::Pars(10_000),
};

const PIXELS: Setup = Setup {
    outputs: 1,
    universes: MAX_UNIVERSES,
    run: Duration::from_secs(60),
    lights: Lights::Strip(10_000),
};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut setup = match args.next().as_deref() {
        None | Some("normal") => NORMAL,
        Some("limits") => LIMITS,
        Some("lights") => LIGHTS,
        Some("pixels") => PIXELS,
        Some(other) => {
            eprintln!(
                "lateness: unknown run {other:?}; none, \"limits\", \"lights\" or \"pixels\""
            );
            return ExitCode::FAILURE;
        }
    };
    if let Some(seconds) = args.next() {
        let Some(seconds) = seconds.parse::<std::num::NonZeroU64>().ok() else {
            eprintln!("lateness: expected a positive number of seconds");
            return ExitCode::FAILURE;
        };
        setup.run = Duration::from_secs(seconds.get());
    }
    if args.next().is_some() {
        eprintln!("usage: lateness [normal|limits|lights|pixels] [seconds]");
        return ExitCode::FAILURE;
    }
    match run(&setup) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(problem) => {
            eprintln!("lateness: {problem}");
            ExitCode::FAILURE
        }
    }
}

fn run(setup: &Setup) -> Result<String, String> {
    let universes = Universes::new(setup.universes).ok_or("universes")?;
    let (mut engine, mut sender) = Engine::start(universes).map_err(|e| e.to_string())?;
    let (address, stop) = drain().map_err(|e| e.to_string())?;
    let rates = [Rate::Sixty, Rate::FortyFour].into_iter().cycle();
    let numbers: Vec<_> = (1..=setup.outputs)
        .map(|n| OutputNumber::try_from(n).map_err(|_| "output number"))
        .collect();
    let set_outputs = |sender: &mut Sender| -> Result<(), String> {
        for (number, rate) in numbers.iter().zip(rates.clone()) {
            let command = Command::SetOutput {
                output: (*number)?,
                kind: OutputKind::Ddp {
                    address,
                    color: Color::Rgbw,
                },
                universes: RunNumbers {
                    first: 1,
                    count: setup.universes,
                },
                rate: Some(rate),
            };
            sender
                .send(command)
                .map_err(|r| format!("refused: {r:?}"))?;
        }
        Ok(())
    };
    // The outputs first: a fixture over universes needs one that streams them. Set again
    // once the lights are patched, their counters start at zero: the lateness is that of
    // a patch that stands, not one being made.
    set_outputs(&mut sender)?;
    patch(&mut sender, setup.lights)?;
    set_outputs(&mut sender)?;
    engine.fresh().unwrap();
    // A reader on every frame: it asks again as soon as a picture is back.
    let (started, mut seen, mut pictures) = (Instant::now(), 0, 0_u64);
    while started.elapsed() < setup.run {
        let frame = engine.latest().frame();
        if frame != seen {
            (seen, pictures) = (frame, pictures + 1);
        }
        thread::sleep(Duration::from_millis(1));
    }
    eprintln!("snapshots read: {pictures}, frames: {seen}");
    let picture = engine.fresh().unwrap().clone();
    let state = picture.performance();
    stop.store(true, Ordering::Relaxed);
    let (mut p99, mut worst, mut off) = (Duration::ZERO, Duration::ZERO, 0);
    for number in numbers {
        let number = number?;
        let lateness = picture.lateness(number).ok_or("no frame")?;
        let output = state.attached_output(number).ok_or("no output")?;
        let c = output.counters;
        eprintln!(
            "output {} at {}/s: {}",
            number.slot() + 1,
            output.frames_per_second(),
            report_line::counts(c, lateness),
        );
        p99 = p99.max(lateness.p99);
        worst = worst.max(lateness.worst);
        off += c.off_beat;
    }
    Ok(format!("{} {} {off}", p99.as_micros(), worst.as_micros()))
}

/// Patches `lights`, each lit in a colour of its own.
fn patch(sender: &mut Sender, lights: Lights) -> Result<(), String> {
    let library: Vec<Fixture> = vec![
        fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB)
            .map_err(|e| format!("{e:?}"))?,
    ];
    let text = |text| Text::new(text).ok_or("a short text");
    let mut send = |command, n| {
        // A full queue empties within a wake: send again.
        loop {
            match sender.begin(command, &library) {
                Ok(
                    Begun::Queued
                    | Begun::Patched { .. }
                    | Begun::Started { .. }
                    | Begun::Grabbed { .. },
                ) => return Ok(()),
                Err(Refusal::QueueFull) => thread::sleep(Duration::from_millis(1)),
                Ok(Begun::Opening(_)) => return Err("a light opened a device".to_string()),
                Err(refused) => return Err(format!("light {n}: refused: {refused:?}")),
            }
        }
    };
    let colour = |target, n: u16| Command::SetColour {
        target,
        red: n.wrapping_mul(97),
        green: n.wrapping_mul(193),
        blue: n.wrapping_mul(389),
    };
    let full = |target| Command::SetIntensity {
        target,
        intensity: u16::MAX,
    };
    let started = Instant::now();
    let count = match lights {
        Lights::Pars(count) => {
            for n in 0..count {
                let target = Target::Fixture { fixture: n + 1 };
                let par = Command::Patch {
                    name: text("PAR")?,
                    maker: text("Generic")?,
                    model: text("Generic RGB")?,
                    mode: text("RGB")?,
                    at: ChannelNumbers {
                        universe: 1 + n / 170,
                        address: 1 + 3 * (n % 170),
                    },
                };
                for command in [par, full(target), colour(target, n)] {
                    send(command, n)?;
                }
            }
            count
        }
        Lights::Strip(count) => {
            let strip = Command::PatchStrip {
                name: text("Strip")?,
                pixels: Pixels::try_from(count)?,
                color: Color::Rgb,
                at: ChannelNumbers {
                    universe: 1,
                    address: 1,
                },
            };
            send(strip, 0)?;
            send(full(Target::Fixture { fixture: 1 }), 0)?;
            for n in 0..count {
                send(colour(Target::Light { light: n + 1 }, n), n)?;
            }
            count
        }
    };
    if count > 0 {
        eprintln!(
            "patched {count} lights in {} ms",
            started.elapsed().as_millis()
        );
    }
    Ok(())
}

/// The test definition of a Generic RGB, read when the example is built.
const GENERIC_RGB: &[u8] =
    include_bytes!("../../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");

/// A receiver on this machine that reads and forgets every packet, so no socket fills.
fn drain() -> std::io::Result<(std::net::SocketAddrV4, Arc<AtomicBool>)> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    socket.set_read_timeout(Some(Duration::from_millis(50)))?;
    let SocketAddr::V4(address) = socket.local_addr()? else {
        return Err(std::io::Error::other("IPv4"));
    };
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&stop);
    thread::spawn(move || {
        let mut packet = [0; 1500];
        while !stopping.load(Ordering::Relaxed) {
            let _ = socket.recv(&mut packet);
        }
    });
    Ok((address, stop))
}
