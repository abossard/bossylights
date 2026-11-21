//! End to end: the engine process reads a folder of fixture definitions while it runs,
//! shows in the state how many it read and how many failed, and answers `Search`,
//! `Fixture` and `Failures`. A broken file is counted and named; the engine runs on.

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
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

/// An engine process and where it listens, killed when the test ends however it ends.
struct Running(Child, String);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts an engine with these extra arguments on a port the system gives, and connects
/// once it listens.
async fn engine(args: &[&str]) -> (Running, Socket) {
    let mut running = Running(
        Command::new(env!("CARGO_BIN_EXE_bossylights-engine"))
            .args(["--listen", "127.0.0.1:0"])
            .args(args)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
        String::new(),
    );
    let mut line = String::new();
    BufReader::new(running.0.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    running.1 = listening_on(&line);
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{}/api", running.1))
        .await
        .unwrap();
    (running, socket)
}

/// The address in the engine's "listening on http://<address>, ..." line.
fn listening_on(line: &str) -> String {
    line.split_once("listening on http://")
        .and_then(|(_, rest)| rest.split_once(','))
        .map(|(address, _)| address.to_string())
        .unwrap_or_else(|| panic!("not listening: {line}"))
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

/// The replies to patching each of `modes` (by name, at addresses 1, 2, ...) from files
/// `mixed.qxf`, then `twin.qxf`, each of maker Example and model Mixed, whose modes are
/// `files`, then the Failures.
async fn patch_each(case: usize, files: &[String], modes: &[&str]) -> (Vec<Value>, Value) {
    let folder =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("mixed-{}-{case}", process::id()));
    fs::create_dir_all(&folder).unwrap();
    for (name, xml) in ["mixed.qxf", "twin.qxf"].into_iter().zip(files) {
        fs::write(
            folder.join(name),
            format!(
                "<FixtureDefinition>\n <Manufacturer>Example</Manufacturer><Model>Mixed</Model><Type>Color Changer</Type>\n <Channel Name=\"Red\" Preset=\"IntensityRed\"/>{xml}\n</FixtureDefinition>"
            ),
        )
        .unwrap();
    }
    let (_running, mut socket) = engine(&["--fixtures", folder.to_str().unwrap()]).await;
    read(&mut socket).await;
    fs::remove_dir_all(folder).unwrap();
    let mut replies = Vec::new();
    for (mode, address) in modes.iter().zip(1..) {
        let patch = json!({"Send": {"command": {"Patch": {
            "name": mode, "maker": "Example", "model": "Mixed", "mode": mode,
            "at": {"universe": 1, "address": address}
        }}}});
        replies.push(ask(&mut socket, &patch).await);
    }
    let failures = ask(&mut socket, &json!("Failures")).await;
    (replies, failures["Failures"]["failures"].clone())
}

/// The files' modes, the modes patched, each reply, the Failures rows (file, line, problem).
type MixedCase<'a> = (
    Vec<String>,
    Vec<&'a str>,
    Vec<Value>,
    Vec<(&'a str, Option<u32>, &'a str)>,
);

