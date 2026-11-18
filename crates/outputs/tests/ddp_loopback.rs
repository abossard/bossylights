//! A DDP output sends to a socket on this machine; every byte that arrives is checked.

use outputs::{Color, Device, Levels, Outcome, OutputKind};
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

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
        let mut device = Device::open(OutputKind::Ddp { address, color }).unwrap();
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
