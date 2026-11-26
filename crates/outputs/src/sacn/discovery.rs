//! Universe discovery: one thread per engine tells the network which universes its
//! multicast sACN outputs send, as `docs/design/outputs-sacn.md` reads ETC's library. Each
//! output stores what it sent into its place in a table of atomics every frame, so the
//! output thread never waits; the discovery thread reads the table and sends the list.
//!
//! The send machine, every cell walked by a test:
//!
//! | state \ tick | before `until` | due, list empty | due, list has universes |
//! |---|---|---|---|
//! | Due | (always due) | Due, nothing sent | Waiting{now + 10 s}, pages sent |
//! | Waiting{until} | Waiting{until} | Due, nothing sent | Waiting{now + 10 s}, pages sent |
//!
//! | Byte | Discovery packet field |
//! |---|---|
//! | 0 to 15 | preamble, as a data packet |
//! | 16 to 17 | root flags and length |
//! | 18 to 21 | root vector 8: extended |
//! | 22 to 37 | CID |
//! | 38 to 39 | framing flags and length |
//! | 40 to 43 | framing vector 2: universe discovery |
//! | 44 to 107 | source name, zero-padded |
//! | 108 to 111 | reserved, zeros |
//! | 112 to 113 | discovery flags and length |
//! | 114 to 117 | discovery vector 1: universe list |
//! | 118 | page, from 0 |
//! | 119 | last page |
//! | 120 on | universes, 2 bytes each, ascending, at most 512 |

use super::{
    Destination, FRAMING_AT, NAME, PORT, PREAMBLE, ROOT_AT, Universe, flags, opened, padded,
};
use crate::Device;
use engine_core::{MAX_OUTPUTS, OutputNumber};
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Where discovery goes on a network: universe 64,214's group, on port 5568.
pub const DISCOVERY: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(239, 255, 250, 214), PORT);

/// How often the list goes out.
const EVERY: Duration = Duration::from_secs(10);
/// How often the thread looks at the table while nothing is listed.
const LOOK: Duration = Duration::from_millis(100);
/// The most universes on one page.
const PAGE: usize = 512;
const HEADER: usize = 120;
const DISCOVERY_AT: usize = 112;
const ROOT_VECTOR: [u8; 4] = [0, 0, 0, 8];
const FRAMING_VECTOR: [u8; 4] = [0, 0, 0, 2];
const RESERVED: [u8; 4] = [0; 4];
const LIST_VECTOR: [u8; 4] = [0, 0, 0, 1];
/// The name of the discovery thread.
const THREAD: &str = "sacn-discovery";

/// Tells the network, while it lives, which universes the outputs it opened send. One per
/// engine: the engine is one source, with one CID.
pub struct Discovery {
    shared: Arc<Shared>,
    cid: [u8; 16],
    thread: Option<JoinHandle<()>>,
}

impl Discovery {
    /// Sends the list to `to` ([`DISCOVERY`] on a network) in packets named `Bossylights`
    /// with `cid`: first as soon as an output sends a universe, then every 10 s. Off the
    /// output thread.
    pub fn start(cid: [u8; 16], to: SocketAddrV4) -> io::Result<Self> {
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            table: Default::default(),
        });
        let thread = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name(THREAD.into())
                .spawn(move || run(&socket, to, &shared, cid))?
        };
        Ok(Self {
            shared,
            cid,
            thread: Some(thread),
        })
    }

    /// Opens an sACN output as [`super::open`] does, with this engine's CID. A multicast
    /// output is listed while it sends; a unicast one never. Off the output thread.
    pub fn open(
        &self,
        to: Destination,
        first: Universe,
        output: OutputNumber,
    ) -> io::Result<Box<dyn Device>> {
        let announce = match to {
            Destination::Multicast(_) => Some(Slot {
                shared: Arc::clone(&self.shared),
                at: output.slot(),
            }),
            Destination::Unicast(_) => None,
        };
        opened(to, first, self.cid, output, announce)
    }
}

