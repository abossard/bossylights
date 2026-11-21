//! Output routes on the production tick, inspected at the other end of UDP and USB.

mod common;
#[path = "../../outputs/tests/common/stream.rs"]
mod stream;
use stream::receiver as ddp;
#[path = "../../outputs/tests/common/mod.rs"]
mod usb;

use engine::{
    Adapter, Begun, ChannelNumbers, Color, Command, Engine, Event, Open, Outcome, OutputKind,
    OutputNumber, Rate, Refusal, Route, RunNumbers, Sender, Span, Text, Universes, Wakes,
};
use engine_core::Pixels;
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
    sender
        .attach_route(output(2), pro, route, Rate::FortyFour)
        .unwrap();
    fake.next_end()
}

#[test]
fn one_v11_rgb_universe_hands_the_device_a_510_byte_box() {
    struct Measured {
        ddp: Box<dyn engine::Device>,
        length: Arc<AtomicUsize>,
    }

    impl engine::Device for Measured {
        fn take(&mut self, bytes: &[u8]) -> Outcome {
            self.length.store(bytes.len(), Ordering::Release);
            self.ddp.take(bytes)
        }
    }

    let (socket, kind) = ddp(Color::Rgb);
    let length = Arc::new(AtomicUsize::new(0));
    let (_engine, mut sender, wakes) = start(Box::new({
        let length = Arc::clone(&length);
        move |kind| {
            Ok(Box::new(Measured {
                ddp: outputs::open(kind)?,
                length: Arc::clone(&length),
            }))
        }
    }));
    sender
        .send(Command::SetOutput {
            output: output(1),
            kind,
            universes: RunNumbers { first: 1, count: 1 },
            rate: None,
        })
        .unwrap();
    hold(&mut sender, 509, 91);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[509], 91);
    assert_eq!(length.load(Ordering::Acquire), 510);
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
fn a_returning_usb_port_sends_only_the_run_accepted_while_it_was_lost() {
    let kind = OutputKind::DmxUsbPro {
        adapter: Adapter::FIRST,
    };
    let (find, mut fake) = usb::Fake::new(true);
    let mut find = Some(find);
    let (monitor, opened) = mpsc::channel();
    let (_engine, mut sender, wakes) = start(Box::new(move |_| {
        let pro = DmxUsbPro::open(find.take().ok_or(std::io::ErrorKind::AddrInUse)?)?;
        monitor.send(pro.lost_with()).unwrap();
        Ok(Box::new(pro))
    }));
    let set = |first| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: None,
    };
    hold(&mut sender, 0, 11);
    hold(&mut sender, 512, 91);
    sender.send(set(1)).unwrap();
    let lost = opened.recv_timeout(usb::NEVER).unwrap();
    fake.plugged().store(false, Ordering::Release);
    drop(fake.next_end());
    // A failed write is normally replayed when the physical port returns.
    let started = Instant::now();
    while lost.get().is_none() {
        assert!(
            started.elapsed() < usb::NEVER,
            "the port never reported a failed write"
        ); // guard
        wakes.wake().unwrap();
    }
    let Begun::Opening(restart) = sender.begin(set(2), &[]).unwrap() else {
        panic!("a shifted route needs a new lane");
    };
    wakes.wake().unwrap();
    sender.finish(restart.open()).unwrap();

    fake.plugged().store(true, Ordering::Release);
    let mut end = fake.next_end();
    wakes.run();
    assert_eq!(usb::read_message(&mut end)[5], 91);
}

