//! Runs the engine with DDP outputs that send to a receiver on this machine, at 60 and 44
//! a second in turn. Prints, in microseconds, the worst p99 lateness of the outputs, the
//! worst lateness of the outputs, then the intervals off by more than 2 ms on all of them
//! together. Each output's numbers go to stderr. A reader takes a snapshot on every frame,
//! as the server does. The scorecard reads it.
//!
//! | Run | Outputs, universes each | For | Command |
//! |---|---|---|---|
//! | normal | 2, 1 | 60 s | `cargo run -q --release -p engine-server --example lateness` |
//! | at the limits the API allows | 16, 64 | 30 s | `cargo run -q --release -p engine-server --example lateness -- limits` |
//! | 10 000 lights, each a Generic RGB, on 59 of 64 universes | 1, 64 | 60 s | `cargo run -q --release -p engine-server --example lateness -- lights` |

use engine::{
    Begun, ChannelNumbers, Color, Command, Engine, Fixture, MAX_OUTPUTS, MAX_UNIVERSES, OutputKind,
    OutputNumber, Rate, Refusal, RunNumbers, Sender, Target, Text, Universes,
};
use std::net::{SocketAddr, UdpSocket};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// How many outputs, how many universes each, for how long, with how many lights patched.
struct Setup {
    outputs: u8,
    universes: u16,
    run: Duration,
    lights: u16,
}

const NORMAL: Setup = Setup {
    outputs: 2,
    universes: 1,
    run: Duration::from_secs(60),
    lights: 0,
};

const LIMITS: Setup = Setup {
    outputs: MAX_OUTPUTS as u8,
    universes: MAX_UNIVERSES,
    run: Duration::from_secs(30),
    lights: 0,
};

const LIGHTS: Setup = Setup {
    outputs: 1,
    universes: MAX_UNIVERSES,
    run: Duration::from_secs(60),
    lights: 10_000,
};

fn main() -> ExitCode {
    let setup = match std::env::args().nth(1).as_deref() {
        None => NORMAL,
        Some("limits") => LIMITS,
        Some("lights") => LIGHTS,
        Some(other) => {
            eprintln!("lateness: unknown run {other:?}; none, \"limits\" or \"lights\"");
            return ExitCode::FAILURE;
        }
    };
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
    // The lights first: their lateness is that of a patch that stands, not one being made.
    patch(&mut sender, setup.lights)?;
    engine.fresh();
    let (address, stop) = drain().map_err(|e| e.to_string())?;
    let rates = [Rate::Sixty, Rate::FortyFour].into_iter().cycle();
    let numbers: Vec<_> = (1..=setup.outputs)
        .map(|n| OutputNumber::try_from(n).map_err(|_| "output number"))
        .collect();
    for (number, rate) in numbers.iter().zip(rates) {
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
    // A reader on every frame: it asks again as soon as a picture is back.
    let (started, mut seen, mut pictures) = (Instant::now(), 0, 0_u64);
    while started.elapsed() < setup.run {
        let frame = engine.latest().state.frame;
        if frame != seen {
            (seen, pictures) = (frame, pictures + 1);
        }
        thread::sleep(Duration::from_millis(1));
    }
    eprintln!("snapshots read: {pictures}, frames: {seen}");
    let picture = *engine.fresh();
    let state = picture.state;
    stop.store(true, Ordering::Relaxed);
    let (mut p99, mut worst, mut off) = (Duration::ZERO, Duration::ZERO, 0);
    for number in numbers {
        let number = number?;
        let lateness = picture.lateness(number).ok_or("no frame")?;
        let output = state
            .outputs
            .get(number.slot())
            .copied()
            .flatten()
            .ok_or("no output")?;
        let c = output.counters;
        eprintln!(
            "output {} at {}/s: accepted {}, dropped {}, failed {}, off by > 2 ms {}, lateness p99 {} µs, worst {} µs",
            number.slot() + 1,
            output.rate.per_second(),
            c.accepted,
            c.dropped,
            c.failed,
            c.off_beat,
            lateness.p99.as_micros(),
            lateness.worst.as_micros(),
        );
        p99 = p99.max(lateness.p99);
        worst = worst.max(lateness.worst);
        off += c.off_beat;
    }
    Ok(format!("{} {} {off}", p99.as_micros(), worst.as_micros()))
}

/// Patches `lights` Generic RGB, 170 to a universe, each lit in a colour of its own.
fn patch(sender: &mut Sender, lights: u16) -> Result<(), String> {
    let library: Vec<Fixture> = vec![
        fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB)
            .map_err(|e| format!("{e:?}"))?,
    ];
    let text = |text| Text::new(text).ok_or("a short text");
    let started = Instant::now();
    for n in 0..lights {
        let target = Target::Fixture { fixture: n + 1 };
        let commands = [
            Command::Patch {
                name: text("PAR")?,
                maker: text("Generic")?,
                model: text("Generic RGB")?,
                mode: text("RGB")?,
                at: ChannelNumbers {
                    universe: 1 + n / 170,
                    address: 1 + 3 * (n % 170),
                },
            },
            Command::SetIntensity {
                target,
                intensity: u16::MAX,
            },
            Command::SetColour {
                target,
                red: n.wrapping_mul(97),
                green: n.wrapping_mul(193),
                blue: n.wrapping_mul(389),
            },
        ];
        for command in commands {
            // A full queue empties within a wake: send again.
            loop {
                match sender.begin(command, &library) {
                    Ok(Begun::Queued | Begun::Patched { .. }) => break,
                    Err(Refusal::QueueFull) => thread::sleep(Duration::from_millis(1)),
                    Ok(Begun::Opening(_)) => return Err("a light opened a device".into()),
                    Err(refused) => return Err(format!("light {n}: refused: {refused:?}")),
                }
            }
        }
    }
    if lights > 0 {
        eprintln!(
            "patched {lights} lights in {} ms",
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
