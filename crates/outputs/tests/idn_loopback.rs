//! An IDN output sends to a fake projector on this machine (`common/idn.rs`); every datagram
//! is decoded and checked against the spec.

#![expect(
    clippy::disallowed_methods,
    reason = "the test waits for the host to answer a send to nobody; it is not the output thread"
)]

#[path = "common/idn.rs"]
mod projector;

use outputs::idn::{self, Arm, PointRate};
use outputs::{Device, Outcome};
use projector::{Datagram, Point, Projector};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

fn open(projector: &Projector, rate: u32) -> (Box<dyn Device>, Arm) {
    idn::open(projector.address(), PointRate::new(rate).unwrap()).unwrap()
}

/// The route of `points`, each given as channel words X, Y and red, green, blue.
fn route(points: &[(u16, u16, u8, u8, u8)]) -> Vec<u8> {
    points
        .iter()
        .flat_map(|&(x, y, red, green, blue)| {
            let [x_high, x_low] = x.to_be_bytes();
            let [y_high, y_low] = y.to_be_bytes();
            [x_high, x_low, y_high, y_low, red, green, blue]
        })
        .collect()
}

fn points(datagram: Datagram) -> Vec<Point> {
    match datagram {
        Datagram::Frame { points, .. } => points,
        close @ (Datagram::CloseChannel { .. } | Datagram::CloseSession) => {
            panic!("a frame, not {close:?}")
        }
    }
}

const THREE: [(u16, u16, u8, u8, u8); 3] = [
    (0x0000, 0xFFFF, 1, 2, 3),
    (0x8000, 0x8000, 255, 128, 1),
    (0xFFFF, 0x7FFF, 10, 20, 30),
];

#[test]
fn a_new_output_sends_empty_frames_until_armed() {
    let mut projector = Projector::new(30_000);
    let (mut device, arm) = open(&projector, 30_000);
    assert!(!arm.armed());
    for frame in 0..3 {
        assert_eq!(device.take(&route(&THREE)), Outcome::Accepted, "{frame}");
        let Datagram::Frame {
            duration, points, ..
        } = projector.next()
        else {
            panic!("a frame");
        };
        assert_eq!((duration, points), (0, vec![]), "{frame}");
    }
    arm.arm();
    device.take(&route(&THREE));
    assert_eq!(points(projector.next()).len(), 3, "armed");
}

#[test]
fn an_armed_output_draws_every_frame_as_the_spec_says() {
    // (points a second, duration): 2 segments / rate, rounded down
    let drawn = vec![
        Point {
            x: -32_768,
            y: 32_767,
            red: 0,
            green: 0,
            blue: 0,
        },
        Point {
            x: 0,
            y: 0,
            red: 255,
            green: 128,
            blue: 1,
        },
        Point {
            x: 32_767,
            y: -1,
            red: 10,
            green: 20,
            blue: 30,
        },
    ];
    let cases = [(30_000, 66), (12_000, 166), (100_000, 20)];
    for (rate, duration) in cases {
        let mut projector = Projector::new(rate);
        let (mut device, arm) = open(&projector, rate);
        arm.arm();
        for frame in 0..300 {
            assert_eq!(
                device.take(&route(&THREE)),
                Outcome::Accepted,
                "{rate} {frame}"
            );
            let Datagram::Frame {
                duration: found,
                points,
                ..
            } = projector.next()
            else {
                panic!("a frame");
            };
            assert_eq!((found, &points), (duration, &drawn), "{rate} {frame}");
        }
    }
}

#[test]
fn fewer_than_two_points_or_disarmed_draw_nothing() {
    let mut projector = Projector::new(30_000);
    let (mut device, arm) = open(&projector, 30_000);
    arm.arm();
    let one = route(&THREE[..1]);
    let three = route(&THREE);
    device.take(&one);
    assert_eq!(points(projector.next()).len(), 0, "armed, 1 point");
    device.take(&[]);
    assert_eq!(points(projector.next()).len(), 0, "armed, no points");
    device.take(&three);
    assert_eq!(points(projector.next()).len(), 3, "armed, 3 points");
    arm.disarm();
    assert!(!arm.armed());
    device.take(&three);
    assert_eq!(points(projector.next()).len(), 0, "disarmed");
}

#[test]
fn the_largest_frame_arrives_whole() {
    let mut projector = Projector::new(30_000);
    let (mut device, arm) = open(&projector, 30_000);
    arm.arm();
    let most: Vec<_> = (0..9_321_u32)
        .map(|n| (n as u16, (n * 7) as u16, 1, (n % 251) as u8, 3))
        .collect();
    assert_eq!(device.take(&route(&most)), Outcome::Accepted);
    let drawn = points(projector.next());
    assert_eq!(drawn.len(), 9_321);
    let last = drawn[9_320];
    assert_eq!(
        (last.x, last.y, last.green),
        (
            (9_320_u16 ^ 0x8000) as i16,
            ((9_320_u32 * 7) as u16 ^ 0x8000) as i16,
            (9_320 % 251) as u8
        )
    );
}

#[test]
fn a_send_to_nobody_disarms() {
    let nobody = UdpSocket::bind("127.0.0.1:0").unwrap();
    let projector_address = match nobody.local_addr().unwrap() {
        std::net::SocketAddr::V4(address) => address,
        std::net::SocketAddr::V6(_) => panic!("IPv4"),
    };
    drop(nobody);
    let (mut device, arm) = idn::open(projector_address, PointRate::DEFAULT).unwrap();
    arm.arm();
    let three = route(&THREE);
    let started = Instant::now();
    while device.take(&three) != Outcome::Failed {
        assert!(started.elapsed() < Duration::from_secs(10), "never refused"); // guard
        thread::sleep(Duration::from_millis(5)); // poll: the host answers a little later
    }
    assert!(!arm.armed(), "a refused send disarms");
}

#[test]
fn terminate_closes_the_channel_then_the_session_and_nothing_follows() {
    let mut projector = Projector::new(30_000);
    let (mut device, arm) = open(&projector, 30_000);
    arm.arm();
    device.take(&route(&THREE));
    assert_eq!(points(projector.next()).len(), 3);
    device.terminate();
    assert!(matches!(projector.next(), Datagram::CloseChannel { .. }));
    assert_eq!(projector.next(), Datagram::CloseSession);
    device.take(&route(&THREE));
    device.terminate();
    projector.nothing_more();
}

#[test]
fn open_refuses_what_is_not_one_projector() {
    // (address, refused)
    let cases = [
        (SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0), true),
        (SocketAddrV4::new(Ipv4Addr::BROADCAST, idn::PORT), true),
        (
            SocketAddrV4::new(Ipv4Addr::new(239, 255, 0, 1), idn::PORT),
            true,
        ),
        (SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, idn::PORT), true),
        (SocketAddrV4::new(Ipv4Addr::LOCALHOST, idn::PORT), false),
    ];
    for (address, refused) in cases {
        assert_eq!(
            idn::open(address, PointRate::DEFAULT).is_err(),
            refused,
            "{address}"
        );
    }
}
