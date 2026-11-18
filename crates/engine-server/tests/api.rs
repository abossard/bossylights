//! End to end: a client adds a DDP output over the WebSocket, sends "set channel", and
//! sees it in the state stream and in the bytes on the DDP socket in the next frame.

use engine::{Engine, Universes};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::net::{TcpStream, UdpSocket};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn connect() -> Socket {
    let (engine, sender) = Engine::start(Universes::new(2).unwrap()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, engine_server::api(engine, sender)).await });
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/api"))
        .await
        .unwrap();
    socket
}

/// A DDP receiver here, and the request that adds it as output 1 for universe 1, RGB.
async fn receiver() -> (UdpSocket, Value) {
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = udp.local_addr().unwrap().to_string();
    let add = json!({"Send": {"command": {"SetOutput": {
        "output": 1,
        "kind": {"Ddp": {"address": address, "color": "Rgb"}},
        "universes": {"first": 1, "count": 1},
        "rate": null,
    }}}});
    (udp, add)
}

/// The next DDP packet: header and data.
async fn packet(udp: &UdpSocket) -> Vec<u8> {
    let mut packet = vec![0; 1500];
    let size = tokio::time::timeout(Duration::from_secs(2), udp.recv(&mut packet))
        .await
        .expect("a packet within 2 s")
        .unwrap();
    packet.truncate(size);
    packet
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
async fn set_channel_reaches_the_state_stream_and_the_ddp_socket_in_the_next_frame() {
    let mut socket = connect().await;
    let (udp, add) = receiver().await;

    send(&mut socket, &json!("Subscribe")).await;
    let picture = next(&mut socket).await;
    let universes = &picture["Picture"]["universes"];
    assert_eq!(universes.as_array().map(Vec::len), Some(2));
    assert_eq!(picture["Picture"]["state"]["outputs"][0], json!(null));

    send(&mut socket, &add).await;
    assert_eq!(answer(&mut socket).await, json!("Accepted"));
    let first = packet(&udp).await;
    assert_eq!(
        first[..4],
        [0x41, 1, 0x0B, 1],
        "flags, sequence, RGB, destination"
    );
    assert_eq!(first.len(), 10 + 510, "170 RGB pixels of universe 1");
    assert!(first[10..].iter().all(|level| *level == 0));

    // Forget the packets sent so far: the next one is computed after the command left.
    let mut stale = vec![0; 1500];
    while udp.try_recv(&mut stale).is_ok() {}
    send(&mut socket, &set(1, 3, 200)).await;
    assert_eq!(answer(&mut socket).await, json!("Accepted"));
    // The frame computed while the command travelled may still lack it; the next has it.
    let mut frames = Vec::new();
    for _ in 0..2 {
        frames.push(packet(&udp).await);
        if frames.last().is_some_and(|p| p[10 + 2] == 200) {
            break;
        }
    }
    let carried = frames.last().unwrap();
    assert_eq!(carried[10 + 2], 200, "channel 3 within the next frame");
    let others = carried[10..].iter().enumerate().filter(|(n, _)| *n != 2);
    assert!(others.into_iter().all(|(_, level)| *level == 0));

    let expected = json!({"universe": 1, "address": 3, "value": 200});
    loop {
        let reply = next(&mut socket).await;
        let changed = &reply["Changed"];
        if changed["channels"]
            .as_array()
            .is_some_and(|c| c.contains(&expected))
        {
            let output = &changed["state"]["outputs"][0];
            assert_eq!(output["status"], json!("Running"));
            assert_eq!(output["rate"], json!("60"), "the default for DDP");
            assert!(output["counters"]["accepted"].as_u64() > Some(0));
            break;
        }
    }
}

#[tokio::test]
async fn a_request_that_cannot_be_right_is_refused_and_changes_nothing() {
    let mut socket = connect().await;
    let (udp, add) = receiver().await;
    send(&mut socket, &add).await;
    assert_eq!(answer(&mut socket).await, json!("Accepted"));
    let refused = |reason: Value| json!({"Refused": {"reason": reason}});
    let address = udp.local_addr().unwrap().to_string();
    let here = json!({"Ddp": {"address": address, "color": "Rgb"}});
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
            json!({"Send": {"command": {"Tick": {"now": 0}}}}),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Send": {"command": {"Handed": {"output": 1, "outcome": "Failed"}}}}),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Send": {"command": {"RemoveOutput": {"output": 0}}}}),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Send": {"command": {"RemoveOutput": {"output": 17}}}}),
            refused(json!("NotACommand")),
        ),
        (
            output(here.clone(), json!({"first": 2, "count": 2}), json!(null)),
            refused(json!({"NoSuchUniverse": {"universes": 2}})),
        ),
        (
            output(here.clone(), json!({"first": 1, "count": 1}), json!("50")),
            refused(json!("NotACommand")),
        ),
        (
            output(json!("Mock"), json!({"first": 1, "count": 1}), json!(null)),
            refused(json!("NotACommand")),
        ),
        (
            output(
                json!({"Ddp": {"address": "wled.local:4048", "color": "Rgb"}}),
                json!({"first": 1, "count": 1}),
                json!(null),
            ),
            refused(json!("NotACommand")),
        ),
        (
            output(
                json!({"Ddp": {"address": "127.0.0.1:0", "color": "Rgb"}}),
                json!({"first": 1, "count": 1}),
                json!(null),
            ),
            refused(json!("CannotOpen")),
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
    let state = &picture["Picture"]["state"];
    assert_eq!(state["blackout"], json!(false));
    assert_eq!(state["outputs"][1], json!(null), "no second output");
    let last = packet(&udp).await;
    assert!(last[10..].iter().all(|level| *level == 0));
}

/// A request that sets output 2.
fn output(kind: Value, universes: Value, rate: Value) -> Value {
    json!({"Send": {"command": {"SetOutput": {
        "output": 2, "kind": kind, "universes": universes, "rate": rate,
    }}}})
}
