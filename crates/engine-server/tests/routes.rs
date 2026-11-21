//! Output routes on the production tick, inspected at the other end of UDP and USB.

mod common;
#[path = "../../outputs/tests/common/mod.rs"]
mod usb;

use engine::{
    Adapter, Begun, ChannelNumbers, Color, Command, Engine, Event, Open, Outcome, OutputKind,
    OutputNumber, Rate, Refusal, Route, RunNumbers, Sender, Span, Universes, Wakes,
};
use outputs::dmx_usb_pro::DmxUsbPro;
use serde_json::json;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn output(number: u8) -> OutputNumber {
    OutputNumber::try_from(number).unwrap()
}

fn ddp(color: Color) -> (UdpSocket, OutputKind) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_read_timeout(Some(usb::NEVER)).unwrap();
    let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        unreachable!("bound to IPv4 loopback");
    };
    (socket, OutputKind::Ddp { address, color })
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
    sender
        .attach_route(output(2), pro, route, Rate::FortyFour)
        .unwrap();
    fake.next_end()
}

#[test]
fn four_rgbw_rows_form_one_packet_without_padding() {
    let (_engine, mut sender, mut wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgbw);
    attach_ddp(&mut sender, kind, &ROWS);
    for (index, value) in [
        (0, 11),
        (512, 22),
        (1024, 33),
        (1536, 44),
        (1855, 55),
        (320, 99),
        (511, 99),
        (832, 99),
    ] {
        hold(&mut sender, index, value);
    }
    let at = wakes.wake().unwrap();
    let (_, _, payload) = packet(&socket, 0x41, 0x1B, 0, 1280);
    let mut expected = vec![0; 1280];
    for (index, value) in [(0, 11), (320, 22), (640, 33), (960, 44), (1279, 55)] {
        expected[index] = value;
    }
    assert_eq!(payload, expected);
    no_more_packets(&socket, &mut wakes, at);
}

#[test]
fn a_routed_rgb_frame_keeps_its_sequence_across_the_packet_boundary() {
    let (_engine, mut sender, mut wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgb);
    attach_ddp(
        &mut sender,
        kind,
        &[span(0, 512), span(512, 512), span(1024, 512)],
    );
    let mut expected = vec![0; 1536];
    for (index, value) in [(0, 11), (1439, 22), (1440, 33), (1535, 44)] {
        hold(&mut sender, index, value);
        expected[usize::from(index)] = value;
    }
    let at = wakes.wake().unwrap();
    let (source, sequence, head) = packet(&socket, 0x40, 0x0B, 0, 1440);
    let (last_source, last_sequence, tail) = packet(&socket, 0x41, 0x0B, 1440, 96);
    assert_eq!((last_source, last_sequence), (source, sequence));
    assert_eq!([head, tail].concat(), expected);
    no_more_packets(&socket, &mut wakes, at);
}

#[test]
fn a_pixel_split_across_spans_is_checked_as_one_unit() {
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgb);
    let (_unused, rgbw) = ddp(Color::Rgbw);
    assert!(Route::new(outputs::limits(rgbw), &[span(0, 319)]).is_err());
    assert_eq!(sender.project().output(output(1)), None);
    attach_ddp(&mut sender, kind, &[span(10, 2), span(100, 1)]);
    for (index, value) in [(10, 11), (11, 22), (100, 33)] {
        hold(&mut sender, index, value);
    }
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 3).2, [11, 22, 33]);
}

#[test]
fn a_route_that_reaches_past_this_engine_is_refused() {
    let (_engine, mut sender, _wakes) = start(Box::new(outputs::open));
    let (_socket, kind) = ddp(Color::Rgb);
    let route = Route::new(outputs::limits(kind), &[span(2047, 3)]).unwrap();
    let one = output(1);
    let before = sender.sent();
    let result = sender.attach_route(one, outputs::open(kind).unwrap(), route, Rate::Sixty);
    assert_eq!(result, Err(Refusal::NoSuchUniverse { universes: 4 }));
    assert_eq!(sender.project().output(one), None);
    assert_eq!(sender.sent(), before);
}

#[test]
fn the_second_universe_reaches_all_512_usb_slots() {
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let mut end = attach_usb(&mut sender, 512);
    for (index, value) in [(0, 99), (511, 99), (512, 11), (1023, 55)] {
        hold(&mut sender, index, value);
    }
    wakes.wake().unwrap();
    let message = usb::read_message(&mut end);
    assert_eq!(&message[..5], [0x7E, 6, 1, 2, 0]);
    let mut expected = [0; 512];
    expected[0] = 11;
    expected[511] = 55;
    assert_eq!(&message[5..517], expected);
    assert_eq!(message[517], 0xE7);
}

