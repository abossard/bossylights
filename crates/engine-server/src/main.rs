//! The headless engine process. Starts the engine and serves its API (see `lib.rs`).

#![forbid(unsafe_code)]

use engine::{Engine, Universes};
use std::net::SocketAddr;
use std::process::exit;

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
    // No outputs yet: a client adds them with `SetOutput`.
    let (engine, sender) = Engine::start(settings.universes).unwrap_or_else(|error| {
        eprintln!("bossylights-engine: the tick thread did not start: {error}");
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
    println!(
        "bossylights-engine listening on http://{}, {} universes",
        settings.listen,
        settings.universes.count()
    );
    if let Err(error) = axum::serve(listener, engine_server::api(engine, sender)).await {
        eprintln!("bossylights-engine: {error}");
        exit(1);
    }
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
