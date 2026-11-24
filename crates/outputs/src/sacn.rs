//! sACN (ANSI E1.31) over UDP, as `docs/design/outputs-sacn.md` reads the spec. A frame is
//! one stream of channel bytes; it goes out as one data packet per universe of 512 slots,
//! to the output's first universe and on, each universe with its own sequence. Multicast to
//! each universe's group by default, unicast to one receiver as the other choice. When the
//! output ends, [`Device::terminate`] tells the receivers so.
//!
//! | Byte | Data packet field |
//! |---|---|
//! | 0 to 15 | preamble size `0x0010`, postamble size 0, `ASC-E1.17` and three zeros |
//! | 16 to 17 | root flags `0x7` and the length from here to the end |
//! | 18 to 21 | root vector 4: data |
//! | 22 to 37 | CID |
//! | 38 to 39 | framing flags and length |
//! | 40 to 43 | framing vector 2: data packet |
//! | 44 to 107 | source name, zero-padded |
//! | 108 | priority 100 |
//! | 109 to 110 | sync address 0: not synchronised |
//! | 111 | sequence, per universe, wraps |
//! | 112 | options: `0x40` stream terminated |
//! | 113 to 114 | universe |
//! | 115 to 116 | DMP flags and length |
//! | 117 to 124 | vector 2, type `0xA1`, first address 0, increment 1, value count |
//! | 125 | start code 0 |
//! | 126 on | slots, 0 to 512 |

use crate::udp::{Link, Protocol, Udp};
use crate::{Device, Limits, Rate};
use engine_core::{ADDRESSES, OutputNumber, ROUTE_MOST};
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};

/// The port sACN sends to.
pub const PORT: u16 = 5568;

const HEADER: usize = 126;
/// The highest universe.
const MOST: u16 = 63_999;
/// Universes in the longest route, each with its own sequence.
const UNIVERSES: usize = ROUTE_MOST as usize / ADDRESSES;
const NAME: usize = 64;
const PRIORITY: u8 = 100;
const TERMINATED: u8 = 0x40;
/// Rounds of terminated packets when a stream ends.
const CLOSING: u8 = 3;
/// Where a data packet holds its universe; the multicast link reads it there.
const UNIVERSE_AT: usize = 113;
/// Where each flags-and-length field starts: its length runs to the end of the packet.
const ROOT_AT: usize = 16;
const FRAMING_AT: usize = 38;
const DMP_AT: usize = 115;

const PREAMBLE: [u8; ROOT_AT] = *b"\x00\x10\x00\x00ASC-E1.17\x00\x00\x00";
const ROOT_VECTOR: [u8; 4] = [0, 0, 0, 4];
const FRAMING_VECTOR: [u8; 4] = [0, 0, 0, 2];
const SYNC: [u8; 2] = [0, 0];
/// Vector "set property", address and data type, first address 0, increment 1.
const DMP: [u8; 6] = [0x02, 0xA1, 0, 0, 0, 1];
const START_CODE: u8 = 0;

/// A universe, 1 to 63,999. Others are reserved and cannot be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Universe(u16);

impl Universe {
    /// The universe `value`, if it is 1 to 63,999.
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

/// Where the packets go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    /// Each universe to its own group, `239.255.{high byte}.{low byte}`, on this port
    /// ([`PORT`] on a real network). Leaves on the interface of the default route.
    Multicast(u16),
    /// Every universe to one receiver.
    Unicast(SocketAddrV4),
}

/// Opens an sACN output that sends every universe of its route `to`, the first as
/// universe `first`, the next as `first` + 1 and on. Its packets name it with `cid` and
/// `Bossylights <output>`. Fails on port 0 and on a unicast address that is not one
/// receiver. Off the output thread.
pub fn open(
    to: Destination,
    first: Universe,
    cid: [u8; 16],
    output: OutputNumber,
) -> io::Result<Box<dyn Device>> {
    let source = Source {
        first,
        cid,
        name: name(output),
    };
    let refused = |why| Err(io::Error::new(io::ErrorKind::InvalidInput, why));
    match to {
        Destination::Unicast(address) => {
            let ip = address.ip();
            if ip.is_broadcast() || ip.is_multicast() || ip.is_unspecified() {
                return refused("unicast goes to one receiver");
            }
            Ok(Box::new(Sacn::open(address, source)?))
        }
        Destination::Multicast(0) => refused("no port"),
        Destination::Multicast(port) => Ok(Box::new(Sacn::with(
            groups(port).ok(),
            Box::new(move || groups(port)),
            source,
        )?)),
    }
}

