//! End to end: a client adds a DDP output over the WebSocket, sends "set channel", and
//! sees it in the state stream and in the bytes on the DDP socket in the next frame.

use engine::{Engine, Universes};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn connect() -> Socket {
    let (_, socket, _) = start().await;
    socket
}

/// A served engine: its address, a connection, and the server's task.
async fn start() -> (SocketAddr, Socket, JoinHandle<std::io::Result<Engine>>) {
    let (engine, sender) = Engine::start(Universes::new(2).unwrap()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let served = tokio::spawn(engine_server::serve(listener, engine, sender, None, None));
    (address, open(address).await, served)
}

async fn open(address: SocketAddr) -> Socket {
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
    // Frames computed while the command travelled may still lack it: read until one has it.
    let started = Instant::now();
    let carried = loop {
        let frame = packet(&udp).await;
        if frame[10 + 2] == 200 {
            break frame;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "channel 3 never arrived"
        );
    };
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
    let group = |members: usize| {
        let members = vec![json!({"Fixture": {"fixture": 1}}); members];
        json!({"Send": {"command": {"SetGroup": {"name": "g", "members": members}}}})
    };
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
            output(
                json!({"DmxUsbPro": {"adapter": 1}}),
                json!({"first": 1, "count": 2}),
                json!(null),
            ),
            refused(json!({"TooManyUniverses": {"most": 1}})),
        ),
        (
            output(
                json!({"DmxUsbPro": {"adapter": 0}}),
                json!({"first": 1, "count": 1}),
                json!(null),
            ),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Send": {"command": "Explode"}}),
            refused(json!("NotACommand")),
        ),
        // a group takes 512 members at most: the 512th is checked, the 513th is refused
        (group(512), refused(json!("NotPatched"))),
        (group(513), refused(json!({"TooManyMembers": {"most": 512}}))),
        (
            json!({"Send": {"command": {"SetGroup": {"name": "g", "members": [{"Group": {"group": "g"}}]}}}}),
            refused(json!("NotACommand")),
        ),
        (json!("Hello"), refused(json!("NotACommand"))),
        (
            json!({"Blackout": {"on": true}}),
            refused(json!("NotACommand")),
        ),
        // A key nobody knows, or a known one misspelled, at every level a client writes.
        (
            json!({"Send": {"command": {"SetChannel": {
                "channel": {"universe": 1, "address": 1}, "value": 200, "vlaue": 200,
            }}}}),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Send": {"command": {"SetChannel": {
                "channel": {"universe": 1, "address": 1, "adress": 2}, "value": 200,
            }}}}),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Send": {"command": {"Blackout": {"on": true, "off": true}}}}),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Send": {"command": {"ReleaseChannel": {
                "channel": {"universe": 1, "address": 1}, "all": true,
            }}}}),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Send": {"command": {"RemoveOutput": {"output": 1, "outputs": 2}}}}),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Send": {"command": {"SetOutput": {
                "output": 2, "kind": here.clone(), "universes": {"first": 1, "count": 1}, "Rate": "30",
            }}}}),
            refused(json!("NotACommand")),
        ),
        (
            output(
                here.clone(),
                json!({"first": 1, "count": 1, "last": 1}),
                json!(null),
            ),
            refused(json!("NotACommand")),
        ),
        (
            output(
                json!({"Ddp": {"address": address, "color": "Rgb", "colour": "Rgbw"}}),
                json!({"first": 1, "count": 1}),
                json!(null),
            ),
            refused(json!("NotACommand")),
        ),
        (
            output(
                json!({"DmxUsbPro": {"adapter": 1, "adaptor": 2}}),
                json!({"first": 1, "count": 1}),
                json!(null),
            ),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Send": {"command": {"Blackout": {"on": true}}, "now": true}}),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Send": {"command": {"Blackout": {"on": true}}}, "Query": null}),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Search": {"text": "dimmer", "limit": 1}}),
            refused(json!("NotACommand")),
        ),
        (
            json!({"Fixture": {"maker": "Generic", "model": "Dimmer", "mode": "1"}}),
            refused(json!("NotACommand")),
        ),
    ];
    for (request, reply) in cases {
        send(&mut socket, &request).await;
        assert_eq!(answer(&mut socket).await, reply, "{request}");
    }
    socket.send(Message::text("not json")).await.unwrap();
    assert_eq!(answer(&mut socket).await, refused(json!("NotACommand")));

    send(&mut socket, &json!("Query")).await;
    let picture = answer(&mut socket).await;
    let universes = picture["Picture"]["universes"].as_array().unwrap();
    let levels = universes.iter().flat_map(|u| u.as_array().unwrap());
    assert!(levels.into_iter().all(|level| level == 0));
    let state = &picture["Picture"]["state"];
    assert_eq!(state["blackout"], json!(false));
    assert_eq!(state["outputs"][1], json!(null), "no second output");
    // Forget the packets sent before the refused requests: the next one is computed after them.
    let mut stale = vec![0; 1500];
    while udp.try_recv(&mut stale).is_ok() {}
    let last = packet(&udp).await;
    assert!(last[10..].iter().all(|level| *level == 0));
}

