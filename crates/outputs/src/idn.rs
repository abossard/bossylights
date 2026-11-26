//! IDN (ILDA Digital Network) for lasers over UDP, as `docs/design/outputs-idn.md` reads
//! ILDA's papers. A frame is a route of points, 7 channels each: X coarse and fine, Y coarse
//! and fine, red, green, blue. It goes out as one discrete frame in one datagram; the
//! projector draws it at the output's points a second and repeats it until the next. A
//! laser is dark on doubt: it starts disarmed and sends empty frames until its [`Arm`] is
//! armed, and a failed send disarms it.
//!
//! | Byte | Field |
//! |---|---|
//! | 0 to 3 | Hello: command `40`, client group 0, sequence |
//! | 4 to 5 | total size of the channel message, from byte 4 |
//! | 6 to 7 | content: channel message, config follows, channel 0, chunk type frame |
//! | 8 to 11 | timestamp, µs |
//! | 12 to 15 | config: 4 words of tags, routing, default service, discrete mode |
//! | 16 to 31 | tags: X 16 bits, Y 16 bits, red, green, blue, void |
//! | 32 to 35 | chunk: flags 0, duration in µs (24 bits) |
//! | 36 on | points: X, Y signed, red, green, blue |

use crate::udp::{Link, Protocol, Udp};
use crate::{Device, Limits};
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::num::NonZeroU16;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// The port a projector listens on.
pub const PORT: u16 = 7255;

/// Channels of one point.
const POINT: usize = 7;
const HELLO: usize = 4;
/// Channel message header, config, tags and chunk header.
const MESSAGE_HEAD: usize = 32;
const CHUNK: usize = 4;
/// The largest channel message.
const MESSAGE_MOST: usize = 65_280;
/// The most points in one frame.
const POINTS_MOST: usize = (MESSAGE_MOST - MESSAGE_HEAD) / POINT;
const PACKET_MOST: usize = HELLO + MESSAGE_HEAD + POINTS_MOST * POINT;

const CHANNEL_MESSAGE: u8 = 0x40;
const CLOSE_SESSION: u8 = 0x44;
/// Content: a channel message (`80`), config follows (`40`), channel 0.
const CONTENT: u8 = 0xC0;
const FRAME_CHUNK: u8 = 0x02;
const VOID_CHUNK: u8 = 0x00;
/// 4 words of tags, routing, the default service, discrete mode.
const OPEN: [u8; 4] = [4, 0x01, 0, 0x02];
/// No tags, close, no service, no mode.
const CLOSE: [u8; 4] = [0, 0x02, 0, 0];
/// X 16 bits, Y 16 bits, red 638 nm, green 532 nm, blue 460 nm, void.
const TAGS: [u8; 16] = [
    0x42, 0x00, 0x40, 0x10, 0x42, 0x10, 0x40, 0x10, 0x52, 0x7E, 0x52, 0x14, 0x51, 0xCC, 0x00, 0x00,
];
/// Flips a coordinate's top bit: a channel word, centre 32,768, becomes IDN's signed value.
const SIGN: u8 = 0x80;

/// How many points a second the projector draws: 1,000 to 100,000. Fewer would make the
/// longest frame overflow its 24-bit duration; every projector plays 100,000 (S 3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointRate(u32);

impl PointRate {
    /// 30,000 a second, as ILDA's samples.
    pub const DEFAULT: Self = Self(30_000);

