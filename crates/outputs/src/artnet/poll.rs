//! ArtPoll and ArtPollReply, as `docs/design/outputs-artnet.md` reads the spec: the poll
//! we send, the reply we give and what a packet from the network says. Calculations only.

use super::{ID, PortAddress};
use std::net::Ipv4Addr;

/// The ESTA manufacturer code: one of the prototype codes, until the owner's own.
const ESTA: u16 = 0x7FF0;
/// The OEM code: `OemUnknown`, until the owner registers one.
const OEM: u16 = 0x00FF;
const OP_POLL: u16 = 0x2000;
const OP_POLL_REPLY: u16 = 0x2100;
const VERSION: u16 = 14;
/// The shortest ArtPoll a receiver takes: older ones end after the priority.
const POLL_SHORTEST: usize = 14;
/// The shortest ArtPollReply a receiver takes.
const REPLY_SHORTEST: usize = 207;
/// ArtPoll flags, bit 5: reply only if a port is in the range.
const TARGETED: u8 = 0x20;
/// PortTypes, bit 7: this port outputs DMX from the network.
const OUTPUTS: u8 = 0x80;
/// Style: a lighting console.
const CONTROLLER: u8 = 1;
/// Status2, bit 3: 15-bit Port-Addresses.
const FIFTEEN_BITS: u8 = 0x08;
const NAME: &[u8] = b"Bossylights";

/// Our ArtPoll: 22 bytes, no targeted mode, replies only when polled, no diagnostics.
pub(crate) const POLL: [u8; 22] = poll();

/// Our ArtPollReply: 239 bytes.
pub(crate) const REPLY: usize = 239;

// Byte offsets of the ArtPollReply.
const AT_IP: usize = 10;
const AT_PORT: usize = 14;
const AT_NET: usize = 18;
const AT_SUB: usize = 19;
const AT_OEM: usize = 20;
const AT_ESTA: usize = 24;
const AT_SHORT_NAME: usize = 26;
const AT_LONG_NAME: usize = 44;
const AT_NODE_REPORT: usize = 108;
const AT_PORT_TYPES: usize = 174;
const AT_SW_OUT: usize = 190;
const AT_STYLE: usize = 200;
const AT_BIND_INDEX: usize = 211;
const AT_STATUS_2: usize = 212;

const fn poll() -> [u8; 22] {
    let [i0, i1, i2, i3, i4, i5, i6, i7] = ID;
    let [o0, o1] = OP_POLL.to_le_bytes();
    let [v0, v1] = VERSION.to_be_bytes();
    let [e0, e1] = ESTA.to_be_bytes();
    let [m0, m1] = OEM.to_be_bytes();
    [
        i0, i1, i2, i3, i4, i5, i6, i7, o0, o1, v0, v1, 0, 0, 0, 0, 0, 0, e0, e1, m0, m1,
    ]
}

/// Our ArtPollReply, from `ip`: a controller with no ports.
pub(crate) fn reply(ip: Ipv4Addr) -> [u8; REPLY] {
    let mut packet = [0; REPLY];
    let fields: [(usize, &[u8]); 11] = [
        (0, &ID),
        (8, &OP_POLL_REPLY.to_le_bytes()),
        (AT_IP, &ip.octets()),
        (AT_PORT, &super::PORT.to_le_bytes()),
        (AT_OEM, &OEM.to_be_bytes()),
        (AT_ESTA, &ESTA.to_le_bytes()),
        (AT_SHORT_NAME, NAME),
        (AT_LONG_NAME, NAME),
        (AT_STYLE, &[CONTROLLER]),
        (AT_BIND_INDEX, &[1]),
        (AT_STATUS_2, &[FIFTEEN_BITS]),
    ];
    for (at, bytes) in fields {
        if let Some(field) = packet.get_mut(at..at + bytes.len()) {
            field.copy_from_slice(bytes);
        }
    }
    packet
}

/// What a node says about itself in its ArtPollReply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reply {
    pub ip: Ipv4Addr,
    /// Which part of a node with more than 4 ports, 1 for the root.
    pub bind_index: u8,
    pub short_name: String,
    pub long_name: String,
    /// The Port-Addresses its ports output, in port order.
    pub outputs: Vec<PortAddress>,
}

