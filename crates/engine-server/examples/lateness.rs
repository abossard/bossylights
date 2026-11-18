//! Runs the engine for 60 seconds with two DDP outputs at different rates (60 and 44 a
//! second) that send to a receiver on this machine. Prints, in microseconds, the worst
//! p99 lateness of the two, the worst lateness of the two, then the intervals off by
//! more than 2 ms on both together. Each output's numbers go to stderr. The scorecard
//! reads it: `cargo run -q -p engine-server --example lateness`.

use engine::{Color, Command, Engine, OutputKind, OutputNumber, Rate, RunNumbers, Universes};
use std::net::{SocketAddr, UdpSocket};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

const RUN: Duration = Duration::from_secs(60);

fn main() -> ExitCode {
    match run() {
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

fn run() -> Result<String, String> {
    let (engine, mut sender) = Engine::start(Universes::ONE).map_err(|e| e.to_string())?;
    let (address, stop) = drain().map_err(|e| e.to_string())?;
    let rates = [Rate::Sixty, Rate::FortyFour];
    let numbers = [1, 2].map(|n| OutputNumber::try_from(n).map_err(|_| "output number"));
    for (number, rate) in numbers.iter().zip(rates) {
        let command = Command::SetOutput {
            output: (*number)?,
            kind: OutputKind::Ddp {
                address,
                color: Color::Rgbw,
            },
            universes: RunNumbers { first: 1, count: 1 },
            rate: Some(rate),
        };
        sender.send(command).map_err(|r| format!("refused: {r:?}"))?;
    }
    thread::sleep(RUN);
    let state = engine.query();
    stop.store(true, Ordering::Relaxed);
    let (mut p99, mut worst, mut off) = (Duration::ZERO, Duration::ZERO, 0);
    for number in numbers {
        let number = number?;
        let lateness = engine.lateness(number).ok_or("no frame")?;
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
