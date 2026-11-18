//! The headless engine process. Starts the engine and serves its API:
//!
//! | Route         | What                                                 |
//! |---------------|------------------------------------------------------|
//! | `GET /health` | `ok` while the engine runs                           |
//! | `GET /state`  | WebSocket, one JSON `State` per new frame: `{"frame":123}` |

#![forbid(unsafe_code)]

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use engine::Engine;
use std::net::SocketAddr;
use std::process::exit;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::MissedTickBehavior;

const DEFAULT_LISTEN: &str = "127.0.0.1:7744";
const USAGE: &str = "usage: bossylights-engine [--listen 127.0.0.1:7744]";

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let listen = listen_address();
    let engine = match Engine::start() {
        Ok(engine) => Arc::new(engine),
        Err(error) => {
            eprintln!("bossylights-engine: the tick thread did not start: {error}");
            exit(1);
        }
    };
    let api = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/state", get(stream_state))
        .with_state(engine);

    let listener = match tokio::net::TcpListener::bind(listen).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("bossylights-engine: cannot listen on {listen}: {error}");
            exit(1);
        }
    };
    println!("bossylights-engine listening on http://{listen}");
    if let Err(error) = axum::serve(listener, api).await {
        eprintln!("bossylights-engine: {error}");
        exit(1);
    }
}

fn listen_address() -> SocketAddr {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let address = match args.as_slice() {
        [] => DEFAULT_LISTEN,
        [flag, address] if flag == "--listen" => address,
        _ => {
            eprintln!("{USAGE}");
            exit(2);
        }
    };
    address.parse().unwrap_or_else(|_| {
        eprintln!("bossylights-engine: not an address: {address}\n{USAGE}");
        exit(2);
    })
}

async fn stream_state(socket: WebSocketUpgrade, State(engine): State<Arc<Engine>>) -> Response {
    socket.on_upgrade(move |socket| send_frames(socket, engine))
}

/// Reads snapshots from outside the core, so the tick thread never waits on a client.
/// Polls at twice the tick rate and sends only when the frame changed.
async fn send_frames(mut socket: WebSocket, engine: Arc<Engine>) {
    let mut poll = tokio::time::interval(Duration::from_secs(1) / (2 * Engine::RATE_HZ));
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last = None;
    loop {
        poll.tick().await;
        let state = engine.query();
        if last == Some(state.frame) {
            continue;
        }
        last = Some(state.frame);
        let Ok(json) = serde_json::to_string(&state) else {
            return; // a State is plain numbers: this cannot happen
        };
        if socket.send(Message::text(json)).await.is_err() {
            return; // client gone
        }
    }
}
