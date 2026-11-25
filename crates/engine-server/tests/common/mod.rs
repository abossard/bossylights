//! What the wire tests share: a served engine, a client on `/api`, and a fake DMX USB Pro
//! (a pseudo terminal) whose every complete message is kept in order.

#![allow(dead_code, reason = "each test binary uses a part")]

use engine::{Adapter, Device, Engine, Open, OutputKind, Sender};
use futures_util::{SinkExt, Stream, StreamExt};
use outputs::dmx_usb_pro::{DmxUsbPro, Find, MESSAGE, never_wait};
use serde_json::Value;
use serialport::{SerialPort, TTYPort};
use std::io::Read;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{Error, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Every complete message the fake adapter got, in order: its 512 channels.
pub type Heard = Arc<Mutex<Vec<[u8; 512]>>>;

/// Reads the test's end of the fake port and keeps the channels of every full message.
/// With no `heard`, nobody reads it: the port fills up and stays full.
fn listen(mut end: TTYPort, heard: Option<Heard>, stop: Arc<AtomicBool>) {
    let mut bytes = Vec::new();
    let mut chunk = [0; MESSAGE];
    while !stop.load(Ordering::Relaxed) {
        let Some(heard) = &heard else {
            thread::sleep(Duration::from_millis(20)); // poll
            continue;
        };
        if let Ok(n) = end.read(&mut chunk) {
            bytes.extend_from_slice(&chunk[..n]);
        }
        // a message: 0x7E, label 6, length 513, start code 0, 512 channels, 0xE7
        while let Some(start) = bytes.iter().position(|b| *b == 0x7E) {
            bytes.drain(..start);
            if bytes.len() < MESSAGE {
                break;
            }
            if bytes[..5] == [0x7E, 6, 0x01, 0x02, 0] && bytes[MESSAGE - 1] == 0xE7 {
                heard
                    .lock()
                    .unwrap()
                    .push(bytes[5..5 + 512].try_into().unwrap());
                bytes.drain(..MESSAGE);
            } else {
                bytes.drain(..1);
            }
        }
    }
}

/// A fake serial port for the adapter: `listen` reads its other end into `heard`, or
/// nobody reads. Its first pseudo terminal is made now: the system makes them one at a
/// time, and under load that takes seconds, which no API answer may wait for.
pub fn find(heard: Option<Heard>, stop: Arc<AtomicBool>) -> Find {
    let mut first = Some(TTYPort::pair());
    Box::new(move || {
        let (mut end, port) = first.take().unwrap_or_else(TTYPort::pair)?;
        never_wait(&port)?;
        end.set_timeout(Duration::from_millis(20))?;
        let (heard, stop) = (heard.clone(), Arc::clone(&stop));
        thread::spawn(move || listen(end, heard, stop));
        Ok(port)
    })
}

/// The engine's `open`: DDP as the engine opens it, adapter 1 a fake serial port whose
/// other end `listen` reads into `heard`, or nobody reads. The port of its first open
/// is made now ([`find`]).
pub fn open(heard: Option<Heard>, stop: Arc<AtomicBool>) -> Open {
    let mut first = Some(find(heard.clone(), Arc::clone(&stop)));
    Box::new(move |kind| match kind {
        OutputKind::DmxUsbPro {
            adapter: Adapter::FIRST,
        } => {
            let find = first
                .take()
                .unwrap_or_else(|| find(heard.clone(), Arc::clone(&stop)));
            Ok(Box::new(DmxUsbPro::open(find)?) as Box<dyn Device>)
        }
        OutputKind::Ddp { .. } | OutputKind::DmxUsbPro { .. } => outputs::open(kind),
    })
}

/// Serves `engine` on a free port, the fixture library read from `fixtures`.
pub async fn serve(engine: Engine, sender: Sender, fixtures: Option<PathBuf>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(engine_server::serve(
        listener, engine, sender, fixtures, None,
    ));
    address
}

pub async fn connect(address: SocketAddr) -> Socket {
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/api"))
        .await
        .unwrap();
    socket
}

pub async fn send(socket: &mut Socket, request: &Value) {
    socket
        .send(Message::text(request.to_string()))
        .await
        .unwrap();
}

/// The reply to a request, skipping subscription updates.
pub async fn answer(socket: &mut (impl Stream<Item = Result<Message, Error>> + Unpin)) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a message within 5 s")
            .unwrap()
            .unwrap();
        let reply: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        if reply.get("Changed").is_none() && reply.get("Health").is_none() {
            return reply;
        }
    }
}

pub async fn command(socket: &mut Socket, command: Value) -> Value {
    send(socket, &serde_json::json!({"Send": {"command": command}})).await;
    answer(socket).await
}

/// Whether `reply` took the command: `Accepted`, or `Grabbed` for a set.
pub fn taken(reply: &Value) -> bool {
    reply == "Accepted" || reply["Grabbed"]["grab"].is_object()
}
