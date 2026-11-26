//! Review tests for aspect 6 (`crates/outputs`): protocol bytes and framing.

#![expect(
    clippy::disallowed_methods,
    reason = "the test waits between frames; it is not the output thread"
)]

mod common;
#[path = "common/artnet.rs"]
mod receiver;

use common::{ASKED, Fake, NEVER};
use outputs::artnet::{Discovery, PortAddress, Presence};
use outputs::dmx_usb_pro::{DmxUsbPro, Settings};
use outputs::{Device, Outcome, Rate};
use serialport::TTYPort;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

fn peer() -> (UdpSocket, u16) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let port = socket.local_addr().unwrap().port();
    (socket, port)
}

fn heard(peer: &UdpSocket) -> Option<(Vec<u8>, SocketAddrV4)> {
    let mut packet = [0; 1_000];
    match peer.recv_from(&mut packet) {
        Ok((size, SocketAddr::V4(from))) => Some((packet[..size].to_vec(), from)),
        Ok((_, SocketAddr::V6(_))) | Err(_) => None,
    }
}

fn opcode(packet: &[u8]) -> u16 {
    u16::from_le_bytes([packet[8], packet[9]])
}

fn next_poll(peer: &UdpSocket) -> SocketAddrV4 {
    let started = Instant::now();
    loop {
        assert!(started.elapsed() < NEVER, "no ArtPoll came"); // guard
        if let Some((packet, from)) = heard(peer)
            && opcode(&packet) == 0x2000
        {
            let _ = receiver::decode_poll(&packet);
            return from;
        }
    }
}

/// Claim: Art-Net discovery accepts non-unicast IPv4 addresses (`255.255.255.255`, `0.0.0.0`) from `ArtPollReply`, causing `Udp::send` to fail on broadcast/unspecified before reaching healthy unicast nodes on later universes.
/// Source: `docs/design/outputs-artnet.md`, "An Art-Net output": "ArtDmx is unicast only: every universe goes to the IP addresses of the nodes that subscribed to it in ArtPollReply, never broadcast."
/// Severity: high.
/// Show impact: One misconfigured or spoofed `ArtPollReply` advertising `255.255.255.255` or `0.0.0.0` on an earlier Port-Address aborts the send loop every frame, flipping the Art-Net output to `Failed` and blacking out real unicast nodes on higher universes.
#[test]
fn finding_outputs_01_artnet_non_unicast_poll_reply() {
    for bad_ip in [Ipv4Addr::BROADCAST, Ipv4Addr::UNSPECIFIED] {
        let (node, port) = peer();
        let discovery = Discovery::start(
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            port,
            vec![Ipv4Addr::LOCALHOST],
        )
        .unwrap();
        let from = next_poll(&node);

        node.send_to(&receiver::node_reply(bad_ip, &[4]), from)
            .unwrap();
        node.send_to(&receiver::node_reply(Ipv4Addr::LOCALHOST, &[5]), from)
            .unwrap();

        let started = Instant::now();
        while !discovery
            .nodes()
            .iter()
            .any(|n| n.ip == Ipv4Addr::LOCALHOST && n.presence == Presence::Seen)
        {
            assert!(started.elapsed() < NEVER, "unicast node never seen"); // guard
            thread::sleep(Duration::from_millis(20)); // poll
        }

        let mut device = discovery.open(PortAddress::new(4).unwrap()).unwrap();
        let stream = [42_u8; 1_024];
        let mut outcomes = Vec::new();
        for _ in 0..6 {
            outcomes.push(device.take(&stream));
            thread::sleep(Rate::FortyFour.period()); // poll
        }
        let mut reached = Vec::new();
        while let Some((packet, _)) = heard(&node) {
            if opcode(&packet) == 0x5000 {
                reached.push(receiver::decode(&packet).port_address);
            }
        }
        assert!(
            !outcomes.contains(&Outcome::Failed),
            "bad PollReply IP {bad_ip} made the Art-Net output fail: {outcomes:?}"
        );
        assert!(
            reached.contains(&5),
            "healthy unicast node on Port-Address 5 received no ArtDmx because {bad_ip} on Port-Address 4 aborted the frame"
        );
        assert!(
            discovery.nodes().iter().all(|n| n.ip != bad_ip),
            "non-unicast {bad_ip} must not enter the unicast ArtDmx destination table"
        );
    }
}

fn read_ask(end: &mut TTYPort) {
    let mut asked = [0; ASKED.len()];
    end.read_exact(&mut asked).expect("a label 3 request");
    assert_eq!(asked, ASKED);
}

fn label5_received_dmx(data_len: usize) -> Vec<u8> {
    let [lsb, msb] = u16::try_from(data_len).unwrap().to_le_bytes();
    let mut packet = vec![0x7E, 5, lsb, msb];
    packet.resize(4 + data_len, 0);
    packet.push(0xE7);
    packet
}

/// Claim: `DmxUsbPro` scans raw serial bytes for `0x7E` instead of framing messages by length, slicing a valid label-3 reply in half whenever firmware minor or `break_steps` is `126` (`0x7E`) at the 64-byte buffer boundary and permanently rejecting the widget as `"not a DMX USB Pro"`.
/// Source: `docs/design/outputs-dmx-usb-pro.md`, "Every message on the serial port": "No checksum, no escaping: the length frames the data", and `crates/outputs/src/dmx_usb_pro.rs` (`kept_from`).
/// Severity: high.
/// Show impact: A genuine Enttec DMX USB Pro in its power-on DMX-receive mode whose firmware minor (`1.126`) or `break_steps` (`126`, valid in `9..=127`) equals `0x7E` is permanently refused (`Link::NotAPro`, `Outcome::Failed`) and never sends DMX for the rest of the session.
#[test]
fn finding_outputs_02_dmx_usb_pro_0x7e_in_settings_payload() {
    let cases = [
        (
            "firmware 1.126 with factory default settings (9, 1, 40)",
            54_usize,
            [0x7E, 3, 5, 0, 126, 1, 9, 1, 40, 0xE7],
            Settings::new(9, 1, 40).unwrap(),
        ),
        (
            "firmware 1.44 with valid break_steps 126 (126, 1, 40)",
            52_usize,
            [0x7E, 3, 5, 0, 44, 1, 126, 1, 40, 0xE7],
            Settings::new(126, 1, 40).unwrap(),
        ),
    ];

    for (case, dmx_in_len, reply, expected) in cases {
        let (find, mut fake) = Fake::silent(true);
        let mut device = DmxUsbPro::open(find).unwrap();
        let lost_with = device.lost_with();
        let reading = device.reading();
        let mut end = fake.next_end();

        let mut wire = label5_received_dmx(dmx_in_len);
        wire.extend_from_slice(&reply);

        read_ask(&mut end);
        end.write_all(&wire).unwrap();

        let started = Instant::now();
        while reading.settings().is_none() && lost_with.get().is_none() {
            let mut extra = [0; ASKED.len()];
            if end.read_exact(&mut extra).is_ok() && extra == ASKED {
                let _ = end.write_all(&wire);
            }
            assert!(started.elapsed() < NEVER, "{case}: ask never settled"); // guard
            thread::sleep(Duration::from_millis(5)); // poll
        }

        assert_eq!(
            lost_with.get().map(|err| err.to_string()),
            None,
            "{case}: valid DMX USB Pro was permanently refused"
        );
        assert_eq!(reading.settings(), Some(expected), "{case}");
        assert_ne!(
            device.take(&[255; 512]),
            Outcome::Failed,
            "{case}: output failed after valid label-3 answers"
        );
    }
}