impl Drop for Discovery {
    /// Stops the thread at once.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

struct Shared {
    stop: AtomicBool,
    /// Per output: its first universe in the high half, how many it sends in the low.
    table: [AtomicU32; MAX_OUTPUTS],
}

/// An output's place in the table. Cleared when the output is dropped.
pub(super) struct Slot {
    shared: Arc<Shared>,
    at: usize,
}

impl Slot {
    /// The output sends `count` universes from `first`. On the output thread: one store.
    pub(super) fn set(&self, first: Universe, count: usize) {
        if let Some(place) = self.shared.table.get(self.at) {
            place.store(entry(first, count), Ordering::Relaxed);
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(place) = self.shared.table.get(self.at) {
            place.store(0, Ordering::Relaxed);
        }
    }
}

/// One place of the table: `first` high, `count` low; 0 when nothing is sent.
fn entry(first: Universe, count: usize) -> u32 {
    if count == 0 {
        0
    } else {
        (u32::from(first.get()) << 16) + (count as u32 & 0xFFFF)
    }
}

/// Every universe the table holds, ascending, each once.
fn listed(entries: impl IntoIterator<Item = u32>) -> Vec<u16> {
    let mut universes: Vec<u16> = entries
        .into_iter()
        .flat_map(|entry| {
            let first = (entry >> 16) as u16;
            let count = (entry & 0xFFFF) as u16;
            (0..count).map(move |index| first.saturating_add(index))
        })
        .collect();
    universes.sort_unstable();
    universes.dedup();
    universes
}

/// The packets that list `universes`: pages of 512, from page 0, each naming the last.
fn pages(cid: &[u8; 16], name: &[u8; NAME], universes: &[u16]) -> Vec<Vec<u8>> {
    let last = u8::try_from(universes.len().saturating_sub(1) / PAGE).unwrap_or(u8::MAX);
    universes
        .chunks(PAGE)
        .zip(0..=u8::MAX)
        .map(|(list, page)| packet(cid, name, page, last, list))
        .collect()
}

/// One discovery packet: page `page` of `last`, listing `universes`.
fn packet(cid: &[u8; 16], name: &[u8; NAME], page: u8, last: u8, universes: &[u16]) -> Vec<u8> {
    let end = HEADER + 2 * universes.len();
    let fields: [&[u8]; 12] = [
        &PREAMBLE,
        &flags(end - ROOT_AT),
        &ROOT_VECTOR,
        cid,
        &flags(end - FRAMING_AT),
        &FRAMING_VECTOR,
        name,
        &RESERVED,
        &flags(end - DISCOVERY_AT),
        &LIST_VECTOR,
        &[page],
        &[last],
    ];
    let mut packet: Vec<u8> = fields.into_iter().flatten().copied().collect();
    packet.extend(universes.iter().flat_map(|universe| universe.to_be_bytes()));
    packet
}

/// Where the sender is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sender {
    /// The list goes out as soon as it holds a universe.
    Due,
    /// The list went out; it goes again at `until`.
    Waiting { until: Instant },
}

/// The one transition function of the sender: whether it sends the list at `now`.
fn next(sender: Sender, now: Instant, empty: bool) -> (Sender, bool) {
    match sender {
        Sender::Waiting { until } if now < until => (sender, false),
        Sender::Due | Sender::Waiting { .. } if empty => (Sender::Due, false),
        Sender::Due | Sender::Waiting { .. } => (Sender::Waiting { until: now + EVERY }, true),
    }
}

/// How long the thread sleeps at `now`.
fn wait(sender: Sender, now: Instant) -> Duration {
    match sender {
        Sender::Due => LOOK,
        Sender::Waiting { until } => until.saturating_duration_since(now),
    }
}

/// The discovery thread.
fn run(socket: &UdpSocket, to: SocketAddrV4, shared: &Shared, cid: [u8; 16]) {
    let name = padded("Bossylights");
    let mut sender = Sender::Due;
    while !shared.stop.load(Ordering::Acquire) {
        let universes = listed(
            shared
                .table
                .iter()
                .map(|place| place.load(Ordering::Relaxed)),
        );
        let now = Instant::now();
        let (after, send) = next(sender, now, universes.is_empty());
        sender = after;
        if send {
            for page in pages(&cid, &name, &universes) {
                let _ = socket.send_to(&page, to);
            }
        }
        thread::park_timeout(wait(sender, now));
    }
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

    fn universe(value: u16) -> Universe {
        Universe::new(value).unwrap()
    }

    #[test]
    fn a_discovery_packet_is_what_the_spec_table_says() {
        // Page 0 of 0, universe 0x1234: worked by hand from outputs-sacn.md. 122 bytes:
        // root length 106, framing 84, discovery 10.
        let mut expected = vec![
            0x00, 0x10, 0x00, 0x00, 0x41, 0x53, 0x43, 0x2D, 0x45, 0x31, 0x2E, 0x31, 0x37, 0x00,
            0x00, 0x00, 0x70, 0x6A, 0x00, 0x00, 0x00, 0x08,
        ];
        expected.extend(CID);
        expected.extend([0x70, 0x54, 0x00, 0x00, 0x00, 0x02]);
        let mut name = b"Bossylights".to_vec();
        name.resize(64, 0);
        expected.extend(name);
        expected.extend([
            0x00, 0x00, 0x00, 0x00, 0x70, 0x0A, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x12, 0x34,
        ]);
        assert_eq!(
            packet(&CID, &padded("Bossylights"), 0, 0, &[0x1234]),
            expected
        );
    }

    #[test]
    fn the_lengths_follow_the_universes() {
        // (universes, length, bytes 16-17, 38-39, 112-113)
        let cases = [
            (1, 122, [0x70, 0x6A], [0x70, 0x54], [0x70, 0x0A]),
            (2, 124, [0x70, 0x6C], [0x70, 0x56], [0x70, 0x0C]),
            (512, 1_144, [0x74, 0x68], [0x74, 0x52], [0x74, 0x08]),
        ];
        for (count, length, root, framing, list) in cases {
            let universes: Vec<u16> = (1..=count).collect();
            let found = packet(&CID, &padded("Bossylights"), 2, 3, &universes);
            assert_eq!(found.len(), length, "{count}");
            assert_eq!(found[16..18], root, "{count}");
            assert_eq!(found[38..40], framing, "{count}");
            assert_eq!(found[112..114], list, "{count}");
            assert_eq!(found[118..120], [2, 3], "page and last page");
            assert_eq!(found[120..122], [0, 1], "first universe");
            assert_eq!(found[length - 2..], count.to_be_bytes(), "last universe");
        }
    }

    #[test]
    fn universes_are_paged_by_512() {
        // (universes, pages: page, last page, first universe, how many)
        let cases = [
            (0, vec![]),
            (1, vec![(0, 0, 1, 1)]),
            (512, vec![(0, 0, 1, 512)]),
            (513, vec![(0, 1, 1, 512), (1, 1, 513, 1)]),
            (
                2_048,
                vec![
                    (0, 3, 1, 512),
                    (1, 3, 513, 512),
                    (2, 3, 1_025, 512),
                    (3, 3, 1_537, 512),
                ],
            ),
        ];
        for (count, expected) in cases {
            let universes: Vec<u16> = (1..=count).collect();
            let found: Vec<_> = pages(&CID, &padded("Bossylights"), &universes)
                .iter()
                .map(|p| {
                    let first = u16::from_be_bytes([p[120], p[121]]);
                    (p[118], p[119], first, (p.len() - HEADER) / 2)
                })
                .collect();
            assert_eq!(found, expected, "{count}");
        }
    }

    #[test]
    fn every_state_and_tick_of_the_sender() {
        let now = Instant::now();
        let later = now + Duration::from_secs(1);
        let waiting = |until| Sender::Waiting { until };
        // ETC's kSacnUniverseDiscoveryInterval: 10 s.
        let sent = waiting(now + Duration::from_secs(10));
        // (sender, list empty, sender after, sends): one case per cell of the module table.
        let cases = [
            (Sender::Due, true, Sender::Due, false),
            (Sender::Due, false, sent, true),
            (waiting(later), true, waiting(later), false),
            (waiting(later), false, waiting(later), false),
            (waiting(now), true, Sender::Due, false),
            (waiting(now), false, sent, true),
        ];
        for (sender, empty, after, sends) in cases {
            assert_eq!(
                next(sender, now, empty),
                (after, sends),
                "{sender:?} {empty}"
            );
        }
    }

    #[test]
    fn the_thread_sleeps_until_the_next_send() {
        let now = Instant::now();
        let ahead = Duration::from_secs(3);
        // (sender, sleep)
        let cases = [
            (Sender::Due, LOOK),
            (Sender::Waiting { until: now + ahead }, ahead),
            (Sender::Waiting { until: now }, Duration::ZERO),
        ];
        for (sender, expected) in cases {
            assert_eq!(wait(sender, now), expected, "{sender:?}");
        }
    }

    #[test]
    fn the_list_is_ascending_and_each_universe_once() {
        // (outputs: first, count; list)
        type Outputs<'a> = &'a [(u16, usize)];
        let cases: [(Outputs, Vec<u16>); 6] = [
            (&[], vec![]),
            (&[(5, 0)], vec![]),
            (&[(7, 2), (1, 3)], vec![1, 2, 3, 7, 8]),
            (&[(1, 3), (2, 3)], vec![1, 2, 3, 4]),
            (&[(63_999, 1)], vec![63_999]),
            (&[(63_872, 128)], (63_872..=63_999).collect()),
        ];
        for (outputs, expected) in cases {
            let entries = outputs.iter().map(|&(f, c)| entry(universe(f), c));
            assert_eq!(listed(entries), expected, "{outputs:?}");
        }
    }

    #[test]
    fn an_entry_holds_the_first_universe_and_the_count() {
        // (first, count, entry)
        let cases = [
            (1, 0, 0),
            (1, 1, 0x0001_0001),
            (0x1234, 128, 0x1234_0080),
            (63_999, 1, 0xF9FF_0001),
        ];
        for (first, count, expected) in cases {
            assert_eq!(entry(universe(first), count), expected, "{first} {count}");
        }
    }

    #[test]
    fn discovery_goes_to_universe_64214() {
        // ETC: the group of universe u is 0xEFFF0000 | u.
        assert_eq!(*DISCOVERY.ip(), Ipv4Addr::from(0xEFFF_0000_u32 | 64_214));
        assert_eq!(DISCOVERY.port(), 5568);
    }

    #[test]
    fn a_slot_is_cleared_when_its_output_goes() {
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            table: Default::default(),
        });
        let read = |at: usize| shared.table[at].load(Ordering::Relaxed);
        let slot = Slot {
            shared: Arc::clone(&shared),
            at: 15,
        };
        slot.set(universe(3), 2);
        assert_eq!(read(15), 0x0003_0002);
        assert_eq!(read(14), 0, "only its own place");
        drop(slot);
        assert_eq!(read(15), 0);
    }
}
