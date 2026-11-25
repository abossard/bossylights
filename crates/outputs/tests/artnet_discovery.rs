//! Art-Net discovery on this machine: a fake node answers our polls with a reply made by
//! `artnet_protocol`, is listed, takes only the universes it lists, and is gone after
//! 3 s of silence. A fake controller polls us and gets our reply. Every packet is decoded
//! by the spec receiver (`common/artnet.rs`).

#![expect(
    clippy::disallowed_methods,
    reason = "the test sends frames at the output's rate; it is not the output thread"
)]

#[path = "common/artnet.rs"]
mod receiver;

use outputs::artnet::{Discovery, Node, PortAddress, Presence};
use outputs::{Device, Outcome, Rate};
use socket2::{Domain, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

/// A guard: what does not happen in this long never happens.
const NEVER: Duration = Duration::from_secs(10);

/// A peer on 127.0.0.1: a fake node or controller, and the port it listens on.
fn peer() -> (UdpSocket, u16) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let port = socket.local_addr().unwrap().port();
    (socket, port)
}

/// Discovery on a random port of 127.0.0.1 that polls the peer at `port`.
fn discovery(port: u16) -> Discovery {
    Discovery::start(
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        port,
        vec![Ipv4Addr::LOCALHOST],
    )
    .unwrap()
}

/// The next packet at a peer and who sent it, if one comes within 50 ms.
fn heard(peer: &UdpSocket) -> Option<(Vec<u8>, SocketAddrV4)> {
    let mut packet = [0; 1_000];
    match peer.recv_from(&mut packet) {
        Ok((size, SocketAddr::V4(from))) => Some((packet[..size].to_vec(), from)),
        _ => None,
    }
}

fn opcode(packet: &[u8]) -> u16 {
    u16::from_le_bytes([packet[8], packet[9]])
}

/// Waits for our next ArtPoll at the peer: decoded, with where it came from.
fn next_poll(peer: &UdpSocket) -> (receiver::ArtPoll, SocketAddrV4) {
    let started = Instant::now();
    loop {
        assert!(started.elapsed() < NEVER, "no ArtPoll came"); // guard
        if let Some((packet, from)) = heard(peer)
            && opcode(&packet) == 0x2000
        {
            return (receiver::decode_poll(&packet), from);
        }
    }
}

/// Waits until the list holds the one node in `presence`.
fn until_listed(discovery: &Discovery, presence: Presence) -> Node {
    let started = Instant::now();
    loop {
        assert!(started.elapsed() < NEVER, "never {presence:?}"); // guard
        let nodes = discovery.nodes();
        if let [node] = nodes.as_slice()
            && node.presence == presence
        {
            return node.clone();
        }
        thread::sleep(Duration::from_millis(20)); // poll
    }
}

/// Sends `frames` frames of `channels` at the rate; the Port-Addresses of every ArtDmx
/// that reached the node, decoded.
fn sent_to(node: &UdpSocket, device: &mut dyn Device, channels: usize, frames: usize) -> Vec<u16> {
    let stream = vec![7; channels];
    let mut port_addresses = vec![];
    for _ in 0..frames {
        assert_eq!(device.take(&stream), Outcome::Accepted);
        thread::sleep(Rate::FortyFour.period()); // poll: frames go out at the output's rate
    }
    while let Some((packet, _)) = heard(node) {
        if opcode(&packet) == 0x5000 {
            port_addresses.push(receiver::decode(&packet).port_address);
        }
    }
    port_addresses
}

