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
    let (record, completed) = mpsc::sync_channel(2);
    let mut first = Some((entered, resume, record));
    let (mut engine, mut sender, mut wakes) = start(Box::new(move |kind| {
        let ddp = outputs::open(kind)?;
        if let Some((entered, resume, record)) = first.take() {
            Ok(Box::new(HoldFirstFrame {
                ddp: Box::new(RecordSend { ddp, record }),
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
    wakes.events();

    // Keep the device on its way back while the blocking worker opens a fresh socket.
    engine.stall_reclamation(true);
    let Begun::Opening(restart) = sender.begin(set(2), &[]).unwrap() else {
        panic!("a shifted run needs a new lane");
    };
    hold(&mut sender, 0, 73); // the probe wake queues one old frame behind 11
    engine.latest();
    let due = wakes.wake().unwrap();
    assert_eq!(
        engine.latest().applied(),
        sender.sent(),
        "the probe wake applied 73"
    );
    let hands: Vec<_> = wakes
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Handed {
                output: number,
                due,
                ..
            } if number == output(1) => Some(due),
            Event::Handed { .. } | Event::Came { .. } | Event::Counted { .. } => None,
        })
        .collect();
    assert_eq!(hands, [due], "the probe wake handed one old frame");
    old_send_at_acceptance(
        &socket,
        release,
        completed,
        &[11, 73],
        || sender.finish(restart.open()).unwrap(),
        || reopened.recv_timeout(usb::NEVER).unwrap(),
    );
    engine.stall_reclamation(false);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 91);
}

/// Record the result after the real adapter has sent and before its worker returns.
struct RecordSend {
    ddp: Box<dyn engine::Device>,
    record: mpsc::SyncSender<(u8, Outcome)>,
}

impl engine::Device for RecordSend {
    fn take(&mut self, frame: &[u8]) -> Outcome {
        let result = self.ddp.take(frame);
        self.record.try_send((frame[0], result)).unwrap();
        result
    }
}

fn old_send_at_acceptance(
    socket: &UdpSocket,
    release: mpsc::Sender<()>,
    completed: mpsc::Receiver<(u8, Outcome)>,
    old_markers: &[u8],
    complete: impl FnOnce() + Send,
    ready: impl FnOnce(),
) {
    let (finished, accepted) = mpsc::channel();
    thread::scope(|threads| {
        threads.spawn(move || {
            complete();
            // Take only sends that have already completed when the writer returns.
            finished
                .send(completed.try_iter().collect::<Vec<_>>())
                .unwrap();
        });
        ready();
        let early = accepted.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        let sent = early.unwrap_or_else(|_| accepted.recv_timeout(usb::NEVER).unwrap());
        let expected: Vec<_> = old_markers
            .iter()
            .map(|&marker| (marker, Outcome::Accepted))
            .collect();
        assert_eq!(sent, expected, "completed adapter writes at acceptance");
    });
    for marker in old_markers {
        assert_eq!(packet(socket, 0x41, 0x0B, 0, 510).2[0], *marker);
    }
}

#[test]
fn an_explicit_reroute_waits_for_the_last_send_in_the_old_lane() {
    let (socket, kind) = ddp(Color::Rgb);
    let (entered, taking) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (record, completed) = mpsc::sync_channel(1);
    let mut gate = Some((entered, resume));
    let (_engine, mut sender, wakes) = start(Box::new(move |kind| {
        Ok(Box::new(HoldFirstFrame {
            ddp: Box::new(RecordSend {
                ddp: outputs::open(kind)?,
                record: record.clone(),
            }),
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
        completed,
        &[11],
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
    let (_engine, mut sender, wakes) = start(Box::new({
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
    assert_eq!(sender.send(set(2, 2)), Err(Unsent::Refused(Refusal::Opening)));
    assert_eq!(
        sender.send(Command::RemoveOutput { output: one }),
        Err(Unsent::Refused(Refusal::Opening))
    );
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2, vec![0; 510]);
    sender.finish(opening.open()).unwrap();
    // Reading the first frame after each edit rules out a stale handover from the old lane.
    for (first, marker) in [(1, 600), (2, 88)] {
        if first == 2 {
            let Begun::Opening(opening) = sender.begin(set(first, 2), &[]).unwrap() else {
                panic!("a shifted run needs a new lane");
            };
            wakes.wake().unwrap();
            assert_eq!(packet(&socket, 0x41, 0x0B, 0, 1023).2[600], 91);
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

#[test]
fn a_prepared_reroute_keeps_the_old_run_sending_until_finish() {
    let (socket, kind) = ddp(Color::Rgb);
    let (_engine, mut sender, mut wakes) = start(Box::new(outputs::open));
    let one = output(1);
    let set = |first| Command::SetOutput {
        output: one,
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    hold(&mut sender, 0, 11);
    hold(&mut sender, 512, 91);
    sender.send(set(1)).unwrap();
    let initial = wakes.wake().unwrap();
    let (peer, _, frame) = packet(&socket, 0x41, 0x0B, 0, 510);
    assert_eq!(frame[0], 11);

    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("shifting the run needs a new lane");
    };
    wakes.wake_at(initial).unwrap();
    let ready = opening.open();
    hold(&mut sender, 0, 73);
    let due = wakes.wake().unwrap();
    assert!(!wakes.events().iter().any(|event| matches!(event,
        Event::Came { output, outcome: Outcome::Dropped, due: when } if *output == one && *when == due
    )), "the old run dropped the press before finish");
    let (old, _, frame) = packet(&socket, 0x41, 0x0B, 0, 510);
    assert_eq!((old, frame[0]), (peer, 73));

    sender.finish(ready).unwrap();
    wakes.wake().unwrap();
    let (new, _, frame) = packet(&socket, 0x41, 0x0B, 0, 510);
    assert_eq!((new, frame[0]), (peer, 91));
}

#[test]
fn a_reroute_after_cutoff_keeps_the_due_press_on_the_old_run() {
    let (socket, kind) = ddp(Color::Rgb);
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let set = |first| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    hold(&mut sender, 0, 11);
    hold(&mut sender, 512, 91);
    sender.send(set(1)).unwrap();
    let initial = wakes.wake().unwrap();
    let (peer, _, frame) = packet(&socket, 0x41, 0x0B, 0, 510);
    assert_eq!(frame[0], 11);

    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("shifting the run needs a new lane");
    };
    wakes.wake_at(initial).unwrap();
    let ready = opening.open();
    hold(&mut sender, 0, 73);
    wakes.cut().unwrap(); // This due wake has taken the press into its FIFO prefix.

    thread::scope(|threads| {
        let (done, finished) = mpsc::channel();
        let receipt = sender.publish_output(ready).unwrap().unwrap();
        threads.spawn(move || {
            receipt.wait();
            done.send(()).unwrap();
        });
        // Allow an early acknowledgement to finish taking the port while the tick
        // is held. A correct handoff may still be waiting for the last old write.
        let early = finished.recv_timeout(Duration::from_secs(1));
        wakes.finish().unwrap();
        wakes.wake().unwrap();
        if early.is_err() {
            finished.recv_timeout(usb::NEVER).unwrap();
        }
    });
    for marker in [73, 91] {
        let (from, _, frame) = packet(&socket, 0x41, 0x0B, 0, 510);
        assert_eq!((from, frame[0]), (peer, marker));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_clients_press_reaches_the_next_due_frame_while_an_open_waits() {
    press_during_setup(Pause::Open).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_clients_press_reaches_the_next_due_frame_while_a_lane_starts() {
    press_during_setup(Pause::LaneStart).await;
}

#[test]
fn finishing_a_prepared_reroute_needs_no_second_blocking_worker() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (socket, kind) = ddp(Color::Rgb);
            let (engine, mut sender, wakes) =
                Engine::start_stepped(Universes::new(2).unwrap(), Box::new(outputs::open)).unwrap();
            sender
                .send(Command::SetOutput {
                    output: output(1),
                    kind,
                    universes: RunNumbers { first: 1, count: 1 },
                    rate: Some(Rate::Sixty),
                })
                .unwrap();
            hold(&mut sender, 512, 91);
            let (entered, starting) = mpsc::channel();
            let (release_start, wait_start) = mpsc::channel();
            sender.pause_lane_start(output(1), entered, wait_start);
            let mut desk = common::connect(common::serve(engine, sender, None).await).await;
            wakes.hold().unwrap();
            let press =
                json!({"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": 11}});
            assert_eq!(common::command(&mut desk, press).await, "Accepted");
            let last = wakes.wake().unwrap();
            // Read through the final old packet, including any from server startup.
            while packet(&socket, 0x41, 0x0B, 0, 510).2[0] != 11 {}
            common::send(
                &mut desk,
                &json!({"Send": {"command": {"SetOutput": {
                    "output": 1, "kind": kind, "universes": {"first": 2, "count": 1}, "rate": "60"
                }}}}),
            )
            .await;
            tokio::time::timeout(usb::NEVER, async {
                while starting.try_recv().is_err() {
                    wakes.wake_at(last).unwrap();
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("replacement reached Lane::start");

            // The queued job occupies the only worker as soon as the opener returns.
            // Dropping its gate also releases it if an assertion unwinds.
            let (release_pool, wait_pool) = mpsc::channel::<()>();
            let queued = tokio::task::spawn_blocking(move || {
                let _ = wait_pool.recv();
            });
            drop(release_start);
            assert_eq!(common::answer(&mut desk).await, "Accepted");
            wakes.wake().unwrap();
            assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 91);
            drop(release_pool);
            queued.await.unwrap();
            wakes.run();
            common::send(&mut desk, &json!("Stop")).await;
        });
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

/// Keeps reading the test's end of an adapter, as a plugged adapter takes every frame:
/// an end nobody reads fills up and the port is lost again. `stop` drops the end; a drop
/// (also a failed assert) tells the reader to stop and leaves it to end on its own.
struct Drain {
    stop: Arc<std::sync::atomic::AtomicBool>,
    reader: Option<thread::JoinHandle<()>>,
}

impl Drain {
    fn start(mut end: serialport::TTYPort) -> Self {
        use std::io::Read;

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let reader = thread::spawn(move || {
            let mut bytes = [0; 1024];
            // frames keep coming while the port runs, so each read returns soon
            while !stopped.load(Ordering::Acquire) && end.read(&mut bytes).is_ok_and(|n| n > 0) {}
        });
        Self {
            stop,
            reader: Some(reader),
        }
    }

    /// Stops reading and drops the end once the read under way returns.
    fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
    }
}

impl Drop for Drain {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
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
        json!({"Refused": {"reason": {"CannotOpen": {"why": {"Device": {"with": null}}}}}})
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

    assert_eq!(result, Err(Unsent::Refused(Refusal::CannotOpen { why: engine::OpenFailure::Device { with: None } })));
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
        assert_eq!(read(11), (first_peer, first_lane));
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
fn an_accepted_reroute_sends_only_the_newest_queued_old_frame_first() {
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
    let (record, completed) = mpsc::sync_channel(3);
    let open = Box::new(move |kind| {
        Ok(Box::new(HoldFirstFrame {
            ddp: Box::new(RecordSend {
                ddp: outputs::open(kind)?,
                record: record.clone(),
            }),
            gate: gate.take(),
        }) as Box<dyn engine::Device>)
    });
    let (_engine, mut sender, mut wakes) =
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
    let first = at;
    taking.recv_timeout(timeout).unwrap();
    // Occupy both remaining lane boxes while the first send is in progress.
    let period = Rate::Sixty.period();
    for marker in [22, 33] {
        sender.send(hold(1, marker)).unwrap();
        at = wakes.wake_at(at + period).unwrap();
    }

    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("the shifted run needs a lane");
    };
    at = wakes.wake_at(at + period).unwrap(); // all three old boxes are occupied
    let handoff = sender.publish_output(opening.open()).unwrap().unwrap();
    wakes.wake_at(at).unwrap();
    release.send(()).unwrap();
    assert!(handoff.wait());
    assert_eq!(
        completed.try_iter().collect::<Vec<_>>(),
        [(11, Outcome::Accepted), (33, Outcome::Accepted)],
        "only the in-flight and newest old writes completed at acceptance"
    );
    assert_eq!(read(), 11, "the frame already inside the device finishes");
    assert_eq!(
        read(),
        33,
        "the newest queued old frame finishes before acceptance"
    );

    wakes.wake_at(at + period).unwrap();
    assert_eq!(read(), 91, "the first new send after accepting the edit");

    let events = wakes.events();
    for (due, outcome) in [
        (first, Outcome::Accepted),
        (first + period, Outcome::Dropped),
        (first + 2 * period, Outcome::Accepted),
        (at, Outcome::Dropped),
    ] {
        let finished: Vec<_> = events
            .iter()
            .filter_map(|event| match *event {
                Event::Came {
                    due: when, outcome, ..
                } if when == due => Some(outcome),
                Event::Handed { .. } | Event::Came { .. } | Event::Counted { .. } => None,
            })
            .collect();
        assert_eq!(
            finished,
            [outcome],
            "one outcome for old frame due at {due:?}"
        );
    }
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_reroute_keeps_the_old_run_sending_during_open() {
    let (socket, kind) = ddp(Color::Rgb);
    let (entered, reopening) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut opened = false;
    let (engine, sender, mut wakes) = Engine::start_stepped(
        Universes::new(2).unwrap(),
        Box::new(move |kind| {
            if !std::mem::replace(&mut opened, true) {
                return outputs::open(kind);
            }
            entered.send(()).unwrap();
            gate.recv_timeout(usb::NEVER).unwrap();
            Err(std::io::ErrorKind::NotFound.into())
        }),
    )
    .unwrap();
    engine.stall_reclamation(true);
    let address = common::serve(engine, sender, None).await;
    let mut writer = common::connect(address).await;
    let mut other = common::connect(address).await;
    wakes.hold().unwrap();
    let set = |first| {
        json!({"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": first, "count": 1}, "rate": "60"
        }})
    };
    let press =
        |value| json!({"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": value}});
    assert_eq!(common::command(&mut writer, set(1)).await, "Accepted");
    assert_eq!(common::command(&mut other, press(11)).await, "Accepted");
    let at = wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 11);
    wakes.events();

    common::send(&mut writer, &json!({"Send": {"command": set(2)}})).await;
    tokio::task::block_in_place(|| reopening.recv_timeout(usb::NEVER)).unwrap();
    assert_eq!(common::command(&mut other, press(91)).await, "Accepted");
    let period = Rate::Sixty.period();
    let at = wakes.wake_at(at + period).unwrap();
    let events = wakes.events();
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Handed { output: number, .. } if *number == output(1)
    )));
    let dropped = events.iter().any(|event| {
        matches!(
            event,
            Event::Came { output: number, outcome: Outcome::Dropped, .. } if *number == output(1)
        )
    });
    let while_pending = (!dropped).then(|| packet(&socket, 0x41, 0x0B, 0, 510).2[0]);

    release.send(()).unwrap();
    assert_eq!(
        common::answer(&mut writer).await,
        json!({"Refused": {"reason": {"CannotOpen": {"why": {"Device": {"with": null}}}}}})
    );
    wakes.wake_at(at + period).unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 91);
    wakes.run();
    common::send(&mut writer, &json!("Stop")).await;
    assert_eq!(
        while_pending,
        Some(91),
        "the due frame on the unchanged old run was dropped until the refusal: {events:?}"
    );
}

async fn output_picture(desk: &mut common::Socket, until: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(usb::NEVER, async {
        loop {
            common::send(desk, &json!("Query")).await;
            let reply = common::answer(desk).await;
            if until(&reply["Picture"]["state"]["outputs"][0]) {
                return reply;
            }
        }
    })
    .await
    .expect("an output picture matching the requested state")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepting_a_reroute_does_not_hide_the_adapters_existing_outage() {
    let kind = OutputKind::DmxUsbPro {
        adapter: Adapter::FIRST,
    };
    let (find, mut fake) = usb::Fake::new(true);
    let mut find = Some(find);
    let (engine, sender, wakes) = start(Box::new(move |_| {
        let pro = DmxUsbPro::open(find.take().ok_or(std::io::ErrorKind::AddrInUse)?)?;
        Ok(Box::new(pro))
    }));
    wakes.run();
    let address = common::serve(engine, sender, None).await;
    let mut writer = common::connect(address).await;
    let mut reader = common::connect(address).await;
    let set = |first| {
        json!({"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": first, "count": 1}, "rate": "60"
        }})
    };
    for (universe, value) in [(1, 11), (2, 91)] {
        let press = json!({"SetChannel": {
            "channel": {"universe": universe, "address": 1}, "value": value
        }});
        assert_eq!(common::command(&mut writer, press).await, "Accepted");
    }
    assert_eq!(common::command(&mut writer, set(1)).await, "Accepted");
    let mut end = tokio::task::block_in_place(|| fake.next_end());
    assert_eq!(
        tokio::task::block_in_place(|| usb::read_message(&mut end))[5],
        11
    );
    output_picture(&mut reader, |out| out["status"] == "Running").await;
    fake.plugged().store(false, Ordering::Release);
    drop(end);
    output_picture(&mut reader, |out| out["status"]["Reconnecting"].is_object()).await;

    assert_eq!(common::command(&mut writer, set(2)).await, "Accepted");
    let after = output_picture(&mut reader, |out| {
        out["universes"]["first"] == 2 && out["counters"]["failed"].as_u64().is_some_and(|n| n > 0)
    })
    .await;
    fake.plugged().store(true, Ordering::Release);
    let first = tokio::task::block_in_place(|| usb::read_message(&mut fake.next_end()));
    assert_eq!(first[5], 91);
    common::send(&mut writer, &json!("Stop")).await;

    let state = &after["Picture"]["state"]["outputs"][0]["status"];
    let health = &after["Picture"]["health"]["outputs"][0]["status"];
    assert!(
        state["Reconnecting"].is_object() && health["Reconnecting"].is_object(),
        "rerouting the same lost adapter reported state={state}, health={health}"
    );
}

fn state_until(engine: &mut Engine, ready: impl Fn(Output) -> bool) -> Output {
    let started = Instant::now();
    loop {
        let state = engine.fresh().unwrap().state.outputs[0];
        if let Some(state) = state.filter(|state| ready(*state)) {
            return state;
        }
        assert!(
            started.elapsed() < usb::NEVER,
            "last output state: {state:?}"
        ); // guard
    }
}

#[test]
fn an_accepted_usb_reroute_stays_reconnecting_until_the_lost_port_returns() {
    let kind = OutputKind::DmxUsbPro {
        adapter: Adapter::FIRST,
    };
    let (find, mut adapter) = usb::Fake::new(true);
    let mut find = Some(find);
    let (mut engine, mut sender) = Engine::start_with(
        Universes::new(2).unwrap(),
        Box::new(move |_| {
            let find = find.take().ok_or(std::io::ErrorKind::AddrInUse)?;
            Ok(Box::new(DmxUsbPro::open(find)?))
        }),
    )
    .unwrap();
    let set = |first| Command::SetOutput {
        output: output(1),
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
    sender.send(set(1)).unwrap();
    let mut port = adapter.next_end();
    assert_eq!(usb::read_message(&mut port)[5], 11);
    state_until(&mut engine, |state| state.status == Status::Running);

    adapter.plugged().store(false, Ordering::Release);
    drop(port);
    state_until(&mut engine, |state| {
        matches!(state.status, Status::Reconnecting { .. })
    });
    sender.send(set(2)).unwrap();
    let after = state_until(&mut engine, |state| {
        state.universes.first() == 2 && state.counters.failed > 0
    });
    let mut health = HealthSample::new(Duration::ZERO, 0, Status::NotFound);
    assert!(engine.fresh().unwrap().health_sample(output(1), &mut health));
    assert!(
        matches!(
            (after.status, health.status),
            (Status::Reconnecting { failures: earlier }, Status::Reconnecting { failures: later })
                if later >= earlier
        ),
        "state {:?}, subsequent health {:?}",
        after.status,
        health.status
    );

    adapter.plugged().store(true, Ordering::Release);
    let mut port = adapter.next_end();
    assert_eq!(usb::read_message(&mut port)[5], 91);
    state_until(&mut engine, |state| state.status == Status::Running);
    assert!(
        matches!(health.status, Status::Reconnecting { .. }),
        "the accepted run must show the existing outage, got {:?}",
        health.status
    );
}

fn first_rgb_slot(socket: &UdpSocket) -> u8 {
    let mut bytes = [0; 1536];
    assert_eq!(socket.recv(&mut bytes).unwrap(), 520);
    assert_eq!(&bytes[2..10], &[0x0B, 1, 0, 0, 0, 0, 1, 254]);
    bytes[10]
}

#[test]
fn a_resize_that_will_be_refused_keeps_sending_the_old_run_while_it_opens() {
    let (wire, kind) = stream::receiver(Color::Rgb);
    let (entered, opening) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut first = true;
    let (engine, mut sender, mut wakes) = Engine::start_stepped(
        Universes::new(2).unwrap(),
        Box::new(move |kind| {
            if std::mem::replace(&mut first, false) {
                return outputs::open(kind);
            }
            entered.send(()).unwrap();
            gate.recv_timeout(usb::NEVER).unwrap();
            Err(std::io::ErrorKind::NotFound.into())
        }),
    )
    .unwrap();
    let set = |count| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first: 1, count },
        rate: None,
    };
    wakes.hold().unwrap();
    sender.send(set(1)).unwrap();
    let initial = wakes.wake().unwrap();
    assert_eq!(first_rgb_slot(&wire), 0);

    engine.stall_reclamation(true);
    let Begun::Opening(restart) = sender.begin(set(2), &[]).unwrap() else {
        panic!("resizing an output needs a new lane");
    };
    wakes.wake_at(initial).unwrap();
    let worker = thread::spawn(|| restart.open());
    opening.recv_timeout(usb::NEVER).unwrap();

    wakes.events();
    sender
        .send(Command::SetChannel {
            channel: ChannelNumbers {
                universe: 1,
                address: 1,
            },
            value: 73,
        })
        .unwrap();
    let due = wakes.wake().unwrap();
    let started = Instant::now();
    let outcome = loop {
        if let Some(outcome) = wakes.events().into_iter().find_map(|event| match event {
            Event::Came {
                output: number,
                outcome,
                due: finished,
            } if number == output(1) && finished == due => Some(outcome),
            Event::Handed { .. } | Event::Came { .. } | Event::Counted { .. } => None,
        }) {
            break outcome;
        }
        assert!(started.elapsed() < usb::NEVER, "due frame never completed"); // guard
        wakes.wake_at(due).unwrap();
    };
    let marker = (outcome == Outcome::Accepted).then(|| first_rgb_slot(&wire));

    release.send(()).unwrap();
    assert_eq!(
        sender.finish(worker.join().unwrap()),
        Err(Unsent::Refused(Refusal::CannotOpen { why: engine::OpenFailure::Device { with: None } }))
    );
    engine.stall_reclamation(false);
    wakes.wake().unwrap();
    assert_eq!(first_rgb_slot(&wire), 73);
    assert_eq!((outcome, marker), (Outcome::Accepted, Some(73)));
}

fn first_channel(socket: &UdpSocket) -> u8 {
    let mut packet = [0; 1500];
    assert_eq!(socket.recv(&mut packet).unwrap(), 520);
    assert_eq!(packet[8..10], 510_u16.to_be_bytes());
    packet[10]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_press_reaches_the_accepted_output_while_its_replacement_lane_starts() {
    let (socket, kind) = stream::receiver(Color::Rgb);
    let (engine, mut sender, mut wakes) =
        Engine::start_stepped(Universes::new(2).unwrap(), Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    for (universe, value) in [(1, 11), (2, 22)] {
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
    sender
        .send(Command::SetOutput {
            output,
            kind,
            universes: RunNumbers { first: 1, count: 1 },
            rate: Some(Rate::Sixty),
        })
        .unwrap();
    wakes.wake().unwrap();
    assert_eq!(first_channel(&socket), 11);

    let (entered, starting) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    sender.pause_lane_start(output, entered, gate);
    wakes.run();
    let address = common::serve(engine, sender, None).await;
    let mut writer = common::connect(address).await;
    let mut performer = common::connect(address).await;
    wakes.hold().unwrap();
    let start = wakes.wake().unwrap();
    common::send(
        &mut writer,
        &json!({"Send": {"command": {"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": 2, "count": 1}, "rate": "60"
        }}}}),
    )
    .await;

    // Apply the pending route work without advancing the output's due time.
    tokio::time::timeout(Duration::from_secs(10), async {
        while starting.try_recv().is_err() {
            wakes.wake_at(start).unwrap();
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the replacement reaches Lane::start");
    let _ = wakes.events();
    socket.set_nonblocking(true).unwrap();
    while socket.recv(&mut [0; 1500]).is_ok() {}
    socket.set_nonblocking(false).unwrap();

    let press = json!({"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": 91}});
    assert_eq!(common::command(&mut performer, press).await, "Accepted");
    let due = wakes.wake_at(start + Rate::Sixty.period()).unwrap();
    let events = wakes.events();
    let dropped = events.iter().any(|event| {
        matches!(event, Event::Came { output: number, outcome: Outcome::Dropped, due: time }
            if *number == output && *time == due)
    });
    let displayed = (!dropped).then(|| first_channel(&socket));

    release.send(()).unwrap();
    assert_eq!(common::answer(&mut writer).await, "Accepted");
    wakes.wake_at(due + Rate::Sixty.period()).unwrap();
    assert_eq!(first_channel(&socket), 22);
    wakes.run();
    common::send(&mut performer, &json!("Stop")).await;

    assert_eq!(
        displayed,
        Some(91),
        "the still-accepted route's first due frame after the press: {events:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_adapter_reports_one_loss_and_one_return_across_a_reroute() {
    let (find, mut adapter) = usb::Fake::new(true);
    let mut find = Some(find);
    let (engine, sender, wakes) = start(Box::new(move |_| {
        let find = find.take().ok_or(std::io::ErrorKind::AddrInUse)?;
        Ok(Box::new(DmxUsbPro::open(find)?))
    }));
    // The exclusive port must move even when the free thread cannot reclaim lanes.
    engine.stall_reclamation(true);
    wakes.run();
    let address = common::serve(engine, sender, None).await;
    let mut desk = common::connect(address).await;
    let mut listener = common::connect(address).await;
    let kind = OutputKind::DmxUsbPro {
        adapter: Adapter::FIRST,
    };
    let route = |first| {
        json!({"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": first, "count": 1}, "rate": null
        }})
    };
    let press = json!({"SetChannel": {"channel": {"universe": 2, "address": 1}, "value": 91}});
    assert_eq!(common::command(&mut desk, press).await, "Accepted");
    assert_eq!(common::command(&mut desk, route(1)).await, "Accepted");
    let end = Drain::start(tokio::task::block_in_place(|| adapter.next_end()));
    output_picture(&mut desk, |out| out["status"] == "Running").await;
    common::send(&mut listener, &json!("Subscribe")).await;
    common::answer(&mut listener).await;

    let mut events = Vec::new();
    let lost = json!("OutputLost");
    let returned = json!({"Recovered": {"reason": "OutputAvailable"}});
    adapter.plugged().store(false, Ordering::Release);
    tokio::task::block_in_place(|| end.stop());
    health_event(&mut listener, &lost, &mut events).await;
    let before = output_picture(&mut desk, |out| {
        out["status"]["Reconnecting"]["failures"]
            .as_u64()
            .is_some_and(|n| n >= 2)
    })
    .await;
    let previous_failures =
        before["Picture"]["state"]["outputs"][0]["status"]["Reconnecting"]["failures"]
            .as_u64()
            .unwrap();
    assert_eq!(common::command(&mut desk, route(2)).await, "Accepted");
    let after = output_picture(&mut desk, |out| {
        out["universes"]["first"] == 2 && out["counters"]["failed"].as_u64().is_some_and(|n| n > 0)
    })
    .await;
    let output = &after["Picture"]["state"]["outputs"][0];
    assert!(
        output["status"]["Reconnecting"]["failures"]
            .as_u64()
            .unwrap()
            >= previous_failures + output["counters"]["failed"].as_u64().unwrap()
    );
    adapter.plugged().store(true, Ordering::Release);
    let mut end = tokio::task::block_in_place(|| adapter.next_end());
    assert_eq!(
        tokio::task::block_in_place(|| usb::read_message(&mut end))[5],
        91
    );
    let end = Drain::start(end);
    let report = health_event(&mut listener, &returned, &mut events).await;
    assert_eq!(
        report["Health"]["health"]["outputs"][0]["status"],
        "Running"
    );
    events.retain(|event| *event == lost || *event == returned);
    assert_eq!(events, [lost, returned]);
    tokio::task::block_in_place(|| end.stop());
    common::send(&mut desk, &json!("Stop")).await;
}

#[test]
fn a_press_already_due_on_the_old_run_reaches_the_wire_before_reroute_acceptance() {
    for publish in [false, true] {
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
        let set = |first| Command::SetOutput {
            output: output(1),
            kind,
            universes: RunNumbers { first, count: 1 },
            rate: Some(Rate::Sixty),
        };
        hold(&mut sender, 0, 11);
        hold(&mut sender, 512, 91);
        sender.send(set(1)).unwrap();
        let first = wakes.wake().unwrap();
        taking.recv_timeout(usb::NEVER).unwrap();

        let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
            panic!("a shifted run needs a new lane");
        };
        wakes.wake_at(first).unwrap();
        let ready = opening.open();
        hold(&mut sender, 0, 73);
        // The only waiting frame is the press, due before publication. Keep the
        // previous datagram inside take until this cutover has reached the old lane.
        wakes.wake().unwrap();
        let handoff = publish.then(|| {
            let handoff = sender.publish_output(ready).unwrap().unwrap();
            wakes.wake().unwrap();
            handoff
        });
        release.send(()).unwrap();
        if let Some(handoff) = handoff {
            handoff.wait();
        }

        let (peer, _, in_flight) = packet(&socket, 0x41, 0x0B, 0, 510);
        let (next_peer, _, next) = packet(&socket, 0x41, 0x0B, 0, 510);
        assert_eq!(
            (in_flight[0], next[0], next_peer),
            (11, 73, peer),
            "publish={publish}: the single press frame due before the cutover"
        );
    }
}

#[test]
fn a_reroute_keeps_the_old_lanes_last_completed_failure() {
    struct FailsOnMarker(Box<dyn engine::Device>, mpsc::Sender<u8>);

    impl engine::Device for FailsOnMarker {
        fn take(&mut self, frame: &[u8]) -> Outcome {
            match frame[0] {
                marker @ (73 | 91) => {
                    self.1.send(marker).unwrap();
                    Outcome::Failed
                }
                _ => self.0.take(frame),
            }
        }
    }

    let (socket, kind) = ddp(Color::Rgb);
    let (failed, failures) = mpsc::channel();
    let (mut engine, mut sender, wakes) = start(Box::new(move |kind| {
        Ok(Box::new(FailsOnMarker(
            outputs::open(kind)?,
            failed.clone(),
        )))
    }));
    let set = |first| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    hold(&mut sender, 0, 11);
    hold(&mut sender, 512, 91);
    sender.send(set(1)).unwrap();
    let first = wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 11);

    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("a shifted run needs a new lane");
    };
    wakes.wake_at(first).unwrap();
    let ready = opening.open();
    let mut read = |at, first, failed| {
        let started = Instant::now();
        loop {
            engine.latest();
            wakes.wake_at(at).unwrap();
            let snapshot = engine.latest();
            if let Some(out) = snapshot.state.outputs[0]
                && out.universes.first() == first
                && out.counters.failed == failed
            {
                let mut health = HealthSample::new(Duration::ZERO, 0, Status::NotFound);
                assert!(snapshot.health_sample(output(1), &mut health));
                break (out.status, health.status);
            }
            assert!(
                started.elapsed() < usb::NEVER,
                "last state: {:?}",
                snapshot.state.outputs[0]
            ); // guard
        }
    };
    hold(&mut sender, 0, 73);
    let due = wakes.wake().unwrap();
    assert_eq!(failures.recv_timeout(usb::NEVER).unwrap(), 73);
    let reconnecting = Status::Reconnecting { failures: 1 };
    assert_eq!(read(due, 1, 1), (reconnecting, reconnecting));

    // The old send completes before finish returns, but there is no intervening
    // tick to collect its result. The first new-route send then fails as well.
    wakes.wake().unwrap();
    sender.finish(ready).unwrap();
    assert_eq!(failures.recv_timeout(usb::NEVER).unwrap(), 73);
    let due = wakes.wake().unwrap();
    assert_eq!(failures.recv_timeout(usb::NEVER).unwrap(), 91);
    let status = read(due, 2, 1);
    hold(&mut sender, 512, 21);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 21);
    let expected = Status::Reconnecting { failures: 3 };
    assert_eq!(status, (expected, expected));
}

#[test]
fn stopping_at_an_unfinished_cutoff_returns_the_writer_without_another_wake() {
    let (socket, kind) = ddp(Color::Rgb);
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let set = |first| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    hold(&mut sender, 0, 11);
    sender.send(set(1)).unwrap();
    let at = wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 11);
    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("rerouting needs another lane");
    };
    wakes.wake_at(at).unwrap();
    let opened = opening.open();
    wakes.cut().unwrap();
    let handoff = sender.publish_output(opened).unwrap().unwrap();
    let (finished, answer) = mpsc::channel();
    thread::scope(|scope| {
        scope.spawn(move || finished.send(handoff.wait()).unwrap());
        sender.stop_outputs();
        let result = answer.recv_timeout(usb::NEVER);
        wakes.finish().unwrap();
        assert_eq!(result, Ok(false), "T13 cancels the writer waiting at T5");
    });
    socket.set_nonblocking(true).unwrap();
    assert_eq!(
        socket.recv(&mut [0]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