    /// The rate `value`, if it is 1,000 to 100,000.
    pub const fn new(value: u32) -> Option<Self> {
        if value < 1_000 || value > 100_000 {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Its number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Whether a laser may draw. Every pair: the table in `tests` and `outputs-idn.md`.
///
/// ```text
/// [*] --> Disarmed --arm--> Armed --disarm, lost--> Disarmed
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Safety {
    /// Empty frames only.
    Disarmed,
    /// Frames are drawn.
    Armed,
}

/// What changes a laser's safety.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    Arm,
    Disarm,
    /// A send failed: nobody listens, or the socket broke.
    Lost,
}

/// The one transition function of the safety: a calculation.
const fn next_safety(safety: Safety, order: Order) -> Safety {
    match (safety, order) {
        (Safety::Disarmed | Safety::Armed, Order::Arm) => Safety::Armed,
        (Safety::Disarmed | Safety::Armed, Order::Disarm | Order::Lost) => Safety::Disarmed,
    }
}

/// The safety of one output, shared by it and its [`Arm`]: one atomic.
struct Shared(AtomicBool);

impl Shared {
    fn get(&self) -> Safety {
        if self.0.load(Ordering::Relaxed) {
            Safety::Armed
        } else {
            Safety::Disarmed
        }
    }

    fn order(&self, order: Order) {
        let armed = next_safety(self.get(), order) == Safety::Armed;
        self.0.store(armed, Ordering::Relaxed);
    }
}

/// Arms and disarms one IDN output, from any thread. The output starts disarmed and
/// disarms itself when a send fails.
pub struct Arm(Arc<Shared>);

impl Arm {
    /// The next frame is drawn.
    pub fn arm(&self) {
        self.0.order(Order::Arm);
    }

    /// The next frame is empty.
    pub fn disarm(&self) {
        self.0.order(Order::Disarm);
    }

    /// Whether the output draws.
    pub fn armed(&self) -> bool {
        self.0.get() == Safety::Armed
    }
}

/// Opens an IDN output that sends each frame to the projector at `to`, drawn at `points`
/// a second, and its [`Arm`]. Fails on port 0 and on an address that is not one projector.
/// Off the output thread.
pub fn open(to: SocketAddrV4, points: PointRate) -> io::Result<(Box<dyn Device>, Arm)> {
    let ip = to.ip();
    if to.port() == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "no port"));
    }
    if ip.is_broadcast() || ip.is_multicast() || ip.is_unspecified() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "IDN goes to one projector",
        ));
    }
    let shared = Arc::new(Shared(AtomicBool::new(false)));
    let settings = Settings {
        points,
        safety: Arc::clone(&shared),
    };
    let device = Idn::with(socket(to).ok(), Box::new(move || socket(to)), settings)?;
    Ok((Box::new(device), Arm(shared)))
}

/// A connected, non-blocking socket whose send buffer holds the largest message: macOS
/// refuses a datagram over 9,216 bytes by default.
fn socket(to: SocketAddrV4) -> io::Result<Box<dyn Link>> {
    use socket2::{Domain, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(socket2::Protocol::UDP))?;
    socket.set_send_buffer_size(PACKET_MOST)?;
    socket.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0).into())?;
    socket.connect(&to.into())?;
    socket.set_nonblocking(true)?;
    Ok(Box::new(UdpSocket::from(socket)))
}

/// An IDN output: the UDP lane with IDN messages.
type Idn = Udp<Frames>;

/// What an IDN output is set with.
pub(crate) struct Settings {
    points: PointRate,
    safety: Arc<Shared>,
}

/// Where an output's channel is. Every pair: the table in `tests` and `outputs-idn.md`.
///
/// ```text
/// [*] --> Open --terminate--> Closing --frame--> Closed
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channel {
    /// Frames go out.
    Open,
    /// The next frame closes the channel and the session.
    Closing,
    /// Silent for good.
    Closed,
}

/// What happens to a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Frame,
    /// The output is removed or the engine stops.
    Terminate,
}

/// What a frame of the channel sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sends {
    /// One frame message: the points, or an empty frame.
    Frame,
    /// Close the channel, then the session.
    Close,
    Nothing,
}

/// The one transition function of a channel: a calculation.
const fn next(channel: Channel, event: Event) -> (Channel, Sends) {
    match (channel, event) {
        (Channel::Open, Event::Frame) => (Channel::Open, Sends::Frame),
        (Channel::Open | Channel::Closing, Event::Terminate) => (Channel::Closing, Sends::Nothing),
        (Channel::Closing, Event::Frame) => (Channel::Closed, Sends::Close),
        (Channel::Closed, Event::Frame | Event::Terminate) => (Channel::Closed, Sends::Nothing),
    }
}

/// The closing frames a channel still sends.
const fn closing_left(channel: Channel) -> u8 {
    match channel {
        Channel::Closing => 1,
        Channel::Open | Channel::Closed => 0,
    }
}

/// One datagram of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cut {
    /// A frame of this many points.
    Points(usize),
    /// A frame of no points: nothing is drawn.
    Empty,
    CloseChannel,
    CloseSession,
}

/// What a frame message holds, for a laser that is `safety` and a stream of `points`.
const fn shown(safety: Safety, points: usize) -> Cut {
    match safety {
        Safety::Armed if points >= 2 => Cut::Points(points),
        Safety::Armed | Safety::Disarmed => Cut::Empty,
    }
}

