//! Output routes on the production tick, inspected at the other end of UDP and USB.

mod common;
#[path = "../../outputs/tests/common/stream.rs"]
mod stream;
use stream::receiver as ddp;
#[path = "../../outputs/tests/common/mod.rs"]
mod usb;

use engine::Unsent;
use engine::{
    Adapter, Begun, ChannelNumbers, Color, Command, Engine, Event, Open, Outcome, Output,
    OutputKind, OutputNumber, Rate, Refusal, Route, RunNumbers, Sender, Span, Status, Text,
    Universes, Wakes,
};
use engine_core::Pixels;
use engine_core::health::HealthSample;
use outputs::dmx_usb_pro::DmxUsbPro;
use serde_json::{Value, json};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

fn output(number: u8) -> OutputNumber {
    OutputNumber::try_from(number).unwrap()
}

fn start(open: Open) -> (Engine, Sender, Wakes) {
    let (engine, sender, wakes) = Engine::start_stepped(Universes::new(4).unwrap(), open).unwrap();
    wakes.hold().unwrap();
    (engine, sender, wakes)
}

fn hold(sender: &mut Sender, index: u16, value: u8) {
    let channel = ChannelNumbers {
        universe: index / 512 + 1,
        address: index % 512 + 1,
    };
    sender.send(Command::SetChannel { channel, value }).unwrap();
}

/// Verifies the header, the declared length and the actual datagram length independently.
fn packet(
    socket: &UdpSocket,
    flags: u8,
    kind: u8,
    offset: u32,
    length: u16,
) -> (SocketAddr, u8, Vec<u8>) {
    let mut bytes = [0; 2048];
    let (len, peer) = socket.recv_from(&mut bytes).unwrap();
    assert_eq!(len, 10 + usize::from(length));
    assert_eq!(bytes[0], flags);
    assert!((1..=15).contains(&bytes[1]));
    assert_eq!(bytes[2..4], [kind, 1]);
    assert_eq!(bytes[4..8], offset.to_be_bytes());
    assert_eq!(bytes[8..10], length.to_be_bytes());
    (peer, bytes[1], bytes[10..len].to_vec())
}

/// Finish the current send with the clock held before the next due time.
fn no_more_packets(socket: &UdpSocket, wakes: &mut Wakes, at: Duration) {
    let started = Instant::now();
    loop {
        wakes.wake_at(at).unwrap();
        if wakes.events().iter().any(|event| {
            matches!(
                event,
                Event::Came {
                    outcome: Outcome::Accepted,
                    ..
                }
            )
        }) {
            break;
        }
        assert!(started.elapsed() < usb::NEVER, "frame never came back"); // guard
    }
    socket.set_nonblocking(true).unwrap();
    assert_eq!(
        socket.recv(&mut [0]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

const fn span(start: u32, len: u32) -> Span {
    Span { start, len }
}

const ROWS: [Span; 4] = [
    span(0, 320),
    span(512, 320),
    span(1024, 320),
    span(1536, 320),
];

fn attach_ddp(sender: &mut Sender, kind: OutputKind, spans: &[Span]) {
    let route = Route::new(outputs::limits(kind), spans).unwrap();
    sender
        .attach_route(output(1), outputs::open(kind).unwrap(), route, Rate::Sixty)
        .unwrap();
}

fn attach_usb(sender: &mut Sender, from: u32) -> serialport::TTYPort {
    let kind = OutputKind::DmxUsbPro {
        adapter: Adapter::FIRST,
    };
    let route = Route::new(outputs::limits(kind), &[span(from, 512)]).unwrap();
    let (find, mut fake) = usb::Fake::new(true);
    let pro = Box::new(DmxUsbPro::open(find).unwrap());
    let end = fake.next_end();
    usb::until_answered(&pro);
    sender
        .attach_route(output(2), pro, route, Rate::FortyFour)
        .unwrap();
    end
}

include!("routes/framing.rs");

include!("routes/route_edits.rs");

include!("routes/cutover.rs");

include!("routes/recovery.rs");

include!("routes/publication.rs");

include!("routes/outage.rs");

include!("routes/completion.rs");