#[test]
fn route_edits_keep_every_copy_of_a_patched_channel_at_its_output_byte() {
    let before = [span(16, 4), span(8, 2), span(16, 2)];
    let split = [span(16, 1), span(17, 3), span(8, 2), span(16, 2)];
    let unpatched = [span(16, 4), span(24, 2), span(16, 2)];
    let dropped_copy = [span(16, 4), span(8, 2), span(24, 2)];
    let swapped = [span(18, 2), span(16, 2), span(8, 2), span(16, 2)];
    let displaced = Err(Refusal::StreamsFixture { fixture: 1 });
    for (after, expected) in [
        (split.as_slice(), Ok(())),
        (unpatched.as_slice(), Ok(())),
        (dropped_copy.as_slice(), displaced),
        (swapped.as_slice(), displaced),
    ] {
        let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
        let (socket, kind) = ddp(Color::Rgbw);
        attach_ddp(&mut sender, kind, &before);
        sender
            .send(Command::PatchStrip {
                name: Text::new("Pixel").unwrap(),
                pixels: Pixels::try_from(1).unwrap(),
                color: Color::Rgbw,
                at: ChannelNumbers {
                    universe: 1,
                    address: 17,
                },
            })
            .unwrap();
        hold(&mut sender, 16, 91);
        let route = Route::new(outputs::limits(kind), after).unwrap();
        let reply =
            sender.attach_route(output(1), outputs::open(kind).unwrap(), route, Rate::Sixty);
        assert_eq!(reply, expected, "{after:?}");
        wakes.wake().unwrap();
        assert_eq!(
            packet(&socket, 0x41, 0x1B, 0, 8).2,
            [91, 0, 0, 0, 0, 0, 91, 0]
        );
    }
}

#[test]
fn a_patch_during_an_open_must_fit_both_routes_at_the_same_output_bytes() {
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgbw);
    let set = |first, count| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count },
        rate: None,
    };
    sender.send(set(1, 2)).unwrap();
    let Begun::Opening(opening) = sender.begin(set(2, 3), &[]).unwrap() else {
        panic!("a run of a new length must restart");
    };
    // Universe 1 would be dropped, universe 2 would move from byte 512 to 0,
    // and universe 3 is new to the output.
    for (universe, accepted) in [(1, false), (2, false), (3, true)] {
        let reply = sender.send(Command::PatchStrip {
            name: Text::new("Pixel").unwrap(),
            pixels: Pixels::try_from(1).unwrap(),
            color: Color::Rgbw,
            at: ChannelNumbers {
                universe,
                address: 1,
            },
        });
        assert_eq!(reply.is_ok(), accepted, "{universe}");
    }
    hold(&mut sender, 1024, 91);
    wakes.wake().unwrap();
    sender.finish(opening.open()).unwrap();
    socket.set_nonblocking(true).unwrap();
    while socket.recv(&mut [0; 2048]).is_ok() {}
    socket.set_nonblocking(false).unwrap();
    wakes.wake().unwrap();
    // After the shift, universe 3 begins at byte 512 of the stream.
    assert_eq!(packet(&socket, 0x40, 0x1B, 0, 1440).2[512], 91);
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
fn a_fresh_device_waits_for_the_last_old_send_before_accepting_a_reroute() {
    let (socket, kind) = ddp(Color::Rgb);
    let (entered, taking) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (fresh, reopened) = mpsc::channel();
    let mut first = Some((entered, resume));
    let (engine, mut sender, wakes) = start(Box::new(move |kind| {
        let ddp = outputs::open(kind)?;
        if let Some((entered, resume)) = first.take() {
            Ok(Box::new(HoldFirstFrame {
                ddp,
                gate: Some((entered, resume)),
            }))
        } else {
            fresh.send(()).unwrap();
            Ok(ddp)
        }
    }));
    let set = |first| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: None,
    };
    for (index, marker) in [(0, 11), (512, 91)] {
        hold(&mut sender, index, marker);
    }
    sender.send(set(1)).unwrap();
    wakes.wake().unwrap();
    taking.recv_timeout(usb::NEVER).unwrap();

    // Keep the device on its way back while the blocking worker opens a fresh socket.
    engine.stall_reclamation(true);
    let Begun::Opening(restart) = sender.begin(set(2), &[]).unwrap() else {
        panic!("a shifted run needs a new lane");
    };
    wakes.wake().unwrap();
    old_send_at_acceptance(
        &socket,
        release,
        520,
        || sender.finish(restart.open()).unwrap(),
        || reopened.recv_timeout(usb::NEVER).unwrap(),
    );
    engine.stall_reclamation(false);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 91);
}