/// The frame's duration in µs: the first point is the start, not drawn (S 6.2).
fn duration(points: usize, rate: PointRate) -> u32 {
    let segments = u64::try_from(points.saturating_sub(1)).unwrap_or(u64::MAX);
    u32::try_from(segments * 1_000_000 / u64::from(rate.get())).unwrap_or(u32::MAX)
}

/// Writes the datagram of `cut` into `packet`: its length. `None` cannot happen: the packet
/// holds the largest frame and the stream holds every point of the cut.
fn write(
    packet: &mut [u8],
    cut: Cut,
    sequence: u16,
    timestamp: u32,
    rate: PointRate,
    stream: &[u8],
) -> Option<usize> {
    let [high, low] = sequence.to_be_bytes();
    let time = timestamp.to_be_bytes();
    let points = match cut {
        Cut::Points(points) => points,
        Cut::Empty | Cut::CloseChannel | Cut::CloseSession => 0,
    };
    let [_, d1, d2, d3] = duration(points, rate).to_be_bytes();
    let frame = message_head(MESSAGE_HEAD + points * POINT, FRAME_CHUNK, time)?;
    let close = message_head(MESSAGE_HEAD - TAGS.len() - CHUNK, VOID_CHUNK, time)?;
    let message = [CHANNEL_MESSAGE, 0, high, low];
    let fields: [&[u8]; 5] = match cut {
        Cut::CloseSession => [&[CLOSE_SESSION, 0, high, low], &[], &[], &[], &[]],
        Cut::CloseChannel => [&message, &close, &CLOSE, &[], &[]],
        // The chunk: flags 0, the duration in its low 24 bits.
        Cut::Empty | Cut::Points(_) => [&message, &frame, &OPEN, &TAGS, &[0, d1, d2, d3]],
    };
    let mut length = 0;
    for (byte, value) in packet.iter_mut().zip(fields.into_iter().flatten()) {
        *byte = *value;
        length += 1;
    }
    let samples = packet.get_mut(length..length + points * POINT)?;
    let from = stream.get(..points * POINT)?;
    for (index, (sample, point)) in samples
        .chunks_exact_mut(POINT)
        .zip(from.chunks_exact(POINT))
        .enumerate()
    {
        let [x_high, x_low, y_high, y_low, red, green, blue] = *point.first_chunk()?;
        // The first point is the start: not drawn (S 6.2), and sent dark.
        let [red, green, blue] = if index == 0 {
            [0; 3]
        } else {
            [red, green, blue]
        };
        sample.copy_from_slice(&[x_high ^ SIGN, x_low, y_high ^ SIGN, y_low, red, green, blue]);
    }
    Some(length + points * POINT)
}

/// A channel message header: its total size, content with config following, channel 0,
/// the chunk type, and the timestamp.
fn message_head(size: usize, chunk: u8, timestamp: [u8; 4]) -> Option<[u8; 8]> {
    let [high, low] = u16::try_from(size).ok()?.to_be_bytes();
    let [t0, t1, t2, t3] = timestamp;
    Some([high, low, CONTENT, chunk, t0, t1, t2, t3])
}

/// IDN's messages: the arm, the channel, the sequence and one datagram's buffer. Made at
/// open: a frame allocates nothing.
pub(crate) struct Frames {
    rate: PointRate,
    safety: Arc<Shared>,
    channel: Channel,
    sequence: u16,
    opened: Instant,
    /// The timestamp of this frame's messages.
    timestamp: u32,
    packet: Box<[u8]>,
}

impl Protocol for Frames {
    type Settings = Settings;
    type Cut = Cut;

    fn new(settings: Settings) -> Self {
        Self {
            rate: settings.points,
            safety: settings.safety,
            channel: Channel::Open,
            sequence: 0,
            opened: Instant::now(),
            timestamp: 0,
            packet: vec![0; PACKET_MOST].into_boxed_slice(),
        }
    }

    /// Whole points, at most 9,321: one message.
    fn limits(&self) -> Limits {
        Limits {
            channels: (POINTS_MOST * POINT) as u32,
            whole: NonZeroU16::new(POINT as u16).unwrap_or(NonZeroU16::MIN),
            ..Limits::RAW
        }
    }

