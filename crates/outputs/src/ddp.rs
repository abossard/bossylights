//! DDP over UDP, as WLED reads it. A frame is one stream of channel bytes;
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

use crate::udp::{Protocol, Udp};
use crate::{Color, Limits};
use std::num::NonZeroU16;
#[cfg(test)]
use {
    crate::udp::{CLEAN, Link, Reopen, SOCKET_THREAD},
    crate::{Device, Outcome},
    std::io::{self, ErrorKind},
    std::sync::{Arc, atomic::Ordering},
    std::thread,
    std::time::Duration,
};

const HEADER: usize = 10;
/// Data bytes per packet: 480 RGB or 360 RGBW pixels, as WLED expects.
const MAX_DATA: usize = 1440;
const VERSION: u8 = 0x40;
const PUSH: u8 = 0x01;
const DESTINATION: u8 = 1;

/// A DDP output: the UDP lane with DDP's packets.
pub(crate) type Ddp = Udp<Pixels>;

/// DDP's packets: the pixel colour, the frame's sequence and one packet's buffer.
pub(crate) struct Pixels {
    color: Color,
    sequence: u8,
    packet: [u8; HEADER + MAX_DATA],
}

impl Protocol for Pixels {
    type Settings = Color;
    /// Offset, length, and whether it is the frame's last packet.
    type Cut = (usize, usize, bool);

    fn new(color: Color) -> Self {
        Self {
            color,
            sequence: 0,
            packet: [0; HEADER + MAX_DATA],
        }
    }

    fn limits(&self) -> Limits {
        Limits::ddp(self.color)
    }

    fn frame(&mut self, channels: usize) -> impl Iterator<Item = Self::Cut> + use<> {
        self.sequence = next_sequence(self.sequence);
        packets(data_length(channels, self.color))
    }

    fn packet(&mut self, (offset, length, last): Self::Cut, stream: &[u8]) -> Option<&[u8]> {
        let (header, data) = self.packet.split_at_mut(HEADER);
        header.copy_from_slice(&self::header(
            self.sequence,
            self.color,
            offset,
            length,
            last,
        ));
        data.get_mut(..length)?
            .copy_from_slice(stream.get(offset..offset + length)?);
        self.packet.get(..HEADER + length)
    }
}

/// The next sequence number: 1 to 15, then 1 again. 0 means "no sequence" to WLED.
fn next_sequence(sequence: u8) -> u8 {
    sequence % 15 + 1
}

/// The channels of one pixel.
pub(crate) const fn pixel(color: Color) -> NonZeroU16 {
    let size = match color {
        Color::Rgb => NonZeroU16::new(3),
        Color::Rgbw => NonZeroU16::new(4),
    };
    match size {
        Some(size) => size,
        None => NonZeroU16::MIN,
    }
}

/// The bytes of whole pixels in `channels` channels.
fn data_length(channels: usize, color: Color) -> usize {
    let size = usize::from(pixel(color).get());
    channels / size * size
}

/// The packets for `total` bytes: offset, length, and whether it is the last.
fn packets(total: usize) -> impl Iterator<Item = (usize, usize, bool)> {
    (0..total).step_by(MAX_DATA).map(move |offset| {
        let length = MAX_DATA.min(total - offset);
        (offset, length, offset + length == total)
    })
}