/// Complete a reroute while its first send is held inside the adapter. At acceptance
/// the old packet must already be on the wire, even when the old lane takes time to stop.
fn old_send_at_acceptance(
    socket: &UdpSocket,
    release: mpsc::Sender<()>,
    length: usize,
    complete: impl FnOnce() + Send,
    ready: impl FnOnce(),
) {
    socket.set_nonblocking(true).unwrap();
    let (finished, accepted) = mpsc::channel();
    thread::scope(|threads| {
        threads.spawn(|| {
            complete();
            let mut packet = [0; 1500];
            finished
                .send(socket.recv(&mut packet).map(|n| (n, packet[10])))
                .unwrap();
        });
        ready();
        // Give a premature acceptance a chance to report the missing datagram.
        let early = accepted.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        let old = early.unwrap_or_else(|_| accepted.recv_timeout(usb::NEVER).unwrap());
        assert_eq!(old.unwrap(), (length, 11));
    });
    socket.set_nonblocking(false).unwrap();
}

#[test]
fn an_explicit_reroute_waits_for_the_last_send_in_the_old_lane() {
    let (socket, kind) = ddp(Color::Rgb);
    let (entered, taking) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let mut gate = Some((entered, resume));
    let (_engine, mut sender, wakes) = start(Box::new(move |kind| {
        Ok(Box::new(HoldFirstFrame {
            ddp: outputs::open(kind)?,
            gate: gate.take(),
        }))
    }));
    for (channel, level) in [(0, 11), (15, 91)] {
        hold(&mut sender, channel, level);
    }
    sender
        .send(Command::SetOutput {
            output: output(1),
            kind,
            universes: RunNumbers { first: 1, count: 1 },
            rate: Some(Rate::Sixty),
        })
        .unwrap();
    wakes.wake().unwrap();
    taking.recv_timeout(usb::NEVER).unwrap();

    // Moving to channel 15 changes the route within the same displayed universe.
    let (started, attaching) = mpsc::channel();
    old_send_at_acceptance(
        &socket,
        release,
        520,
        || {
            started.send(()).unwrap();
            attach_ddp(&mut sender, kind, &[span(15, 3)]);
        },
        || attaching.recv_timeout(usb::NEVER).unwrap(),
    );
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 3).2[0], 91);
}

#[test]
fn an_explicit_reroute_refused_by_a_full_queue_keeps_sending_the_old_route() {
    let (socket, kind) = ddp(Color::Rgb);
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    attach_ddp(&mut sender, kind, &[span(15, 3)]);
    hold(&mut sender, 15, 11);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 3).2[0], 11);

    while sender.send(Command::Blackout { on: false }).is_ok() {}
    let new_route = Route::new(outputs::limits(kind), &[span(123, 3)]).unwrap();
    let result = sender.attach_route(
        output(1),
        outputs::open(kind).unwrap(),
        new_route,
        Rate::Sixty,
    );
    assert_eq!(result, Err(Refusal::QueueFull));
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 3).2[0], 11);
}