#[test]
fn a_node_is_found_listed_takes_its_universe_and_is_gone_after_3_s() {
    let (node, port) = peer();
    let discovery = discovery(port);
    let (poll, from) = next_poll(&node);
    assert_eq!(
        (poll.flags, poll.esta, poll.oem),
        (0, 0x7FF0, 0x00FF),
        "our poll"
    );
    node.send_to(&receiver::node_reply(Ipv4Addr::LOCALHOST, &[5]), from)
        .unwrap();
    let replied = Instant::now();

    let found = until_listed(&discovery, Presence::Seen);
    assert_eq!(found.ip, Ipv4Addr::LOCALHOST);
    assert_eq!(found.short_name, "Fake node");
    assert_eq!(found.outputs, [PortAddress::new(5).unwrap()]);

    // Port-Addresses 4, 5 and 6: only 5 reaches the node, once a frame.
    let mut device = discovery.open(PortAddress::new(4).unwrap()).unwrap();
    let reached = sent_to(&node, device.as_mut(), 1_536, 10);
    assert_eq!(reached, [5; 10], "only the universe the node lists");

    let gone = until_listed(&discovery, Presence::Gone);
    assert!(
        replied.elapsed() > Duration::from_secs(3),
        "gone only after 3 s"
    );
    assert_eq!(
        gone.outputs, found.outputs,
        "a gone node keeps what it said"
    );
    let reached = sent_to(&node, device.as_mut(), 1_536, 10);
    assert_eq!(reached, [], "a gone node takes nothing");

    // It answers again: it takes its universe again.
    let (_, from) = next_poll(&node);
    node.send_to(&receiver::node_reply(Ipv4Addr::LOCALHOST, &[5, 6]), from)
        .unwrap();
    until_listed(&discovery, Presence::Seen);
    let reached = sent_to(&node, device.as_mut(), 1_536, 5);
    assert_eq!(reached, [5, 6, 5, 6, 5, 6, 5, 6, 5, 6], "its new ports");
}

#[test]
fn an_output_with_no_node_sends_nothing_and_is_not_failed() {
    let (node, port) = peer();
    let discovery = discovery(port);
    let (_, from) = next_poll(&node);
    node.send_to(&receiver::node_reply(Ipv4Addr::LOCALHOST, &[9]), from)
        .unwrap();
    until_listed(&discovery, Presence::Seen);
    let mut device = discovery.open(PortAddress::new(1).unwrap()).unwrap();
    assert_eq!(sent_to(&node, device.as_mut(), 1_024, 10), []);
}

#[test]
fn a_poll_gets_our_reply_unicast() {
    let (controller, port) = peer();
    let discovery = discovery(port);
    let (_, ours) = next_poll(&controller);
    // It polls from another port: the reply goes to where peers listen, not back to it.
    let (asks, _) = peer();
    asks.send_to(&receiver::old_poll(), ours).unwrap();
    let started = Instant::now();
    let reply = loop {
        assert!(started.elapsed() < NEVER, "no ArtPollReply came"); // guard
        if let Some((packet, _)) = heard(&controller)
            && opcode(&packet) == 0x2100
        {
            break receiver::decode_reply(&packet);
        }
    };
    let expected = receiver::ArtPollReply {
        ip: Ipv4Addr::LOCALHOST,
        oem: 0x00FF,
        esta: 0x7FF0,
        short_name: "Bossylights".into(),
        long_name: "Bossylights".into(),
        ports: 0,
        style: 1,
        bind_index: 1,
        status_2: 0x08,
    };
    assert_eq!(reply, expected);
    drop(discovery);
}

/// A socket on `port` of 127.0.0.1 as another program holds it: shared or not.
fn held(shared: bool) -> (Socket, u16) {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, None).unwrap();
    socket.set_reuse_address(shared).unwrap();
    socket.set_reuse_port(shared).unwrap();
    socket
        .bind(&SocketAddr::from((Ipv4Addr::LOCALHOST, 0)).into())
        .unwrap();
    let port = socket.local_addr().unwrap().as_socket().unwrap().port();
    (socket, port)
}

#[test]
fn the_port_is_shared_with_a_program_that_shares_it_else_refused() {
    // (the other program shares the port, discovery starts)
    let cases = [(true, true), (false, false)];
    for (shared, starts) in cases {
        let (_other, port) = held(shared);
        let on = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        let started = Discovery::start(on, port, vec![]);
        assert_eq!(started.is_ok(), starts, "shared: {shared}");
    }
}

#[test]
fn a_dropped_discovery_ends_its_thread_and_frees_the_port() {
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let on = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let discovery = Discovery::start(on, port, vec![]).unwrap();
    let dropping = thread::spawn(move || drop(discovery));
    let started = Instant::now();
    while !dropping.is_finished() {
        assert!(started.elapsed() < NEVER, "drop never returned"); // guard
        thread::sleep(Duration::from_millis(5)); // poll
    }
    dropping.join().unwrap();
    assert!(
        UdpSocket::bind(on).is_ok(),
        "the port is free for a program that does not share it"
    );
}