    /// Reads the clock: the frame's timestamp.
    fn frame(&mut self, channels: usize) -> impl Iterator<Item = Cut> + use<> {
        let (channel, sends) = next(self.channel, Event::Frame);
        self.channel = channel;
        self.timestamp = micros(self.opened);
        let points = (channels / POINT).min(POINTS_MOST);
        let cuts = match sends {
            Sends::Frame => [Some(shown(self.safety.get(), points)), None],
            Sends::Close => [Some(Cut::CloseChannel), Some(Cut::CloseSession)],
            Sends::Nothing => [None, None],
        };
        cuts.into_iter().flatten()
    }

    fn packet(&mut self, cut: Cut, stream: &[u8]) -> Option<&[u8]> {
        let sequence = self.sequence;
        self.sequence = sequence.wrapping_add(1);
        let length = write(
            &mut self.packet,
            cut,
            sequence,
            self.timestamp,
            self.rate,
            stream,
        )?;
        self.packet.get(..length)
    }

    fn stop(&mut self) -> u8 {
        self.channel = next(self.channel, Event::Terminate).0;
        closing_left(self.channel)
    }

    fn lost(&mut self) {
        self.safety.order(Order::Lost);
    }
}

/// µs since `opened`, wrapping as IDN's timestamp does.
fn micros(opened: Instant) -> u32 {
    // The low 32 bits: IDN's timestamp wraps every 71 minutes.
    opened.elapsed().as_micros() as u32
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
#[expect(
    clippy::indexing_slicing,
    reason = "a test reads fields at the byte the spec names"
)]
mod tests {
    use super::*;

    /// The stream of points given as IDN's signed X, Y and red, green, blue: each
    /// coordinate as the channel word (top bit flipped back), coarse then fine.
    fn stream(points: &[(i16, i16, u8, u8, u8)]) -> Vec<u8> {
        points
            .iter()
            .flat_map(|&(x, y, red, green, blue)| {
                let [x_hi, x_lo] = x.to_be_bytes();
                let [y_hi, y_lo] = y.to_be_bytes();
                [x_hi ^ 0x80, x_lo, y_hi ^ 0x80, y_lo, red, green, blue]
            })
            .collect()
    }

    fn written(cut: Cut, sequence: u16, timestamp: u32, rate: u32, stream: &[u8]) -> Vec<u8> {
        let mut packet = vec![0; PACKET_MOST];
        let rate = PointRate::new(rate).unwrap();
        let length = write(&mut packet, cut, sequence, timestamp, rate, stream).unwrap();
        packet[..length].to_vec()
    }

    #[test]
    fn a_frame_is_what_the_spec_table_says() {
        // Worked by hand from outputs-idn.md and DexLogic's MIT sender (idn-C-idtfPlayer,
        // main.c): 3 points at 30,000 a second, sequence 0x0102, timestamp 0x0A0B0C0D.
        // Channel message 53 bytes; duration 2 / 30,000 s = 66 µs. The first point's
        // colour 1, 2, 3 goes out as 0.
        let points = [
            (-32_768, 32_767, 1, 2, 3),
            (0, 0, 255, 128, 1),
            (32_767, -1, 10, 20, 30),
        ];
        let expected = [
            0x40, 0x00, 0x01, 0x02, // Hello
            0x00, 0x35, 0xC0, 0x02, 0x0A, 0x0B, 0x0C, 0x0D, // channel message
            0x04, 0x01, 0x00, 0x02, // config
            0x42, 0x00, 0x40, 0x10, 0x42, 0x10, 0x40, 0x10, 0x52, 0x7E, 0x52, 0x14, 0x51, 0xCC,
            0x00, 0x00, // tags
            0x00, 0x00, 0x00, 0x42, // chunk
            0x80, 0x00, 0x7F, 0xFF, 0x00, 0x00, 0x00, // start point, dark
            0x00, 0x00, 0x00, 0x00, 0xFF, 0x80, 0x01, //
            0x7F, 0xFF, 0xFF, 0xFF, 0x0A, 0x14, 0x1E,
        ];
        let found = written(
            Cut::Points(3),
            0x0102,
            0x0A0B_0C0D,
            30_000,
            &stream(&points),
        );
        assert_eq!(found, expected);
    }

