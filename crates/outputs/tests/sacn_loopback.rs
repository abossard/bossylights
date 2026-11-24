//! An sACN output sends to a socket on this machine; every packet that arrives is decoded by
//! the spec receiver (`common/sacn.rs`) and checked.

#![expect(
    clippy::disallowed_methods,
    reason = "the test sends frames at the output's rate; it is not the output thread"
)]

#[path = "common/sacn.rs"]
mod receiver;

use engine_core::OutputNumber;
use outputs::sacn::{self, Destination, Universe};
use outputs::{Device, Outcome, Rate};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

const CID: [u8; 16] = [
    0x6B, 0xA7, 0xB8, 0x10, 0x9D, 0xAD, 0x11, 0xD1, 0x80, 0xB4, 0x00, 0xC0, 0x4F, 0xD4, 0x30, 0xC8,
];

fn receiver_at(ip: Ipv4Addr) -> (UdpSocket, SocketAddrV4) {
    let socket = UdpSocket::bind(SocketAddrV4::new(ip, 0)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        panic!("IPv4");
    };
    (socket, address)
}

fn open(to: Destination, first: u16, output: u8) -> Box<dyn Device> {
    sacn::open(
        to,
        Universe::new(first).unwrap(),
        CID,
        OutputNumber::try_from(output).unwrap(),
    )
    .unwrap()
}

fn unicast(first: u16) -> (UdpSocket, Box<dyn Device>) {
    let (receiver, address) = receiver_at(Ipv4Addr::LOCALHOST);
    (receiver, open(Destination::Unicast(address), first, 1))
}

/// The next packet at the receiver, decoded and checked.
fn next(receiver: &UdpSocket) -> receiver::Data {
    let mut packet = [0; 1000];
    let size = receiver.recv(&mut packet).unwrap();
    receiver::decode(&packet[..size])
}

/// Sends a marker from another socket: the next packet the receiver reads proves that
/// nothing else came before it, without waiting for a time.
fn nothing_more(receiver: &UdpSocket) {
    let SocketAddr::V4(address) = receiver.local_addr().unwrap() else {
        panic!("IPv4");
    };
    let to = SocketAddrV4::new(Ipv4Addr::LOCALHOST, address.port());
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .send_to(b"marker", to)
        .unwrap();
    let mut packet = [0; 1000];
    let size = receiver.recv(&mut packet).unwrap();
    assert_eq!(
        &packet[..size],
        b"marker",
        "a packet came before the marker"
    );
}

fn at_its_rate(device: &mut Box<dyn Device>, frame: &[u8]) -> Outcome {
    let outcome = device.take(frame);
    thread::sleep(Rate::FortyFour.period()); // poll: frames go out at the output's rate
    outcome
}

/// Channels whose channel `n` (counted from 0 across the route) holds `n % 251`.
fn counting(channels: usize) -> Vec<u8> {
    (0..channels).map(|n| (n % 251) as u8).collect()
}

#[test]
fn every_packet_arrives_as_the_spec_says() {
    // (channels, first universe, output, packets: universe, slots of the stream)
    let cases = [
        (1, 1, 1, vec![(1, 1)]),
        (512, 1, 16, vec![(1, 512)]),
        (513, 7, 3, vec![(7, 512), (8, 1)]),
        (
            1_027,
            0x1234,
            10,
            vec![(0x1234, 512), (0x1235, 512), (0x1236, 3)],
        ),
        (1_024, 63_999, 1, vec![(63_999, 512)]),
    ];
    for (channels, first, output, expected) in cases {
        let (receiver, address) = receiver_at(Ipv4Addr::LOCALHOST);
        let mut device = open(Destination::Unicast(address), first, output);
        let frame = counting(channels);
        for sequence in [0, 1] {
            assert_eq!(device.take(&frame), Outcome::Accepted, "{channels}");
            for (index, &(universe, slots)) in expected.iter().enumerate() {
                let offset = index * 512;
                assert_eq!(
                    next(&receiver),
                    receiver::Data {
                        cid: CID,
                        name: format!("Bossylights {output}"),
                        priority: 100,
                        sequence,
                        options: 0,
                        universe,
                        slots: frame[offset..offset + slots].to_vec(),
                    },
                    "{channels} from {first}, universe {index}"
                );
            }
        }
        nothing_more(&receiver);
    }
}

