//! Sends "set channel" at uneven times for ten seconds and prints, in microseconds,
//! how long 95 of 100 commands took from `send` to the frame that carried them.
//! The scorecard reads it: `cargo run -q -p engine-server --example send_to_frame`.

use engine::{ChannelNumbers, Command, Engine, Universes};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

const RUN: Duration = Duration::from_secs(10);

fn main() -> ExitCode {
    let Ok((engine, mut sender)) = Engine::start(Universes::ONE, |_| {}) else {
        eprintln!("send_to_frame: the engine did not start");
        return ExitCode::FAILURE;
    };
    let started = Instant::now();
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut value = 0_u8;
    while started.elapsed() < RUN {
        // Uneven gaps of 5 to 36 ms, so sends land everywhere within a frame.
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        thread::sleep(Duration::from_millis(5 + seed % 32));
        value = value.wrapping_add(1);
        let channel = ChannelNumbers {
            universe: 1,
            address: 1,
        };
        if let Err(refusal) = sender.send(Command::SetChannel { channel, value }) {
            eprintln!("send_to_frame: refused: {refusal:?}");
            return ExitCode::FAILURE;
        }
    }
    thread::sleep(Duration::from_millis(100)); // the last frame
    match engine.send_to_frame_p95() {
        Some(p95) => {
            println!("{}", p95.as_micros());
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("send_to_frame: no command reached a frame");
            ExitCode::FAILURE
        }
    }
}
