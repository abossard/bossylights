//! The desktop shell. Adopts the running engine or starts one, and shows the web UI.
//! The UI reaches the engine only through its API, exactly as in a browser.
//!
//! The engine is its own process and outlives the window: a killed or crashed shell
//! leaves it running, and the next shell adopts it. Quitting the app stops it.
//!
//! ```text
//! GET /version --> nothing answers --> start one   (Own)
//!              --> same version    --> adopt it    (Adopted)
//!              --> anything else   --> start nothing, say so (Foreign)
//! ```

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

/// Where the engine listens. `BOSSYLIGHTS_ENGINE=127.0.0.1:7798` picks another port (tests).
const ADDRESS: &str = "127.0.0.1:7744";
/// How long an orderly stop may take before the shell kills its own engine.
const STOP: Duration = Duration::from_secs(3);

/// What answered on the engine's address.
#[derive(Debug, PartialEq, Eq)]
enum Found {
    Nothing,
    /// An engine that speaks this shell's API version.
    Same,
    /// Something else: another API version, an engine too old to say, or no engine.
    Other(String),
}

/// The engine this shell works with, and so what quitting does to it.
enum Engine {
    /// Started by this shell.
    Own(Child),
    Adopted,
    /// Not ours to stop: another version, or none could be started.
    NotOurs,
}

fn main() {
    let address = std::env::var("BOSSYLIGHTS_ENGINE").unwrap_or_else(|_| ADDRESS.to_owned());
    let Ok(address) = address.parse::<SocketAddr>() else {
        eprintln!("bossylights: not an address: {address}");
        std::process::exit(2);
    };
    let mut engine = meet(address);
    tauri::Builder::default()
        .build(tauri::generate_context!())
        .expect("the window could not be created")
        .run(move |_, event| {
            if let tauri::RunEvent::Exit = event {
                stop(address, &mut engine);
            }
        });
}

/// Adopts the engine on `address`, or starts one, or says why it starts nothing.
fn meet(address: SocketAddr) -> Engine {
    match found(ask_version(address).as_deref()) {
        Found::Same => {
            eprintln!(
                "bossylights: adopted the engine on {address} (api {})",
                engine_core::API_VERSION
            );
            Engine::Adopted
        }
        Found::Other(answer) => {
            eprintln!(
                "bossylights: the engine on {address} speaks api {answer}, this app {}: starting nothing",
                engine_core::API_VERSION
            );
            Engine::NotOurs
        }
        Found::Nothing => start_engine(address).map_or(Engine::NotOurs, |child| {
            eprintln!("bossylights: started the engine on {address}");
            Engine::Own(child)
        }),
    }
}

/// Cargo builds the engine binary into the same folder as this one.
/// If it does not start, the UI shows "disconnected".
fn start_engine(address: SocketAddr) -> Option<Child> {
    let name = format!("bossylights-engine{}", std::env::consts::EXE_SUFFIX);
    let path = std::env::current_exe().ok()?.with_file_name(name);
    Command::new(&path)
        .args(["--listen", &address.to_string()])
        .spawn()
        .inspect_err(|error| eprintln!("bossylights: cannot start {}: {error}", path.display()))
        .ok()
}

/// The whole HTTP answer to `GET /version`, or `None` when nothing listens.
fn ask_version(address: SocketAddr) -> Option<String> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1)).ok()?;
    let mut answer = String::new();
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = write!(stream, "GET /version HTTP/1.0\r\nHost: {address}\r\n\r\n")
        .and_then(|()| stream.read_to_string(&mut answer));
    Some(answer)
}

/// What an answer to `GET /version` says. A calculation.
fn found(answer: Option<&str>) -> Found {
    let Some(answer) = answer else {
        return Found::Nothing;
    };
    let (head, body) = answer.split_once("\r\n\r\n").unwrap_or((answer, ""));
    let status = head.lines().next().unwrap_or("");
    let ok = status.split_whitespace().nth(1) == Some("200");
    match (ok, body.trim()) {
        (true, version) if version == engine_core::API_VERSION.to_string() => Found::Same,
        (true, version) => Found::Other(version.to_owned()),
        (false, _) if status.is_empty() => Found::Other("unknown".to_owned()),
        (false, _) => Found::Other(format!("unknown ({status})")),
    }
}

/// Quitting stops the engine in an orderly way. Its own engine is killed if it does not
/// end in time.
fn stop(address: SocketAddr, engine: &mut Engine) {
    match engine {
        Engine::NotOurs => {}
        Engine::Adopted => send_stop(address),
        Engine::Own(child) => {
            send_stop(address);
            let until = Instant::now() + STOP;
            while matches!(child.try_wait(), Ok(None)) && Instant::now() < until {
                thread::sleep(Duration::from_millis(20));
            }
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

/// Sends the API's `Stop` and waits for its answer.
fn send_stop(address: SocketAddr) {
    let stopped =
        tungstenite::connect(format!("ws://{address}/api")).and_then(|(mut socket, _)| {
            socket.send(tungstenite::Message::text("\"Stop\""))?;
            socket.read().map(drop)
        });
    if let Err(error) = stopped {
        eprintln!("bossylights: cannot stop the engine on {address}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(body: &str) -> String {
        format!("HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\n\r\n{body}")
    }

    #[test]
    fn what_answered_on_the_engine_address() {
        let mine = engine_core::API_VERSION.to_string();
        let other = (engine_core::API_VERSION + 1).to_string();
        // (answer to GET /version, found)
        let cases = [
            (None, Found::Nothing),
            (Some(ok(&mine)), Found::Same),
            (Some(ok(&format!("{mine}\n"))), Found::Same),
            (Some(ok(&other)), Found::Other(other.clone())),
            (Some(ok("")), Found::Other(String::new())),
            (
                Some("HTTP/1.1 404 Not Found\r\n\r\n".to_owned()),
                Found::Other("unknown (HTTP/1.1 404 Not Found)".to_owned()),
            ),
            (Some(String::new()), Found::Other("unknown".to_owned())),
            (
                Some(format!("HTTP/1.1 500 Oops\r\n\r\n{mine}")),
                Found::Other("unknown (HTTP/1.1 500 Oops)".to_owned()),
            ),
        ];
        for (answer, expected) in cases {
            assert_eq!(found(answer.as_deref()), expected, "{answer:?}");
        }
    }
}
