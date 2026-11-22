//! End to end: a client adds a DDP output over the WebSocket, sends "set channel", and
//! sees it in the state stream and in the bytes on the DDP socket in the next frame.

mod common;

use common::{Socket, answer, connect as open, send};
use engine::{Engine, Universes};
use futures_util::{FutureExt, SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

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

/// The next message from the engine, as JSON.
async fn next(socket: &mut Socket) -> Value {
    let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("a message within 2 s")
        .unwrap()
        .unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

fn set(universe: u16, address: u16, value: u16) -> Value {
    json!({"Send": {"command": {"SetChannel": {
        "channel": {"universe": universe, "address": address},
        "value": value,
    }}}})
}

#[tokio::test]
async fn subscribed_requests_skip_queued_health() {
    let (address, mut desk, _) = start().await;
    let (udp, attach) = receiver().await;
    let mut clients = [open(address).await, open(address).await];
    for socket in &mut clients {
        send(socket, &json!("Subscribe")).await;
        assert!(next(socket).await.get("Picture").is_some());
    }
    send(&mut desk, &attach).await;
    assert_eq!(answer(&mut desk).await, "Accepted");
    assert_eq!(&packet(&udp).await[10..], &[0; 510]);

    let mut received = Vec::new();
    for (socket, request) in clients.into_iter().zip([set(1, 1, 91), json!("Query")]) {
        let mut socket = std::pin::pin!(socket.peekable());
        // Leave the first Health at the head of this client's queue.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let message = socket.as_mut().peek().await.unwrap().as_ref().unwrap();
                let reply: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                if reply.get("Health").is_some() {
                    break;
                }
                socket.next().await.unwrap().unwrap();
            }
        })
        .await
        .expect("the first Health after attach");
        socket
            .send(Message::text(request.to_string()))
            .await
            .unwrap();
        let reply = answer(&mut socket).await;
        received.push(if reply.get("Picture").is_some() {
            json!("Picture")
        } else {
            reply
        });
    }
    assert_eq!(received, [json!("Accepted"), json!("Picture")]);
    tokio::time::timeout(Duration::from_secs(5), async {
        while packet(&udp).await[10] != 91 {}
    })
    .await
    .expect("channel 1 reaches the DDP output");
}

/// docs/engine.md: health at most once per 500 ms of engine time. The first Health may
/// picture an older time than the pictures after it; the second still waits 500 ms from
/// the first one's engine time. That a snapshot requested before a send never starts the
/// wait is the `HealthPacing` table's cell (`health_pacing_follows_its_table`).
#[test]
fn a_second_health_comes_no_closer_than_500_ms_of_engine_time_to_the_first() {
    let reader = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let sockets = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    for request in ["Query", "Subscribe"] {
        sockets.block_on(check_health_spacing(request, &reader));
    }
}