#[test]
fn resizing_and_shifting_a_run_restart_its_lane_on_the_same_device() {
    let (socket, kind) = ddp(Color::Rgb);
    let opens = Arc::new(AtomicUsize::new(0));
    let (_engine, mut sender, mut wakes) = start(Box::new({
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
            let Begun::Opening(opening) = sender.begin(set(first, 2), &[]).unwrap() else {
                panic!("a shifted run needs a new lane");
            };
            wakes.wake().unwrap();
            sender.finish(opening.open()).unwrap();
        }
        wakes.wake().unwrap();
        let (source, _, payload) = packet(&socket, 0x41, 0x0B, 0, 1023);
        let mut expected = vec![0; 1023];
        expected[marker] = 91;
        assert_eq!(payload, expected);
        assert_eq!(source, peer);
    }
    assert_eq!(opens.load(Ordering::Relaxed), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_clients_press_reaches_the_next_due_frame_while_an_open_waits() {
    press_during_setup(Pause::Open).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_clients_press_reaches_the_next_due_frame_while_a_lane_starts() {
    press_during_setup(Pause::LaneStart).await;
}

enum Pause {
    Open,
    LaneStart,
}

async fn press_during_setup(at: Pause) {
    let (running, running_kind) = ddp(Color::Rgb);
    let (_paused, paused_kind) = ddp(Color::Rgb);
    let (entered, opening) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (engine, sender, wakes) = match at {
        Pause::Open => Engine::start_stepped(
            Universes::ONE,
            Box::new(move |kind| {
                if kind == paused_kind {
                    entered.send(()).unwrap();
                    gate.recv_timeout(usb::NEVER).unwrap();
                }
                outputs::open(kind)
            }),
        )
        .unwrap(),
        Pause::LaneStart => {
            let (engine, mut sender, wakes) =
                Engine::start_stepped(Universes::ONE, Box::new(outputs::open)).unwrap();
            sender.pause_lane_start(output(1), entered, gate);
            (engine, sender, wakes)
        }
    };
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_device_reopens_the_old_run_after_a_refused_resize() {
    for ever_sent in [true, false] {
        check_recovery(ever_sent).await;
    }
}

async fn health_event(
    watch: &mut common::Socket,
    wanted: &Value,
    events: &mut Vec<Value>,
) -> Value {
    use futures_util::StreamExt;

    tokio::time::timeout(usb::NEVER, async {
        loop {
            let message = watch.next().await.unwrap().unwrap();
            let reply: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            if let Some(rows) = reply["Health"]["events"].as_array() {
                events.extend(
                    rows.iter()
                        .filter(|e| e["output"] == 1)
                        .map(|e| e["event"].clone()),
                );
                if events.contains(wanted) {
                    return reply;
                }
            }
        }
    })
    .await
    .expect("health event for output 1")
}

async fn check_recovery(ever_sent: bool) {
    use std::sync::atomic::AtomicBool;

    struct Removable {
        ddp: Box<dyn engine::Device>,
        missing: Arc<AtomicBool>,
        accepts: bool,
    }
    impl engine::Device for Removable {
        fn take(&mut self, frame: &[u8]) -> Outcome {
            if self.missing.load(Ordering::Acquire) {
                Outcome::Failed
            } else if !self.accepts {
                Outcome::Dropped
            } else {
                self.ddp.take(frame)
            }
        }
    }

    let (socket, kind) = ddp(Color::Rgb);
    let missing = Arc::new(AtomicBool::new(false));
    let (entered, retrying) = mpsc::channel();
    let (release, available) = mpsc::channel();
    let mut attempt = 0;
    let (engine, sender, wakes) = start(Box::new(move |kind| {
        attempt += 1;
        match attempt {
            1 => Ok(Box::new(Removable {
                ddp: outputs::open(kind)?,
                missing: Arc::clone(&missing),
                accepts: ever_sent,
            })),
            2 | 3 => {
                // The resize and the first attempt to restore the old run both fail.
                missing.store(true, Ordering::Release);
                Err(std::io::ErrorKind::NotFound.into())
            }
            _ => {
                entered.send(()).unwrap();
                available.recv_timeout(usb::NEVER).unwrap();
                outputs::open(kind)
            }
        }
    }));
    wakes.run();
    let address = common::serve(engine, sender, None).await;
    let mut writer = common::connect(address).await;
    let mut other = common::connect(address).await;
    let mut watch = common::connect(address).await;
    let set = |count| {
        json!({"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": 1, "count": count}, "rate": "60"
        }})
    };
    assert_eq!(common::command(&mut writer, set(2)).await, "Accepted");
    common::send(&mut watch, &json!("Subscribe")).await;
    common::answer(&mut watch).await;
    let (counter, expected) = if ever_sent {
        ("accepted", "Running")
    } else {
        ("dropped", "NotFound")
    };
    let before = tokio::time::timeout(usb::NEVER, async {
        loop {
            common::send(&mut other, &json!("Query")).await;
            let picture = common::answer(&mut other).await;
            let old = &picture["Picture"]["state"]["outputs"][0];
            if old["counters"][counter].as_u64().is_some_and(|n| n >= 2) {
                break old.clone();
            }
        }
    })
    .await
    .expect("old device handed two frames");
    assert_eq!(before["status"], expected);

    // Hold the tick before it can return the old device, forcing a fresh open.
    wakes.hold().unwrap();
    assert_eq!(
        common::command(&mut writer, set(3)).await,
        json!({"Refused": {"reason": "CannotOpen"}})
    );
    wakes.run();
    tokio::task::block_in_place(|| retrying.recv_timeout(usb::NEVER)).expect("retry the old run");
    let mut events = Vec::new();
    let lost = json!("OutputLost");
    let report = health_event(&mut watch, &lost, &mut events).await;
    assert!(
        report["Health"]["health"]["outputs"][0]["status"]["Reconnecting"]["failures"]
            .as_u64()
            .unwrap()
            >= 1
    );

    // This press reaches the front while the retry worker waits for the port.
    let press = json!({"SetChannel": {"channel": {"universe": 2, "address": 510}, "value": 73}});
    assert_eq!(common::command(&mut other, press).await, "Accepted");
    release.send(()).unwrap();
    tokio::task::block_in_place(|| {
        let started = Instant::now();
        loop {
            let (_, _, payload) = packet(&socket, 0x41, 0x0B, 0, 1023);
            if payload[1021] == 73 {
                break;
            }
            assert!(started.elapsed() < usb::NEVER, "old run never resumed"); // guard
        }
    });
    let recovered = json!({"Recovered": {"reason": "OutputAvailable"}});
    let report = health_event(&mut watch, &recovered, &mut events).await;
    assert_eq!(
        report["Health"]["health"]["outputs"][0]["status"],
        "Running"
    );
    assert_eq!(events.iter().filter(|event| **event == lost).count(), 1);
    common::send(&mut writer, &json!("Stop")).await;
}

#[test]
fn a_refused_restart_keeps_the_only_stream_for_a_patched_strip() {
    let (socket, kind) = ddp(Color::Rgb);
    let mut opened = false;
    let (engine, mut sender, wakes) = start(Box::new(move |kind| {
        if std::mem::replace(&mut opened, true) {
            return Err(std::io::ErrorKind::AddrNotAvailable.into());
        }
        outputs::open(kind)
    }));
    let one = output(1);
    let set = |count| Command::SetOutput {
        output: one,
        kind,
        universes: RunNumbers { first: 1, count },
        rate: None,
    };
    sender.send(set(2)).unwrap();
    sender
        .send(Command::PatchStrip {
            name: Text::new("Strip").unwrap(),
            pixels: Pixels::try_from(2).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 512,
            },
        })
        .unwrap();
    hold(&mut sender, 516, 91);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 1023).2[516], 91);
    let before = sender.project();
    assert_eq!(sender.patch().unstreamed(&before), None);

    engine.stall_reclamation(true);
    let Begun::Opening(restart) = sender.begin(set(3), &[]).unwrap() else {
        panic!("changing stream length must prepare a new lane");
    };
    wakes.wake().unwrap();
    let result = sender.finish(restart.open());
    engine.stall_reclamation(false);

    assert_eq!(result, Err(Refusal::CannotOpen));
    let after = sender.project();
    assert_eq!(
        (after.output(one), sender.patch().unstreamed(&after)),
        (before.output(one), None),
        "a refused restart must leave the fixture's old stream in place"
    );
}