    #[test]
    fn an_empty_frame_and_the_close_are_what_the_spec_says() {
        // (cut, sequence, timestamp, datagram): outputs-idn.md, by hand
        let mut empty = vec![0x40, 0x00, 0xFF, 0xFF, 0x00, 0x20, 0xC0, 0x02, 0, 0, 0, 9];
        empty.extend([0x04, 0x01, 0x00, 0x02]);
        empty.extend(TAGS);
        empty.extend([0, 0, 0, 0]);
        let cases = [
            (Cut::Empty, 0xFFFF, 9, empty),
            (
                Cut::CloseChannel,
                7,
                0x0102_0304,
                vec![
                    0x40, 0x00, 0x00, 0x07, 0x00, 0x0C, 0xC0, 0x00, 0x01, 0x02, 0x03, 0x04, 0x00,
                    0x02, 0x00, 0x00,
                ],
            ),
            (
                Cut::CloseSession,
                8,
                0x0102_0304,
                vec![0x44, 0x00, 0x00, 0x08],
            ),
        ];
        for (cut, sequence, timestamp, expected) in cases {
            let points = stream(&[(1, 2, 3, 4, 5), (6, 7, 8, 9, 10)]);
            assert_eq!(
                written(cut, sequence, timestamp, 30_000, &points),
                expected,
                "{cut:?}"
            );
        }
    }

    #[test]
    fn x_and_y_are_the_word_with_its_top_bit_flipped() {
        // (channel word, IDN's signed value): protocols.md section 4
        let cases = [
            (0x0000_u16, -32_768_i16),
            (0x0001, -32_767),
            (0x7FFF, -1),
            (0x8000, 0),
            (0x8001, 1),
            (0xFFFF, 32_767),
        ];
        for (word, expected) in cases {
            let [hi, lo] = word.to_be_bytes();
            let point = [hi, lo, lo, hi, 0, 0, 0];
            let frame = [point, point].concat();
            let found = written(Cut::Points(2), 0, 0, 30_000, &frame);
            let second = &found[HELLO + MESSAGE_HEAD + POINT..];
            assert_eq!(
                i16::from_be_bytes([second[0], second[1]]),
                expected,
                "x {word:#06x}"
            );
            let flipped = u16::from_be_bytes([lo, hi]) ^ 0x8000;
            assert_eq!(second[2..4], flipped.to_be_bytes(), "y {word:#06x}");
        }
    }

    #[test]
    fn the_first_point_is_dark_and_the_others_keep_their_colour() {
        let points = stream(&[(0, 0, 9, 8, 7), (1, 1, 6, 5, 4), (2, 2, 3, 2, 1)]);
        let found = written(Cut::Points(3), 0, 0, 30_000, &points);
        // (point, red, green, blue)
        let cases = [(0, [0, 0, 0]), (1, [6, 5, 4]), (2, [3, 2, 1])];
        for (point, colour) in cases {
            let at = HELLO + MESSAGE_HEAD + point * POINT + 4;
            assert_eq!(found[at..at + 3], colour, "point {point}");
        }
    }

    #[test]
    fn the_duration_is_the_segments_at_the_point_rate() {
        // (points, points a second, µs): (points - 1) / rate, rounded down
        let cases = [
            (0, 30_000, 0),
            (1, 30_000, 0),
            (2, 30_000, 33),
            (3, 30_000, 66),
            (500, 30_000, 16_633),
            (101, 100_000, 1_000),
            (9_321, 1_000, 9_320_000),
        ];
        for (points, rate, expected) in cases {
            let rate = PointRate::new(rate).unwrap();
            assert_eq!(duration(points, rate), expected, "{points} at {rate:?}");
        }
        // The longest frame at the slowest rate fits the 24-bit field.
        assert!(duration(POINTS_MOST, PointRate::new(1_000).unwrap()) < 1 << 24);
    }

    #[test]
    fn a_point_rate_is_1000_to_100000() {
        // (value, a rate)
        let cases = [
            (0, false),
            (999, false),
            (1_000, true),
            (30_000, true),
            (100_000, true),
            (100_001, false),
            (u32::MAX, false),
        ];
        for (value, valid) in cases {
            assert_eq!(
                PointRate::new(value).map(PointRate::get),
                valid.then_some(value),
                "{value}"
            );
        }
        assert_eq!(PointRate::DEFAULT.get(), 30_000);
    }

