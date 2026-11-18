//! Runs the engine for five seconds and prints how late 99 of 100 ticks woke up, in
//! microseconds. The scorecard reads it: `cargo run -q -p engine-server --example lateness`.

use engine::Engine;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

fn main() -> ExitCode {
    let p99 = Engine::start().map(|engine| {
        thread::sleep(Duration::from_secs(5));
        engine.lateness_p99()
    });
    match p99 {
        Ok(Some(p99)) => {
            println!("{}", p99.as_micros());
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("lateness: no tick in five seconds");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("lateness: the engine did not start: {error}");
            ExitCode::FAILURE
        }
    }
}