#[test]
fn replacing_a_route_cannot_drop_a_patched_pixel() {
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgbw);
    let one = output(1);
    sender
        .send(Command::SetOutput {
            output: one,
            kind,
            universes: RunNumbers { first: 1, count: 1 },
            rate: None,
        })
        .unwrap();
    sender
        .send(Command::PatchStrip {
            name: Text::new("Pixel").unwrap(),
            pixels: Pixels::try_from(1).unwrap(),
            color: Color::Rgbw,
            at: ChannelNumbers {
                universe: 1,
                address: 317,
            },
        })
        .unwrap();
    hold(&mut sender, 319, 91);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x1B, 0, 512).2[319], 91);

    let route = Route::new(outputs::limits(kind), &[span(0, 316), span(512, 4)]).unwrap();
    let result = sender.attach_route(one, outputs::open(kind).unwrap(), route, Rate::Sixty);
    if result.is_ok() {
        wakes.wake().unwrap();
        let (_, _, payload) = packet(&socket, 0x41, 0x1B, 0, 320);
        assert_eq!(
            payload[319], 91,
            "the accepted reroute dropped the patched pixel"
        );
    }
    assert_eq!(result, Err(Refusal::StreamsFixture { fixture: 1 }));
}

#[test]
fn the_engine_refuses_a_319_byte_route_for_an_rgbw_device() {
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgbw);
    let spans = [span(0, 319)];
    let one = output(1);
    let route = Route::new(outputs::Limits::RAW, &spans).unwrap();
    let result = sender.attach_route(one, outputs::open(kind).unwrap(), route, Rate::Sixty);
    if result.is_ok() {
        wakes.wake().unwrap();
        let sent = packet(&socket, 0x41, 0x1B, 0, 316).2.len();
        panic!("accepted a 319-byte RGBW route and sent only {sent} bytes");
    }
    assert_eq!(sender.project().output(one), None);
}