    #[test]
    fn a_frame_holds_at_most_9321_points_in_whole_points() {
        // 32 bytes of header, config, tags and chunk, 7 a point, at most 65,280: S 2.1.
        assert_eq!(POINTS_MOST, 9_321);
        // The largest datagram: Hello 4, head 32, 9,321 points of 7.
        assert_eq!(PACKET_MOST, 65_283);
        let frames = Frames::new(Settings {
            points: PointRate::DEFAULT,
            safety: Arc::new(Shared(AtomicBool::new(false))),
        });
        let limits = frames.limits();
        assert_eq!(limits.channels, 65_247);
        assert_eq!(limits.whole.get(), 7);
        assert_eq!(limits.rate, Limits::RAW.rate);
    }

    #[test]
    fn every_state_and_order_of_the_safety() {
        use Order::{Arm, Disarm, Lost};
        use Safety::{Armed, Disarmed};
        // (safety, order, safety after)
        let cases = [
            (Disarmed, Arm, Armed),
            (Disarmed, Disarm, Disarmed),
            (Disarmed, Lost, Disarmed),
            (Armed, Arm, Armed),
            (Armed, Disarm, Disarmed),
            (Armed, Lost, Disarmed),
        ];
        for (safety, order, after) in cases {
            assert_eq!(next_safety(safety, order), after, "{safety:?} {order:?}");
            let shared = Shared(AtomicBool::new(safety == Armed));
            shared.order(order);
            assert_eq!(shared.get(), after, "shared: {safety:?} {order:?}");
        }
    }

    #[test]
    fn a_frame_is_drawn_only_armed_and_with_two_points() {
        // (safety, points in the stream, the frame message): outputs-idn.md, Laser safety
        let cases = [
            (Safety::Disarmed, 0, Cut::Empty),
            (Safety::Disarmed, 3, Cut::Empty),
            (Safety::Disarmed, POINTS_MOST, Cut::Empty),
            (Safety::Armed, 0, Cut::Empty),
            (Safety::Armed, 1, Cut::Empty),
            (Safety::Armed, 2, Cut::Points(2)),
            (Safety::Armed, POINTS_MOST, Cut::Points(POINTS_MOST)),
        ];
        for (safety, points, expected) in cases {
            assert_eq!(shown(safety, points), expected, "{safety:?} {points}");
        }
    }

    #[test]
    fn the_timestamp_counts_microseconds_since_open() {
        let Some(opened) = Instant::now().checked_sub(std::time::Duration::from_secs(2)) else {
            return; // a clock younger than 2 s: nothing to compare
        };
        assert!(micros(opened) >= 2_000_000);
        // Each message carries the time of its frame.
        let mut frames = Frames::new(Settings {
            points: PointRate::DEFAULT,
            safety: Arc::new(Shared(AtomicBool::new(false))),
        });
        frames.opened = opened;
        let cuts: Vec<_> = frames.frame(0).collect();
        for cut in cuts {
            let datagram = frames.packet(cut, &[]).unwrap();
            let timestamp = u32::from_be_bytes(datagram[8..12].try_into().unwrap());
            assert!(timestamp >= 2_000_000, "{cut:?}");
        }
    }

    #[test]
    fn every_state_and_event_of_a_channel() {
        use Channel::{Closed, Closing, Open};
        use Event::{Frame, Terminate};
        // (channel, event, channel after, what the frame sends, closing frames left)
        let cases = [
            (Open, Frame, Open, Sends::Frame, 0),
            (Open, Terminate, Closing, Sends::Nothing, 1),
            (Closing, Frame, Closed, Sends::Close, 0),
            (Closing, Terminate, Closing, Sends::Nothing, 1),
            (Closed, Frame, Closed, Sends::Nothing, 0),
            (Closed, Terminate, Closed, Sends::Nothing, 0),
        ];
        for (channel, event, after, sends, left) in cases {
            assert_eq!(
                next(channel, event),
                (after, sends),
                "{channel:?} {event:?}"
            );
            assert_eq!(closing_left(after), left, "{channel:?} {event:?}");
        }
    }

    /// A link that records every datagram, and refuses the sends whose number (from 1)
    /// is listed.
    struct Recording {
        refuse: Vec<usize>,
        sent: rtrb::Producer<Vec<u8>>,
        tried: usize,
    }
    impl Link for Recording {
        fn send(&mut self, packet: &[u8]) -> io::Result<usize> {
            self.tried += 1;
            if self.refuse.contains(&self.tried) {
                return Err(io::ErrorKind::ConnectionRefused.into());
            }
            self.sent.push(packet.to_vec()).unwrap();
            Ok(packet.len())
        }
    }