/// The header of one packet.
fn header(sequence: u8, color: Color, offset: usize, length: usize, last: bool) -> [u8; HEADER] {
    let kind = match color {
        Color::Rgb => 0x0B,
        Color::Rgbw => 0x1B,
    };
    let [o0, o1, o2, o3] = u32::try_from(offset).unwrap_or(u32::MAX).to_be_bytes();
    let [l0, l1] = u16::try_from(length).unwrap_or(u16::MAX).to_be_bytes();
    let flags = if last { VERSION | PUSH } else { VERSION };
    [flags, sequence, kind, DESTINATION, o0, o1, o2, o3, l0, l1]
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
mod tests {
    use super::*;

    #[test]
    fn every_frame_steps_the_sequence_also_without_a_whole_pixel() {
        // (channels of each frame, the sequence of the last frame's packet)
        let cases = [(vec![3], 1), (vec![2, 3], 2), (vec![0, 1, 3], 3)];
        for (frames, expected) in cases {
            let mut pixels = Pixels::new(Color::Rgb);
            let cuts: Vec<_> = frames
                .iter()
                .flat_map(|c| pixels.frame(*c).collect::<Vec<_>>())
                .collect();
            let packet = pixels.packet(*cuts.first().unwrap(), &[0; 3]).unwrap();
            assert_eq!(packet.get(1), Some(&expected), "{frames:?}");
        }
    }

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

    /// A link that fails every send the same way, and says on which thread it was dropped:
    /// 0 not yet, 1 the socket thread, 2 another.
    struct Fails(ErrorKind, Arc<std::sync::atomic::AtomicU8>);
    impl Link for Fails {
        fn send(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(self.0.into())
        }
    }
    impl Drop for Fails {
        fn drop(&mut self) {
            let on_socket_thread = thread::current().name() == Some(SOCKET_THREAD);
            self.1
                .store(if on_socket_thread { 1 } else { 2 }, Ordering::SeqCst);
        }
    }

    /// A link that is refused on every `n`th send, as a host that rate-limits its answers.
    struct Refused(u32, u32);
    impl Link for Refused {
        fn send(&mut self, packet: &[u8]) -> io::Result<usize> {
            self.1 += 1;
            if self.1.is_multiple_of(self.0) {
                Err(ErrorKind::ConnectionRefused.into())
            } else {
                Ok(packet.len())
            }
        }
    }

    #[test]
    fn refusals_now_and_then_keep_the_output_failed_until_a_clean_stretch() {
        let reopen: Reopen = Box::new(|| Err(io::Error::other("not needed")));
        let mut ddp = Ddp::with(Some(Box::new(Refused(6, 0))), reopen, Color::Rgb).unwrap();
        let frame = [0; engine_core::ADDRESSES];
        let outcomes: Vec<_> = (0..198).map(|_| ddp.take(&frame)).collect(); // ends on a refusal
        // The first five frames went out before the first refusal.
        assert!(
            outcomes.iter().take(5).all(|o| *o == Outcome::Accepted),
            "{outcomes:?}"
        );
        assert!(
            outcomes.iter().skip(5).all(|o| *o == Outcome::Failed),
            "{outcomes:?}"
        );
        // The refusals stop: `CLEAN` frames later the output runs again.
        ddp.link = Box::new(Counts(Arc::default()));
        let after: Vec<_> = (0..=CLEAN).map(|_| ddp.take(&frame)).collect();
        let clean = usize::try_from(CLEAN).unwrap();
        assert!(
            after.iter().take(clean - 1).all(|o| *o == Outcome::Failed),
            "{after:?}"
        );
        assert!(
            after
                .iter()
                .skip(clean - 1)
                .all(|o| *o == Outcome::Accepted),
            "{after:?}"
        );
    }

    /// A link that sends, counting what it was given.
    struct Counts(Arc<std::sync::atomic::AtomicUsize>);
    impl Link for Counts {
        fn send(&mut self, packet: &[u8]) -> io::Result<usize> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(packet.len())
        }
    }

    #[test]
    fn a_broken_socket_is_replaced_once_off_the_output_thread() {
        let sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let opened = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let reopen: Reopen = {
            let (sent, opened) = (Arc::clone(&sent), Arc::clone(&opened));
            Box::new(move || {
                opened.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(Counts(Arc::clone(&sent))) as Box<dyn Link>)
            })
        };
        let dropped = Arc::new(std::sync::atomic::AtomicU8::new(0));
        let mut ddp = Ddp::with(
            Some(Box::new(Fails(
                ErrorKind::AddrNotAvailable,
                Arc::clone(&dropped),
            ))),
            reopen,
            Color::Rgb,
        )
        .unwrap();
        let frame = [0; engine_core::ADDRESSES];
        assert_eq!(
            ddp.take(&frame),
            Outcome::Failed,
            "the old socket is broken"
        );
        let started = std::time::Instant::now();
        while ddp.take(&frame) != Outcome::Accepted {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "never replaced"
            ); // guard
            thread::yield_now();
        }
        assert_eq!(
            opened.load(Ordering::SeqCst),
            1,
            "opened once, however many frames failed meanwhile"
        );
        assert!(
            sent.load(Ordering::SeqCst) > 0,
            "the new socket carries the frames"
        );
        drop(ddp);
        let started = std::time::Instant::now();
        while dropped.load(Ordering::SeqCst) == 0 {
            assert!(started.elapsed() < Duration::from_secs(10), "never dropped"); // guard
            thread::yield_now();
        }
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            1,
            "the broken socket is closed and freed on the socket thread"
        );
    }

    /// A socket the system refuses at the start is opened by the socket thread, never by
    /// the engine: frames fail until then, then `CLEAN` frames prove it, as after a
    /// refusal (`docs/engine.md`, each output's states).
    #[test]
    fn an_output_whose_socket_did_not_open_is_opened_by_its_own_thread() {
        let opened = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let reopen: Reopen = {
            let (opened, sent) = (Arc::clone(&opened), Arc::clone(&sent));
            Box::new(move || {
                if opened.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(ErrorKind::NetworkUnreachable.into());
                }
                Ok(Box::new(Counts(Arc::clone(&sent))) as Box<dyn Link>)
            })
        };
        let mut ddp = Ddp::with(None, reopen, Color::Rgb).unwrap();
        let frame = [0; engine_core::ADDRESSES];
        let (mut seen, started) = (vec![], std::time::Instant::now());
        while seen.last() != Some(&Outcome::Accepted) {
            assert!(started.elapsed() < Duration::from_secs(10), "{seen:?}"); // guard
            seen.push(ddp.take(&frame));
            thread::yield_now();
        }
        let failed = seen.iter().filter(|o| **o == Outcome::Failed).count();
        let clean = usize::try_from(CLEAN).unwrap();
        assert!(failed >= clean - 1, "{failed} failed");
        assert_eq!(
            opened.load(Ordering::SeqCst),
            2,
            "one failed open, one retry"
        );
        assert_eq!(
            sent.load(Ordering::SeqCst),
            clean,
            "the proving frames went out"
        );
    }
}