async fn check_health_spacing(request: &str, reader: &tokio::runtime::Runtime) {
    async fn published(desk: &mut Socket, wakes: &engine::Wakes, at: Duration) {
        let previous = wakes.woke();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                assert_eq!(wakes.wake_at(at).unwrap(), at);
                send(desk, &json!("Query")).await;
                let picture = answer(desk).await;
                if picture["Picture"]["state"]["frame"].as_u64().unwrap() > previous {
                    break;
                }
            }
        })
        .await
        .expect("the reader publishes a new frame");
    }

    let (first_udp, first_attach) = receiver().await;
    let (second_udp, mut second_attach) = receiver().await;
    second_attach["Send"]["command"]["SetOutput"]["output"] = json!(2);
    let slow_address = second_udp.local_addr().unwrap();
    let (opening, opened) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let slow_open: engine::Open = Box::new(move |kind| {
        if matches!(kind, engine::OutputKind::Ddp { address, .. } if SocketAddr::V4(address) == slow_address)
        {
            opening.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        outputs::open(kind)
    });
    let (engine, sender, wakes) = Engine::start_stepped(Universes::ONE, slow_open).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut serving = Box::pin(engine_server::serve(listener, engine, sender, None, None));
    // Start the reader on its own runtime. Once serve waits for a connection,
    // the sockets can run even while the test holds the reader between polls.
    {
        let _reader = reader.enter();
        assert!(serving.as_mut().now_or_never().is_none());
    }
    let served = tokio::spawn(serving);
    let (mut desk, mut subscriber) = (open(address).await, open(address).await);
    tokio::task::block_in_place(|| wakes.hold()).unwrap();
    let before = tokio::task::block_in_place(|| wakes.wake()).unwrap();
    let publish = |desk: &mut Socket, at| {
        tokio::task::block_in_place(|| reader.block_on(published(desk, &wakes, at)));
    };
    send(&mut subscriber, &json!("Subscribe")).await;
    assert_eq!(
        next(&mut subscriber).await["Picture"]["health"]["outputs"],
        json!([])
    );
    // Sample the Picture's delivery time before the socket blocks on the open.
    send(&mut subscriber, &json!("Query")).await;
    assert!(answer(&mut subscriber).await.get("Picture").is_some());
    for _ in 0..2 {
        publish(&mut desk, before);
    }
    send(&mut subscriber, &json!("Query")).await;
    assert!(answer(&mut subscriber).await.get("Picture").is_some());

    for request in [set(1, 1, 91), first_attach] {
        send(&mut desk, &request).await;
        assert_eq!(answer(&mut desk).await, "Accepted");
    }
    // The open waits on the subscriber's worker: its socket keeps sending pictures and health.
    send(&mut subscriber, &second_attach).await;
    tokio::task::spawn_blocking(move || opened.recv_timeout(Duration::from_secs(5)))
        .await
        .unwrap()
        .unwrap();
    let first = before + Duration::from_secs(1);
    publish(&mut desk, first);
    assert_eq!(packet(&first_udp).await[10], 91);

    // Fill the reader's outstanding request at the old time. Leave the reader
    // paused so it can only collect that snapshot after the first Health.
    assert_eq!(wakes.wake_at(first).unwrap(), first);
    // The first Health pictures `first` at the latest: no second one before
    // `first` + 500 ms, whichever snapshot starts the wait.
    let now = first + Duration::from_millis(400);
    wakes.set_time(now);
    release.send(()).unwrap();
    // The first Health may go out before the open's reply, or after it.
    let (mut accepted, mut health) = (false, false);
    while !(accepted && health) {
        let reply = next(&mut subscriber).await;
        if let Some(reply) = reply.get("Health") {
            let rows = reply["health"]["outputs"].as_array().unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0]["output"], 1);
            health = true;
        } else if reply == "Accepted" {
            accepted = true;
        }
    }
    // A reply on the same socket confirms the preceding send has finished.
    send(&mut subscriber, &json!(request)).await;
    let frame = answer(&mut subscriber).await["Picture"]["state"]["frame"]
        .as_u64()
        .expect("the frame before the reader resumes");
    // Let the socket see the in-flight snapshot before granting a fresh wake.
    tokio::task::block_in_place(|| {
        reader.block_on(async {
            loop {
                let reply = next(&mut subscriber).await;
                if reply["Changed"]["state"]["frame"]
                    .as_u64()
                    .is_some_and(|next_frame| next_frame > frame)
                {
                    break;
                }
            }
        });
    });
    send(&mut subscriber, &json!("Query")).await;
    assert!(answer(&mut subscriber).await.get("Picture").is_some());
    publish(&mut desk, now);
    assert_eq!(packet(&second_udp).await[10], 91);
    let due = now + Duration::from_millis(500);
    for at in [now, first + Duration::from_millis(500) - Duration::from_nanos(1)] {
        // At `now` this is a fresh snapshot requested after the first Health.
        publish(&mut desk, at);
        // One Query can race the new frame. The next drains any Health the
        // socket sent once that frame was already available to it.
        for _ in 0..2 {
            send(&mut subscriber, &json!("Query")).await;
            loop {
                let reply = next(&mut subscriber).await;
                assert!(
                    reply.get("Health").is_none(),
                    "second Health {:?} after the first ({request}): {reply}",
                    at - first
                );
                if reply.get("Picture").is_some() {
                    break;
                }
            }
        }
    }
    publish(&mut desk, due);
    loop {
        if let Some(health) = next(&mut subscriber).await.get("Health") {
            let rows = health["health"]["outputs"].as_array().unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[1]["output"], 2);
            break;
        }
    }
    send(&mut desk, &json!("Stop")).await;
    assert_eq!(answer(&mut desk).await, "Stopping");
    tokio::task::block_in_place(|| reader.block_on(served))
        .unwrap()
        .unwrap();
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
            assert_eq!(output["rate"], json!("60"), "the default for DDP");
            assert!(output["counters"]["accepted"].as_u64() > Some(0));
            break;
        }
    }
    // The packet is checked above; the status may come a wake later: what came of a frame is
    // counted when its box comes back from the output's thread (#174).
    let started = Instant::now();
    loop {
        let reply = next(&mut socket).await;
        if reply["Changed"]["state"]["outputs"][0]["status"] == json!("Running") {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the output never ran"
        );
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
            refused(json!({"CannotOpen": {"why": {"Device": {"with": null}}}})),
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
        (
            group(513),
            refused(json!({"TooManyMembers": {"most": 512}})),
        ),
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
    let version = wire::API_VERSION.to_string();
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
        // the end-to-end page, only by BOSSYLIGHTS_ORIGINS
        (Some("http://127.0.0.1:5299"), StatusCode::FORBIDDEN),
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

/// The socket table in docs/engine.md: a press after a pending edit on the same
/// connection runs at once, against the state before that edit.
#[tokio::test(flavor = "multi_thread")]
async fn a_press_behind_a_pending_open_on_its_connection_reaches_the_output_at_once() {
    let (opening, opened) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let mut opening = Some(opening);
    let mut calls = 0;
    let open_device: engine::Open = Box::new(move |kind| {
        calls += 1;
        if calls == 2 {
            opening.take().unwrap().send(()).unwrap();
            let _ = released.recv();
        }
        outputs::open(kind)
    });
    let (engine, sender) = Engine::start_with(Universes::new(2).unwrap(), open_device).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let served = tokio::spawn(engine_server::serve(listener, engine, sender, None, None));
    let (mut desk, mut other) = (open(address).await, open(address).await);
    let (udp, first) = receiver().await;
    send(&mut desk, &first).await;
    assert_eq!(answer(&mut desk).await, "Accepted");

    let (_second_udp, mut second) = receiver().await;
    second["Send"]["command"]["SetOutput"]["output"] = json!(2);
    second["Send"]["command"]["SetOutput"]["universes"]["first"] = json!(2);
    send(&mut desk, &second).await;
    tokio::time::timeout(Duration::from_secs(5), opened)
        .await
        .unwrap()
        .unwrap();
    send(&mut desk, &set(1, 1, 123)).await;
    send(&mut desk, &set(1, 513, 9)).await;
    send(&mut other, &set(1, 2, 42)).await;
    assert_eq!(answer(&mut other).await, "Accepted");

    let mut seen = [0, 0];
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        while seen != [123, 42] {
            let frame = packet(&udp).await;
            seen.copy_from_slice(&frame[10..12]);
        }
    })
    .await;
    release.send(()).unwrap();
    // the replies in request order: the edit, the press, the refused press
    assert_eq!(answer(&mut desk).await, "Accepted");
    assert_eq!(answer(&mut desk).await, "Accepted");
    assert_eq!(
        answer(&mut desk).await,
        json!({"Refused": {"reason": "NoSuchAddress"}})
    );
    send(&mut other, &json!("Stop")).await;
    assert_eq!(answer(&mut other).await, "Stopping");
    let _ = served.await.unwrap().unwrap();
    assert_eq!(seen, [123, 42], "DDP bytes while output 2 is paused opening");
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

