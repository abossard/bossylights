//! An Art-Net output sends to a socket on this machine; every packet that arrives is
//! decoded by the spec receiver (`common/artnet.rs`) and checked.

#![expect(
    clippy::disallowed_methods,
    reason = "the test sends frames at the output's rate; it is not the output thread"
)]

#[path = "common/artnet.rs"]
mod receiver;

use outputs::artnet::{self, PortAddress};
use outputs::{Device, Outcome, Rate};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

fn node() -> (UdpSocket, SocketAddrV4) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        panic!("IPv4");
    };
    (socket, address)
}

fn open(to: SocketAddrV4, first: u16) -> Box<dyn Device> {
    artnet::open(to, PortAddress::new(first).unwrap()).unwrap()
}

/// The next packet at the node, decoded and checked.
fn next(node: &UdpSocket) -> receiver::ArtDmx {
    let mut packet = [0; 1000];
    let size = node.recv(&mut packet).unwrap();
    receiver::decode(&packet[..size])
}

/// Channels whose channel `n` (counted from 0 across the route) holds `n % 251`.
fn counting(channels: usize) -> Vec<u8> {
    (0..channels).map(|n| (n % 251) as u8).collect()
}

#[test]
fn every_packet_arrives_as_the_spec_says() {
    // (channels, first Port-Address, packets: Port-Address, slots of the stream, length)
    let cases = [
        (1, 1, vec![(1, 1, 2)]),
        (512, 1, vec![(1, 512, 512)]),
        (513, 1, vec![(1, 512, 512), (2, 1, 2)]),
        (
            1_027,
            0x1234,
            vec![(0x1234, 512, 512), (0x1235, 512, 512), (0x1236, 3, 4)],
        ),
    ];
    for (channels, first, expected) in cases {
        let (node, to) = node();
        let mut device = open(to, first);
        let frame = counting(channels);
        for sequence in [1, 2] {
            assert_eq!(device.take(&frame), Outcome::Accepted, "{channels}");
            for (index, &(port_address, slots, length)) in expected.iter().enumerate() {
                let dmx = next(&node);
                let offset = index * 512;
                let mut data = frame[offset..offset + slots].to_vec();
                data.resize(length, 0);
                assert_eq!(
                    dmx,
                    receiver::ArtDmx {
                        sequence,
                        physical: 0,
                        port_address,
                        slots: data,
                    },
                    "{channels} from {first}, universe {index}"
                );
            }
        }
    }
}

#[test]
fn each_universe_runs_its_sequence_1_to_255_and_on_never_0() {
    let (node, to) = node();
    let mut device = open(to, 1);
    let frame = counting(1_024);
    for count in 1..=300 {
        assert_eq!(device.take(&frame), Outcome::Accepted);
        let sequence = u8::try_from((count - 1) % 255 + 1).unwrap();
        for port_address in [1, 2] {
            let dmx = next(&node);
            assert_eq!(
                (dmx.port_address, dmx.sequence),
                (port_address, sequence),
                "frame {count}"
            );
        }
    }
}

#[test]
fn one_device_sending_another_route_carries_each_universes_sequence_on() {
    let (node, to) = node();
    let mut device = open(to, 1);
    // (channels of the route, what arrives: Port-Address, sequence)
    let cases = [
        (1_536, vec![(1, 1), (2, 1), (3, 1)]),
        (512, vec![(1, 2)]),
        (1_536, vec![(1, 3), (2, 2), (3, 2)]),
    ];
    for (channels, expected) in cases {
        assert_eq!(device.take(&counting(channels)), Outcome::Accepted);
        let found: Vec<_> = expected
            .iter()
            .map(|_| {
                let dmx = next(&node);
                (dmx.port_address, dmx.sequence)
            })
            .collect();
        assert_eq!(found, expected, "{channels}");
    }
}

#[test]
fn an_output_to_no_port_or_to_more_than_one_node_is_refused() {
    // (address, opened): ArtDmx is unicast only
    let cases = [
        (SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0), false),
        (SocketAddrV4::new(Ipv4Addr::BROADCAST, artnet::PORT), false),
        (
            SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, artnet::PORT),
            false,
        ),
        (
            SocketAddrV4::new(Ipv4Addr::new(239, 255, 0, 1), artnet::PORT),
            false,
        ),
        (
            SocketAddrV4::new(Ipv4Addr::new(2, 0, 0, 10), artnet::PORT),
            true,
        ),
    ];
    for (to, opened) in cases {
        let device = artnet::open(to, PortAddress::new(1).unwrap());
        assert_eq!(device.is_ok(), opened, "{to}");
    }
}

/// One frame, then a wait of one period: the output's own rate.
fn at_its_rate(device: &mut Box<dyn Device>, frame: &[u8]) -> Outcome {
    let outcome = device.take(frame);
    thread::sleep(Rate::FortyFour.period()); // poll: frames go out at the output's rate
    outcome
}

#[test]
fn a_node_nobody_listens_to_is_failed_and_runs_again_when_one_listens() {
    let (socket, to) = node();
    drop(socket); // nobody listens on this port now
    let mut device = open(to, 1);
    let frame = [7; 512];
    let started = Instant::now();
    while at_its_rate(&mut device, &frame) != Outcome::Failed {
        assert!(started.elapsed() < Duration::from_secs(10), "never failed"); // guard
    }
    let listener = UdpSocket::bind(to).unwrap();
    listener
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let started = Instant::now();
    while at_its_rate(&mut device, &frame) != Outcome::Accepted {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "never runs again"
        ); // guard
    }
    let dmx = next(&listener);
    assert_eq!(dmx.port_address, 1);
    assert_eq!(dmx.slots, [7; 512]);
}
