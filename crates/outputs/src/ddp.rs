//! DDP over UDP, as WLED reads it. A frame's run of universes is one stream of channels;
//! it goes out in packets of at most [`MAX_DATA`] bytes, the push flag on the last one.
//!
//! | Byte | Header field |
//! |---|---|
//! | 0 | flags: version 1 (`0x40`), push (`0x01`) on the last packet of a frame |
//! | 1 | sequence, 1 to 15, the same for every packet of a frame |
//! | 2 | data type: RGB 8 bit `0x0B`, RGBW 8 bit `0x1B` |
//! | 3 | destination: 1, the default output |
//! | 4 to 7 | offset of this packet's data in the frame, bytes, big endian |
//! | 8 to 9 | length of this packet's data, bytes, big endian |

use crate::{Color, Levels, Outcome};
use engine_core::ADDRESSES;
use std::io::{self, ErrorKind};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};

pub const HEADER: usize = 10;
/// Data bytes per packet: 480 RGB or 360 RGBW pixels, as WLED expects.
pub const MAX_DATA: usize = 1440;
const VERSION: u8 = 0x40;
const PUSH: u8 = 0x01;
const DESTINATION: u8 = 1;

/// A DDP output: a connected, non-blocking UDP socket and one packet's buffer.
pub struct Ddp {
    socket: UdpSocket,
    color: Color,
    sequence: u8,
    packet: [u8; HEADER + MAX_DATA],
}

impl Ddp {
    /// Opens a socket to `address`. Off the output thread.
    pub fn open(address: SocketAddrV4, color: Color) -> io::Result<Self> {
        if address.port() == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "no port"));
        }
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
        socket.connect(address)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            color,
            sequence: 0,
            packet: [0; HEADER + MAX_DATA],
        })
    }

    /// Sends the frame as packets. A full socket drops the rest of the frame.
    pub fn take(&mut self, frame: &[Levels]) -> Outcome {
        self.sequence = next_sequence(self.sequence);
        let total = data_length(frame.len() * ADDRESSES, self.color);
        for (offset, length, last) in packets(total) {
            let (header, data) = self.packet.split_at_mut(HEADER);
            header.copy_from_slice(&self::header(
                self.sequence,
                self.color,
                offset,
                length,
                last,
            ));
            let Some(data) = data.get_mut(..length) else {
                return Outcome::Failed; // cannot happen: length is at most MAX_DATA
            };
            channels(frame, offset, data);
            let Some(packet) = self.packet.get(..HEADER + length) else {
                return Outcome::Failed;
            };
            match self.socket.send(packet) {
                Ok(sent) if sent == packet.len() => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Outcome::Dropped,
                Ok(_) | Err(_) => return Outcome::Failed,
            }
        }
        Outcome::Accepted
    }
}

/// The next sequence number: 1 to 15, then 1 again. 0 means "no sequence" to WLED.
pub fn next_sequence(sequence: u8) -> u8 {
    sequence % 15 + 1
}

/// The bytes of whole pixels in `channels` channels.
pub fn data_length(channels: usize, color: Color) -> usize {
    let size = match color {
        Color::Rgb => 3,
        Color::Rgbw => 4,
    };
    channels / size * size
}

/// The packets for `total` bytes: offset, length, and whether it is the last.
pub fn packets(total: usize) -> impl Iterator<Item = (usize, usize, bool)> {
    (0..total).step_by(MAX_DATA).map(move |offset| {
        let length = MAX_DATA.min(total - offset);
        (offset, length, offset + length == total)
    })
}

/// The header of one packet.
pub fn header(
    sequence: u8,
    color: Color,
    offset: usize,
    length: usize,
    last: bool,
) -> [u8; HEADER] {
    let kind = match color {
        Color::Rgb => 0x0B,
        Color::Rgbw => 0x1B,
    };
    let [o0, o1, o2, o3] = u32::try_from(offset).unwrap_or(u32::MAX).to_be_bytes();
    let [l0, l1] = u16::try_from(length).unwrap_or(u16::MAX).to_be_bytes();
    let flags = if last { VERSION | PUSH } else { VERSION };
    [flags, sequence, kind, DESTINATION, o0, o1, o2, o3, l0, l1]
}

/// Copies the channels from `offset` of the frame's stream into `out`.
pub fn channels(frame: &[Levels], offset: usize, out: &mut [u8]) {
    let stream = frame.iter().flat_map(|universe| universe.0.iter());
    for (byte, level) in out.iter_mut().zip(stream.skip(offset)) {
        *byte = *level;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sequence_runs_1_to_15() {
        // (now, next)
        let cases = [(0, 1), (1, 2), (14, 15), (15, 1)];
        for (now, next) in cases {
            assert_eq!(next_sequence(now), next, "{now}");
        }
    }

    #[test]
    fn a_frame_splits_into_packets_of_whole_pixels() {
        // (channels, color, packets: offset, length, last)
        let cases = [
            (512, Color::Rgb, vec![(0, 510, true)]),
            (512, Color::Rgbw, vec![(0, 512, true)]),
            (1536, Color::Rgb, vec![(0, 1440, false), (1440, 96, true)]),
            (2048, Color::Rgbw, vec![(0, 1440, false), (1440, 608, true)]),
            (2880, Color::Rgb, vec![(0, 1440, false), (1440, 1440, true)]),
            (2, Color::Rgb, vec![]),
        ];
        for (channels, color, expected) in cases {
            let found: Vec<_> = packets(data_length(channels, color)).collect();
            assert_eq!(found, expected, "{channels} {color:?}");
        }
    }

    #[test]
    fn the_header_says_what_wled_expects() {
        // (sequence, color, offset, length, last, header)
        let cases = [
            (
                1,
                Color::Rgb,
                0,
                510,
                true,
                [0x41, 1, 0x0B, 1, 0, 0, 0, 0, 0x01, 0xFE],
            ),
            (
                15,
                Color::Rgbw,
                0,
                1440,
                false,
                [0x40, 15, 0x1B, 1, 0, 0, 0, 0, 0x05, 0xA0],
            ),
            (
                7,
                Color::Rgbw,
                1440,
                608,
                true,
                [0x41, 7, 0x1B, 1, 0, 0, 0x05, 0xA0, 0x02, 0x60],
            ),
            (
                2,
                Color::Rgb,
                0x0102_0304,
                3,
                true,
                [0x41, 2, 0x0B, 1, 1, 2, 3, 4, 0, 3],
            ),
        ];
        for (sequence, color, offset, length, last, expected) in cases {
            assert_eq!(header(sequence, color, offset, length, last), expected);
        }
    }

    #[test]
    fn channels_run_across_universes() {
        let mut frame = [Levels::ZERO; 2];
        frame[0].0[510] = 1;
        frame[0].0[511] = 2;
        frame[1].0[0] = 3;
        // (offset, length, bytes)
        let cases = [
            (510, 3, vec![1, 2, 3]),
            (0, 2, vec![0, 0]),
            (1023, 2, vec![0, 0]),
        ];
        for (offset, length, expected) in cases {
            let mut out = vec![0; length];
            channels(&frame, offset, &mut out);
            assert_eq!(out, expected, "{offset}");
        }
    }
}
