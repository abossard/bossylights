//! End to end: the engine process reads a folder of fixture definitions while it runs,
//! shows in the state how many it read and how many failed, and answers `Search`,
//! `Fixture` and `Failures`. A broken file is counted and named; the engine runs on.

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use std::{fs, process};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The test definitions of the fixture library, and one broken file in a maker folder.
fn folder() -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fixtures-{}", process::id()));
    let definitions =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixture-library/tests/definitions");
    fs::create_dir_all(folder.join("Broken")).unwrap();
    for entry in fs::read_dir(definitions).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(&path, folder.join(path.file_name().unwrap())).unwrap();
    }
    fs::write(folder.join("Broken/broken.qxf"), "<FixtureDefinition>").unwrap();
    folder
}

/// An engine process, killed when the test ends however it ends.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts an engine with these extra arguments and connects once it listens.
async fn engine(args: &[&str]) -> (Running, Socket) {
    let listen = format!(
        "127.0.0.1:{}",
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    );
    let mut running = Running(
        Command::new(env!("CARGO_BIN_EXE_bossylights-engine"))
            .args(["--listen", &listen])
            .args(args)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    BufReader::new(running.0.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("listening"), "{line}");
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{listen}/api"))
        .await
        .unwrap();
    (running, socket)
}

async fn next(socket: &mut Socket) -> Value {
    let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
        .await
        .expect("a message within 10 s")
        .unwrap()
        .unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

/// Subscribes and waits for the library to be past `Reading`; returns its state.
async fn read(socket: &mut Socket) -> Value {
    socket.send(Message::text("\"Subscribe\"")).await.unwrap();
    loop {
        let reply = next(socket).await;
        let library = reply
            .get("Picture")
            .or_else(|| reply.get("Changed"))
            .unwrap()["library"]
            .clone();
        if library != json!("Reading") {
            return library;
        }
    }
}

/// The answer to `request`, past the `Changed` of a subscription.
async fn ask(socket: &mut Socket, request: &Value) -> Value {
    socket
        .send(Message::text(request.to_string()))
        .await
        .unwrap();
    loop {
        let reply = next(socket).await;
        if reply.get("Changed").is_none() {
            return reply;
        }
    }
}

#[tokio::test]
async fn the_state_shows_how_far_the_library_is_read() {
    let folder = folder();
    let missing = folder.join("missing");
    let cases: [(Vec<&str>, Value); 3] = [
        (vec![], json!("NoFolder")),
        (
            vec!["--fixtures", missing.to_str().unwrap()],
            json!("CannotOpen"),
        ),
        (
            vec!["--fixtures", folder.to_str().unwrap()],
            json!({"Read": {"read": 10, "failed": 1}}),
        ),
    ];
    for (args, expected) in cases {
        let (_running, mut socket) = engine(&args).await;
        assert_eq!(read(&mut socket).await, expected, "{args:?}");
    }
}

#[tokio::test]
async fn the_library_answers_search_fixture_and_failures() {
    let folder = folder();
    let (_running, mut socket) = engine(&["--fixtures", folder.to_str().unwrap()]).await;
    read(&mut socket).await;
    let rgbw = json!({
        "maker": "Generic",
        "model": "Generic RGBW",
        "kind": "ColorChanger",
        "modes": [
            {"name": "RGBW", "channels": 4},
            {"name": "WRGB", "channels": 4},
            {"name": "RGBW Dimmer", "channels": 5},
            {"name": "WRGB Dimmer", "channels": 5},
            {"name": "Dimmer RGBW", "channels": 5},
            {"name": "Dimmer WRGB", "channels": 5},
        ],
    });
    let first_mode = json!({
        "name": "RGBW",
        "channels": [
            {"name": "Red", "function": "Red"},
            {"name": "Green", "function": "Green"},
            {"name": "Blue", "function": "Blue"},
            {"name": "White", "function": "White"},
        ],
    });
    let failure = json!({
        "file": "Broken/broken.qxf",
        "line": 1,
        "problem": "not valid xml: the root node was opened but never closed",
    });
    // (request, where in the answer, expected)
    let cases: [(Value, &str, Value); 6] = [
        (
            json!({"Search": {"text": "rgbw"}}),
            "/Found/fixtures/0",
            rgbw,
        ),
        (
            json!({"Search": {"text": "nothing like it"}}),
            "/Found/fixtures",
            json!([]),
        ),
        (
            json!({"Fixture": {"maker": "Generic", "model": "Generic RGBW"}}),
            "/Fixture/modes/0",
            first_mode,
        ),
        (
            json!({"Fixture": {"maker": "Generic", "model": "BL63 10\" LED Bar"}}),
            "",
            json!({"Refused": {"reason": "NoSuchFixture"}}),
        ),
        (json!("Failures"), "/Failures/failures", json!([failure])),
        (
            json!("Query"),
            "/Picture/library",
            json!({"Read": {"read": 10, "failed": 1}}),
        ),
    ];
    for (request, pointer, expected) in cases {
        let answer = ask(&mut socket, &request).await;
        assert_eq!(
            answer.pointer(pointer),
            Some(&expected),
            "{request}: {answer}"
        );
    }
}