/// An sACN output: the UDP lane with sACN data packets.
type Sacn = Udp<Streams>;

/// What every packet of one output says about its sender.
pub(crate) struct Source {
    first: Universe,
    cid: [u8; 16],
    name: [u8; NAME],
}

/// `Bossylights <output>`, zero-padded to 64 bytes.
fn name(output: OutputNumber) -> [u8; NAME] {
    let mut name = [0; NAME];
    let text = format!("Bossylights {}", output.slot() + 1);
    for (byte, value) in name.iter_mut().zip(text.bytes()) {
        *byte = value;
    }
    name
}

/// Where an output's stream is. Every pair: the table in `tests` and `docs/design/outputs-sacn.md`.
///
/// ```text
/// [*] --> Live --terminate--> Terminating{1} --frame--> Terminating{2} --frame--> Terminating{3} --frame--> Ended
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stream {
    /// Frames go out as data packets.
    Live,
    /// The stream ends: the next frame is round `round` of terminated packets.
    Terminating { round: u8 },
    /// Silent for good.
    Ended,
}

/// What happens to a stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    /// A frame is cut into packets.
    Frame,
    /// The output is removed or the engine stops.
    Terminate,
}

/// What a frame sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sends {
    /// One data packet per universe of the stream.
    Data,
    /// One terminated packet, the start code only, per universe of the last data frame.
    Terminated,
    Nothing,
}

/// The one transition function of a stream: a calculation.
const fn next(stream: Stream, event: Event) -> (Stream, Sends) {
    match (stream, event) {
        (Stream::Live, Event::Frame) => (Stream::Live, Sends::Data),
        (Stream::Live, Event::Terminate) => (Stream::Terminating { round: 1 }, Sends::Nothing),
        (Stream::Terminating { round }, Event::Frame) if round < CLOSING => {
            (Stream::Terminating { round: round + 1 }, Sends::Terminated)
        }
        (Stream::Terminating { .. }, Event::Frame) => (Stream::Ended, Sends::Terminated),
        (same @ (Stream::Terminating { .. } | Stream::Ended), Event::Terminate)
        | (same @ Stream::Ended, Event::Frame) => (same, Sends::Nothing),
    }
}

/// The closing frames a stream still sends.
const fn rounds_left(stream: Stream) -> u8 {
    match stream {
        Stream::Terminating { round } => CLOSING + 1 - round,
        Stream::Live | Stream::Ended => 0,
    }
}

/// One packet of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cut {
    /// The universe's place in the route.
    index: usize,
    /// Where its slots start in the stream, and how many.
    from: usize,
    slots: usize,
    options: u8,
}

/// sACN's packets: the source, the stream's state, each universe's next sequence and one
/// packet's buffer. Made at open: a frame allocates nothing.
pub(crate) struct Streams {
    source: Source,
    stream: Stream,
    /// Universes of the last data frame: the ones a terminate closes.
    sent: usize,
    sequences: [u8; UNIVERSES],
    packet: [u8; HEADER + ADDRESSES],
}

impl Protocol for Streams {
    type Settings = Source;
    type Cut = Cut;

    fn new(source: Source) -> Self {
        Self {
            source,
            stream: Stream::Live,
            sent: 0,
            sequences: [0; UNIVERSES],
            packet: [0; HEADER + ADDRESSES],
        }
    }

    /// 44 a second, as a DMX universe. A route whose last universe would pass 63,999
    /// does not fit.
    fn limits(&self) -> Limits {
        let fit = u32::from(MOST - self.source.first.get() + 1) * ADDRESSES as u32;
        Limits {
            channels: ROUTE_MOST.min(fit),
            rate: Rate::FortyFour,
            ..Limits::RAW
        }
    }

