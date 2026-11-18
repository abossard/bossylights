//! The engine API on one WebSocket. What the messages are: `engine_core::Request` and
//! `engine_core::Reply`. This file only moves them.
//!
//! | Route | What |
//! |---|---|
//! | `GET /health` | `ok` while the engine runs |
//! | `GET /version` | the API version, as text: a client checks it before it talks |
//! | `GET /api` | WebSocket: `Send`, `Query`, `Subscribe`, `Stop`, `Search`, `Fixture`, `Failures` in, JSON replies out |
//!
//! The plain routes allow any origin, so a web page can read them: they change nothing.
//! The WebSocket commands the lights, so only the pages in `ORIGINS` may open it, and
//! clients that are no web page (they send no `Origin`). Any other page gets 403.

#![forbid(unsafe_code)]

mod library;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use engine::{
    API_VERSION, ChannelLevel, Engine, Refusal, Reply, Request, Sender, Snapshot, changes,
};
use library::Shelf;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, watch};
use tokio::time::MissedTickBehavior;

/// Serves the API on `listener` until a client sends `Stop`. Then every connection is
/// closed and this returns the engine: the caller reports and drops it. The fixture
/// library is read from `fixtures` while the engine runs.
pub async fn serve(
    listener: TcpListener,
    mut engine: Engine,
    sender: Sender,
    fixtures: Option<PathBuf>,
) -> io::Result<Engine> {
    let (stop, mut stopped) = watch::channel(false);
    let (painted, pictures) = watch::channel(Arc::new(*engine.latest()));
    let reader = tokio::spawn(read(engine, painted, stop.subscribe()));
    let api = Arc::new(Api {
        sender: Mutex::new(sender),
        stop,
        pictures,
        shelf: library::start(fixtures),
    });
    let any = || [(ACCESS_CONTROL_ALLOW_ORIGIN, "*")];
    let routes = Router::new()
        .route("/health", get(move || async move { (any(), "ok") }))
        .route(
            "/version",
            get(move || async move { (any(), API_VERSION.to_string()) }),
        )
        .route("/api", get(connect))
        .with_state(Arc::clone(&api));
    let served = axum::serve(listener, routes)
        .with_graceful_shutdown(async move {
            let _ = stopped.wait_for(|stop| *stop).await;
        })
        .await;
    api.stop.send_replace(true);
    let engine = reader.await.map_err(io::Error::other)?;
    served.map(|()| engine)
}

struct Api {
    /// The queue has one producer; connections take turns. The tick thread never sees
    /// this lock.
    sender: Mutex<Sender>,
    /// Turns true once, on `Stop`. Every connection and the server watch it.
    stop: watch::Sender<bool>,
    /// The newest snapshot, one per frame, the same for every connection.
    pictures: watch::Receiver<Arc<Snapshot>>,
    /// The fixture library, once it is read.
    shelf: watch::Receiver<Arc<Shelf>>,
}

/// The one reader of the engine: takes the newest snapshot twice a beat and shares it
/// when it is of a new frame. Until `Stop`, then it gives the engine back.
async fn read(
    mut engine: Engine,
    painted: watch::Sender<Arc<Snapshot>>,
    mut stopped: watch::Receiver<bool>,
) -> Engine {
    let mut poll = tokio::time::interval(engine::BEAT / 2);
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = stopped.wait_for(|stop| *stop) => return engine,
            _ = poll.tick() => {
                let picture = engine.latest();
                if picture.state.frame != painted.borrow().state.frame {
                    painted.send_replace(Arc::new(*picture));
                }
            }
        }
    }
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
/// new snapshot. Never waits on the tick thread, the tick thread never waits on a client.
async fn talk(mut socket: WebSocket, api: Arc<Api>) {
    let mut pictures = api.pictures.clone();
    // What a subscribed connection sent last.
    let mut subscription: Option<Arc<Snapshot>> = None;
    let mut stopped = api.stop.subscribe();
    loop {
        let reply = tokio::select! {
            () = async { let _ = stopped.wait_for(|stop| *stop).await; } => {
                let _ = socket.send(Message::Close(None)).await;
                return;
            }
            message = socket.recv() => match message {
                Some(Ok(Message::Text(text))) => answer(&api, &text, &mut pictures, &mut subscription).await,
                Some(Ok(Message::Binary(_))) => json(&Reply::Refused { reason: Refusal::NotACommand }),
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => None,
                Some(Ok(Message::Close(_)) | Err(_)) | None => return,
            },
            Ok(()) = pictures.changed(), if subscription.is_some() => {
                changed(&api, &mut pictures, &mut subscription)
            }
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
    pictures: &mut watch::Receiver<Arc<Snapshot>>,
    subscription: &mut Option<Arc<Snapshot>>,
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
        Request::Query => picture(api, &pictures.borrow()),
        Request::Stop => {
            api.stop.send_replace(true);
            json(&Reply::Stopping)
        }
        Request::Subscribe => {
            let now = Arc::clone(&pictures.borrow_and_update());
            let reply = picture(api, &now);
            *subscription = Some(now);
            reply
        }
        Request::Search { text } => api.shelf.borrow().found(text.as_str()),
        Request::Fixture { maker, model } => {
            api.shelf.borrow().fixture(maker.as_str(), model.as_str())
        }
        Request::Failures => api.shelf.borrow().failures(),
    }
}

fn picture(api: &Api, now: &Snapshot) -> Option<String> {
    json(&Reply::Picture {
        state: now.state,
        library: api.shelf.borrow().library(),
        universes: now.universes(),
    })
}

/// `Changed`: the channels that differ from the last snapshot this connection sent.
fn changed(
    api: &Api,
    pictures: &mut watch::Receiver<Arc<Snapshot>>,
    subscription: &mut Option<Arc<Snapshot>>,
) -> Option<String> {
    let last = subscription.as_mut()?;
    let now = Arc::clone(&pictures.borrow_and_update());
    let channels: Vec<ChannelLevel> = changes(last.universes(), now.universes()).collect();
    let reply = json(&Reply::Changed {
        state: now.state,
        library: api.shelf.borrow().library(),
        channels: &channels,
    });
    *last = now;
    reply
}

/// A reply is plain numbers and names: this does not fail.
fn json(reply: &Reply) -> Option<String> {
    serde_json::to_string(reply).ok()
}
