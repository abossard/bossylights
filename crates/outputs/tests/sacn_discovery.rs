//! sACN discovery sends to a socket on this machine, not to its multicast group (a Mac
//! often loops no multicast back); every packet that arrives is decoded by the spec
//! receiver (`common/sacn.rs`) and checked. The outputs send their data to a multicast
//! port nobody listens on: only the list matters here.

#![expect(
    clippy::disallowed_methods,
    reason = "the test waits for the discovery thread; it is not the output thread"
)]

#[path = "common/sacn.rs"]
mod receiver;

use engine_core::OutputNumber;
use outputs::sacn::{Destination, Discovery, Universe};
use outputs::{Device, Outcome};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

const CID: [u8; 16] = [
    0x6B, 0xA7, 0xB8, 0x10, 0x9D, 0xAD, 0x11, 0xD1, 0x80, 0xB4, 0x00, 0xC0, 0x4F, 0xD4, 0x30, 0xC8,
];
/// Longer than the 10 s between two lists.
const NEVER: Duration = Duration::from_secs(15);
/// No packet for this long: the pages of one list are all in.
const QUIET: Duration = Duration::from_secs(2);
/// A port no receiver listens on: the outputs' data goes nowhere.
const NOBODY: u16 = 9;

/// A receiver on this machine and a discovery that sends to it.
fn discovery() -> (UdpSocket, Discovery) {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket.set_read_timeout(Some(NEVER)).unwrap();
    let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        panic!("IPv4");
    };
    (socket, Discovery::start(CID, address).unwrap())
}

fn open(discovery: &Discovery, to: Destination, first: u16, output: u8) -> Box<dyn Device> {
    discovery
        .open(
            to,
            Universe::new(first).unwrap(),
            OutputNumber::try_from(output).unwrap(),
        )
        .unwrap()
}

/// The next discovery packet at the receiver, decoded and checked.
fn next(receiver: &UdpSocket) -> receiver::Listing {
    let mut packet = [0; 2_000];
    let size = receiver.recv(&mut packet).unwrap();
    receiver::decode_discovery(&packet[..size])
}

/// When the first page of one list came, and its pages in the order they came, until the
/// receiver is quiet.
fn one_list(receiver: &UdpSocket) -> (Instant, Vec<receiver::Listing>) {
    let mut pages = vec![next(receiver)];
    let came = Instant::now();
    receiver.set_read_timeout(Some(QUIET)).unwrap();
    let mut packet = [0; 2_000];
    while let Ok(size) = receiver.recv(&mut packet) {
        pages.push(receiver::decode_discovery(&packet[..size]));
    }
    receiver.set_read_timeout(Some(NEVER)).unwrap();
    (came, pages)
}

#[test]
fn the_first_list_names_the_universes_an_output_sends() {
    let (receiver, discovery) = discovery();
    let mut output = open(&discovery, Destination::Multicast(NOBODY), 7, 3);
    let _ = output.take(&[1; 1_027]);
    assert_eq!(
        next(&receiver),
        receiver::Listing {
            cid: CID,
            name: "Bossylights".into(),
            page: 0,
            last_page: 0,
            universes: vec![7, 8, 9],
        }
    );
}

#[test]
fn a_unicast_output_is_never_listed() {
    let (receiver, discovery) = discovery();
    let sink = UdpSocket::bind("127.0.0.1:0").unwrap();
    let SocketAddr::V4(sink_address) = sink.local_addr().unwrap() else {
        panic!("IPv4");
    };
    let mut unicast = open(&discovery, Destination::Unicast(sink_address), 1, 1);
    assert_eq!(unicast.take(&[1; 512]), Outcome::Accepted);
    thread::sleep(Duration::from_millis(500)); // poll: the thread looks every 100 ms
    let mut multicast = open(&discovery, Destination::Multicast(NOBODY), 9, 2);
    let _ = multicast.take(&[1; 512]);
    assert_eq!(next(&receiver).universes, [9], "only the multicast output");
}

#[test]
fn ten_seconds_later_the_list_follows_the_outputs_in_pages() {
    let (receiver, discovery) = discovery();
    let full = vec![1; 65_536];
    // Outputs 1 to 5: 128 universes each, the fifth over the fourth's last 13.
    let mut kept: Vec<_> = [1, 129, 257, 385, 500]
        .into_iter()
        .zip(1..)
        .map(|(first, output)| open(&discovery, Destination::Multicast(NOBODY), first, output))
        .collect();
    let mut dropped = open(&discovery, Destination::Multicast(NOBODY), 1_000, 6);
    let mut ended = open(&discovery, Destination::Multicast(NOBODY), 2_000, 7);
    // Nothing is listed before the first frame: the first list goes after this.
    let started = Instant::now();
    for output in &mut kept {
        let _ = output.take(&full);
    }
    let _ = dropped.take(&[1; 512]);
    let _ = ended.take(&[1; 512]);
    let (_, first) = one_list(&receiver);
    let listed: Vec<u16> = first.iter().flat_map(|p| p.universes.clone()).collect();
    assert!(
        listed
            .iter()
            .all(|u| *u <= 627 || *u == 1_000 || *u == 2_000),
        "only what the outputs send"
    );
    drop(dropped);
    ended.terminate();
    let (came, second) = one_list(&receiver);
    let between = came - started;
    assert!(
        between >= Duration::from_secs(10),
        "the next list came 10 s after the first: {between:?}"
    );
    // (page, last page, universes)
    let found: Vec<_> = second
        .into_iter()
        .map(|p| (p.page, p.last_page, p.universes))
        .collect();
    assert_eq!(
        found,
        [
            (0, 1, (1..=512).collect::<Vec<u16>>()),
            (1, 1, (513..=627).collect()),
        ]
    );
}

#[test]
fn a_dropped_discovery_lists_nothing_more_and_its_outputs_go_on() {
    let (receiver, discovery) = discovery();
    let mut output = open(&discovery, Destination::Multicast(NOBODY), 1, 1);
    let started = Instant::now();
    drop(discovery);
    assert!(started.elapsed() < NEVER, "drop never returned"); // guard
    let _ = output.take(&[1; 512]);
    thread::sleep(Duration::from_millis(500)); // poll: a running thread looks every 100 ms
    output.terminate();
    nothing_more(&receiver);
}

#[test]
fn a_discovery_waiting_for_its_next_list_stops_at_once() {
    let (receiver, discovery) = discovery();
    let mut output = open(&discovery, Destination::Multicast(NOBODY), 1, 1);
    let _ = output.take(&[1; 512]);
    assert_eq!(next(&receiver).universes, [1], "a list went out");
    let started = Instant::now();
    drop(discovery);
    let took = started.elapsed();
    assert!(took < Duration::from_secs(5), "not the 10 s wait: {took:?}");
}

/// Sends a marker from another socket: the next packet the receiver reads proves that
/// nothing came before it.
fn nothing_more(receiver: &UdpSocket) {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .send_to(b"marker", receiver.local_addr().unwrap())
        .unwrap();
    let mut packet = [0; 2_000];
    let size = receiver.recv(&mut packet).unwrap();
    assert_eq!(&packet[..size], b"marker", "a list came after the drop");
}