/// The socket table in docs/engine.md: a device open pending on a connection holds none
/// of that connection's pictures, and its replies go out in the order of its requests.
#[tokio::test(flavor = "multi_thread")]
async fn a_pending_open_holds_no_picture_of_its_connection_and_replies_keep_their_order() {
    let (opening, opened) = std::sync::mpsc::channel::<()>();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let slow_open: engine::Open = Box::new(move |kind| {
        opening.send(()).unwrap();
        let _ = released.recv_timeout(Duration::from_secs(5));
        outputs::open(kind)
    });
    let (engine, sender) = Engine::start_with(Universes::ONE, slow_open).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let served = tokio::spawn(engine_server::serve(listener, engine, sender, None, None));
    let (mut desk, mut other) = (open(address).await, open(address).await);
    send(&mut desk, &json!("Subscribe")).await;
    assert!(answer(&mut desk).await.get("Picture").is_some());
    let (_udp, attach) = receiver().await;
    send(&mut desk, &attach).await;
    tokio::task::spawn_blocking(move || opened.recv_timeout(Duration::from_secs(5)))
        .await
        .unwrap()
        .unwrap();
    send(&mut desk, &json!("Query")).await;
    send(&mut other, &set(1, 7, 77)).await;
    assert_eq!(answer(&mut other).await, "Accepted");

    let expected = json!({"universe": 1, "address": 7, "value": 77});
    loop {
        let reply = next(&mut desk).await;
        assert!(reply.get("Changed").is_some(), "only pictures before the open ends: {reply}");
        if reply["Changed"]["channels"]
            .as_array()
            .is_some_and(|channels| channels.contains(&expected))
        {
            break;
        }
    }
    release.send(()).unwrap();
    assert_eq!(answer(&mut desk).await, "Accepted");
    assert!(answer(&mut desk).await.get("Picture").is_some());
    send(&mut other, &json!("Stop")).await;
    assert_eq!(answer(&mut other).await, "Stopping");
    served.await.unwrap().unwrap();
}

