//! End to end over the API (#163): a client reads back what a light holds. The Generic
//! RGB PAR at 1:37 is light 1. Each step's command is in the picture a `Query` answers
//! and in the lights a subscriber's `Changed` carries: set, set, release, release.

mod common;

use common::taken;
use engine::{Engine, Universes};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn send(socket: &mut Socket, request: &Value) {
    socket
        .send(Message::text(request.to_string()))
        .await
        .unwrap();
}

async fn next(socket: &mut Socket) -> Value {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("a message within 5 s")
        .unwrap()
        .unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

async fn command(socket: &mut Socket, command: Value) -> Value {
    send(socket, &json!({"Send": {"command": command}})).await;
    next(socket).await
}

async fn serve() -> SocketAddr {
    let (engine, sender) = Engine::start(Universes::ONE).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let definitions =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixture-library/tests/definitions");
    tokio::spawn(engine_server::serve(
        listener,
        engine,
        sender,
        Some(definitions),
        None,
    ));
    address
}

async fn connect(address: SocketAddr) -> Socket {
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/api"))
        .await
        .unwrap();
    socket
}

/// Asks until the picture's lights are `wanted`: the frame after a command may not be
/// taken yet.
async fn until_queried(socket: &mut Socket, wanted: &Value) {
    let started = Instant::now();
    loop {
        send(socket, &json!("Query")).await;
        let seen = next(socket).await["Picture"]["lights"].clone();
        if seen == *wanted {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the lights: {seen}, wanted {wanted}"
        );
    }
}

/// Reads `Changed` until light 1, as the subscriber last heard it, is `wanted`. Every
/// light a `Changed` carries changed: a light it did not hold before, or held otherwise.
async fn until_heard(socket: &mut Socket, heard: &mut Value, wanted: &Value) {
    let started = Instant::now();
    while heard != wanted {
        let reply = next(socket).await;
        for light in reply["Changed"]["lights"].as_array().unwrap() {
            assert_eq!(light["light"], json!(1), "only light 1 changes: {light}");
            assert_ne!(light, heard, "carried, but not changed");
            *heard = light.clone();
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "light 1: {heard}, wanted {wanted}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_reads_back_what_a_light_holds_after_each_set_and_release() {
    let address = serve().await;
    let (mut asker, mut subscriber) = (connect(address).await, connect(address).await);
    let par = json!({"Patch": {
        "name": "PAR 1", "maker": "Generic", "model": "Generic RGB", "mode": "RGB",
        "at": {"universe": 1, "address": 37},
    }});
    let started = Instant::now();
    while command(&mut asker, par.clone()).await != json!({"Patched": {"fixture": 1}}) {
        assert!(started.elapsed() < Duration::from_secs(10), "no library");
    }
    until_queried(&mut asker, &json!([])).await;
    send(&mut subscriber, &json!("Subscribe")).await;
    assert_eq!(next(&mut subscriber).await["Picture"]["lights"], json!([]));
    let mut heard = json!({"light": 1, "intensity": null, "colour": null});
    let target = json!({"Fixture": {"fixture": 1}});
    let red = json!({"red": 65535, "green": 0, "blue": 0});
    // (command, light 1 after it)
    let steps = [
        (
            json!({"SetColour": {"target": target, "red": 65535, "green": 0, "blue": 0}}),
            json!({"light": 1, "intensity": null, "colour": red}),
        ),
        (
            json!({"SetIntensity": {"target": target, "intensity": 32768}}),
            json!({"light": 1, "intensity": 32768, "colour": red}),
        ),
        (
            json!({"ReleaseColour": {"target": target}}),
            json!({"light": 1, "intensity": 32768, "colour": null}),
        ),
        (
            json!({"ReleaseIntensity": {"target": target}}),
            json!({"light": 1, "intensity": null, "colour": null}),
        ),
    ];
    for (step, light) in steps {
        assert!(taken(&command(&mut asker, step.clone()).await), "{step}");
        let held = if light["intensity"].is_null() && light["colour"].is_null() {
            json!([])
        } else {
            json!([light])
        };
        until_queried(&mut asker, &held).await;
        until_heard(&mut subscriber, &mut heard, &light).await;
    }
}
