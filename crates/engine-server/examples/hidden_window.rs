//! For the hand measurement of #107 (`docs/spikes/hidden-window.md`): talks to the engine
//! the shell runs, over its API. Adds two DDP outputs (60 and 44 a second) that send to a
//! receiver here, waits, prints their counters, then sends `Stop`. The engine prints its
//! lateness on stop, to the shell's stderr.
//!
//! `cargo run --release -p engine-server --example hidden_window -- 600 [127.0.0.1:7744]`

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::net::UdpSocket;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("hidden_window: {problem}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seconds: u64 = args
        .first()
        .and_then(|s| s.parse().ok())
        .ok_or("seconds?")?;
    let engine = args.get(1).map_or("127.0.0.1:7744", String::as_str);
    let drain = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let address = drain.local_addr().map_err(|e| e.to_string())?.to_string();
    thread::spawn(move || {
        let mut packet = [0; 1500];
        while drain.recv(&mut packet).is_ok() {}
    });
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{engine}/api"))
        .await
        .map_err(|e| e.to_string())?;
    for (output, rate) in [(1, "60"), (2, "44")] {
        let kind = json!({"Ddp": {"address": address, "color": "Rgbw"}});
        let command = json!({"SetOutput": {"output": output, "kind": kind,
            "universes": {"first": 1, "count": 1}, "rate": rate}});
        let reply = ask(&mut socket, json!({"Send": {"command": command}})).await?;
        if reply != json!("Accepted") {
            return Err(format!("output {output}: {reply}"));
        }
    }
    eprintln!("hidden_window: two outputs set, waiting {seconds} s");
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    let picture = ask(&mut socket, json!("Query")).await?;
    let state = &picture["Picture"]["state"];
    println!("frame {}", state["frame"]);
    for (n, output) in state["outputs"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        if !output.is_null() {
            println!("output {}: {}", n + 1, output["counters"]);
        }
    }
    println!("stop: {}", ask(&mut socket, json!("Stop")).await?);
    Ok(())
}

/// The first answer that is not a `Changed`.
async fn ask<S>(socket: &mut S, request: Value) -> Result<Value, String>
where
    S: SinkExt<Message>
        + StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    socket
        .send(Message::text(request.to_string()))
        .await
        .map_err(|_| "send failed")?;
    while let Some(message) = socket.next().await {
        let text = message.map_err(|e| e.to_string())?;
        let reply: Value = serde_json::from_str(text.to_text().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if reply.get("Changed").is_none() {
            return Ok(reply);
        }
    }
    Err("the engine closed the connection".to_owned())
}