#[test]
fn each_universe_runs_its_sequence_0_to_255_and_round() {
    let (receiver, mut device) = unicast(1);
    let frame = counting(1_024);
    for count in 0..300_u32 {
        assert_eq!(device.take(&frame), Outcome::Accepted);
        let sequence = u8::try_from(count % 256).unwrap();
        for universe in [1, 2] {
            let data = next(&receiver);
            assert_eq!(
                (data.universe, data.sequence),
                (universe, sequence),
                "frame {count}"
            );
        }
    }
}

#[test]
fn one_device_sending_another_route_carries_each_universes_sequence_on() {
    let (receiver, mut device) = unicast(1);
    // (channels of the route, what arrives: universe, sequence)
    let cases = [
        (1_536, vec![(1, 0), (2, 0), (3, 0)]),
        (512, vec![(1, 1)]),
        (1_536, vec![(1, 2), (2, 1), (3, 1)]),
    ];
    for (channels, expected) in cases {
        assert_eq!(device.take(&counting(channels)), Outcome::Accepted);
        let found: Vec<_> = expected
            .iter()
            .map(|_| {
                let data = next(&receiver);
                (data.universe, data.sequence)
            })
            .collect();
        assert_eq!(found, expected, "{channels}");
    }
}

#[test]
fn terminate_sends_three_rounds_of_stream_terminated_then_nothing() {
    let (receiver, mut device) = unicast(5);
    let frame = counting(600);
    for _ in 0..2 {
        assert_eq!(device.take(&frame), Outcome::Accepted);
    }
    for _ in 0..4 {
        next(&receiver);
    }
    device.terminate();
    // (universe, sequence) of each terminated packet: the start code only
    let expected = [(5, 2), (6, 2), (5, 3), (6, 3), (5, 4), (6, 4)];
    for (universe, sequence) in expected {
        let data = next(&receiver);
        assert_eq!(
            (data.universe, data.sequence, data.options, data.slots.len()),
            (universe, sequence, 0x40, 0),
            "{universe} {sequence}"
        );
    }
    // Ended: a frame and another terminate send nothing.
    assert_eq!(device.take(&frame), Outcome::Accepted);
    device.terminate();
    nothing_more(&receiver);
}

#[test]
fn terminate_closes_the_universes_of_the_last_frame() {
    let (receiver, mut device) = unicast(1);
    for channels in [1_536, 512] {
        assert_eq!(device.take(&counting(channels)), Outcome::Accepted);
    }
    for _ in 0..4 {
        next(&receiver);
    }
    device.terminate();
    for sequence in [2, 3, 4] {
        let data = next(&receiver);
        assert_eq!(
            (data.universe, data.sequence, data.options),
            (1, sequence, 0x40)
        );
    }
    nothing_more(&receiver);
}

#[test]
fn a_device_that_never_sent_terminates_nothing() {
    let (receiver, mut device) = unicast(1);
    device.terminate();
    assert_eq!(device.take(&counting(512)), Outcome::Accepted);
    nothing_more(&receiver);
}

/// What multicast to `group` does on this machine. It needs a multicast route and a
/// system that lets this process use it: a Mac asks for local network access and lets a
/// new process through only now and then, a CI Mac sends none. A capability check of a
/// few probes, not a timing assert.
#[derive(Debug, PartialEq, Eq)]
enum Multicast {
    /// A probe came back to `receiver`.
    LoopsBack,
    /// Probes went out, none came back.
    SentOnly,
    /// No probe could be sent.
    NotSent,
}