// the files' modes, the modes patched, each reply, the Failures rows
#[tokio::test]
async fn a_refused_mode_is_listed_and_the_files_other_modes_patch() {
    let mode = |name: &str, head: &str| {
        format!(
            "\n <Mode Name=\"{name}\"><Channel Number=\"0\">Red</Channel><Head><Channel>{head}</Channel></Head></Mode>"
        )
    };
    let good = "\n <Mode Name=\"Good\"><Channel Number=\"0\">Red</Channel></Mode>";
    let patched = json!({"Patched": {"fixture": 1}});
    let refused = |reason: &str| json!({"Refused": {"reason": reason}});
    let past = "mode \"Broken\": head 0 names channel 1, the mode has 1";
    let cases: [MixedCase; 5] = [
        (
            vec![format!("{}{good}", mode("Broken", "1"))],
            vec!["Broken", "Good"],
            vec![refused("NoSuchMode"), patched.clone()],
            vec![("mixed.qxf", Some(4), past)],
        ),
        (
            vec![format!("{}{good}", mode("Broken", "bad"))],
            vec!["Broken", "Good"],
            vec![refused("NoSuchMode"), patched.clone()],
            vec![(
                "mixed.qxf",
                Some(4),
                "mode \"Broken\": Head channel=\"bad\" is not a number in range",
            )],
        ),
        (
            vec![format!("{}{}", mode("Broken", "1"), mode("Also", "1"))],
            vec!["Broken"],
            vec![refused("NoSuchFixture")],
            vec![
                ("mixed.qxf", Some(4), past),
                (
                    "mixed.qxf",
                    Some(5),
                    "mode \"Also\": head 0 names channel 1, the mode has 1",
                ),
            ],
        ),
        (
            vec![String::new()],
            vec!["Good"],
            vec![refused("NoSuchFixture")],
            vec![("mixed.qxf", Some(1), "the file has no <Mode>")],
        ),
        // a second file of the same maker and model stays off the shelf, its refused mode listed
        (
            vec![good.to_string(), format!("{good}{}", mode("Broken", "1"))],
            vec!["Good", "Broken"],
            vec![patched, refused("NoSuchMode")],
            vec![
                ("twin.qxf", None, "the same maker and model as mixed.qxf"),
                ("twin.qxf", Some(5), past),
            ],
        ),
    ];
    for (case, (files, modes, replies, rows)) in cases.into_iter().enumerate() {
        let rows: Vec<Value> = (rows.into_iter())
            .map(|(file, line, problem)| json!({"file": file, "line": line, "problem": problem}))
            .collect();
        let answer = patch_each(case, &files, &modes).await;
        assert_eq!(answer, (replies, Value::from(rows)), "{files:?}");
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
            json!({"Read": {"read": 11, "failed": 2}}),
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
    let failures = json!([
        {
            "file": "Broken/broken.qxf",
            "line": 1,
            "problem": "not valid xml: the root node was opened but never closed",
        },
        {
            "file": "Eurolite-LED-IP-PIX-Strobe-Frost.qxf",
            "line": 475,
            "problem": "mode \"32 Channel\": head 11 names channel 32, the mode has 32",
        },
    ]);
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
        (json!("Failures"), "/Failures/failures", failures),
        (
            json!("Query"),
            "/Picture/library",
            json!({"Read": {"read": 11, "failed": 2}}),
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

/// A named pipe never ends while nobody writes to it: it is a failure, and the rest is read.
#[cfg(unix)]
#[tokio::test]
async fn a_named_pipe_is_a_failure_and_the_rest_is_read() {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("pipe-{}", process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    let rgb = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");
    fs::copy(rgb, folder.join("rgb.qxf")).unwrap();
    let made = Command::new("mkfifo")
        .arg(folder.join("pipe.qxf"))
        .status()
        .unwrap();
    assert!(made.success());
    let (_running, mut socket) = engine(&["--fixtures", folder.to_str().unwrap()]).await;
    let library = tokio::time::timeout(Duration::from_secs(10), read(&mut socket))
        .await
        .expect("the library is read within 10 s");
    assert_eq!(library, json!({"Read": {"read": 1, "failed": 1}}));
    let failures = ask(&mut socket, &json!("Failures")).await;
    assert_eq!(
        failures,
        json!({"Failures": {"failures": [
            {"file": "pipe.qxf", "line": null, "problem": "not a file"},
        ]}})
    );
}

/// Every client sees one library per frame: a subscriber and a client that asks again and
/// again see the same one for the same frame, also in the frame the library is read.
#[tokio::test]
async fn every_client_sees_the_same_library_in_the_same_frame() {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("many-{}", process::id()));
    let definitions =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixture-library/tests/definitions");
    for copy in 0..50 {
        let into = folder.join(copy.to_string());
        fs::create_dir_all(&into).unwrap();
        for entry in fs::read_dir(&definitions).unwrap() {
            let path = entry.unwrap().path();
            fs::copy(&path, into.join(path.file_name().unwrap())).unwrap();
        }
    }
    let (running, mut subscriber) = engine(&["--fixtures", folder.to_str().unwrap()]).await;
    let (mut asker, _) = tokio_tungstenite::connect_async(format!("ws://{}/api", running.1))
        .await
        .unwrap();
    // (frame, library) as each client saw them, until a few frames after the library is read.
    let seen = |reply: &Value| {
        let picture = reply
            .get("Picture")
            .or_else(|| reply.get("Changed"))
            .unwrap();
        (
            picture["state"]["frame"].as_u64().unwrap(),
            picture["library"].clone(),
        )
    };
    let subscribed = async {
        subscriber
            .send(Message::text("\"Subscribe\""))
            .await
            .unwrap();
        let mut frames = Vec::new();
        while frames
            .iter()
            .filter(|(_, l)| *l != json!("Reading"))
            .count()
            < 5
        {
            frames.push(seen(&next(&mut subscriber).await));
        }
        frames
    };
    let asked = async {
        let mut frames = Vec::new();
        while frames
            .iter()
            .filter(|(_, l)| *l != json!("Reading"))
            .count()
            < 200
        {
            frames.push(seen(&ask(&mut asker, &json!("Query")).await));
        }
        frames
    };
    let (subscribed, asked) = tokio::join!(subscribed, asked);
    assert!(
        subscribed
            .iter()
            .any(|(_, library)| *library == json!("Reading")),
        "the folder was read before the test could look: make it larger"
    );
    for (frame, library) in &asked {
        if let Some((_, seen)) = subscribed.iter().find(|(f, _)| f == frame) {
            assert_eq!(library, seen, "frame {frame}");
        }
    }
}
