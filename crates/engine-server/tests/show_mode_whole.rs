//! A show file's pinned mode is checked as the fixture library checks it (#354, finding
//! API-01 of #349): one that breaks a rule of the model leaves the show alone.

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

// Opening a show must refuse a pinned mode the fixture library would refuse: no channel,
// a head or an ActsOn past its channels, a range that runs backwards, a layout without one
// cell per head. Source: docs/engine.md, Show file; AGENTS.md, no invalid project state;
// owner, 27 Nov: a mode with zero channels is refused. Finding API-01 of #349.
#[tokio::test]
async fn a_show_whose_pinned_mode_the_library_would_refuse_is_left_alone() {
    let rules =
        r#""rules":{"brightness":"Latest","manual_end":"UntilRelease","other_end":"Reveal"}"#;
    let red = r#"{"name":"Red","byte":"Coarse","function":"Red","default":0,"direction":null,"ranges":[]}"#;
    let acts_on_past_end = r#"{"name":"Red","byte":"Coarse","function":"Red","declared":{"acts_on":2},"default":0,"direction":null,"ranges":[]}"#;
    let backwards_range = r#"{"name":"Macro","byte":"Coarse","function":"ColorWheel","default":0,"direction":null,"ranges":[{"min":200,"max":10,"label":"Broken","does":null}]}"#;
    // (case, channels, pixels, layout, refused): the whole mode opens, so the data of each
    // broken one is read up to its broken rule.
    let cases = [
        (
            "whole",
            format!("{red},{red}"),
            "[[0],[1]]",
            r#"{"width":2,"height":1}"#,
            false,
        ),
        ("zero_channels", String::new(), "[]", "null", true),
        (
            "pixels_out_of_bounds",
            format!("{red},{red}"),
            "[[0],[2]]",
            "null",
            true,
        ),
        (
            "acts_on_past_end",
            format!("{red},{acts_on_past_end}"),
            "[]",
            "null",
            true,
        ),
        (
            "inverted_range_min_gt_max",
            format!("{red},{backwards_range}"),
            "[]",
            "null",
            true,
        ),
        (
            "layout_not_one_cell_per_head",
            format!("{red},{red}"),
            "[[0],[1]]",
            r#"{"width":3,"height":1}"#,
            true,
        ),
    ];

    for (case, channels, pixels, layout, refused) in cases {
        let contents = format!(
            r#"{{"version":{},"patch":[{{"fixture":1,"name":"Bar","mode":"m","at":{{"universe":1,"address":1}},"first":1}}],"groups":[],"looks":[],{rules},"modes":{{"m":{{"maker":"Test","model":"Bar","mode":{{"name":"2ch","channels":[{channels}],"pixels":{pixels},"layout":{layout},"degrees":null}}}}}}}}"#,
            show_file::VERSION
        );
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

        if !refused {
            assert_eq!(
                picture["show"],
                json!("Kept"),
                "{case}: {}",
                picture["show"]
            );
            continue;
        }
        assert!(
            picture["show"].get("LeftAlone").is_some(),
            "{case}: broken Mode invariant in show file was accepted into live patch (show = {}, patch = {})",
            picture["show"],
            picture["patch"]
        );
        assert!(
            picture["show"]["LeftAlone"]["why"].get("Broken").is_some(),
            "{case}: {}",
            picture["show"]
        );
    }
}
