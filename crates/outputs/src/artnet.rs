//! Art-Net 4 over UDP, as `docs/design/outputs-artnet.md` reads the spec. A frame is one
//! stream of channel bytes; it goes out as one ArtDmx per universe of 512 slots, to the
//! output's first Port-Address and on, each universe with its own sequence.
//!
//! | Byte | ArtDmx field |
//! |---|---|
//! | 0 to 7 | `Art-Net` and `0x00` |
//! | 8 to 9 | opcode `0x5000`, low byte first |
//! | 10 to 11 | protocol version 14, high byte first |
//! | 12 | sequence, 1 to 255 per universe, then 1 again. 0 means "off" |
//! | 13 | physical: 0 |
//! | 14 | Port-Address bits 7 to 0 (Sub-Net and Universe) |
//! | 15 | Port-Address bits 14 to 8 (Net) |
//! | 16 to 17 | length of the slots, high byte first, even, 2 to 512 |

use crate::udp::{Protocol, Udp};
use crate::{Device, Limits, Rate};
use engine_core::{ADDRESSES, ROUTE_MOST};
use std::io;
use std::net::SocketAddrV4;

/// The port Art-Net sends to and listens on.
pub const PORT: u16 = 6454;

const HEADER: usize = 18;
const ID: [u8; 8] = *b"Art-Net\0";
const OP_DMX: [u8; 2] = [0x00, 0x50];
const VERSION: [u8; 2] = [0, 14];
const PHYSICAL: u8 = 0;
/// The highest Port-Address: 15 bits.
const MOST: u16 = 0x7FFF;
/// Universes in the longest route, each with its own sequence.
const UNIVERSES: usize = ROUTE_MOST as usize / ADDRESSES;

/// Where a universe goes: Net, Sub-Net and Universe as one 15-bit number, 1 to 32,767.
/// 0 is deprecated by the spec (sACN has no universe 0) and cannot be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortAddress(u16);

impl PortAddress {
    /// The Port-Address `value`, if it is 1 to 32,767.
    pub const fn new(value: u16) -> Option<Self> {
        if value == 0 || value > MOST {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Its number.
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Opens an Art-Net output that sends every universe of its route to `to`, the first to
/// `first`, the next to `first` + 1 and on. Fails on port 0 and on an address that is not
/// one node: ArtDmx is unicast only. Off the output thread.
pub fn open(to: SocketAddrV4, first: PortAddress) -> io::Result<Box<dyn Device>> {
    let ip = to.ip();
    if ip.is_broadcast() || ip.is_multicast() || ip.is_unspecified() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ArtDmx goes to one node, never broadcast",
        ));
    }
    Ok(Box::new(ArtNet::open(to, first)?))
}

/// An Art-Net output: the UDP lane with ArtDmx packets.
type ArtNet = Udp<Universes>;

/// Art-Net's packets: the first Port-Address, each universe's last sequence and one
/// packet's buffer. Made at open: a frame allocates nothing.
pub(crate) struct Universes {
    first: PortAddress,
    sequences: [u8; UNIVERSES],
    packet: [u8; HEADER + ADDRESSES],
}

impl Protocol for Universes {
    type Settings = PortAddress;
    /// The universe's place in the route, and how many of its slots the stream holds.
    type Cut = (usize, usize);

    fn new(first: PortAddress) -> Self {
        Self {
            first,
            sequences: [0; UNIVERSES],
            packet: [0; HEADER + ADDRESSES],
        }
    }

    /// 44 a second, a DMX gateway's rate. A route whose last universe would pass
    /// Port-Address 32,767 does not fit.
    fn limits(&self) -> Limits {
        let fit = u32::from(MOST - self.first.get() + 1) * ADDRESSES as u32;
        Limits {
            channels: ROUTE_MOST.min(fit),
            rate: Rate::FortyFour,
            ..Limits::RAW
        }
    }

    fn frame(&mut self, channels: usize) -> impl Iterator<Item = Self::Cut> + use<> {
        cut(channels, self.first)
    }

    fn packet(&mut self, (index, slots): Self::Cut, stream: &[u8]) -> Option<&[u8]> {
        let sequence = self.sequences.get_mut(index)?;
        *sequence = next_sequence(*sequence);
        let port_address = self.first.get() + u16::try_from(index).ok()?;
        let length = slots + slots % 2;
        let offset = index * ADDRESSES;
        let (head, data) = self.packet.split_at_mut(HEADER);
        head.copy_from_slice(&header(
            port_address,
            *sequence,
            u16::try_from(length).ok()?,
        ));
        data.get_mut(..slots)?
            .copy_from_slice(stream.get(offset..offset + slots)?);
        data.get_mut(slots..length)?.fill(0);
        self.packet.get(..HEADER + length)
    }
}

