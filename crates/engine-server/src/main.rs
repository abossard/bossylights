//! The headless engine process. Starts the engine and serves its API (see `lib.rs`).
//!
//! One engine per machine and port: a second one finds the port's lock taken and ends
//! before it starts anything. The system frees the lock when the process ends, however.
//! An engine that got `Stop` stops answering before its process ends, so a new one waits
//! up to [`FREED`] for the lock before it gives up.
//! On `Stop` it saves the show file, prints what it measured to stderr and ends.
//!
//! With `--show <file>` the patch and the groups are opened from the file at start and
//! saved to it after every change. A file of a newer version, or a broken one, is left
//! alone: the engine starts with an empty patch and the picture says why.

#![forbid(unsafe_code)]

use engine::{Counters, Engine, Lateness, MAX_OUTPUTS, OutputNumber, State, Universes};
use std::fs::{File, TryLockError};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::exit;
use std::thread;
use std::time::{Duration, Instant};

#[path = "report_line.rs"]
mod report_line;

const USAGE: &str = "usage: bossylights-engine [--listen 127.0.0.1:7744] [--universes 4] [--fixtures <folder>] [--show <file>]";

/// How long a new engine waits for the lock of an engine that is stopping.
const FREED: Duration = Duration::from_secs(3);
/// How often it looks again meanwhile.
const LOOK: Duration = Duration::from_millis(20);

/// What the process is started with.
struct Settings {
    listen: SocketAddr,
    universes: Universes,
    /// The folder of fixture definitions (QLC+ `.qxf`), read while the engine runs.
    fixtures: Option<PathBuf>,
    /// The show file: the patch and the groups.
    show: Option<PathBuf>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let settings = settings(&args).unwrap_or_else(|problem| {
        eprintln!("bossylights-engine: {problem}\n{USAGE}");
        exit(2);
    });
    let locked = |port| {
        lock(port).unwrap_or_else(|problem| {
            eprintln!("bossylights-engine: {problem}");
            exit(1);
        })
    };
    // A port asked for is locked before it is bound: an engine that is stopping still
    // listens until it lets go. Port 0 is the free port the system gives, locked once known.
    let asked = settings.listen.port();
    let early = (asked != 0).then(|| locked(asked));
    let listener = tokio::net::TcpListener::bind(settings.listen)
        .await
        .unwrap_or_else(|error| {
            eprintln!(
                "bossylights-engine: cannot listen on {}: {error}",
                settings.listen
            );
            exit(1);
        });
    let listening = listener.local_addr().unwrap_or_else(|error| {
        eprintln!("bossylights-engine: no address to listen on: {error}");
        exit(1);
    });
    let _lock = early.unwrap_or_else(|| locked(listening.port()));
    // No outputs yet: a client adds them with `SetOutput`.
    let (engine, sender) = Engine::start(settings.universes).unwrap_or_else(|error| {
        eprintln!("bossylights-engine: the tick thread did not start: {error}");
        exit(1);
    });
    println!(
        "bossylights-engine listening on http://{listening}, {} universes",
        settings.universes.count()
    );
    let mut engine =
        engine_server::serve(listener, engine, sender, settings.fixtures, settings.show)
            .await
            .unwrap_or_else(|error| {
                eprintln!("bossylights-engine: {error}");
                exit(1);
            });
    eprint!("{}", report(&mut engine));
}

/// Holds the lock of `port` for as long as the process runs, or says who has it. Waits up
/// to [`FREED`] while another engine has it: one that is stopping lets go when it ends.
fn lock(port: u16) -> Result<File, String> {
    let path = std::env::temp_dir().join(format!("bossylights-engine-{port}.lock"));
    // Opened for reading when it exists: the file stays in the temp folder, and another
    // user cannot write it. A lock does not need write access.
    let file = File::open(&path)
        .or_else(|_| File::create(&path))
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let until = Instant::now() + FREED;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if Instant::now() < until => thread::sleep(LOOK),
            Err(TryLockError::WouldBlock) => {
                return Err(format!(
                    "another engine already runs on port {port}. Stop it first (the API's Stop), or pick another port with --listen"
                ));
            }
            Err(TryLockError::Error(e)) => {
                return Err(format!("cannot lock {}: {e}", path.display()));
            }
        }
    }
}

/// What the engine measured, one line each: the frames, then every output's lateness.
fn report(engine: &mut Engine) -> String {
    let picture = engine.fresh();
    let State { frame, outputs, .. } = picture.state;
    let lines = (1..=MAX_OUTPUTS).filter_map(|n| {
        let number = OutputNumber::try_from(u8::try_from(n).ok()?).ok()?;
        let output = outputs.get(number.slot()).copied().flatten()?;
        let lateness = picture.lateness(number)?;
        Some(line(n, output.counters, lateness))
    });
    format!("bossylights-engine stopped at frame {frame}\n") + &lines.collect::<String>()
}

/// One output's line.
fn line(n: usize, c: Counters, lateness: Lateness) -> String {
    format!("output {n}: {}\n", report_line::counts(c, lateness))
}

fn settings(args: &[String]) -> Result<Settings, String> {
    let mut settings = Settings {
        listen: SocketAddr::from(([127, 0, 0, 1], 7744)),
        universes: Universes::new(4).ok_or("4 universes")?,
        fixtures: None,
        show: None,
    };
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--listen" => {
                settings.listen = value
                    .parse()
                    .map_err(|_| format!("not an address: {value}"))?;
            }
            "--universes" => {
                settings.universes = value
                    .parse()
                    .ok()
                    .and_then(Universes::new)
                    .ok_or(format!("universes are 1 to {}", engine::MAX_UNIVERSES))?;
            }
            "--fixtures" => settings.fixtures = Some(PathBuf::from(value)),
            "--show" => settings.show = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown flag: {flag}")),
        }
    }
    Ok(settings)
}