struct ObserveLane {
    ddp: Box<dyn engine::Device>,
    sent: mpsc::Sender<ThreadId>,
}

impl engine::Device for ObserveLane {
    fn take(&mut self, frame: &[u8]) -> Outcome {
        let outcome = self.ddp.take(frame);
        let _ = self.sent.send(thread::current().id());
        outcome
    }
}

#[test]
fn shifting_one_v11_universe_restarts_the_lane_and_keeps_the_device() {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let timeout = Duration::from_secs(10);
    socket.set_read_timeout(Some(timeout)).unwrap();
    let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        unreachable!("bound to IPv4 loopback");
    };
    let kind = OutputKind::Ddp {
        address,
        color: Color::Rgb,
    };
    let (sent, lane) = mpsc::channel();
    let open = Box::new(move |kind| {
        Ok(Box::new(ObserveLane {
            ddp: outputs::open(kind)?,
            sent: sent.clone(),
        }) as Box<dyn engine::Device>)
    });
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(Universes::new(2).unwrap(), open).unwrap();
    wakes.hold().unwrap();
    let set = |first| Command::SetOutput {
        output: OutputNumber::try_from(1).unwrap(),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: None,
    };
    for (universe, value) in [(1, 11), (2, 91)] {
        sender
            .send(Command::SetChannel {
                channel: ChannelNumbers {
                    universe,
                    address: 1,
                },
                value,
            })
            .unwrap();
    }
    let read = |marker| {
        let mut packet = [0; 1500];
        let (size, peer) = socket.recv_from(&mut packet).unwrap();
        assert_eq!(size, 520, "510 RGB bytes and the 10-byte DDP header");
        assert_eq!(packet[10], marker);
        (peer, lane.recv_timeout(timeout).unwrap())
    };

    sender.send(set(1)).unwrap();
    wakes.wake().unwrap();
    let (first_peer, first_lane) = read(11);
    if let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() {
        wakes.wake().unwrap();
        sender.finish(opening.open()).unwrap();
    }
    wakes.wake().unwrap();
    let (second_peer, second_lane) = read(91);
    assert_eq!(first_peer, second_peer, "keep the DDP socket");
    assert_ne!(
        first_lane, second_lane,
        "restart the lane after moving the run"
    );
}

async fn picture_after(desk: &mut common::Socket, wakes: &Wakes) -> Value {
    let previous = wakes.woke();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            wakes.wake().unwrap();
            common::send(desk, &json!("Query")).await;
            let reply = common::answer(desk).await;
            if reply["Picture"]["state"]["frame"].as_u64().unwrap() > previous {
                return reply;
            }
        }
    })
    .await
    .expect("the reader publishes a new frame")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_v11_picture_has_one_run_for_an_output_during_a_paused_restart() {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        unreachable!("bound to IPv4 loopback");
    };
    let kind = OutputKind::Ddp {
        address,
        color: Color::Rgb,
    };
    let (entered, reopening) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut first_open = true;
    let open = Box::new(move |kind| {
        if first_open {
            first_open = false;
        } else {
            entered.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(20)).unwrap();
        }
        outputs::open(kind)
    });
    let (engine, sender, wakes) = Engine::start_stepped(Universes::new(2).unwrap(), open).unwrap();
    engine.stall_reclamation(true);
    let address = common::serve(engine, sender, None).await;
    let (mut writer, mut reader) = tokio::time::timeout(Duration::from_secs(10), async {
        (
            common::connect(address).await,
            common::connect(address).await,
        )
    })
    .await
    .expect("the server starts and accepts both connections");
    wakes.hold().unwrap();
    let set = |count| {
        json!({"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": 1, "count": count}, "rate": null
        }})
    };
    assert_eq!(common::command(&mut writer, set(1)).await, "Accepted");
    let before = picture_after(&mut reader, &wakes).await;
    common::send(&mut writer, &json!({"Send": {"command": set(2)}})).await;
    // Reclamation is paused, so the restart falls back to an open, gated by this test.
    tokio::task::block_in_place(|| reopening.recv_timeout(Duration::from_secs(10))).unwrap();
    let pending = picture_after(&mut reader, &wakes).await;
    release.send(()).unwrap();
    assert_eq!(common::answer(&mut writer).await, "Accepted");
    let after = picture_after(&mut reader, &wakes).await;
    wakes.run();
    common::send(&mut writer, &json!("Stop")).await;

    for (picture, count) in [(before, 1), (pending, 1), (after, 2)] {
        let picture = &picture["Picture"];
        let project = &picture["project"]["outputs"][0]["Kind"]["universes"];
        let state = &picture["state"]["outputs"][0]["universes"];
        assert_eq!(project, &json!({"first": 1, "count": count}));
        assert_eq!(state, project, "a picture must agree on the output run");
    }
}