/// What a packet from the network says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Heard {
    /// Someone polls: everyone answers.
    Poll,
    /// A node or a controller said who it is.
    Reply(Reply),
    /// Nothing for us: another packet, a targeted poll, an old version or broken bytes.
    Nothing,
}

/// Reads one packet.
pub(crate) fn read(packet: &[u8]) -> Heard {
    if packet.get(..8) != Some(&ID) {
        return Heard::Nothing;
    }
    match u16_le(packet, 8) {
        Some(OP_POLL) => read_poll(packet),
        Some(OP_POLL_REPLY) => read_reply(packet).map_or(Heard::Nothing, Heard::Reply),
        _ => Heard::Nothing,
    }
}

fn read_poll(packet: &[u8]) -> Heard {
    let version = packet
        .get(10..12)
        .and_then(|b| b.first_chunk())
        .map(|b| u16::from_be_bytes(*b));
    let flags = packet.get(12).copied().unwrap_or(0);
    match version {
        Some(version)
            if packet.len() >= POLL_SHORTEST && version >= VERSION && flags & TARGETED == 0 =>
        {
            Heard::Poll
        }
        _ => Heard::Nothing,
    }
}

fn read_reply(packet: &[u8]) -> Option<Reply> {
    if packet.len() < REPLY_SHORTEST {
        return None;
    }
    let ip: [u8; 4] = *packet.get(AT_IP..)?.first_chunk()?;
    let net = u16::from(packet.get(AT_NET)? & 0x7F) << 8;
    let sub = u16::from(packet.get(AT_SUB)? & 0x0F) << 4;
    let types = packet.get(AT_PORT_TYPES..AT_PORT_TYPES + 4)?;
    let sw_out = packet.get(AT_SW_OUT..AT_SW_OUT + 4)?;
    let outputs = types
        .iter()
        .zip(sw_out)
        .filter(|(kind, _)| *kind & OUTPUTS != 0)
        .filter_map(|(_, universe)| PortAddress::new(net + sub + u16::from(universe & 0x0F)))
        .collect();
    Some(Reply {
        ip: Ipv4Addr::from(ip),
        bind_index: packet.get(AT_BIND_INDEX).copied().unwrap_or(0).max(1),
        short_name: name(packet.get(AT_SHORT_NAME..AT_LONG_NAME)?),
        long_name: name(packet.get(AT_LONG_NAME..AT_NODE_REPORT)?),
        outputs,
    })
}

/// A fixed field holding a name up to its first 0.
fn name(field: &[u8]) -> String {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(field.get(..end).unwrap_or_default()).into_owned()
}

fn u16_le(packet: &[u8], at: usize) -> Option<u16> {
    let bytes = packet.get(at..at + 2)?.first_chunk()?;
    Some(u16::from_le_bytes(*bytes))
}

/// The directed broadcast of an interface: its address with every host bit set.
pub(crate) fn directed(address: Ipv4Addr, mask: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(address.to_bits() | !mask.to_bits())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
#[expect(
    clippy::indexing_slicing,
    reason = "a test reads fields at the byte the spec names"
)]
mod tests {
    use super::*;

    const ART_NET: [u8; 8] = [0x41, 0x72, 0x74, 0x2D, 0x4E, 0x65, 0x74, 0x00];

    #[test]
    fn our_poll_is_what_the_spec_table_says() {
        // Worked by hand from outputs-artnet.md: id, OpPoll low first, version 14 high
        // first, flags, priority, target top and bottom, ESTA high first, OEM high first.
        let mut expected = ART_NET.to_vec();
        expected.extend([
            0x00, 0x20, 0x00, 0x0E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F, 0xF0, 0x00, 0xFF,
        ]);
        assert_eq!(POLL.to_vec(), expected);
    }