/// The universes of `channels` channels from `first`: place in the route and slots. Never
/// past Port-Address 32,767, never more than a route holds.
fn cut(channels: usize, first: PortAddress) -> impl Iterator<Item = (usize, usize)> {
    let room = usize::from(MOST - first.get() + 1).min(UNIVERSES);
    let universes = channels.div_ceil(ADDRESSES).min(room);
    (0..universes).map(move |index| (index, (channels - index * ADDRESSES).min(ADDRESSES)))
}

/// The next sequence of a universe: 1 to 255, then 1 again. 0 is "off" to a receiver.
const fn next_sequence(sequence: u8) -> u8 {
    sequence % 255 + 1
}

/// The header of one ArtDmx.
fn header(port_address: u16, sequence: u8, length: u16) -> [u8; HEADER] {
    let [i0, i1, i2, i3, i4, i5, i6, i7] = ID;
    let [o0, o1] = OP_DMX;
    let [v0, v1] = VERSION;
    let [sub_uni, net] = port_address.to_le_bytes();
    let [l0, l1] = length.to_be_bytes();
    [
        i0, i1, i2, i3, i4, i5, i6, i7, o0, o1, v0, v1, sequence, PHYSICAL, sub_uni, net, l0, l1,
    ]
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
#[expect(
    clippy::indexing_slicing,
    reason = "a test reads fields at the byte the spec names"
)]
mod tests {
    use super::*;

    const ART_NET: [u8; 12] = [
        0x41, 0x72, 0x74, 0x2D, 0x4E, 0x65, 0x74, 0x00, 0x00, 0x50, 0x00, 0x0E,
    ];

    #[test]
    fn the_header_says_what_the_spec_says() {
        // (Port-Address, sequence, length, bytes 12 to 17), worked by hand from
        // outputs-artnet.md: sequence, physical, sub-uni, net, length high, length low.
        let cases = [
            (1, 1, 2, [0x01, 0x00, 0x01, 0x00, 0x00, 0x02]),
            (0x1234, 7, 512, [0x07, 0x00, 0x34, 0x12, 0x02, 0x00]),
            (0x7FFF, 255, 512, [0xFF, 0x00, 0xFF, 0x7F, 0x02, 0x00]),
            (256, 2, 100, [0x02, 0x00, 0x00, 0x01, 0x00, 0x64]),
        ];
        for (port_address, sequence, length, rest) in cases {
            let found = header(port_address, sequence, length);
            assert_eq!(found[..12], ART_NET, "{port_address}");
            assert_eq!(found[12..], rest, "{port_address}");
        }
    }

    #[test]
    fn a_packet_is_the_header_then_the_slots_padded_even() {
        // (first Port-Address, the stream of a frame, its one packet), worked by hand
        let cases = [
            (
                1,
                vec![0x2A],
                vec![
                    0x41, 0x72, 0x74, 0x2D, 0x4E, 0x65, 0x74, 0x00, 0x00, 0x50, 0x00, 0x0E, 0x01,
                    0x00, 0x01, 0x00, 0x00, 0x02, 0x2A, 0x00,
                ],
            ),
            (
                0x0123,
                vec![0xFF, 0x10, 0x00, 0x80],
                vec![
                    0x41, 0x72, 0x74, 0x2D, 0x4E, 0x65, 0x74, 0x00, 0x00, 0x50, 0x00, 0x0E, 0x01,
                    0x00, 0x23, 0x01, 0x00, 0x04, 0xFF, 0x10, 0x00, 0x80,
                ],
            ),
        ];
        for (first, stream, expected) in cases {
            let mut universes = Universes::new(PortAddress::new(first).unwrap());
            let packets = sent(&mut universes, &stream);
            let found: Vec<_> = packets.into_iter().map(|p| p.3).collect();
            assert_eq!(found, [expected], "{first}");
        }
    }

    #[test]
    fn the_pad_is_0_also_after_a_longer_frame() {
        let mut universes = Universes::new(PortAddress::new(1).unwrap());
        sent(&mut universes, &[9; 1_027]);
        let last = sent(&mut universes, &[9; 1_025]).pop().unwrap().3;
        assert_eq!(last[HEADER..], [9, 0], "one slot, then the pad");
    }

    #[test]
    fn a_port_address_is_1_to_32767() {
        // (number, a Port-Address)
        let cases = [
            (0, false),
            (1, true),
            (2, true),
            (32_766, true),
            (32_767, true),
            (32_768, false),
            (u16::MAX, false),
        ];
        for (number, valid) in cases {
            let found = PortAddress::new(number);
            assert_eq!(found.is_some(), valid, "{number}");
            assert_eq!(
                found.map(PortAddress::get),
                valid.then_some(number),
                "{number}"
            );
        }
    }

