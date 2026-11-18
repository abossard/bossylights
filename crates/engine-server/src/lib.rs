//! The engine API on one WebSocket. What the messages are: `engine_core::Request` and
//! `engine_core::Reply`. This file only moves them.
//!
//! | Route | What |
//! |---|---|
//! | `GET /health` | `ok` while the engine runs |
//! | `GET /api` | WebSocket: `Send`, `Query`, `Subscribe` in, JSON replies out |

#![forbid(unsafe_code)]

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use engine::{ChannelLevel, Engine, Levels, Refusal, Reply, Request, Sender, changes};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::MissedTickBehavior;

/// The routes, around a started engine.
pub fn api(engine: Engine, sender: Sender) -> Router {
    let api = Arc::new(Api {
        engine,
        sender: Mutex::new(sender),
    });
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api", get(connect))
        .with_state(api)
}

struct Api {
    engine: Engine,
    /// The queue has one producer; connections take turns. The tick thread never sees
    /// this lock.
    sender: Mutex<Sender>,
}

/// What a subscribed connection sent last.
struct Subscription {
    frame: u64,
    universes: Vec<Levels>,
}

async fn connect(socket: WebSocketUpgrade, State(api): State<Arc<Api>>) -> Response {
    socket.on_upgrade(move |socket| serve(socket, api))
}

/// One connection: answers each request; once subscribed, sends what changed with every
/// new frame. Reads from outside the tick thread, so it never waits on a client.
async fn serve(mut socket: WebSocket, api: Arc<Api>) {
    let count = usize::from(api.engine.universes().count());
    let mut now = vec![Levels::ZERO; count];
    let mut subscription = None;
    let mut poll = tokio::time::interval(Duration::from_secs(1) / (2 * Engine::RATE_HZ));
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        let reply = tokio::select! {
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