    #[test]
    fn our_reply_is_what_the_spec_table_says() {
        let found = reply(Ipv4Addr::new(192, 168, 1, 20));
        let mut name = b"Bossylights".to_vec();
        // (first byte, the bytes there), worked by hand from outputs-artnet.md
        name.resize(18, 0);
        let mut long_name = b"Bossylights".to_vec();
        long_name.resize(64, 0);
        let cases: [(usize, Vec<u8>); 15] = [
            (0, ART_NET.to_vec()),
            (8, vec![0x00, 0x21]),
            (10, vec![192, 168, 1, 20]),
            (14, vec![0x36, 0x19]),
            (16, vec![0; 4]),
            (20, vec![0x00, 0xFF]),
            (22, vec![0, 0]),
            (24, vec![0xF0, 0x7F]),
            (26, name),
            (44, long_name),
            (108, vec![0; 92]),
            (200, vec![0x01]),
            (201, vec![0; 10]),
            (211, vec![0x01, 0x08]),
            (213, vec![0; 26]),
        ];
        assert_eq!(found.len(), 239);
        let mut covered = 0;
        for (at, bytes) in cases {
            assert_eq!(found[at..at + bytes.len()], bytes, "byte {at}");
            covered += bytes.len();
        }
        assert_eq!(covered, 239, "every byte is in the table");
    }

    /// A reply with `fields` set over 239 zero bytes.
    fn a_reply(len: usize, fields: &[(usize, &[u8])]) -> Vec<u8> {
        let mut packet = vec![0; 239];
        packet[..8].copy_from_slice(&ART_NET);
        packet[8..10].copy_from_slice(&[0x00, 0x21]);
        for (at, bytes) in fields {
            packet[*at..at + bytes.len()].copy_from_slice(bytes);
        }
        packet.truncate(len);
        packet
    }

    fn a_poll(bytes: &[u8]) -> Vec<u8> {
        let mut packet = ART_NET.to_vec();
        packet.extend(bytes);
        packet
    }

    #[test]
    fn a_poll_is_answered_unless_targeted_or_old() {
        // (bytes after the id, heard)
        let cases = [
            (vec![0x00, 0x20, 0, 14, 0, 0], Heard::Poll),
            (vec![0x00, 0x20, 0, 14, 0x02, 0x10], Heard::Poll),
            (POLL[8..].to_vec(), Heard::Poll),
            (vec![0x00, 0x20, 0, 15, 0, 0], Heard::Poll),
            (vec![0x00, 0x20, 1, 0, 0, 0], Heard::Poll),
            (vec![0x00, 0x20, 0, 14, 0x20, 0], Heard::Nothing),
            (vec![0x00, 0x20, 0, 14, 0x3F, 0], Heard::Nothing),
            (vec![0x00, 0x20, 0, 13, 0, 0], Heard::Nothing),
            (vec![0x00, 0x20, 0, 14, 0], Heard::Nothing),
            (vec![0x00, 0x20, 0, 14], Heard::Nothing),
            (vec![0x20, 0x00, 0, 14, 0, 0], Heard::Nothing),
            (vec![0x00, 0x50, 0, 14, 0, 0], Heard::Nothing),
        ];
        for (bytes, heard) in cases {
            assert_eq!(read(&a_poll(&bytes)), heard, "{bytes:?}");
        }
        let mut other_id = a_poll(&[0x00, 0x20, 0, 14, 0, 0]);
        other_id[7] = b'X';
        assert_eq!(read(&other_id), Heard::Nothing, "another id");
        assert_eq!(read(&[]), Heard::Nothing, "empty");
    }

