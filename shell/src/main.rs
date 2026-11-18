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

use std::fs::File;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

/// Where the engine listens. `BOSSYLIGHTS_ENGINE=127.0.0.1:7798` picks another port (tests).
const ADDRESS: &str = "127.0.0.1:7744";
/// How long an orderly stop may take before the shell kills its own engine.
const STOP: Duration = Duration::from_secs(3);
/// How long an engine may take to connect and to answer `Stop`, each. An engine that does
/// not answer is killed, if it is ours, after that.
const ANSWER: Duration = Duration::from_secs(1);
/// How long an engine this shell started may take to answer, or to end. Longer than the
/// engine waits for the port of an engine that is stopping (3 s).
const START: Duration = Duration::from_secs(5);

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
    /// Not ours to stop: another version.
    NotOurs,
    /// None could be started, or ours ended before it answered.
    Gone,
}

fn main() {
    let address = std::env::var("BOSSYLIGHTS_ENGINE").unwrap_or_else(|_| ADDRESS.to_owned());
    let Ok(address) = address.parse::<SocketAddr>() else {
        eprintln!("bossylights: not an address: {address}");
        std::process::exit(2);
    };
    // One window per engine: a second window closing would stop an engine in use.
    let Some(_window) = one_window(address) else {
        eprintln!("bossylights: a window for the engine on {address} is already open");
        return;
    };
    let mut engine = meet(address);
    let window = window_engine(address, &engine);
    tauri::Builder::default()
        .setup(move |app| {
            eprintln!("bossylights: {window}");
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                .title("Bossylights")
                .inner_size(640.0, 420.0)
                .initialization_script(window_script(address))
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the window could not be created")
        .run(move |_, event| {
            if let tauri::RunEvent::Exit = event {
                stop(address, &mut engine);
            }
        });
}

/// The lock that keeps one window per engine address. It goes with the process, however it ends.
fn one_window(address: SocketAddr) -> Option<File> {
    let name = format!("bossylights-{}-{}.lock", address.ip(), address.port());
    let file = File::create(std::env::temp_dir().join(name)).ok()?;
    file.try_lock().ok()?;
    Some(file)
}

/// Which engine the window talks to, if any. A calculation.
fn window_engine(address: SocketAddr, engine: &Engine) -> String {
    match engine {
        Engine::Own(_) | Engine::Adopted | Engine::NotOurs => {
            format!("the window talks to the engine on {address}")
        }
        Engine::Gone => format!("the window has no engine on {address}"),
    }
}

/// What the window runs before its page: which engine it talks to.
fn window_script(address: SocketAddr) -> String {
    format!("window.__BOSSYLIGHTS_ENGINE__ = \"ws://{address}/api\";")
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
        Found::Nothing => {
            start_engine(address).map_or(Engine::Gone, |child| answered(address, child))
        }
    }
}

/// Waits until the engine this shell started answers on `address`, or ends. One that ends
/// is gone; one that is still silent after [`START`] is kept, it may answer later.
fn answered(address: SocketAddr, mut child: Child) -> Engine {
    let until = Instant::now() + START;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            eprintln!("bossylights: the engine on {address} ended at start ({status})");
            return Engine::Gone;
        }
        if found(ask_version(address).as_deref()) == Found::Same || Instant::now() >= until {
            eprintln!("bossylights: started the engine on {address}");
            return Engine::Own(child);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Cargo builds the engine binary into the same folder as this one.
/// If it does not start, the UI shows "disconnected".
fn start_engine(address: SocketAddr) -> Option<Child> {
    let name = format!("bossylights-engine{}", std::env::consts::EXE_SUFFIX);
    let path = std::env::current_exe().ok()?.with_file_name(name);
    detach(Command::new(&path).args(["--listen", &address.to_string()]))
        .spawn()
        .inspect_err(|error| eprintln!("bossylights: cannot start {}: {error}", path.display()))
        .ok()
}

/// The engine gets a process group of its own: a signal to the shell (Ctrl-C, a closing
/// terminal) does not reach it.
fn detach(command: &mut Command) -> &mut Command {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(command, 0);
    command
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

/// Whether quitting orders the engine to stop. An engine of our own that has died is gone:
/// whoever listens on its address now is another shell's engine.
fn orders_stop(engine: &mut Engine) -> bool {
    match engine {
        Engine::NotOurs | Engine::Gone => false,
        Engine::Adopted => true,
        Engine::Own(child) => matches!(child.try_wait(), Ok(None)),
    }
}

/// Quitting stops the engine in an orderly way. Its own engine is killed if it does not
/// end in time.
fn stop(address: SocketAddr, engine: &mut Engine) {
    if !orders_stop(engine) {
        return;
    }
    send_stop(address, ANSWER);
    if let Engine::Own(child) = engine {
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

/// Sends the API's `Stop` and waits for its answer, each step at most `timeout`.
fn send_stop(address: SocketAddr, timeout: Duration) {
    let stopped = TcpStream::connect_timeout(&address, timeout)
        .map_err(tungstenite::Error::Io)
        .and_then(|stream| {
            stream
                .set_read_timeout(Some(timeout))
                .and_then(|()| stream.set_write_timeout(Some(timeout)))
                .map_err(tungstenite::Error::Io)?;
            let (mut socket, _) = tungstenite::client(format!("ws://{address}/api"), stream)
                .map_err(|error| match error {
                    tungstenite::HandshakeError::Failure(error) => error,
                    tungstenite::HandshakeError::Interrupted(_) => {
                        tungstenite::Error::Io(std::io::ErrorKind::TimedOut.into())
                    }
                })?;
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

    /// A listener that accepts and never answers.
    fn mute() -> (std::net::TcpListener, SocketAddr) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        (listener, address)
    }

    #[test]
    fn an_engine_that_never_answers_does_not_hold_the_stop() {
        let (listener, address) = mute();
        let holding = thread::spawn(move || listener.accept().map(|(stream, _)| stream));
        let (done, returned) = std::sync::mpsc::channel();
        thread::spawn(move || {
            send_stop(address, Duration::from_millis(100));
            let _ = done.send(());
        });
        // A hang guard of seconds: `send_stop` must return at all.
        assert_eq!(returned.recv_timeout(Duration::from_secs(10)), Ok(())); // guard
        drop(holding);
    }

    fn dead() -> Child {
        let mut child = Command::new("true").spawn().unwrap();
        child.wait().unwrap();
        child
    }

    #[test]
    fn a_shell_orders_a_stop_only_for_an_engine_it_may_stop() {
        // (the engine, whether a Stop is ordered)
        let cases = [
            ("adopted", Engine::Adopted, true),
            ("not ours", Engine::NotOurs, false),
            ("own, died meanwhile", Engine::Own(dead()), false),
            (
                "own, running",
                Engine::Own(Command::new("sleep").arg("30").spawn().unwrap()),
                true,
            ),
        ];
        for (name, mut engine, ordered) in cases {
            assert_eq!(orders_stop(&mut engine), ordered, "{name}");
            if let Engine::Own(child) = &mut engine {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    #[test]
    fn a_dead_engine_of_our_own_gets_no_stop() {
        let (listener, address) = mute();
        listener.set_nonblocking(true).unwrap();
        stop(address, &mut Engine::Own(dead()));
        assert!(
            listener.accept().is_err(),
            "a Stop was sent to whoever listens now"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_engine_starts_in_a_process_group_of_its_own() {
        let mut command = Command::new("sh");
        command.args(["-c", "ps -o pgid= -p $$"]);
        let output = detach(&mut command).output().unwrap();
        let group: u32 = String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // `ps` ran in the shell's process; the group is not ours.
        let ours = Command::new("sh")
            .args(["-c", "ps -o pgid= -p $$"])
            .output()
            .unwrap();
        let ours: u32 = String::from_utf8(ours.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_ne!(group, ours);
    }

    #[test]
    fn one_window_per_engine_address() {
        // Addresses nobody else uses right now: the lock is per address, and tests may run side by side.
        let (_first_taken, first) = mute();
        let (_other_taken, other) = mute();
        let held = one_window(first);
        assert!(held.is_some(), "the first window");
        assert!(
            one_window(first).is_none(),
            "a second window for the same engine"
        );
        assert!(one_window(other).is_some(), "a window for another engine");
        drop(held);
        // The lock is free once every copy of the file is closed. A child that another test
        // is starting right now holds one until it runs its program.
        let started = Instant::now();
        while one_window(first).is_none() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the lock stays held after the window is gone"
            );
            thread::yield_now();
        }
    }

    #[test]
    fn the_window_is_told_which_engine_to_talk_to() {
        // (address, the script the window runs before the page)
        let cases = [
            (
                "127.0.0.1:7744",
                "window.__BOSSYLIGHTS_ENGINE__ = \"ws://127.0.0.1:7744/api\";",
            ),
            (
                "127.0.0.1:7798",
                "window.__BOSSYLIGHTS_ENGINE__ = \"ws://127.0.0.1:7798/api\";",
            ),
        ];
        for (address, script) in cases {
            assert_eq!(window_script(address.parse().unwrap()), script);
        }
    }
}