/// docs/coding.md rule 6: work that panics answers `Failed`, never `Stopping`, and the
/// engine goes on answering the same connection.
#[tokio::test(flavor = "multi_thread")]
async fn a_prepare_that_panics_answers_failed_and_the_next_press_is_taken() {
    let (address, mut desk, served) = start().await;
    let remove = json!({"Send": {"command": {"RemoveGroup": {"name": engine_server::PANICS}}}});
    send(&mut desk, &remove).await;
    assert_eq!(answer(&mut desk).await, "Failed");
    send(&mut desk, &set(1, 1, 5)).await;
    assert_eq!(answer(&mut desk).await, "Accepted");
    let mut other = open(address).await;
    send(&mut other, &json!("Stop")).await;
    assert_eq!(answer(&mut other).await, "Stopping");
    served.await.unwrap().unwrap();
}

/// A server whose device opens wait until the test releases them, and the release.
async fn held_open() -> (
    SocketAddr,
    JoinHandle<std::io::Result<Engine>>,
    std::sync::mpsc::Receiver<()>,
    std::sync::mpsc::Sender<()>,
) {
    let (opening, opened) = std::sync::mpsc::channel::<()>();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let slow_open: engine::Open = Box::new(move |kind| {
        opening.send(()).unwrap();
        let _ = released.recv_timeout(Duration::from_secs(5));
        outputs::open(kind)
    });
    let (engine, sender) = Engine::start_with(Universes::ONE, slow_open).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let served = tokio::spawn(engine_server::serve(listener, engine, sender, None, None));
    (address, served, opened, release)
}