    #[test]
    fn a_reply_says_which_port_addresses_the_node_outputs() {
        // (length, fields, Port-Addresses), worked by hand: Net bits 14-8, Sub-Net 7-4,
        // SwOut 3-0, only ports whose PortTypes bit 7 is set
        let pa = |n| PortAddress::new(n).unwrap();
        let cases = [
            (239, vec![(174, &[0x80][..]), (190, &[0x05])], vec![pa(5)]),
            (
                239,
                vec![
                    (18, &[0x12][..]),
                    (19, &[0x03]),
                    (174, &[0xC0]),
                    (190, &[0x0A]),
                ],
                vec![pa(0x123A)],
            ),
            (
                239,
                vec![(174, &[0x80, 0x40, 0x85, 0x80][..]), (190, &[1, 2, 3, 4])],
                vec![pa(1), pa(3), pa(4)],
            ),
            (
                239,
                vec![
                    (18, &[0xFF][..]),
                    (19, &[0xFF]),
                    (174, &[0x80]),
                    (190, &[0xFF]),
                ],
                vec![pa(0x7FFF)],
            ),
            (239, vec![(174, &[0x80][..]), (190, &[0])], vec![]),
            (239, vec![(174, &[0x40][..]), (186, &[7])], vec![]),
            (239, vec![(172, &[0, 4][..])], vec![]),
            (207, vec![(174, &[0x80][..]), (190, &[0x05])], vec![pa(5)]),
        ];
        for (len, fields, outputs) in cases {
            let found = match read(&a_reply(len, &fields)) {
                Heard::Reply(reply) => Some(reply.outputs),
                Heard::Poll | Heard::Nothing => None,
            };
            assert_eq!(found, Some(outputs), "{fields:?}");
        }
    }

    #[test]
    fn a_reply_names_the_node() {
        // (length, fields, IP, BindIndex, short name, long name)
        let cases = [
            (
                239,
                vec![
                    (10, &[10, 0, 0, 7][..]),
                    (26, b"Node A\0junk"),
                    (44, b"Node A long"),
                    (211, &[3]),
                ],
                Ipv4Addr::new(10, 0, 0, 7),
                3,
                "Node A",
                "Node A long",
            ),
            (
                239,
                vec![(26, &[b'x'; 18][..]), (211, &[0])],
                Ipv4Addr::UNSPECIFIED,
                1,
                "xxxxxxxxxxxxxxxxxx",
                "",
            ),
            (239, vec![(211, &[1][..])], Ipv4Addr::UNSPECIFIED, 1, "", ""),
            (207, vec![], Ipv4Addr::UNSPECIFIED, 1, "", ""),
            (211, vec![], Ipv4Addr::UNSPECIFIED, 1, "", ""),
            (212, vec![(211, &[2][..])], Ipv4Addr::UNSPECIFIED, 2, "", ""),
        ];
        for (len, fields, ip, bind_index, short_name, long_name) in cases {
            let heard = read(&a_reply(len, &fields));
            let expected = Heard::Reply(Reply {
                ip,
                bind_index,
                short_name: short_name.into(),
                long_name: long_name.into(),
                outputs: vec![],
            });
            assert_eq!(heard, expected, "{len} {fields:?}");
        }
    }

    #[test]
    fn a_short_or_foreign_reply_says_nothing() {
        // (packet, why)
        let mut other_op = a_reply(239, &[]);
        other_op[8] = 0x01;
        let cases = [
            (a_reply(206, &[]), "206 bytes"),
            (other_op, "another opcode"),
            (a_reply(239, &[(0, b"Art-Nex")]), "another id"),
        ];
        for (packet, why) in cases {
            assert_eq!(read(&packet), Heard::Nothing, "{why}");
        }
    }

    #[test]
    fn our_reply_reads_back_as_a_controller_with_no_ports() {
        let ip = Ipv4Addr::new(2, 0, 0, 1);
        let expected = Heard::Reply(Reply {
            ip,
            bind_index: 1,
            short_name: "Bossylights".into(),
            long_name: "Bossylights".into(),
            outputs: vec![],
        });
        assert_eq!(read(&reply(ip)), expected);
    }

    #[test]
    fn the_directed_broadcast_sets_every_host_bit() {
        // (address, mask, directed broadcast), by hand
        let cases = [
            ([192, 168, 1, 20], [255, 255, 255, 0], [192, 168, 1, 255]),
            ([2, 1, 2, 3], [255, 0, 0, 0], [2, 255, 255, 255]),
            ([10, 0, 5, 9], [255, 255, 252, 0], [10, 0, 7, 255]),
            ([172, 16, 0, 1], [255, 255, 255, 255], [172, 16, 0, 1]),
        ];
        for (address, mask, expected) in cases {
            let found = directed(Ipv4Addr::from(address), Ipv4Addr::from(mask));
            assert_eq!(found, Ipv4Addr::from(expected), "{address:?}");
        }
    }
}