    fn frame(&mut self, channels: usize) -> impl Iterator<Item = Cut> + use<> {
        let (stream, sends) = next(self.stream, Event::Frame);
        self.stream = stream;
        let (universes, terminated) = match sends {
            Sends::Data => {
                self.sent = universes(channels, self.source.first);
                (self.sent, false)
            }
            Sends::Terminated => (self.sent, true),
            Sends::Nothing => (0, false),
        };
        (0..universes).map(move |index| {
            if terminated {
                Cut {
                    index,
                    from: 0,
                    slots: 0,
                    options: TERMINATED,
                }
            } else {
                let from = index * ADDRESSES;
                Cut {
                    index,
                    from,
                    slots: (channels - from).min(ADDRESSES),
                    options: 0,
                }
            }
        })
    }

    fn packet(&mut self, cut: Cut, stream: &[u8]) -> Option<&[u8]> {
        let next_sequence = self.sequences.get_mut(cut.index)?;
        let sequence = *next_sequence;
        *next_sequence = sequence.wrapping_add(1);
        let universe = self.source.first.get() + u16::try_from(cut.index).ok()?;
        let (head, data) = self.packet.split_at_mut(HEADER);
        head.copy_from_slice(&header(
            &self.source,
            sequence,
            cut.options,
            universe,
            u16::try_from(cut.slots).ok()?,
        ));
        data.get_mut(..cut.slots)?
            .copy_from_slice(stream.get(cut.from..cut.from + cut.slots)?);
        self.packet.get(..HEADER + cut.slots)
    }

    fn stop(&mut self) -> u8 {
        self.stream = next(self.stream, Event::Terminate).0;
        rounds_left(self.stream)
    }
}

/// The universes `channels` channels fill from `first`. Never past 63,999, never more
/// than a route holds.
fn universes(channels: usize, first: Universe) -> usize {
    let room = usize::from(MOST - first.get() + 1).min(UNIVERSES);
    channels.div_ceil(ADDRESSES).min(room)
}

/// A flags-and-length field for `length` bytes.
const fn flags(length: usize) -> [u8; 2] {
    (0x7000 | (length as u16 & 0x0FFF)).to_be_bytes()
}

/// The header of one data packet of `slots` slots.
fn header(source: &Source, sequence: u8, options: u8, universe: u16, slots: u16) -> [u8; HEADER] {
    let end = HEADER + usize::from(slots);
    let count = (slots + 1).to_be_bytes();
    let fields: [&[u8]; 16] = [
        &PREAMBLE,
        &flags(end - ROOT_AT),
        &ROOT_VECTOR,
        &source.cid,
        &flags(end - FRAMING_AT),
        &FRAMING_VECTOR,
        &source.name,
        &[PRIORITY],
        &SYNC,
        &[sequence],
        &[options],
        &universe.to_be_bytes(),
        &flags(end - DMP_AT),
        &DMP,
        &count,
        &[START_CODE],
    ];
    let mut header = [0; HEADER];
    for (byte, value) in header.iter_mut().zip(fields.into_iter().flatten()) {
        *byte = *value;
    }
    header
}

/// The multicast group of the data packet's universe: `239.255.{high}.{low}`. `None`
/// cannot happen: every packet holds a universe.
fn group(packet: &[u8]) -> Option<Ipv4Addr> {
    let [high, low] = *packet.get(UNIVERSE_AT..UNIVERSE_AT + 2)?.first_chunk()?;
    Some(Ipv4Addr::new(239, 255, high, low))
}

/// A socket that sends each data packet to its universe's group.
struct Groups {
    socket: UdpSocket,
    port: u16,
}

impl Link for Groups {
    fn send(&mut self, packet: &[u8]) -> io::Result<usize> {
        let to = group(packet).ok_or(io::ErrorKind::InvalidData)?;
        self.socket
            .send_to(packet, SocketAddrV4::new(to, self.port))
    }
}

fn groups(port: u16) -> io::Result<Box<dyn Link>> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_nonblocking(true)?;
    Ok(Box::new(Groups { socket, port }))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
#[expect(
    clippy::indexing_slicing,
    reason = "a test reads fields at the byte the spec names"
)]
mod tests {
    use super::*;

    const CID: [u8; 16] = [
        0x6B, 0xA7, 0xB8, 0x10, 0x9D, 0xAD, 0x11, 0xD1, 0x80, 0xB4, 0x00, 0xC0, 0x4F, 0xD4, 0x30,
        0xC8,
    ];

