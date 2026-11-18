//! Runs the engine for 60 seconds and prints how late its ticks woke up, in
//! microseconds: the p99, then the worst. The scorecard reads it:
//! `cargo run -q -p engine-server --example lateness`.

use engine::{Engine, Universes};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

const RUN: Duration = Duration::from_secs(60);

fn main() -> ExitCode {
    let lateness = Engine::start(Universes::ONE, |_| {}).map(|(engine, _sender)| {
        thread::sleep(RUN);
        engine.lateness_p99().zip(engine.lateness_worst())
    });
    match lateness {
        Ok(Some((p99, worst))) => {
            println!("{} {}", p99.as_micros(), worst.as_micros());
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("lateness: no tick in {} seconds", RUN.as_secs());
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("lateness: the engine did not start: {error}");
            ExitCode::FAILURE
        }
    }
}
