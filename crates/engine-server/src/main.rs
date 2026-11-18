//! The headless engine process. Starts the engine and serves its API (see `lib.rs`).
//!
//! One engine per machine and port: a second one finds the port's lock taken and ends
//! before it starts anything. The system frees the lock when the process ends, however.
//! On `Stop` it prints what it measured to stderr and ends.

#![forbid(unsafe_code)]

use engine::{Engine, MAX_OUTPUTS, OutputNumber, State, Universes};
use std::fs::{File, TryLockError};
use std::net::SocketAddr;
use std::process::exit;
use std::sync::Arc;

const USAGE: &str = "usage: bossylights-engine [--listen 127.0.0.1:7744] [--universes 4]";

/// What the process is started with.
struct Settings {
    listen: SocketAddr,
    universes: Universes,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let settings = settings(&args).unwrap_or_else(|problem| {
        eprintln!("bossylights-engine: {problem}\n{USAGE}");
        exit(2);
    });
    let port = settings.listen.port();
    let _lock = lock(port).unwrap_or_else(|problem| {
        eprintln!("bossylights-engine: {problem}");
        exit(1);
    });
    let listener = tokio::net::TcpListener::bind(settings.listen)
        .await
        .unwrap_or_else(|error| {
            eprintln!(
                "bossylights-engine: cannot listen on {}: {error}",
                settings.listen
            );
            exit(1);
        });
    // No outputs yet: a client adds them with `SetOutput`.
    let (engine, sender) = Engine::start(settings.universes).unwrap_or_else(|error| {
        eprintln!("bossylights-engine: the tick thread did not start: {error}");
        exit(1);
    });
    let engine = Arc::new(engine);
    println!(
        "bossylights-engine listening on http://{}, {} universes",
        settings.listen,
        settings.universes.count()
    );
    if let Err(error) = engine_server::serve(listener, Arc::clone(&engine), sender).await {
        eprintln!("bossylights-engine: {error}");
        exit(1);
    }
    eprint!("{}", report(&engine));
}

/// Holds the lock of `port` for as long as the process runs, or says who has it.
fn lock(port: u16) -> Result<File, String> {
    let path = std::env::temp_dir().join(format!("bossylights-engine-{port}.lock"));
    let file = File::create(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(format!(
            "another engine already runs on port {port}. Stop it first (the API's Stop), or pick another port with --listen"
        )),
        Err(TryLockError::Error(e)) => Err(format!("cannot lock {}: {e}", path.display())),
    }
}

/// What the engine measured, one line each: the frames, then every output's lateness.
fn report(engine: &Engine) -> String {
    let State { frame, outputs, .. } = engine.query();
    let lines = (1..=MAX_OUTPUTS).filter_map(|n| {
        let number = OutputNumber::try_from(u8::try_from(n).ok()?).ok()?;
        let output = outputs.get(number.slot()).copied().flatten()?;
        let lateness = engine.lateness(number)?;
        let c = output.counters;
        Some(format!(
            "output {n}: lateness p99 {} us, worst {} us, accepted {}, dropped {}, failed {}\n",
            lateness.p99.as_micros(),
            lateness.worst.as_micros(),
            c.accepted,
            c.dropped,
            c.failed
        ))
    });
    format!("bossylights-engine stopped at frame {frame}\n") + &lines.collect::<String>()
}

fn settings(args: &[String]) -> Result<Settings, String> {
    let mut settings = Settings {
        listen: SocketAddr::from(([127, 0, 0, 1], 7744)),
        universes: Universes::new(4).ok_or("4 universes")?,
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
            _ => return Err(format!("unknown flag: {flag}")),
        }
    }
    Ok(settings)
}