    fn source(first: u16, output: u8) -> Source {
        Source {
            first: Universe::new(first).unwrap(),
            cid: CID,
            name: name(OutputNumber::try_from(output).unwrap()),
        }
    }

    #[test]
    fn a_data_packet_is_what_the_spec_table_says() {
        // Output 1, universe 1, sequence 7, one slot 0x2A: worked by hand from
        // outputs-sacn.md. 127 bytes: root length 111, framing 89, DMP 12, count 2.
        let mut expected = vec![
            0x00, 0x10, 0x00, 0x00, 0x41, 0x53, 0x43, 0x2D, 0x45, 0x31, 0x2E, 0x31, 0x37, 0x00,
            0x00, 0x00, 0x70, 0x6F, 0x00, 0x00, 0x00, 0x04,
        ];
        expected.extend(CID);
        expected.extend([0x70, 0x59, 0x00, 0x00, 0x00, 0x02]);
        let mut name = b"Bossylights 1".to_vec();
        name.resize(64, 0);
        expected.extend(name);
        expected.extend([
            0x64, 0x00, 0x00, 0x07, 0x00, 0x00, 0x01, 0x70, 0x0C, 0x02, 0xA1, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x02, 0x00, 0x2A,
        ]);
        let mut streams = Streams::new(source(1, 1));
        streams.sequences[0] = 7;
        let cut = streams.frame(1).next().unwrap();
        assert_eq!(streams.packet(cut, &[0x2A]).unwrap(), expected);
    }

    #[test]
    fn the_header_fields_follow_the_slots_and_the_universe() {
        // (slots, sequence, options, universe, bytes 16-17, 38-39, 111-114, 115-116, 123-124)
        let cases = [
            (
                512,
                0,
                0,
                1,
                [0x72, 0x6E],
                [0x72, 0x58],
                [0, 0, 0x00, 0x01],
                [0x72, 0x0B],
                [0x02, 0x01],
            ),
            (
                1,
                255,
                0,
                0x1234,
                [0x70, 0x6F],
                [0x70, 0x59],
                [255, 0, 0x12, 0x34],
                [0x70, 0x0C],
                [0x00, 0x02],
            ),
            (
                0,
                9,
                0x40,
                63_999,
                [0x70, 0x6E],
                [0x70, 0x58],
                [9, 0x40, 0xF9, 0xFF],
                [0x70, 0x0B],
                [0x00, 0x01],
            ),
        ];
        for (slots, sequence, options, universe, root, framing, middle, dmp, count) in cases {
            let found = header(&source(1, 16), sequence, options, universe, slots);
            assert_eq!(found[16..18], root, "{slots}");
            assert_eq!(found[38..40], framing, "{slots}");
            assert_eq!(found[111..115], middle, "{slots}");
            assert_eq!(found[115..117], dmp, "{slots}");
            assert_eq!(found[123..125], count, "{slots}");
            assert_eq!(&found[44..58], b"Bossylights 16", "{slots}");
            assert!(found[58..108].iter().all(|b| *b == 0), "{slots}");
            assert_eq!(found[108], 100, "priority");
        }
    }

    #[test]
    fn the_source_name_is_bossylights_and_the_output() {
        // (output, the name up to its zeros)
        let cases = [
            (1, "Bossylights 1"),
            (9, "Bossylights 9"),
            (16, "Bossylights 16"),
        ];
        for (output, expected) in cases {
            let found = name(OutputNumber::try_from(output).unwrap());
            assert_eq!(&found[..expected.len()], expected.as_bytes(), "{output}");
            assert!(found[expected.len()..].iter().all(|b| *b == 0), "{output}");
        }
    }

    #[test]
    fn a_universe_is_1_to_63999() {
        // (value, a universe)
        let cases = [
            (0, false),
            (1, true),
            (63_999, true),
            (64_000, false),
            (64_214, false),
            (u16::MAX, false),
        ];
        for (value, valid) in cases {
            assert_eq!(
                Universe::new(value).map(Universe::get),
                valid.then_some(value),
                "{value}"
            );
        }
    }

