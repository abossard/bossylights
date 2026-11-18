//! The engine API on one WebSocket. What the messages are: `engine_core::Request` and
//! `engine_core::Reply`. This file only moves them.
//!
//! | Route | What |
//! |---|---|
//! | `GET /health` | `ok` while the engine runs |
//! | `GET /version` | the API version, as text: a client checks it before it talks |
//! | `GET /api` | WebSocket: `Send`, `Query`, `Subscribe`, `Stop` in, JSON replies out |
//!
//! The plain routes allow any origin, so a web page can read them: they change nothing.
//! The WebSocket commands the lights, so only the pages in `ORIGINS` may open it, and
//! clients that are no web page (they send no `Origin`). Any other page gets 403.

#![forbid(unsafe_code)]

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use engine::{API_VERSION, ChannelLevel, Engine, Levels, Refusal, Reply, Request, Sender, changes};
use std::io;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, watch};
use tokio::time::MissedTickBehavior;

/// Serves the API on `listener` until a client sends `Stop`. Then every connection is
/// closed and this returns. The caller still holds the engine: it reports and drops it.
pub async fn serve(listener: TcpListener, engine: Arc<Engine>, sender: Sender) -> io::Result<()> {
    let (stop, mut stopped) = watch::channel(false);
    let api = Arc::new(Api {
        engine,
        sender: Mutex::new(sender),
        stop,
    });
    let any = || [(ACCESS_CONTROL_ALLOW_ORIGIN, "*")];
    let routes = Router::new()
        .route("/health", get(move || async move { (any(), "ok") }))
        .route(
            "/version",
            get(move || async move { (any(), API_VERSION.to_string()) }),
        )
        .route("/api", get(connect))
        .with_state(api);
    axum::serve(listener, routes)
        .with_graceful_shutdown(async move {
            let _ = stopped.wait_for(|stop| *stop).await;
        })
        .await
}

struct Api {
    engine: Arc<Engine>,
    /// The queue has one producer; connections take turns. The tick thread never sees
    /// this lock.
    sender: Mutex<Sender>,
    /// Turns true once, on `Stop`. Every connection and the server watch it.
    stop: watch::Sender<bool>,
}

/// What a subscribed connection sent last.
struct Subscription {
    frame: u64,
    universes: Vec<Levels>,
}

/// The web pages that may open the API, and why. A browser always sends the page's
/// origin; the shell, tests and scripts send none.
const ORIGINS: [&str; 4] = [
    "tauri://localhost",      // the app window on macOS and Linux
    "http://tauri.localhost", // the app window on Windows
    "http://127.0.0.1:5174",  // the UI in a browser, `npm run ui`
    "http://127.0.0.1:5299",  // the same UI under the end-to-end tests, `playwright.config.ts`
];

async fn connect(
    headers: HeaderMap,
    socket: WebSocketUpgrade,
    State(api): State<Arc<Api>>,
) -> Response {
    match headers.get(ORIGIN) {
        Some(origin) if !ORIGINS.iter().any(|allowed| origin == allowed) => {
            StatusCode::FORBIDDEN.into_response()
        }
        _ => socket.on_upgrade(move |socket| talk(socket, api)),
    }
}

/// One connection: answers each request; once subscribed, sends what changed with every
/// new frame. Reads from outside the tick thread, so it never waits on a client.
async fn talk(mut socket: WebSocket, api: Arc<Api>) {
    let count = usize::from(api.engine.universes().count());
    let mut now = vec![Levels::ZERO; count];
    let mut subscription = None;
    let mut poll = tokio::time::interval(engine::BEAT / 2);
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut stopped = api.stop.subscribe();
    loop {
        let reply = tokio::select! {
            () = async { let _ = stopped.wait_for(|stop| *stop).await; } => {
                let _ = socket.send(Message::Close(None)).await;
                return;
            }
            message = socket.recv() => match message {
                Some(Ok(Message::Text(text))) => answer(&api, &text, &mut subscription, &mut now).await,
                Some(Ok(Message::Binary(_))) => json(&Reply::Refused { reason: Refusal::NotACommand }),
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => None,
                Some(Ok(Message::Close(_)) | Err(_)) | None => return,
            },
            _ = poll.tick(), if subscription.is_some() => changed(&api, &mut subscription, &mut now),
        };
        if let Some(reply) = reply
            && socket.send(Message::text(reply)).await.is_err()
        {
            return; // client gone
        }
    }
}

async fn answer(
    api: &Api,
    text: &str,
    subscription: &mut Option<Subscription>,
    now: &mut [Levels],
) -> Option<String> {
    let Ok(request) = serde_json::from_str::<Request>(text) else {
        return json(&Reply::Refused {
            reason: Refusal::NotACommand,
        });
    };
    match request {
        Request::Send { command } => {
            let sent = api.sender.lock().await.send(command);
            json(&sent.map_or_else(|reason| Reply::Refused { reason }, |()| Reply::Accepted))
        }
        Request::Query => {
            let state = api.engine.read(now);
            json(&Reply::Picture {
                state,
                universes: now,
            })
        }
        Request::Stop => {
            api.stop.send_replace(true);
            json(&Reply::Stopping)
        }
        Request::Subscribe => {
            let state = api.engine.read(now);
            *subscription = Some(Subscription {
                frame: state.frame,
                universes: now.to_vec(),
            });
            json(&Reply::Picture {
                state,
                universes: now,
            })
        }
    }
}

/// `Changed` when a new frame was computed since the last message, else nothing.
fn changed(
    api: &Api,
    subscription: &mut Option<Subscription>,
    now: &mut [Levels],
) -> Option<String> {
    let last = subscription.as_mut()?;
    let state = api.engine.read(now);
    if state.frame == last.frame {
        return None;
    }
    let channels: Vec<ChannelLevel> = changes(&last.universes, now).collect();
    last.frame = state.frame;
    last.universes.copy_from_slice(now);
    json(&Reply::Changed {
        state,
        channels: &channels,
    })
}

/// A reply is plain numbers and names: this does not fail.
fn json(reply: &Reply) -> Option<String> {
    serde_json::to_string(reply).ok()
}
