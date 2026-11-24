//! An effect moving in the terminal: each cell one coloured block, on a counted beat at
//! 120 bpm, measure 4, every setting at its default. Ctrl-C stops it.
//!
//! `cargo run -p effects --example preview -- <wave|stripes|plasma|sparkle> [WIDTHxHEIGHT, 80x4 if none]`

use effects::{Beats, CellKind, Effect, Kind, Place, Rgb, Size};
use std::io::{self, Write};
use std::num::NonZeroU16;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

const BPM: u64 = 120;
const FRAME: Duration = Duration::from_millis(33);
const RED: Rgb = rgb(65535, 0, 0);
const GREEN: Rgb = rgb(0, 65535, 0);
const BLUE: Rgb = rgb(0, 0, 65535);
const WHITE: Rgb = rgb(65535, 65535, 65535);

const fn rgb(red: u16, green: u16, blue: u16) -> Rgb {
    Rgb { red, green, blue }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok((kind, size)) => match run(kind, size) {
            Ok(()) => ExitCode::SUCCESS,
            Err(problem) => {
                eprintln!("preview: {problem}");
                ExitCode::FAILURE
            }
        },
        Err(problem) => {
            eprintln!("preview: {problem}");
            eprintln!("usage: preview <wave|stripes|plasma|sparkle> [WIDTHxHEIGHT]");
            ExitCode::from(2)
        }
    }
}

fn parse(args: &[String]) -> Result<(Kind, Size), String> {
    let kind = match args.first().map(String::as_str) {
        Some("wave") => Kind::Wave,
        Some("stripes") => Kind::Stripes,
        Some("plasma") => Kind::Plasma,
        Some("sparkle") => Kind::Sparkle,
        Some(other) => return Err(format!("no effect called {other}")),
        None => return Err("which effect?".into()),
    };
    let text = args.get(1).map_or("80x4", String::as_str);
    let (width, height) = text
        .split_once('x')
        .ok_or_else(|| format!("{text} is not WIDTHxHEIGHT"))?;
    let side = |s: &str| {
        s.parse::<u16>()
            .ok()
            .filter(|n| *n <= 1024)
            .and_then(NonZeroU16::new)
            .ok_or_else(|| format!("{s} is not 1 to 1024"))
    };
    Ok((
        kind,
        Size {
            width: side(width)?,
            height: side(height)?,
        },
    ))
}

fn run(kind: Kind, size: Size) -> Result<(), String> {
    let colours: &[Rgb] = match kind {
        Kind::Plasma => &[RED, GREEN, BLUE, WHITE],
        Kind::Wave | Kind::Stripes | Kind::Sparkle => &[RED, BLUE],
    };
    let effect = Effect::new(kind, 4, colours, &[]).map_err(|r| format!("{r:?}"))?;
    let start = Instant::now();
    let mut out = io::stdout().lock();
    let mut first = true;
    loop {
        // Q32 beats since the start: nanoseconds x bpm / 60 s.
        let beats = (start.elapsed().as_nanos() << 32) * u128::from(BPM) / 60_000_000_000;
        let painter = effect.painter(size, Beats(beats as u64));
        let mut screen = String::new();
        if !first {
            screen.push_str(&format!("\x1b[{}A", size.height.get() + 1));
        }
        first = false;
        let whole = beats >> 32;
        let quarter = (beats >> 30) & 3;
        screen.push_str(&format!(
            "\x1b[2K{kind:?}  beat {}.{:02}  ({BPM} bpm, measure 4)\n",
            whole + 1,
            quarter * 25
        ));
        for y in 0..size.height.get() {
            for x in 0..size.width.get() {
                let index = u32::from(y) * u32::from(size.width.get()) + u32::from(x);
                match painter.paint(Place { x, y, index }) {
                    Some(colour) => {
                        let lanes = CellKind::Rgb.lower(colour);
                        let [r, g, b] = [0, 1, 2].map(|i| lanes.as_slice()[i] >> 8);
                        screen.push_str(&format!("\x1b[38;2;{r};{g};{b}m█"));
                    }
                    None => screen.push_str("\x1b[0m "),
                }
            }
            screen.push_str("\x1b[0m\n");
        }
        out.write_all(screen.as_bytes())
            .and_then(|()| out.flush())
            .map_err(|e| e.to_string())?;
        thread::sleep(FRAME);
    }
}