struct HoldFirstFrame {
    ddp: Box<dyn engine::Device>,
    gate: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
}

impl engine::Device for HoldFirstFrame {
    fn take(&mut self, frame: &[u8]) -> Outcome {
        if let Some((entered, release)) = self.gate.take() {
            entered.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        self.ddp.take(frame)
    }
}

#[test]
fn an_accepted_reroute_does_not_send_queued_old_route_bytes() {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let timeout = Duration::from_secs(10);
    socket.set_read_timeout(Some(timeout)).unwrap();
    let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        unreachable!("bound to IPv4 loopback");
    };
    let kind = OutputKind::Ddp {
        address,
        color: Color::Rgb,
    };
    let (entered, taking) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut gate = Some((entered, wait));
    let open = Box::new(move |kind| {
        Ok(Box::new(HoldFirstFrame {
            ddp: outputs::open(kind)?,
            gate: gate.take(),
        }) as Box<dyn engine::Device>)
    });
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(Universes::new(2).unwrap(), open).unwrap();
    wakes.hold().unwrap();
    let set = |first| Command::SetOutput {
        output: OutputNumber::try_from(1).unwrap(),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    let hold = |universe, value| Command::SetChannel {
        channel: ChannelNumbers {
            universe,
            address: 1,
        },
        value,
    };
    let read = || {
        let mut packet = [0; 1500];
        assert_eq!(socket.recv(&mut packet).unwrap(), 520);
        packet[10]
    };
    sender.send(hold(1, 11)).unwrap();
    sender.send(hold(2, 91)).unwrap();
    sender.send(set(1)).unwrap();
    let mut at = wakes.wake().unwrap();
    taking.recv_timeout(timeout).unwrap();
    // Occupy both remaining lane boxes while the first send is in progress.
    let period = Rate::Sixty.period();
    for marker in [22, 33] {
        sender.send(hold(1, marker)).unwrap();
        at = wakes.wake_at(at + period).unwrap();
    }

    let move_run = sender.begin(set(2), &[]).unwrap();
    at = wakes.wake_at(at + period).unwrap();
    release.send(()).unwrap();
    assert_eq!(read(), 11, "the frame already inside the device finishes");
    if let Begun::Opening(opening) = move_run {
        sender.finish(opening.open()).unwrap();
        // Earlier frames may have finished before this branch accepted the edit.
        socket.set_nonblocking(true).unwrap();
        while socket.recv(&mut [0; 1500]).is_ok() {}
        socket.set_nonblocking(false).unwrap();
        wakes.wake_at(at + period).unwrap();
    }
    assert_eq!(read(), 91, "the first new send after accepting the edit");
}

#[test]
fn a_319_byte_route_cannot_be_attached_to_rgbw() {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let kind = OutputKind::Ddp {
        address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, socket.local_addr().unwrap().port()),
        color: Color::Rgbw,
    };
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(Universes::ONE, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let route = Route::new(outputs::Limits::RAW, &[Span { start: 0, len: 319 }]).unwrap();
    let before = sender.sent();
    let result = sender.attach_route(output, outputs::open(kind).unwrap(), route, Rate::Sixty);

    let packet_length = if result.is_ok() {
        wakes.wake().unwrap();
        Some(socket.recv(&mut [0; 2048]).unwrap())
    } else {
        None
    };
    assert!(
        result.is_err(),
        "accepted an invalid RGBW route and emitted a datagram of {packet_length:?} bytes including its 10-byte header"
    );
    assert_eq!(sender.project().output(output), None);
    assert_eq!(sender.sent(), before);
}
