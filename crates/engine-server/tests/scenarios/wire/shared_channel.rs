//! End to end over the API (#173): a mode whose two heads name one dimmer is patched at
//! 1:37. The dimmer is the fixture's own light (light 1), each head a light after it. The
//! fixture full and head 1 red send 255 255 0 at 37 to 39 on the DDP socket. A PAR on
//! the fixture's channels is refused, `Overlaps`.

use crate::wire_common as common;

use common::taken;
use engine::{Engine, Universes};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{fs, process};
use tokio::net::{TcpStream, UdpSocket};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Two heads, each a red channel, both naming the dimmer.
const SHARED: &str = r#"<?xml version="1.0"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Test</Manufacturer><Model>Shared</Model><Type>LED Bar (Pixels)</Type>
 <Channel Name="Dimmer" Preset="IntensityDimmer"/>
 <Channel Name="Red 1" Preset="IntensityRed"/>
 <Channel Name="Red 2" Preset="IntensityRed"/>
 <Mode Name="Shared">
  <Channel Number="0">Dimmer</Channel>
  <Channel Number="1">Red 1</Channel>
  <Channel Number="2">Red 2</Channel>
  <Head><Channel>0</Channel><Channel>1</Channel></Head>
  <Head><Channel>0</Channel><Channel>2</Channel></Head>
 </Mode>
</FixtureDefinition>"#;

/// A folder of the shared mode and the Generic RGB PAR.
fn folder() -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("shared-{}", process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("Test-Shared.qxf"), SHARED).unwrap();
    let rgb = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");
    fs::copy(rgb, folder.join("Generic-Generic-RGB.qxf")).unwrap();
    folder
}

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

/// Sends `command` again until the library is read. The reply.
async fn once_read(socket: &mut Socket, patch: &Value) -> Value {
    let started = Instant::now();
    loop {
        let reply = command(socket, patch.clone()).await;
        if reply != json!({"Refused": {"reason": "NoSuchFixture"}}) {
            return reply;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "no library");
        tokio::task::yield_now().await;
    }
}

async fn serve() -> SocketAddr {
    let (engine, sender) = Engine::start(Universes::ONE).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(engine_server::serve(
        listener,
        engine,
        sender,
        Some(folder()),
        None,
    ));
    address
}

/// Waits until universe 1 from 37 on is `bytes` on the DDP socket.
async fn until(udp: &UdpSocket, bytes: [u8; 3]) {
    let started = Instant::now();
    let mut packet = vec![0; 1500];
    loop {
        let size = tokio::time::timeout(Duration::from_secs(5), udp.recv(&mut packet))
            .await
            .expect("a packet within 5 s")
            .unwrap();
        let seen = packet.get(10 + 36..10 + 39).filter(|_| size >= 10 + 39);
        if seen == Some(&bytes[..]) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "from 37: {seen:?}, wanted {bytes:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dimmer_two_heads_name_is_the_fixtures_own_light_and_nothing_overlaps_it() {
    let address = serve().await;
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/api"))
        .await
        .unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ddp = json!({"Ddp": {"address": udp.local_addr().unwrap().to_string(), "color": "Rgb"}});
    let set = json!({"SetOutput": {
        "output": 1, "kind": ddp, "universes": {"first": 1, "count": 1}, "rate": null,
    }});
    assert_eq!(command(&mut socket, set).await, json!("Accepted"));
    let at = |model: &str, mode: &str, address: u16| {
        json!({"Patch": {
            "name": model, "maker": if model == "Shared" { "Test" } else { "Generic" },
            "model": model, "mode": mode, "at": {"universe": 1, "address": address},
        }})
    };
    assert_eq!(
        once_read(&mut socket, &at("Shared", "Shared", 37)).await,
        json!({"Patched": {"fixture": 1}})
    );
    assert_eq!(
        once_read(&mut socket, &at("Generic RGB", "RGB", 39)).await,
        json!({"Refused": {"reason": {"Overlaps": {"fixture": 1}}}})
    );

    let light = |light: u16| json!({"Light": {"light": light}});
    let full = |target: Value| json!({"SetIntensity": {"target": target, "intensity": 65535}});
    let red = json!({"SetColour": {"target": light(2), "red": 65535, "green": 0, "blue": 0}});
    for set in [full(light(1)), full(light(2)), red] {
        assert!(taken(&command(&mut socket, set).await));
    }
    until(&udp, [255, 255, 0]).await;
}
