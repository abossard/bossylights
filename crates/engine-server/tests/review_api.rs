//! Review tests for aspect 9 (`crates/wire`, `crates/show-file`, `crates/engine-server`, `ui/src/engine-api.ts`).

mod common;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn show_file(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "bossylights-review-api-{}-{name}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(engine_server::show::saving(&path));
    path
}

fn start_engine(show: &Path) -> (Running, String) {
    let mut running = Running(
        Command::new(env!("CARGO_BIN_EXE_bossylights-engine"))
            .args(["--listen", "127.0.0.1:0"])
            .arg("--show")
            .arg(show)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    BufReader::new(running.0.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    let listen = line
        .split_once("listening on http://")
        .and_then(|(_, rest)| rest.split_once(','))
        .map(|(address, _)| address.to_string())
        .unwrap_or_else(|| panic!("not listening: {line}"));
    (running, listen)
}

async fn connect(listen: &str) -> Socket {
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{listen}/api"))
        .await
        .unwrap();
    socket
}

async fn answer_to(socket: &mut Socket, request: &Value) -> Value {
    socket
        .send(Message::text(request.to_string()))
        .await
        .unwrap();
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a message within 5 s")
            .unwrap()
            .unwrap();
        let reply: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        if reply.get("Changed").is_none() {
            return reply;
        }
    }
}

async fn stop(socket: &mut Socket, Running(engine): &mut Running) {
    assert_eq!(answer_to(socket, &json!("Stop")).await, json!("Stopping"));
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = engine.try_wait().unwrap() {
            assert!(status.success(), "{status}");
            return;
        }
        assert!(Instant::now() < until, "engine did not stop within 10 s"); // guard
        thread::sleep(Duration::from_millis(50)); // poll
    }
}

/// Claim: `bossylights-engine --show <file>` deserializes `fixture_library::Mode` from JSON without validating `Mode` invariants, loading 0-channel modes, out-of-bounds pixel channel indices, and inverted `min > max` capability ranges into the live patch as `show: "Kept"` instead of leaving the broken show file alone (`LeftAlone`).
/// Source: `docs/engine.md`, "Show file" & "`Patched { channels: 1.. }`", and `AGENTS.md`, "No invalid project state: a command that would make one is refused... Never allow and flag."
/// Severity: medium.
/// Show impact: A corrupted or hand-edited show file puts invalid fixture definitions into the live engine (`Patched { channels: 0 }` that skips address-overlap checks, phantom pixel lights pointing past the mode's channels, or inverted `min > max` ranges) and overwrites the file on the next edit instead of refusing it as `LeftAlone`.
#[tokio::test]
async fn finding_api_01_show_file_loads_broken_mode_invariants() {
    let rules =
        r#""rules":{"brightness":"Latest","manual_end":"UntilRelease","other_end":"Reveal"}"#;
    let channel = r#"{"name":"Red","byte":"Coarse","function":"Red","default":0,"degrees":null,"direction":null,"ranges":[]}"#;
    let inverted_range_channel = r#"{"name":"Macro","byte":"Coarse","function":"ColorMacro","default":0,"degrees":null,"direction":null,"ranges":[{"min":200,"max":10,"label":"Broken","does":"Nothing","colours":null}]}"#;

    let cases = [
        (
            "zero_channels",
            format!(
                r#"{{"version":{},"patch":[{{"fixture":1,"name":"Ghost","mode":"m","at":{{"universe":1,"address":1}},"first":1}}],"groups":[],"looks":[],{rules},"modes":{{"m":{{"maker":"Test","model":"Zero","mode":{{"name":"Empty","channels":[],"pixels":[],"layout":null,"degrees":null}}}}}}}}"#,
                show_file::VERSION
            ),
        ),
        (
            "pixels_out_of_bounds",
            format!(
                r#"{{"version":{},"patch":[{{"fixture":1,"name":"BrokenBar","mode":"m","at":{{"universe":1,"address":1}},"first":1}}],"groups":[],"looks":[],{rules},"modes":{{"m":{{"maker":"Test","model":"Bar","mode":{{"name":"2px","channels":[{channel}],"pixels":[[9],[10]],"layout":null,"degrees":null}}}}}}}}"#,
                show_file::VERSION
            ),
        ),
        (
            "inverted_range_min_gt_max",
            format!(
                r#"{{"version":{},"patch":[{{"fixture":1,"name":"InvertedRange","mode":"m","at":{{"universe":1,"address":1}},"first":1}}],"groups":[],"looks":[],{rules},"modes":{{"m":{{"maker":"Test","model":"Macro","mode":{{"name":"1ch","channels":[{inverted_range_channel}],"pixels":[],"layout":null,"degrees":null}}}}}}}}"#,
                show_file::VERSION
            ),
        ),
    ];

    for (case, contents) in cases {
        let path = show_file(case);
        std::fs::write(&path, &contents).unwrap();
        let (mut running, listen) = start_engine(&path);
        let mut socket = connect(&listen).await;

        let started = Instant::now();
        let picture = loop {
            let pic = answer_to(&mut socket, &json!("Query")).await["Picture"].clone();
            if pic["show"] != json!("Opening") {
                break pic;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "{case}: stuck Opening"
            ); // guard
            thread::sleep(Duration::from_millis(20)); // poll
        };

        stop(&mut socket, &mut running).await;
        let _ = std::fs::remove_file(&path);

        assert!(
            picture["show"].get("LeftAlone").is_some(),
            "{case}: broken Mode invariant in show file was accepted into live patch (show = {}, patch = {})",
            picture["show"],
            picture["patch"]
        );
    }
}