fn multicast_here(receiver: &UdpSocket, group: Ipv4Addr) -> Multicast {
    let SocketAddr::V4(address) = receiver.local_addr().unwrap() else {
        panic!("IPv4");
    };
    let probe = UdpSocket::bind("0.0.0.0:0").unwrap();
    let mut packet = [0; 16];
    let mut sent = false;
    for _ in 0..5 {
        if probe
            .send_to(b"probe", SocketAddrV4::new(group, address.port()))
            .is_err()
        {
            continue;
        }
        sent = true;
        if receiver
            .recv(&mut packet)
            .is_ok_and(|size| &packet[..size] == b"probe")
        {
            return Multicast::LoopsBack;
        }
    }
    if sent {
        Multicast::SentOnly
    } else {
        Multicast::NotSent
    }
}

/// A receiver that joined only universe 1's group gets universe 1 and never universe 2.
/// Where multicast does not loop back, this is the by-hand check of
/// `docs/design/outputs-sacn.md`; on CI where multicast is sent, it must loop back.
#[test]
#[expect(
    clippy::disallowed_macros,
    reason = "the test says why it checked nothing; it is not the output thread"
)]
fn multicast_sends_each_universe_to_its_own_group() {
    let group = Ipv4Addr::new(239, 255, 0, 1);
    let (receiver, address) = receiver_at(Ipv4Addr::UNSPECIFIED);
    receiver
        .join_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED)
        .unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let here = multicast_here(&receiver, group);
    if here != Multicast::LoopsBack {
        assert!(
            here == Multicast::NotSent || std::env::var_os("CI").is_none(),
            "multicast is sent on CI, so it must loop back"
        );
        eprintln!("multicast {here:?} here: checked by hand instead");
        return;
    }
    let mut device = open(Destination::Multicast(address.port()), 1, 2);
    let frame = counting(1_024);
    // Frames at the rate until one arrives: the system may let the first ones go.
    let mut packet = [0; 1000];
    let started = Instant::now();
    let size = loop {
        assert_eq!(at_its_rate(&mut device, &frame), Outcome::Accepted);
        // A probe that came late is not the device's.
        if let Ok(size) = receiver.recv(&mut packet)
            && &packet[..size] != b"probe"
        {
            break size;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "never came"); // guard
    };
    receiver
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    // Every packet before the marker is universe 1's.
    let mut data = receiver::decode(&packet[..size]);
    let to = SocketAddrV4::new(Ipv4Addr::LOCALHOST, address.port());
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .send_to(b"marker", to)
        .unwrap();
    loop {
        assert_eq!(
            (data.universe, &data.slots[..]),
            (1, &frame[..512]),
            "only universe 1 at 239.255.0.1"
        );
        let size = receiver.recv(&mut packet).unwrap();
        match &packet[..size] {
            b"marker" => break,
            b"probe" => {}
            device => data = receiver::decode(device),
        }
    }
}

#[test]
fn a_receiver_that_is_not_there_fails_the_output() {
    let (gone, address) = receiver_at(Ipv4Addr::LOCALHOST);
    drop(gone);
    let mut device = open(Destination::Unicast(address), 1, 1);
    // The host answers the first packet with a refusal, reported on a later send.
    let frame = counting(512);
    let started = Instant::now();
    loop {
        let outcome = at_its_rate(&mut device, &frame);
        assert_ne!(outcome, Outcome::Dropped);
        if outcome == Outcome::Failed {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "never failed"); // guard
    }
}

#[test]
fn a_unicast_address_that_is_not_one_receiver_is_refused() {
    let to = |ip| Destination::Unicast(SocketAddrV4::new(ip, sacn::PORT));
    let cases = [
        to(Ipv4Addr::new(239, 255, 0, 1)),
        to(Ipv4Addr::BROADCAST),
        to(Ipv4Addr::UNSPECIFIED),
        Destination::Unicast(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        Destination::Multicast(0),
    ];
    for to in cases {
        let opened = sacn::open(
            to,
            Universe::new(1).unwrap(),
            CID,
            OutputNumber::try_from(1).unwrap(),
        );
        assert!(opened.is_err(), "{to:?}");
    }
}