/// A request that sets output 2.
fn output(kind: Value, universes: Value, rate: Value) -> Value {
    json!({"Send": {"command": {"SetOutput": {
        "output": 2, "kind": kind, "universes": universes, "rate": rate,
    }}}})
}

#[tokio::test]
async fn the_plain_routes_answer_in_text() {
    let (address, _socket, _served) = start().await;
    let version = engine::API_VERSION.to_string();
    // (route, body)
    let cases = [("/health", "ok"), ("/version", version.as_str())];
    for (route, body) in cases {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!("GET {route} HTTP/1.0\r\nHost: {address}\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        let lower = response.to_lowercase();
        assert!(lower.starts_with("http/1.0 200"), "{route}: {response}");
        assert!(
            lower.contains("access-control-allow-origin: *"),
            "{route}: {response}"
        );
        assert!(
            response.ends_with(&format!("\r\n\r\n{body}")),
            "{route}: {response}"
        );
    }
}

/// What a subscriber saw of one frame: the state, and the levels it built from the stream.
type Seen = BTreeMap<u64, (Value, Vec<Vec<Value>>)>;

/// Reads a subscriber's stream until channel 1 of universe 1 is `value`, keeping each frame.
async fn see_until(socket: &mut Socket, levels: &mut [Vec<Value>], seen: &mut Seen, value: u16) {
    while levels[0][0] != json!(value) {
        let reply = next(socket).await;
        let Some(changed) = reply.get("Changed") else {
            continue;
        };
        for channel in changed["channels"].as_array().unwrap() {
            let universe = channel["universe"].as_u64().unwrap() as usize - 1;
            let address = channel["address"].as_u64().unwrap() as usize - 1;
            levels[universe][address] = channel["value"].clone();
        }
        let frame = changed["state"]["frame"].as_u64().unwrap();
        seen.insert(frame, (changed["state"].clone(), levels.to_vec()));
    }
}

#[tokio::test]
async fn every_subscriber_gets_the_same_picture_of_a_frame() {
    // (subscribers)
    for subscribers in [2, 3] {
        let (address, mut commands, _served) = start().await;
        let (_udp, add) = receiver().await;
        send(&mut commands, &add).await;
        assert_eq!(answer(&mut commands).await, json!("Accepted"));
        let mut watching = vec![];
        for _ in 0..subscribers {
            let mut socket = open(address).await;
            send(&mut socket, &json!("Subscribe")).await;
            let picture = next(&mut socket).await;
            let universes = picture["Picture"]["universes"].as_array().unwrap();
            let levels: Vec<Vec<Value>> = universes
                .iter()
                .map(|u| u.as_array().unwrap().clone())
                .collect();
            watching.push((socket, levels, Seen::new()));
        }
        for value in 1..=20 {
            send(&mut commands, &set(1, 1, value)).await;
            assert_eq!(answer(&mut commands).await, json!("Accepted"));
            for (socket, levels, seen) in &mut watching {
                see_until(socket, levels, seen, value).await;
            }
        }
        let (_, _, first) = &watching[0];
        for (n, (_, _, seen)) in watching.iter().enumerate().skip(1) {
            let both: Vec<_> = seen
                .keys()
                .filter(|frame| first.contains_key(frame))
                .collect();
            assert!(
                both.len() >= 10,
                "{subscribers}: subscriber {n} shares {} frames",
                both.len()
            );
            for frame in both {
                assert_eq!(
                    seen[frame], first[frame],
                    "{subscribers}: subscriber {n}, frame {frame}"
                );
            }
        }
    }
}

#[tokio::test]
async fn stop_answers_stopping_closes_every_connection_and_ends_the_server() {
    let (address, mut socket, served) = start().await;
    let mut subscribed = open(address).await;
    send(&mut subscribed, &json!("Subscribe")).await;
    assert!(next(&mut subscribed).await.get("Picture").is_some());

    send(&mut socket, &json!("Stop")).await;
    assert_eq!(answer(&mut socket).await, json!("Stopping"));
    for connection in [&mut socket, &mut subscribed] {
        loop {
            let message = tokio::time::timeout(Duration::from_secs(2), connection.next())
                .await
                .expect("closed within 2 s");
            match message {
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                Some(Ok(_)) => {}
            }
        }
    }
    let ended = tokio::time::timeout(Duration::from_secs(2), served)
        .await
        .expect("the server ends within 2 s");
    assert!(matches!(ended, Ok(Ok(_))), "the server ended with an error");
    assert!(TcpStream::connect(address).await.is_err(), "nobody listens");
}

#[tokio::test]
async fn only_the_app_and_the_dev_ui_may_open_the_api() {
    use tokio_tungstenite::tungstenite::Error;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::{HeaderValue, StatusCode};
    let (address, _socket, _served) = start().await;
    // (the page's origin, or none, and the answer to the handshake)
    let cases = [
        (None, StatusCode::SWITCHING_PROTOCOLS), // the shell, tests, scripts
        (Some("tauri://localhost"), StatusCode::SWITCHING_PROTOCOLS),
        (
            Some("http://tauri.localhost"),
            StatusCode::SWITCHING_PROTOCOLS,
        ),
        (
            Some("http://127.0.0.1:5174"),
            StatusCode::SWITCHING_PROTOCOLS,
        ),
        (
            Some("http://127.0.0.1:5299"),
            StatusCode::SWITCHING_PROTOCOLS,
        ),
        (Some("https://evil.example"), StatusCode::FORBIDDEN),
        (Some("http://127.0.0.1:5175"), StatusCode::FORBIDDEN), // the catalog needs none
        (Some("http://localhost:5174"), StatusCode::FORBIDDEN),
        (Some("http://127.0.0.1:51740"), StatusCode::FORBIDDEN),
        (Some("null"), StatusCode::FORBIDDEN), // a sandboxed page or a file
        (
            Some("tauri://localhost.evil.example"),
            StatusCode::FORBIDDEN,
        ),
    ];
    for (origin, expected) in cases {
        let mut request = format!("ws://{address}/api").into_client_request().unwrap();
        if let Some(origin) = origin {
            let origin = HeaderValue::from_static(origin);
            request.headers_mut().insert("origin", origin);
        }
        let status = match tokio_tungstenite::connect_async(request).await {
            Ok((_, response)) => response.status(),
            Err(Error::Http(response)) => response.status(),
            Err(error) => panic!("{origin:?}: {error}"),
        };
        assert_eq!(status, expected, "{origin:?}");
    }
}

// Threads of its own: if an open blocked the server, the test still runs and fails.
#[tokio::test(flavor = "multi_thread")]
async fn a_device_that_opens_slowly_stalls_no_other_client_and_the_project_shows_its_kind() {
    // The device of output 1 opens only once the test says so.
    let (opening, opened) = std::sync::mpsc::channel::<()>();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let slow_open: engine::Open = Box::new(move |kind| {
        opening.send(()).unwrap();
        // Never longer than 5 s: a server that waits for it still ends the test.
        let _ = released.recv_timeout(Duration::from_secs(5));
        outputs::open(kind)
    });
    let (engine, sender) = Engine::start_with(Universes::new(2).unwrap(), slow_open).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(engine_server::serve(listener, engine, sender, None, None));
    let (mut slow, mut other) = (open(address).await, open(address).await);
    let (_udp, add) = receiver().await;

    send(&mut slow, &add).await;
    tokio::task::spawn_blocking(move || opened.recv().unwrap())
        .await
        .unwrap();
    // While output 1 opens: (request of another client, its answer)
    let add_2 = json!({"Send": {"command": {"SetOutput": {
        "output": 2,
        "kind": add["Send"]["command"]["SetOutput"]["kind"],
        "universes": {"first": 1, "count": 1},
        "rate": null,
    }}}});
    let remove_1 = json!({"Send": {"command": {"RemoveOutput": {"output": 1}}}});
    let refused = |reason: &str| json!({"Refused": {"reason": reason}});
    let cases = [
        (set(1, 1, 9), json!("Accepted")),
        (add_2, refused("Opening")),
        (remove_1, refused("Opening")),
    ];
    for (request, expected) in cases {
        send(&mut other, &request).await;
        assert_eq!(answer(&mut other).await, expected, "{request}");
    }
    send(&mut other, &json!("Query")).await;
    let outputs = answer(&mut other).await["Picture"]["project"]["outputs"].clone();
    assert_eq!(outputs[0], json!(null), "not open yet");

    release.send(()).unwrap();
    assert_eq!(answer(&mut slow).await, json!("Accepted"));
    // The project is in each frame's picture: the next frame shows the kind.
    let kind = &add["Send"]["command"]["SetOutput"]["kind"];
    let universes = &add["Send"]["command"]["SetOutput"]["universes"];
    let started = Instant::now();
    loop {
        send(&mut other, &json!("Query")).await;
        let outputs = answer(&mut other).await["Picture"]["project"]["outputs"].clone();
        if outputs[0] == json!({"Kind": {"kind": kind, "universes": universes}}) {
            break;
        }
        assert_eq!(outputs[0], json!(null), "only the kind comes after none");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no frame shows the kind"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn no_picture_shows_an_output_in_the_state_and_not_in_the_project_or_the_other_way() {
    let (address, mut commander, _) = start().await;
    let mut subscriber = open(address).await;
    let (_udp, add) = receiver().await;
    let remove = json!({"Send": {"command": {"RemoveOutput": {"output": 1}}}});

    send(&mut subscriber, &json!("Subscribe")).await;
    let watched = tokio::spawn(async move {
        // (pictures seen, the first torn one: frame, in the state, in the project)
        let (mut seen, mut torn) = (0, Vec::new());
        while let Ok(Some(Ok(message))) =
            tokio::time::timeout(Duration::from_secs(1), subscriber.next()).await
        {
            let Ok(reply) = serde_json::from_str::<Value>(message.to_text().unwrap_or("")) else {
                break;
            };
            let Some(picture) = reply.get("Picture").or_else(|| reply.get("Changed")) else {
                continue;
            };
            seen += 1;
            let state = !picture["state"]["outputs"][0].is_null();
            let project = !picture["project"]["outputs"][0].is_null();
            if state != project {
                torn.push((picture["state"]["frame"].clone(), state, project));
            }
        }
        (seen, torn)
    });
    // Each change is seen in a picture, then a few more queries put the next change at
    // another point between two reads of the engine.
    for round in 0..100 {
        for (request, running) in [(&add, true), (&remove, false)] {
            send(&mut commander, request).await;
            assert_eq!(answer(&mut commander).await, json!("Accepted"));
            let mut more = round * 7 % 64;
            loop {
                send(&mut commander, &json!("Query")).await;
                let picture = answer(&mut commander).await;
                if picture["Picture"]["state"]["outputs"][0].is_null() != running {
                    if more == 0 {
                        break;
                    }
                    more -= 1;
                }
            }
        }
    }
    send(&mut commander, &json!("Stop")).await;
    let (seen, torn) = watched.await.unwrap();
    assert!(seen > 100, "only {seen} pictures");
    assert!(
        torn.is_empty(),
        "{} of {seen} pictures show output 1 in one of state and project only, first: {:?}",
        torn.len(),
        torn.first()
    );
}
