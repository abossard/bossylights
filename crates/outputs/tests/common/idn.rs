//! A fake IDN projector, written from `docs/design/outputs-idn.md`, not from the code it
//! checks. It decodes every datagram an IDN output sends and fails on any field or rule the
//! spec breaks; across datagrams it checks the sequence and the timestamps.

#![allow(dead_code, reason = "each test file uses its own part")]

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::Duration;

/// One point as the projector draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i16,
    pub y: i16,
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

/// What one datagram asks of the projector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Datagram {
    /// A discrete frame: no points voids the repeat.
    Frame {
        timestamp: u32,
        duration: u32,
        points: Vec<Point>,
    },
    /// Close channel 0.
    CloseChannel { timestamp: u32 },
    /// Close the session.
    CloseSession,
}

/// The tags of the default colour set: X, Y 16 bits, red 638, green 532, blue 460, void.
const TAGS: [u16; 8] = [
    0x4200, 0x4010, 0x4210, 0x4010, 0x527E, 0x5214, 0x51CC, 0x0000,
];

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Decodes one datagram at `rate` points a second, with its Hello sequence. Panics on
/// anything the spec does not allow.
pub fn decode(datagram: &[u8], rate: u32) -> (u16, Datagram) {
    assert!(datagram.len() >= 4, "a Hello header");
    let (command, flags, sequence) = (datagram[0], datagram[1], u16_at(datagram, 2));
    assert_eq!(flags, 0, "client group 0");
    if command == 0x44 {
        assert_eq!(datagram.len(), 4, "a close session carries nothing");
        return (sequence, Datagram::CloseSession);
    }
    assert_eq!(command, 0x40, "a channel message");
    let message = &datagram[4..];
    assert!(message.len() >= 8, "a channel message header");
    assert_eq!(usize::from(u16_at(message, 0)), message.len(), "total size");
    assert!(message.len() <= 65_280, "at most 65,280");
    let content = u16_at(message, 2);
    assert_ne!(content & 0x8000, 0, "a channel message");
    assert_eq!(content & 0x3F00, 0, "channel 0");
    assert_ne!(content & 0x4000, 0, "config follows, every message");
    let timestamp = u32_at(message, 4);
    let config = &message[8..12];
    match content & 0x00FF {
        0x00 => {
            assert_eq!(config, [0, 0x02, 0, 0], "close: no tags, close only");
            assert_eq!(message.len(), 12, "a void chunk carries no data");
            (sequence, Datagram::CloseChannel { timestamp })
        }
        0x02 => {
            assert_eq!(
                config,
                [4, 0x01, 0, 0x02],
                "routing, default service, discrete"
            );
            let tags: Vec<u16> = (0..8).map(|n| u16_at(message, 12 + 2 * n)).collect();
            assert_eq!(tags, TAGS, "the default colour set");
            let chunk = u32_at(message, 28);
            assert_eq!(chunk >> 24, 0, "chunk flags: config match 0, not once");
            let duration = chunk & 0x00FF_FFFF;
            let samples = &message[32..];
            assert_eq!(samples.len() % 7, 0, "whole samples");
            let count = samples.len() / 7;
            assert_ne!(count, 1, "a frame is empty or has at least 2 samples");
            let expected = (count.saturating_sub(1) as u64 * 1_000_000 / u64::from(rate)) as u32;
            assert_eq!(duration, expected, "duration of {count} samples");
            let points: Vec<Point> = samples
                .chunks_exact(7)
                .map(|s| Point {
                    x: i16::from_be_bytes([s[0], s[1]]),
                    y: i16::from_be_bytes([s[2], s[3]]),
                    red: s[4],
                    green: s[5],
                    blue: s[6],
                })
                .collect();
            if let Some(start) = points.first() {
                assert_eq!(
                    (start.red, start.green, start.blue),
                    (0, 0, 0),
                    "the start point is dark"
                );
            }
            (
                sequence,
                Datagram::Frame {
                    timestamp,
                    duration,
                    points,
                },
            )
        }
        other => panic!("chunk type {other:#04x}"),
    }
}

/// A projector on this machine that checks each datagram against the ones before.
pub struct Projector {
    pub socket: UdpSocket,
    rate: u32,
    last: Option<(u16, Option<u32>)>,
}

impl Projector {
    /// Listens on 127.0.0.1, any port; the timeout only guards a missing datagram.
    pub fn new(rate: u32) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        Self {
            socket,
            rate,
            last: None,
        }
    }

    pub fn address(&self) -> SocketAddrV4 {
        let SocketAddr::V4(address) = self.socket.local_addr().unwrap() else {
            panic!("IPv4");
        };
        address
    }

    /// The next datagram, decoded: its sequence follows the last, its timestamp never falls.
    pub fn next(&mut self) -> Datagram {
        let mut bytes = vec![0; 70_000];
        let size = self.socket.recv(&mut bytes).unwrap();
        let (sequence, datagram) = decode(&bytes[..size], self.rate);
        let timestamp = match &datagram {
            Datagram::Frame { timestamp, .. } | Datagram::CloseChannel { timestamp } => {
                Some(*timestamp)
            }
            Datagram::CloseSession => None,
        };
        if let Some((last, last_time)) = self.last {
            assert_eq!(sequence, last.wrapping_add(1), "the sequence counts by 1");
            if let (Some(before), Some(now)) = (last_time, timestamp) {
                assert!(now >= before, "the timestamp fell: {before} then {now}");
            }
        }
        let time = timestamp.or(self.last.and_then(|(_, time)| time));
        self.last = Some((sequence, time));
        datagram
    }

    /// Sends a marker from another socket: the next datagram the projector reads proves
    /// that nothing else came before it, without waiting for a time.
    pub fn nothing_more(&self) {
        let to = SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.address().port());
        UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .send_to(b"marker", to)
            .unwrap();
        let mut bytes = [0; 100];
        let size = self.socket.recv(&mut bytes).unwrap();
        assert_eq!(
            &bytes[..size],
            b"marker",
            "a datagram came before the marker"
        );
    }
}