    #[test]
    fn the_sequence_runs_1_to_255() {
        // (now, next)
        let cases = [(0, 1), (1, 2), (254, 255), (255, 1)];
        for (now, next) in cases {
            assert_eq!(next_sequence(now), next, "{now}");
        }
    }

    /// Every packet of one frame of `channels` channels: (Port-Address, sequence, length,
    /// the whole packet), read from the bytes.
    fn sent(universes: &mut Universes, stream: &[u8]) -> Vec<(u16, u8, u16, Vec<u8>)> {
        let cuts: Vec<_> = universes.frame(stream.len()).collect();
        cuts.into_iter()
            .map(|cut| {
                let packet = universes.packet(cut, stream).unwrap().to_vec();
                let port_address = u16::from_le_bytes([packet[14], packet[15]]);
                let length = u16::from_be_bytes([packet[16], packet[17]]);
                (port_address, packet[12], length, packet)
            })
            .collect()
    }

    #[test]
    fn a_frame_is_cut_into_universes_of_512_the_last_padded_even() {
        // (channels, first Port-Address, packets: Port-Address, length)
        let cases = [
            (0, 1, vec![]),
            (1, 1, vec![(1, 2)]),
            (2, 1, vec![(1, 2)]),
            (511, 1, vec![(1, 512)]),
            (512, 1, vec![(1, 512)]),
            (513, 1, vec![(1, 512), (2, 2)]),
            (1_027, 1, vec![(1, 512), (2, 512), (3, 4)]),
            (1_024, 100, vec![(100, 512), (101, 512)]),
            (1_024, 32_767, vec![(32_767, 512)]),
            (1_536, 32_766, vec![(32_766, 512), (32_767, 512)]),
            (
                usize::try_from(ROUTE_MOST).unwrap(),
                1,
                (1..=128).map(|n| (n, 512)).collect(),
            ),
            (
                usize::try_from(ROUTE_MOST).unwrap() + 1,
                1,
                (1..=128).map(|n| (n, 512)).collect(),
            ),
        ];
        for (channels, first, expected) in cases {
            let mut universes = Universes::new(PortAddress::new(first).unwrap());
            let stream: Vec<u8> = (0..channels).map(|n| (n % 251) as u8).collect();
            let packets = sent(&mut universes, &stream);
            let found: Vec<_> = packets.iter().map(|p| (p.0, p.2)).collect();
            assert_eq!(found, expected, "{channels} from {first}");
            for (index, (_, _, length, packet)) in packets.iter().enumerate() {
                assert_eq!(packet.len(), HEADER + usize::from(*length), "{channels}");
                let offset = index * ADDRESSES;
                let slots = channels.saturating_sub(offset).min(ADDRESSES);
                assert_eq!(
                    packet[HEADER..HEADER + slots],
                    stream[offset..offset + slots],
                    "{channels} universe {index}"
                );
                assert!(
                    packet[HEADER + slots..].iter().all(|b| *b == 0),
                    "{channels}: the pad is 0"
                );
            }
        }
    }

    #[test]
    fn each_universe_counts_its_own_sequence_across_routes() {
        // (channels of each frame, the sequence each universe sent in the last frame)
        let cases = [
            (vec![512], vec![(1, 1)]),
            (vec![512, 512, 512], vec![(1, 3)]),
            (vec![1_536, 512, 1_536], vec![(1, 3), (2, 2), (3, 2)]),
            (vec![1_024, 0, 1_024], vec![(1, 2), (2, 2)]),
            (vec![512; 255], vec![(1, 255)]),
            (vec![512; 256], vec![(1, 1)]),
            (vec![1_024; 257], vec![(1, 2), (2, 2)]),
        ];
        for (frames, expected) in cases {
            let mut universes = Universes::new(PortAddress::new(1).unwrap());
            let mut last = vec![];
            for channels in &frames {
                last = sent(&mut universes, &vec![0; *channels]);
                assert!(last.iter().all(|p| p.1 != 0), "never 0");
            }
            let found: Vec<_> = last.iter().map(|p| (p.0, p.1)).collect();
            assert_eq!(found, expected, "{:?}", frames.len());
        }
    }

    #[test]
    fn a_route_is_refused_when_its_last_port_address_passes_32767() {
        // (first Port-Address, channels the limit allows)
        let cases = [
            (1, ROUTE_MOST),
            (32_640, ROUTE_MOST),
            (32_641, 127 * 512),
            (32_766, 1_024),
            (32_767, 512),
        ];
        for (first, channels) in cases {
            let limits = Universes::new(PortAddress::new(first).unwrap()).limits();
            assert_eq!(limits.channels, channels, "{first}");
            assert_eq!(limits.rate, Rate::FortyFour, "{first}");
            assert_eq!(limits.whole.get(), 1, "{first}");
        }
    }
}
