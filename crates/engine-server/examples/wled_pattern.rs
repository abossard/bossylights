//! Sends a moving pattern to a WLED device or simulator over DDP, for 30 seconds:
//! four dots (red, green, blue, white) chase along 320 RGBW pixels.
//!
//! `cargo run -p engine-server --example wled_pattern -- 192.168.1.50` (port 4048), or
//! `-- 127.0.0.1:4048`, optionally followed by the seconds to run.
//!
//! It drives the engine like any client: one `SetOutput`, then `SetChannel` and
//! `ReleaseChannel` commands. At the end it prints the output's state and counters.

use engine::{
    ChannelNumbers, Color, Command, Engine, OutputKind, OutputNumber, RunNumbers, Universes,
};
use std::net::SocketAddrV4;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

const PIXELS: u16 = 320;
const USAGE: &str = "usage: wled_pattern <ip>[:port] [seconds]";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("wled_pattern: {problem}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let target = args.first().ok_or("no address")?;
    let address: SocketAddrV4 = target
        .parse()
        .or_else(|_| format!("{target}:4048").parse())
        .map_err(|_| format!("not an IPv4 address: {target}"))?;
    let seconds: u64 = args
        .get(1)
        .map_or(Ok(30), |s| s.parse())
        .map_err(|_| "seconds")?;

    // 320 RGBW pixels are 1280 channels: three universes, sent as one stream.
    let universes = Universes::new(3).ok_or("universes")?;
    let (engine, mut sender) = Engine::start(universes).map_err(|e| e.to_string())?;
    let output = OutputNumber::try_from(1).map_err(|_| "output")?;
    let add = Command::SetOutput {
        output,
        kind: OutputKind::Ddp {
            address,
            color: Color::Rgbw,
        },
        universes: RunNumbers { first: 1, count: 3 },
        rate: None,
    };
    sender.send(add).map_err(|r| format!("refused: {r:?}"))?;

    // (colour channels of a pixel, starting pixel, pixels per step)
    let dots: [(&[u16], u16, u16); 4] = [
        (&[0], 0, 1),
        (&[1], 80, 2),
        (&[2], 160, 3),
        (&[0, 1, 2], 240, 1),
    ];
    let started = Instant::now();
    let mut step: u32 = 0;
    while started.elapsed() < Duration::from_secs(seconds) {
        for (colours, first, speed) in dots {
            for (pixel, value) in [
                (pixel_at(first, step, speed), 0),
                (pixel_at(first, step + 1, speed), 255),
            ] {
                for colour in colours {
                    let command = if value == 0 {
                        Command::ReleaseChannel {
                            channel: channel(pixel, *colour),
                        }
                    } else {
                        Command::SetChannel {
                            channel: channel(pixel, *colour),
                            value,
                        }
                    };
                    sender
                        .send(command)
                        .map_err(|r| format!("refused: {r:?}"))?;
                }
            }
        }
        step += 1;
        thread::sleep(Duration::from_millis(25));
    }
    let state = engine.query();
    let sent = state
        .outputs
        .first()
        .copied()
        .flatten()
        .ok_or("no output")?;
    eprintln!(
        "{:?} at {}/s: {:?}",
        sent.status,
        sent.rate.per_second(),
        sent.counters
    );
    Ok(())
}

/// Where a dot is after `step` steps: it chases along the pixels and starts again at the first.
fn pixel_at(first: u16, step: u32, speed: u16) -> u16 {
    let pixel = (u64::from(first) + u64::from(step) * u64::from(speed)) % u64::from(PIXELS);
    u16::try_from(pixel).unwrap_or_default()
}

/// The channel of a pixel's colour (0 red, 1 green, 2 blue, 3 white), counted from 1.
fn channel(pixel: u16, colour: u16) -> ChannelNumbers {
    let n = (pixel % PIXELS) * 4 + colour;
    ChannelNumbers {
        universe: n / 512 + 1,
        address: n % 512 + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // first pixel, step, pixels per step, the pixel
    #[test]
    fn a_dot_wraps_around_the_pixels_however_long_the_run() {
        let cases = [
            (0, 0, 1, 0),
            (0, 319, 1, 319),
            (0, 320, 1, 0),
            (80, 2, 2, 84),
            (160, 100, 3, 140),
            (240, 21_845, 3, 175),
            (240, 100_000, 3, 80),
            (0, u32::MAX, 3, 125),
        ];
        for (first, step, speed, pixel) in cases {
            assert_eq!(
                pixel_at(first, step, speed),
                pixel,
                "{first} {step} {speed}"
            );
        }
    }
}