/// Sends the attach of a DDP output on `desk` and waits until its device open began.
async fn attach_held(desk: &mut Socket, opened: std::sync::mpsc::Receiver<()>) -> UdpSocket {
    let (udp, attach) = receiver().await;
    send(desk, &attach).await;
    tokio::task::spawn_blocking(move || opened.recv_timeout(Duration::from_secs(5)))
        .await
        .unwrap()
        .unwrap();
    udp
}

/// The socket table in docs/engine.md: at most 32 requests of a connection wait. A `Stop`
/// past them is not read until a reply goes out, so the engine runs on meanwhile.
#[tokio::test(flavor = "multi_thread")]
async fn a_request_past_the_32_waiting_is_not_read_until_a_reply_goes_out() {
    let (address, served, opened, release) = held_open().await;
    let (mut desk, mut other) = (open(address).await, open(address).await);
    let _udp = attach_held(&mut desk, opened).await;
    for _ in 1..32 {
        send(&mut desk, &json!("Query")).await;
    }
    send(&mut desk, &json!("Stop")).await;
    send(&mut other, &set(1, 1, 5)).await;
    assert_eq!(answer(&mut other).await, "Accepted", "the Stop was read");
    release.send(()).unwrap();
    assert_eq!(answer(&mut desk).await, "Accepted", "the attach");
    for query in 1..32 {
        assert!(answer(&mut desk).await.get("Picture").is_some(), "query {query}");
    }
    assert_eq!(answer(&mut desk).await, "Stopping");
    served.await.unwrap().unwrap();
}

/// The socket table in docs/engine.md: two commands on one connection's worker answer in
/// the order they came, whichever answer is ready first.
#[tokio::test(flavor = "multi_thread")]
async fn commands_on_one_connections_worker_answer_in_the_order_they_came() {
    let (address, served, opened, release) = held_open().await;
    let mut desk = open(address).await;
    let _udp = attach_held(&mut desk, opened).await;
    let unpatch = json!({"Send": {"command": {"Unpatch": {"fixture": 9}}}});
    send(&mut desk, &unpatch).await;
    send(&mut desk, &json!("Query")).await;
    release.send(()).unwrap();
    assert_eq!(answer(&mut desk).await, "Accepted", "the attach");
    assert_eq!(
        answer(&mut desk).await,
        json!({"Refused": {"reason": "NotPatched"}})
    );
    assert!(answer(&mut desk).await.get("Picture").is_some());
    send(&mut desk, &json!("Stop")).await;
    assert_eq!(answer(&mut desk).await, "Stopping");
    served.await.unwrap().unwrap();
}

/// The socket table in docs/engine.md (C1): a `Subscribe` behind a pending edit starts as
/// its picture goes out, so no `Changed` comes before that picture.
#[tokio::test(flavor = "multi_thread")]
async fn a_subscribe_behind_a_pending_edit_sends_no_change_before_its_picture() {
    let (address, served, opened, release) = held_open().await;
    let (mut desk, mut other) = (open(address).await, open(address).await);
    let _udp = attach_held(&mut desk, opened).await;
    send(&mut desk, &json!("Subscribe")).await;
    send(&mut other, &set(1, 7, 77)).await;
    assert_eq!(answer(&mut other).await, "Accepted");
    // a frame with the press in it is painted while the subscription waits its turn
    loop {
        send(&mut other, &json!("Query")).await;
        if answer(&mut other).await["Picture"]["universes"][0][6] == 77 {
            break;
        }
    }
    release.send(()).unwrap();
    let first = next(&mut desk).await;
    assert_eq!(first, "Accepted", "the attach before anything else: {first}");
    let second = next(&mut desk).await;
    assert!(second.get("Picture").is_some(), "the picture first: {second}");
    send(&mut other, &json!("Stop")).await;
    assert_eq!(answer(&mut other).await, "Stopping");
    served.await.unwrap().unwrap();
}