    /// An IDN output on a recording link, and its arm.
    fn recorded(refuse: Vec<usize>) -> (Idn, Arm, rtrb::Consumer<Vec<u8>>) {
        let (give, sent) = rtrb::RingBuffer::new(16);
        let link = Recording {
            refuse,
            sent: give,
            tried: 0,
        };
        let shared = Arc::new(Shared(AtomicBool::new(false)));
        let settings = Settings {
            points: PointRate::DEFAULT,
            safety: Arc::clone(&shared),
        };
        let reopen: crate::udp::Reopen = Box::new(|| Err(io::Error::other("not needed")));
        let idn = Idn::with(Some(Box::new(link)), reopen, settings).unwrap();
        (idn, Arm(shared), sent)
    }

    /// Samples in each datagram sent so far: `None` for a close.
    fn samples(sent: &mut rtrb::Consumer<Vec<u8>>) -> Vec<Option<usize>> {
        std::iter::from_fn(|| sent.pop().ok())
            .map(|datagram| {
                (datagram[0] == 0x40 && datagram[7] == 0x02)
                    .then(|| (datagram.len() - HELLO - MESSAGE_HEAD) / POINT)
            })
            .collect()
    }

    #[test]
    fn a_failed_send_disarms_until_armed_again() {
        use crate::Outcome;
        let points = stream(&[(0, 0, 1, 1, 1), (1, 1, 1, 1, 1), (2, 2, 1, 1, 1)]);
        let (mut idn, arm, mut sent) = recorded(vec![3]);
        // (step, what the engine is told, armed after, samples sent)
        assert_eq!(idn.take(&points), Outcome::Accepted, "opened");
        assert!(!arm.armed(), "opened");
        assert_eq!(samples(&mut sent), [Some(0)], "opened: empty");
        arm.arm();
        assert!(arm.armed(), "armed");
        assert_eq!(idn.take(&points), Outcome::Accepted, "armed");
        assert_eq!(samples(&mut sent), [Some(3)], "armed: drawn");
        assert_eq!(idn.take(&points), Outcome::Failed, "refused");
        assert!(!arm.armed(), "refused: disarmed");
        assert_eq!(idn.take(&points), Outcome::Failed, "back, proving");
        assert_eq!(samples(&mut sent), [Some(0)], "back: empty");
        arm.arm();
        idn.take(&points);
        assert_eq!(samples(&mut sent), [Some(3)], "armed again: drawn");
        arm.disarm();
        idn.take(&points);
        assert_eq!(samples(&mut sent), [Some(0)], "disarmed: empty");
    }

    #[test]
    fn terminate_closes_the_channel_then_the_session_then_nothing() {
        let (mut idn, arm, mut sent) = recorded(vec![]);
        arm.arm();
        let points = stream(&[(0, 0, 1, 1, 1), (1, 1, 1, 1, 1)]);
        idn.take(&points);
        idn.terminate();
        assert_eq!(
            idn.take(&points),
            crate::Outcome::Accepted,
            "closed: nothing failed"
        );
        idn.terminate();
        let commands: Vec<_> = std::iter::from_fn(|| sent.pop().ok())
            .map(|datagram| (datagram[0], datagram.get(7).copied(), datagram.len()))
            .collect();
        assert_eq!(
            commands,
            [
                (0x40, Some(0x02), HELLO + MESSAGE_HEAD + 2 * POINT),
                (0x40, Some(0x00), 16),
                (0x44, None, 4)
            ]
        );
    }

    #[test]
    fn the_sequence_counts_every_datagram_and_wraps() {
        let mut frames = Frames::new(Settings {
            points: PointRate::DEFAULT,
            safety: Arc::new(Shared(AtomicBool::new(false))),
        });
        frames.sequence = 0xFFFE;
        let mut sequences = vec![];
        for closing in [false, false, true] {
            if closing {
                frames.stop();
            }
            let cuts: Vec<_> = frames.frame(0).collect();
            for cut in cuts {
                let datagram = frames.packet(cut, &[]).unwrap();
                sequences.push(u16::from_be_bytes([datagram[2], datagram[3]]));
            }
        }
        assert_eq!(sequences, [0xFFFE, 0xFFFF, 0, 1]);
    }
}