    #[test]
    fn each_universe_goes_to_its_group() {
        // (universe, its group): outputs-sacn.md, Addressing
        let cases = [
            (1, Ipv4Addr::new(239, 255, 0, 1)),
            (256, Ipv4Addr::new(239, 255, 1, 0)),
            (0x1234, Ipv4Addr::new(239, 255, 0x12, 0x34)),
            (63_999, Ipv4Addr::new(239, 255, 249, 255)),
        ];
        for (universe, expected) in cases {
            let packet = header(&source(1, 1), 0, 0, universe, 0);
            assert_eq!(group(&packet), Some(expected), "{universe}");
        }
        assert_eq!(group(&[0; UNIVERSE_AT + 1]), None, "too short");
    }

    #[test]
    fn a_frame_is_cut_into_universes_of_512() {
        // (channels, first universe, universes)
        let cases = [
            (0, 1, 0),
            (1, 1, 1),
            (512, 1, 1),
            (513, 1, 2),
            (65_536, 1, 128),
            (65_537, 1, 128),
            (1_024, 63_999, 1),
            (65_536, 63_873, 127),
            (65_536, 63_872, 128),
        ];
        for (channels, first, expected) in cases {
            assert_eq!(
                universes(channels, Universe::new(first).unwrap()),
                expected,
                "{channels} from {first}"
            );
        }
        // The last universe holds what is left.
        let cuts: Vec<_> = Streams::new(source(1, 1))
            .frame(1_027)
            .map(|c| (c.from, c.slots))
            .collect();
        assert_eq!(cuts, [(0, 512), (512, 512), (1_024, 3)]);
    }

    #[test]
    fn the_route_fits_below_63999() {
        // (first universe, most channels)
        let cases = [
            (1, 65_536),
            (63_872, 65_536),
            (63_873, 65_024),
            (63_999, 512),
        ];
        for (first, expected) in cases {
            let limits = Streams::new(source(first, 1)).limits();
            assert_eq!(limits.channels, expected, "{first}");
            assert_eq!(limits.rate, Rate::FortyFour, "{first}");
        }
    }

    #[test]
    fn every_state_and_event_of_a_stream() {
        use Event::{Frame, Terminate};
        use Stream::{Ended, Live, Terminating};
        let t = |round| Terminating { round };
        // (stream, event, stream after, what the frame sends, closing frames left)
        let cases = [
            (Live, Frame, Live, Sends::Data, 0),
            (Live, Terminate, t(1), Sends::Nothing, 3),
            (t(1), Frame, t(2), Sends::Terminated, 2),
            (t(1), Terminate, t(1), Sends::Nothing, 3),
            (t(2), Frame, t(3), Sends::Terminated, 1),
            (t(2), Terminate, t(2), Sends::Nothing, 2),
            (t(3), Frame, Ended, Sends::Terminated, 0),
            (t(3), Terminate, t(3), Sends::Nothing, 1),
            (Ended, Frame, Ended, Sends::Nothing, 0),
            (Ended, Terminate, Ended, Sends::Nothing, 0),
        ];
        for (stream, event, after, sends, left) in cases {
            assert_eq!(next(stream, event), (after, sends), "{stream:?} {event:?}");
            assert_eq!(rounds_left(after), left, "{stream:?} {event:?}");
        }
    }

    #[test]
    fn terminated_packets_carry_the_start_code_only_and_the_next_sequence() {
        let mut streams = Streams::new(source(1, 1));
        let frame = [5; 600];
        let cuts: Vec<_> = streams.frame(frame.len()).collect();
        for cut in cuts {
            streams.packet(cut, &frame).unwrap();
        }
        assert_eq!(streams.stop(), 3);
        // (round, packets: universe, sequence, options, length)
        for round in 1..=3_u8 {
            let cuts: Vec<_> = streams.frame(0).collect();
            let found: Vec<_> = cuts
                .into_iter()
                .map(|cut| {
                    let packet = streams.packet(cut, &[]).unwrap();
                    (packet[114], packet[111], packet[112], packet.len())
                })
                .collect();
            assert_eq!(
                found,
                [(1, round, 0x40, 126), (2, round, 0x40, 126)],
                "round {round}"
            );
        }
        assert_eq!(streams.frame(600).count(), 0, "ended");
        assert_eq!(streams.stop(), 0, "ended");
    }
}
