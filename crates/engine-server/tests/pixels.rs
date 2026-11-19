//! End to end over the API: a generic strip of 300 RGBW pixels on universes 1 to 3, one
//! DDP output streaming the three. Pixel 171 green: 0 255 0 0 at bytes 680 to 683 of the
//! stream (universe 2, addresses 169 to 172), every other byte 0. The strip is refused
//! until an output streams its universes, and that output cannot go while it is patched.

use engine::{Engine, Universes};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::{TcpStream, UdpSocket};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Three universes as one RGBW stream: 1 536 bytes.
const STREAM: usize = 3 * 512;

async fn send(socket: &mut Socket, request: &Value) {
    socket
        .send(Message::text(request.to_string()))
        .await
        .unwrap();
}

/// The next message that is not a `Changed`.
async fn answer(socket: &mut Socket) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a message within 5 s")
            .unwrap()
            .unwrap();
        let reply: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        if reply.get("Changed").is_none() {
            return reply;
        }
    }
}

async fn command(socket: &mut Socket, command: Value) -> Value {
    send(socket, &json!({"Send": {"command": command}})).await;
    answer(socket).await
}

/// A served engine of three universes, its outputs opened as the engine opens them.
async fn serve() -> SocketAddr {
    let (engine, sender) = Engine::start(Universes::new(3).unwrap()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(engine_server::serve(listener, engine, sender, None, None));
    address
}

/// The bytes of one whole frame on the DDP socket: its packets put together by their
/// offsets, the frame ending at the packet with the push flag.
async fn frame(udp: &UdpSocket) -> Vec<u8> {
    let mut stream = vec![None; STREAM];
    let mut packet = vec![0; 1500];
    loop {
        let size = tokio::time::timeout(Duration::from_secs(5), udp.recv(&mut packet))
            .await
            .expect("a packet within 5 s")
            .unwrap();
        let offset = u32::from_be_bytes(packet[4..8].try_into().unwrap()) as usize;
        let length = usize::from(u16::from_be_bytes(packet[8..10].try_into().unwrap()));
        assert_eq!(size, 10 + length);
        for (i, byte) in packet[10..size].iter().enumerate() {
            stream[offset + i] = Some(*byte);
        }
        if packet[0] & 0x01 == 0x01 {
            if let Some(bytes) = stream.iter().copied().collect::<Option<Vec<u8>>>() {
                return bytes;
            }
            stream = vec![None; STREAM];
        }
    }
}

/// Waits until a whole frame on the DDP socket is `expected`. Frames computed while a
/// command travelled may still lack it.
async fn until_frame(udp: &UdpSocket, expected: &[u8]) {
    let started = Instant::now();
    loop {
        let bytes = frame(udp).await;
        if bytes == expected {
            return;
        }
        let lit: Vec<(usize, u8)> = bytes
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, b)| b != 0)
            .collect();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the bytes not 0: {lit:?}"
        );
    }
}

/// Asks until the picture shows `patch`.
async fn until_patch(socket: &mut Socket, patch: &Value) {
    let started = Instant::now();
    loop {
        send(socket, &json!("Query")).await;
        let seen = answer(socket).await["Picture"]["patch"].clone();
        if seen == *patch {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the patch: {seen}, wanted {patch}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_strip_of_300_rgbw_pixels_on_universes_1_to_3_pixel_171_green_sends_0_255_0_0() {
    let address = serve().await;
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/api"))
        .await
        .unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ddp = json!({"Ddp": {"address": udp.local_addr().unwrap().to_string(), "color": "Rgbw"}});
    let set = json!({"SetOutput": {
        "output": 1, "kind": ddp, "universes": {"first": 1, "count": 3}, "rate": null,
    }});
    let strip = |pixels: u16| {
        json!({"PatchStrip": {
            "name": "Strip", "pixels": pixels, "color": "Rgbw",
            "at": {"universe": 1, "address": 1},
        }})
    };
    // no invalid patch: universes 1 to 3 need one output that streams them
    assert_eq!(
        command(&mut socket, strip(300)).await,
        json!({"Refused": {"reason": {"NotStreamed": {"first": 1, "last": 3}}}})
    );
    assert_eq!(command(&mut socket, set).await, json!("Accepted"));
    assert_eq!(
        command(&mut socket, strip(0)).await,
        json!({"Refused": {"reason": "NotACommand"}})
    );
    assert_eq!(
        command(&mut socket, strip(300)).await,
        json!({"Patched": {"fixture": 1}})
    );
    assert_eq!(
        command(&mut socket, json!({"RemoveOutput": {"output": 1}})).await,
        json!({"Refused": {"reason": {"StreamsFixture": {"fixture": 1}}}})
    );
    until_patch(
        &mut socket,
        &json!([{
            "fixture": 1, "name": "Strip", "maker": "Generic", "model": "RGBW strip",
            "mode": "300 pixels", "universe": 1, "address": 1, "channels": 1200, "first": 1,
            "lights": 300, "layout": {"width": 300, "height": 1}, "clashes": [],
        }]),
    )
    .await;

    let pixel = json!({"Light": {"light": 171}});
    let full = json!({"SetIntensity": {"target": pixel, "intensity": 65535}});
    let green = json!({"SetColour": {"target": pixel, "red": 0, "green": 65535, "blue": 0}});
    assert_eq!(command(&mut socket, full).await, json!("Accepted"));
    assert_eq!(command(&mut socket, green).await, json!("Accepted"));
    let mut expected = vec![0; STREAM];
    expected[680..684].copy_from_slice(&[0, 255, 0, 0]);
    until_frame(&udp, &expected).await;

    // white is the part red, green and blue share
    let white =
        json!({"SetColour": {"target": pixel, "red": 65535, "green": 65535, "blue": 65535}});
    assert_eq!(command(&mut socket, white).await, json!("Accepted"));
    expected[680..684].copy_from_slice(&[0, 0, 0, 255]);
    until_frame(&udp, &expected).await;
}