#[test]
fn the_first_ddp_row_and_usb_mirror_carry_the_same_channel_values() {
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgbw);
    let mut row = [0; 320];
    // Fill the row before attaching the outputs. Each batch fits in the command queue.
    for (index, value) in (0u16..).zip(&mut row) {
        *value = (index % 251 + 1) as u8;
        hold(&mut sender, index, *value);
        if usize::from(index) + 1 == engine::QUEUE {
            wakes.wake().unwrap();
        }
    }
    let mut end = attach_usb(&mut sender, 0);
    attach_ddp(&mut sender, kind, &ROWS);
    wakes.wake().unwrap();
    let (_, _, payload) = packet(&socket, 0x41, 0x1B, 0, 1280);
    let message = usb::read_message(&mut end);
    assert_eq!(message[5..325], row);
    assert_eq!(&payload[..320], &message[5..325]);
}

#[test]
fn a_run_changes_length_on_the_same_device_and_shifts_in_place() {
    let (socket, kind) = ddp(Color::Rgb);
    let opens = Arc::new(AtomicUsize::new(0));
    let (engine, mut sender, mut wakes) = start(Box::new({
        let opens = Arc::clone(&opens);
        move |kind| {
            opens.fetch_add(1, Ordering::Relaxed);
            outputs::open(kind)
        }
    }));
    let one = output(1);
    let set = |first, count| Command::SetOutput {
        output: one,
        kind,
        universes: RunNumbers { first, count },
        rate: None,
    };
    sender.send(set(1, 1)).unwrap();
    hold(&mut sender, 600, 91);
    wakes.wake().unwrap();
    let (peer, _, payload) = packet(&socket, 0x41, 0x0B, 0, 510);
    assert_eq!(payload, vec![0; 510]);

    let Begun::Opening(opening) = sender.begin(set(1, 2), &[]).unwrap() else {
        panic!("a changed length needs a new lane");
    };
    assert_eq!(sender.send(set(2, 2)), Err(Refusal::Opening));
    assert_eq!(
        sender.send(Command::RemoveOutput { output: one }),
        Err(Refusal::Opening)
    );
    wakes.wake().unwrap();
    assert!(wakes.events().iter().any(|event| matches!(event,
        Event::Came { output, outcome: Outcome::Dropped, .. } if *output == one
    )));
    sender.finish(opening.open()).unwrap();
    // Reading the first frame after each edit rules out a stale handover from the old lane.
    for (first, marker) in [(1, 600), (2, 88)] {
        if first == 2 {
            engine.stall_reclamation(true); // a kept lane can shift with the free thread stalled
            sender.send(set(first, 2)).unwrap();
        }
        wakes.wake().unwrap();
        let (source, _, payload) = packet(&socket, 0x41, 0x0B, 0, 1023);
        let mut expected = vec![0; 1023];
        expected[marker] = 91;
        assert_eq!(payload, expected);
        assert_eq!(source, peer);
    }
    engine.stall_reclamation(false);
    assert_eq!(opens.load(Ordering::Relaxed), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_clients_press_reaches_the_next_due_frame_while_an_open_waits() {
    let (running, running_kind) = ddp(Color::Rgb);
    let (_paused, paused_kind) = ddp(Color::Rgb);
    let (entered, opening) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let open: Open = Box::new(move |kind| {
        if kind == paused_kind {
            entered.send(()).unwrap();
            gate.recv_timeout(usb::NEVER).unwrap();
        }
        outputs::open(kind)
    });
    let (engine, sender, wakes) = Engine::start_stepped(Universes::ONE, open).unwrap();
    let address = common::serve(engine, sender, None).await;
    let mut first = common::connect(address).await;
    let mut second = common::connect(address).await;
    wakes.hold().unwrap();
    let set = |number, kind| {
        json!({"SetOutput": {
            "output": number, "kind": kind, "universes": {"first": 1, "count": 1}, "rate": "60"
        }})
    };
    assert_eq!(
        common::command(&mut second, set(2, running_kind)).await,
        "Accepted"
    );
    wakes.wake().unwrap();
    assert_eq!(packet(&running, 0x41, 0x0B, 0, 510).2[0], 0);

    common::send(
        &mut first,
        &json!({"Send": {"command": set(1, paused_kind)}}),
    )
    .await;
    tokio::task::block_in_place(|| opening.recv_timeout(usb::NEVER)).unwrap();
    let press = json!({"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": 91}});
    assert_eq!(common::command(&mut second, press).await, "Accepted");
    wakes.wake().unwrap();
    assert_eq!(packet(&running, 0x41, 0x0B, 0, 510).2[0], 91);
    release.send(()).unwrap();
    assert_eq!(common::answer(&mut first).await, "Accepted");
    wakes.run();
    common::send(&mut second, &json!("Stop")).await;
}
