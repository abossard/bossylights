//! End to end over the API (#155): one DDP RGB output streaming universes 1 and 2 sends
//! whole pixels, 1023 of their 1024 channels. A strip whose last channel is 2:512 is
//! refused, `NotSent`. A strip of 341 pixels from 1:1 ends at 2:511, the last channel
//! sent: pixel 341 blue is byte 1023 of the stream. An output that would stream it and
//! not send it is refused, `StreamsFixture`.

use crate::wire_common as common;

use common::taken;
use engine::{Engine, Universes};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::{TcpStream, UdpSocket};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Two universes as one RGB stream: 341 whole pixels.
const STREAM: usize = 2 * 512 / 3 * 3;

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

/// A served engine of two universes, its outputs opened as the engine opens them.
async fn serve() -> SocketAddr {
    let (engine, sender) = Engine::start(Universes::new(2).unwrap()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(engine_server::serve(listener, engine, sender, None, None));
    address
}

/// The bytes of one whole frame on the DDP socket: its packets put together by their
/// offsets, the frame ending at the packet with the push flag. Never more than `STREAM`.
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
        assert!(offset + length <= STREAM, "{} bytes sent", offset + length);
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

#[tokio::test(flavor = "multi_thread")]
async fn the_last_pixel_an_rgb_output_sends_is_patched_and_lit_one_channel_over_is_refused() {
    let address = serve().await;
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/api"))
        .await
        .unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ddp = json!({"Ddp": {"address": udp.local_addr().unwrap().to_string(), "color": "Rgb"}});
    let set = |output: u8, first: u16, count: u16| {
        json!({"SetOutput": {
            "output": output, "kind": ddp, "universes": {"first": first, "count": count},
            "rate": null,
        }})
    };
    let strip = |pixels: u16, universe: u16, address: u16| {
        json!({"PatchStrip": {
            "name": "Strip", "pixels": pixels, "color": "Rgb",
            "at": {"universe": universe, "address": address},
        }})
    };
    let not_sent = json!({"Refused": {"reason": {"NotSent": {
        "output": 1, "universe": 2, "address": 512,
    }}}});
    assert_eq!(command(&mut socket, set(1, 1, 2)).await, json!("Accepted"));
    // (strip, answer): the review's case, one pixel in one universe, then the last sent
    let cases = [
        (strip(171, 1, 512), not_sent.clone()),
        (strip(1, 2, 510), not_sent),
        (strip(341, 1, 1), json!({"Patched": {"fixture": 1}})),
    ];
    for (strip, expected) in cases {
        assert_eq!(
            command(&mut socket, strip.clone()).await,
            expected,
            "{strip}"
        );
    }
    // output 2 on universe 2 alone would send it to 2:510 only
    assert_eq!(
        command(&mut socket, set(2, 2, 1)).await,
        json!({"Refused": {"reason": {"StreamsFixture": {"fixture": 1}}}})
    );

    let pixel = json!({"Light": {"light": 341}});
    let full = json!({"SetIntensity": {"target": pixel, "intensity": 65535}});
    let blue = json!({"SetColour": {"target": pixel, "red": 0, "green": 0, "blue": 65535}});
    assert!(taken(&command(&mut socket, full).await));
    assert!(taken(&command(&mut socket, blue).await));
    let mut expected = vec![0; STREAM];
    expected[1020..1023].copy_from_slice(&[0, 0, 255]);
    until_frame(&udp, &expected).await;
}
