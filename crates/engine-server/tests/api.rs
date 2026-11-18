//! End to end: a client sends "set channel" over the WebSocket and sees it in the
//! state stream and in the frame a mock output receives.

use engine::{ADDRESSES, Engine, Levels, Universes};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The smallest output: keeps the last frame of universe 1.
struct Mock([AtomicU8; ADDRESSES]);

async fn connect() -> (Socket, Arc<Mock>) {
    let mock = Arc::new(Mock([const { AtomicU8::new(0) }; ADDRESSES]));
    let output = {
        let mock = Arc::clone(&mock);
        move |frame: &[Levels]| {
            if let Some(universe) = frame.first() {
                for (cell, level) in mock.0.iter().zip(universe.0) {
                    cell.store(level, Ordering::Relaxed);
                }
            }
        }
    };
    let (engine, sender) = Engine::start(Universes::new(2).unwrap(), output).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, engine_server::api(engine, sender)).await });
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/api"))
        .await
        .unwrap();
    (socket, mock)
}

async fn send(socket: &mut Socket, request: &Value) {
    socket
        .send(Message::text(request.to_string()))
        .await
        .unwrap();
}

/// The next message from the engine, as JSON.
async fn next(socket: &mut Socket) -> Value {
    let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("a message within 2 s")
        .unwrap()
        .unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

/// The next message that is not a `Changed`.
async fn answer(socket: &mut Socket) -> Value {
    loop {
        let reply = next(socket).await;
        if reply.get("Changed").is_none() {
            return reply;
        }
    }
}

fn set(universe: u16, address: u16, value: u16) -> Value {
    json!({"Send": {"command": {"SetChannel": {
        "channel": {"universe": universe, "address": address},
        "value": value,
    }}}})
}

#[tokio::test]
async fn set_channel_reaches_the_state_stream_and_the_output() {
    let (mut socket, mock) = connect().await;

    send(&mut socket, &json!("Subscribe")).await;
    let picture = next(&mut socket).await;
    let universes = &picture["Picture"]["universes"];
    assert_eq!(universes.as_array().map(Vec::len), Some(2));
    assert!(
        universes[0]
            .as_array()
            .unwrap()
            .iter()
            .all(|level| level == 0)
    );

    send(&mut socket, &set(1, 3, 200)).await;
    assert_eq!(answer(&mut socket).await, json!("Accepted"));

    let expected = json!({"universe": 1, "address": 3, "value": 200});
    loop {
        let reply = next(&mut socket).await;
        let channels = &reply["Changed"]["channels"];
        if channels.as_array().is_some_and(|c| c.contains(&expected)) {
            break;
        }
    }
    // The state is published after the frame went out, so the output has it already.
    assert_eq!(mock.0[2].load(Ordering::Relaxed), 200);
}

#[tokio::test]
async fn a_request_that_cannot_be_right_is_refused_and_changes_nothing() {
    let (mut socket, mock) = connect().await;
    let refused = |reason: Value| json!({"Refused": {"reason": reason}});
    // (request, reply)
    let cases = [
        (
            set(3, 1, 9),
            refused(json!({"NoSuchUniverse": {"universes": 2}})),
        ),
        (
            set(0, 1, 9),
            refused(json!({"NoSuchUniverse": {"universes": 2}})),
        ),
        (set(1, 0, 9), refused(json!("NoSuchAddress"))),
        (set(1, 513, 9), refused(json!("NoSuchAddress"))),
        (set(1, 1, 256), refused(json!("NotACommand"))),
        (
            json!({"Send": {"command": "Tick"}}),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Send": {"command": "Explode"}}),
            refused(json!("NotACommand")),
        ),
        (json!("Hello"), refused(json!("NotACommand"))),
        (
            json!({"Blackout": {"on": true}}),
            refused(json!("NotACommand")),
        ),
    ];
    for (request, reply) in cases {
        send(&mut socket, &request).await;
        assert_eq!(answer(&mut socket).await, reply, "{request}");
    }
    socket.send(Message::text("not json")).await.unwrap();
    assert_eq!(answer(&mut socket).await, refused(json!("NotACommand")));

    tokio::time::sleep(Duration::from_millis(100)).await; // a few frames
    send(&mut socket, &json!("Query")).await;
    let picture = answer(&mut socket).await;
    let universes = picture["Picture"]["universes"].as_array().unwrap();
    let levels = universes.iter().flat_map(|u| u.as_array().unwrap());
    assert!(levels.into_iter().all(|level| level == 0));
    assert_eq!(picture["Picture"]["state"]["blackout"], json!(false));
    assert!(
        mock.0
            .iter()
            .all(|level| level.load(Ordering::Relaxed) == 0)
    );
}
