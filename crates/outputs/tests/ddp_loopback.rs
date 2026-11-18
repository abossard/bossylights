//! A DDP output sends to a socket on this machine; every byte that arrives is checked.

#![expect(
    clippy::disallowed_methods,
    reason = "the test sends frames at the output's rate; it is not the output thread"
)]

use outputs::{Adapter, Color, Device, Levels, Outcome, OutputKind, Rate};
use std::net::{SocketAddr, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

fn receiver() -> (UdpSocket, std::net::SocketAddrV4) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        panic!("IPv4");
    };
    (socket, address)
}

/// Universes whose channel `n` (counted from 0 across the run) holds `n % 251`.
fn counting(universes: usize) -> Vec<Levels> {
    (0..universes)
        .map(|u| Levels(std::array::from_fn(|a| ((u * 512 + a) % 251) as u8)))
        .collect()
}

#[test]
fn every_byte_arrives_as_wled_expects() {
    // (universes, color, packets: flags, data type, offset, length)
    let cases = [
        (1, Color::Rgbw, vec![(0x41, 0x1B, 0, 512)]),
        (1, Color::Rgb, vec![(0x41, 0x0B, 0, 510)]),
        (
            3,
            Color::Rgb,
            vec![(0x40, 0x0B, 0, 1440), (0x41, 0x0B, 1440, 96)],
        ),
        (
            4,
            Color::Rgbw,
            vec![(0x40, 0x1B, 0, 1440), (0x41, 0x1B, 1440, 608)],
        ),
    ];
    for (universes, color, expected) in cases {
        let (socket, address) = receiver();
        let mut device = outputs::open(OutputKind::Ddp { address, color }).unwrap();
        let frame = counting(universes);
        for sequence in [1, 2] {
            assert_eq!(device.take(&frame), Outcome::Accepted);
            for &(flags, kind, offset, length) in &expected {
                let mut packet = [0; 2000];
                let size = socket.recv(&mut packet).unwrap();
                assert_eq!(size, 10 + length, "{universes} {color:?}");
                let [o0, o1, o2, o3] = (offset as u32).to_be_bytes();
                let [l0, l1] = (length as u16).to_be_bytes();
                let header = [flags, sequence, kind, 1, o0, o1, o2, o3, l0, l1];
                assert_eq!(packet[..10], header, "{universes} {color:?} at {offset}");
                let data: Vec<u8> = (offset..offset + length).map(|n| (n % 251) as u8).collect();
                assert_eq!(
                    packet[10..size],
                    data[..],
                    "{universes} {color:?} at {offset}"
                );
            }
        }
    }
}

#[test]
fn each_kind_has_its_default_rate() {
    let (_socket, address) = receiver();
    // (kind, default rate)
    let cases = [
        (
            OutputKind::Ddp {
                address,
                color: Color::Rgb,
            },
            Rate::Sixty,
        ),
        (
            OutputKind::DmxUsbPro {
                adapter: Adapter::FIRST,
            },
            Rate::FortyFour,
        ),
    ];
    for (kind, rate) in cases {
        assert_eq!(outputs::default_rate(kind), rate, "{kind:?}");
    }
}

/// One frame, then a wait of one period: the output's own rate.
fn at_its_rate(device: &mut Box<dyn Device>, frame: &[Levels]) -> Outcome {
    let outcome = device.take(frame);
    thread::sleep(Rate::Sixty.period()); // poll: frames go out at the output's rate
    outcome
}

#[test]
fn a_ddp_output_nobody_listens_to_is_steadily_failed_and_runs_again_when_one_listens() {
    let (socket, address) = receiver();
    drop(socket); // nobody listens on this port now
    let mut device = outputs::open(OutputKind::Ddp {
        address,
        color: Color::Rgb,
    })
    .unwrap();
    let frame = [Levels([7; 512])];
    // Two seconds. The host answers the first packet with a refusal, reported on a later
    // send; from then on the output is failed, frame after frame.
    let outcomes: Vec<_> = (0..120).map(|_| at_its_rate(&mut device, &frame)).collect();
    let refused = outcomes.iter().position(|o| *o == Outcome::Failed);
    assert!(refused.is_some_and(|at| at < 5), "{outcomes:?}");
    let after = &outcomes[refused.unwrap()..];
    assert!(after.iter().all(|o| *o == Outcome::Failed), "{outcomes:?}");

    let listener = UdpSocket::bind(address).unwrap();
    listener
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let started = Instant::now();
    while at_its_rate(&mut device, &frame) != Outcome::Accepted {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "never runs again"
        ); // guard
    }
    let mut packet = [0; 2000];
    let size = listener.recv(&mut packet).unwrap();
    assert_eq!(size, 10 + 510);
    assert_eq!(packet[0], 0x41, "version 1 and push");
    assert!(
        packet[10..size].iter().all(|b| *b == 7),
        "the frame's bytes"
    );
}
