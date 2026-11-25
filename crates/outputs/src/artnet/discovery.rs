//! Finding the nodes: one thread off the tick polls every 2.5 s, answers every poll and
//! keeps the node table. An output it opens sends each universe to every node that lists
//! its Port-Address, and only there. The output thread reads the subscribers from a
//! snapshot the discovery thread made; it hands the old one back to be freed there.
//!
//! One feed per output link, on the discovery thread:
//!
//! | feed \ event | snapshot changed | `fresh` has room | link dropped |
//! |---|---|---|---|
//! | Current | Behind | Current | Dropped |
//! | Behind | Behind | Current, the newest pushed | Dropped |
//! | Dropped | Dropped | Dropped | Dropped |

#![expect(
    clippy::disallowed_types,
    reason = "the board is locked by the discovery thread, the API and the socket thread, never the output thread"
)]

use super::nodes::{Node, Nodes, Subscriber};
use super::poll::{self, Heard};
use super::{PortAddress, Universes};
use crate::Device;
use crate::udp::{Link, Udp};
use engine_core::MAX_OUTPUTS;
use nix::net::if_::InterfaceFlags;
use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// How often we poll: the spec asks for 2.5 to 3 s; a node is gone after 3.
const POLL_EVERY: Duration = Duration::from_millis(2_500);
/// How long the thread listens before it looks at the clock again.
const LISTEN: Duration = Duration::from_millis(100);
/// The name of the discovery thread.
const THREAD: &str = "artnet-discovery";

/// The subscribers at one moment, sorted by Port-Address.
#[derive(Debug, Default, PartialEq, Eq)]
struct Snapshot(Vec<Subscriber>);

/// Finds the Art-Net nodes on the network while it lives. One per engine: two of our
/// sockets on one port would split the replies between them.
pub struct Discovery {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Discovery {
    /// Listens on `on`, shared with other programs, and polls each of `polls` every 2.5 s.
    /// Replies and ArtDmx go to a peer at `port`: 6454 by the spec, both ways. Fails when
    /// `on` is held by a program that does not share it: send to a typed address then.
    /// Off the output thread.
    pub fn start(on: SocketAddrV4, port: u16, polls: Vec<Ipv4Addr>) -> io::Result<Self> {
        let socket = listen(on)?;
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            port,
            board: Mutex::new(Board::default()),
        });
        let thread = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name(THREAD.into())
                .spawn(move || run(&socket, &shared, &polls))?
        };
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Opens an Art-Net output that sends every universe of its route, the first to
    /// `first`, to each node that lists it. Nobody lists it: nothing is sent. Off the
    /// output thread.
    pub fn open(&self, first: PortAddress) -> io::Result<Box<dyn Device>> {
        let shared = Arc::clone(&self.shared);
        let output = Udp::<Universes>::with(
            self.shared.found().ok(),
            Box::new(move || shared.found()),
            first,
        )?;
        Ok(Box::new(output))
    }

    /// Every node the network told of, Seen or Gone, ordered by IP and BindIndex. Our own
    /// reply to our own poll is among them.
    pub fn nodes(&self) -> Vec<Node> {
        self.shared.board().nodes.clone()
    }
}

impl Drop for Discovery {
    /// Stops the thread; it ends within 100 ms and frees the port.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The directed broadcast of each IPv4 interface that is up, loopback aside: where an
/// ArtPoll goes.
pub fn broadcasts() -> Vec<Ipv4Addr> {
    let Ok(interfaces) = nix::ifaddrs::getifaddrs() else {
        return vec![];
    };
    let mut found: Vec<_> = interfaces
        .filter_map(|i| {
            let ipv4 = |a: Option<nix::sys::socket::SockaddrStorage>| {
                a.and_then(|a| a.as_sockaddr_in().map(|a| Ipv4Addr::from(a.ip())))
            };
            poll_target(i.flags, ipv4(i.address), ipv4(i.netmask))
        })
        .collect();
    found.sort_unstable();
    found.dedup();
    found
}

/// Where an interface's ArtPoll goes: the directed broadcast of an IPv4 interface that is
/// up and can broadcast, loopback aside.
fn poll_target(
    flags: InterfaceFlags,
    address: Option<Ipv4Addr>,
    mask: Option<Ipv4Addr>,
) -> Option<Ipv4Addr> {
    let wanted = flags.contains(InterfaceFlags::IFF_UP | InterfaceFlags::IFF_BROADCAST)
        && !flags.contains(InterfaceFlags::IFF_LOOPBACK);
    wanted.then_some(poll::directed(address?, mask?))
}

struct Shared {
    stop: AtomicBool,
    /// Where peers listen.
    port: u16,
    board: Mutex<Board>,
}

/// What the discovery thread tells the others.
#[derive(Default)]
struct Board {
    nodes: Vec<Node>,
    /// The newest subscribers: a new feed starts with them.
    current: Vec<Subscriber>,
    /// Feeds made since the thread last looked.
    joining: Vec<Feed>,
}

impl Shared {
    fn board(&self) -> MutexGuard<'_, Board> {
        self.board.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A new link for an output: its own non-blocking socket and a feed that starts with
    /// the newest subscribers. Off the output thread.
    fn found(&self) -> io::Result<Box<dyn Link>> {
        let socket = link_socket()?;
        let (fresh, taken) = rtrb::RingBuffer::new(1);
        let (handed, spent) = rtrb::RingBuffer::new(1);
        let mut board = self.board();
        let now = Box::new(Snapshot(board.current.clone()));
        board.joining.push(Feed {
            fresh,
            spent,
            state: FeedState::Current,
        });
        Ok(Box::new(Found {
            socket,
            now,
            fresh: taken,
            spent: handed,
        }))
    }
}

/// The discovery socket on `on`: shared with other programs, may broadcast, listens
/// [`LISTEN`] at a time.
fn listen(on: SocketAddrV4) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_reuse_port(true)?;
    socket.set_broadcast(true)?;
    socket.bind(&SocketAddr::V4(on).into())?;
    let socket = UdpSocket::from(socket);
    socket.set_read_timeout(Some(LISTEN))?;
    Ok(socket)
}

/// Whether to poll at `now`, and when next.
fn poll_due(now: Instant, next: Instant) -> (bool, Instant) {
    if now >= next {
        (true, now + POLL_EVERY)
    } else {
        (false, next)
    }
}

/// The socket a link sends from: any port, never waits.
fn link_socket() -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_nonblocking(true)?;
    Ok(socket)
}

/// The discovery thread.
fn run(socket: &UdpSocket, shared: &Shared, polls: &[Ipv4Addr]) {
    let mut nodes = Nodes::default();
    // A feed per output and one spare for a reopen: taking in a new one allocates nothing.
    let mut feeds: Vec<Feed> = Vec::with_capacity(MAX_OUTPUTS + 1);
    let mut current: Vec<Subscriber> = vec![];
    let mut next_poll = Instant::now();
    let mut packet = [0; 1_024];
    while !shared.stop.load(Ordering::Acquire) {
        let (due, next) = poll_due(Instant::now(), next_poll);
        next_poll = next;
        if due {
            for ip in polls {
                let _ = socket.send_to(&poll::POLL, SocketAddrV4::new(*ip, shared.port));
            }
        }
        let mut heard = false;
        if let Ok((size, SocketAddr::V4(from))) = socket.recv_from(&mut packet) {
            match poll::read(packet.get(..size).unwrap_or_default()) {
                Heard::Poll => answer(socket, *from.ip(), shared.port),
                Heard::Reply(reply) => {
                    nodes.heard(reply, Instant::now());
                    heard = true;
                }
                Heard::Nothing => {}
            }
        }
        let went = nodes.expire(Instant::now());
        let mut changed = false;
        let mut board = shared.board();
        if heard || went {
            let subscribers = nodes.subscribers(shared.port);
            if subscribers != current {
                current = subscribers;
                board.current.clone_from(&current);
                changed = true;
            }
        }
        feeds.append(&mut board.joining);
        feeds.retain_mut(|feed| feed.serve(changed, &current));
        // The list changes last: what it shows, every link with room was given already.
        if heard || went {
            board.nodes = nodes.list();
        }
        drop(board);
    }
}

/// Answers a poll from `ip`: our reply, unicast, from the address the system uses toward it.
fn answer(socket: &UdpSocket, ip: Ipv4Addr, port: u16) {
    let to = SocketAddrV4::new(ip, port);
    let ours = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        .and_then(|probe| {
            probe.connect(to)?;
            probe.local_addr()
        })
        .ok()
        .and_then(|local| match local {
            SocketAddr::V4(local) => Some(*local.ip()),
            SocketAddr::V6(_) => None,
        });
    if let Some(ours) = ours {
        let _ = socket.send_to(&poll::reply(ours), to);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FeedState {
    /// The link has, or will take next, the newest snapshot.
    Current,
    /// A newer snapshot waits for room in `fresh`.
    Behind,
    /// The link is gone: the feed is dropped.
    Dropped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FeedEvent {
    Changed,
    Room,
    LinkDropped,
}

/// The one transition function of a feed.
const fn next_feed(state: FeedState, event: FeedEvent) -> FeedState {
    match (state, event) {
        (FeedState::Dropped, _) | (_, FeedEvent::LinkDropped) => FeedState::Dropped,
        (FeedState::Current | FeedState::Behind, FeedEvent::Changed) => FeedState::Behind,
        (FeedState::Current | FeedState::Behind, FeedEvent::Room) => FeedState::Current,
    }
}

/// The discovery thread's end of one link's feed.
struct Feed {
    fresh: rtrb::Producer<Box<Snapshot>>,
    spent: rtrb::Consumer<Box<Snapshot>>,
    state: FeedState,
}

impl Feed {
    /// Frees what the link handed back and gives it the newest snapshot when it is behind
    /// and has room. Whether the feed stays.
    fn serve(&mut self, changed: bool, current: &[Subscriber]) -> bool {
        while self.spent.pop().is_ok() {}
        if changed {
            self.state = next_feed(self.state, FeedEvent::Changed);
        }
        if self.fresh.is_abandoned() {
            self.state = next_feed(self.state, FeedEvent::LinkDropped);
        }
        if self.state == FeedState::Behind && self.fresh.slots() > 0 {
            let _ = self.fresh.push(Box::new(Snapshot(current.to_vec())));
            self.state = next_feed(self.state, FeedEvent::Room);
        }
        self.state != FeedState::Dropped
    }
}

/// The output's link to the found nodes: its own non-blocking socket, the snapshot it
/// sends by and its end of the feed.
struct Found {
    socket: UdpSocket,
    now: Box<Snapshot>,
    fresh: rtrb::Consumer<Box<Snapshot>>,
    spent: rtrb::Producer<Box<Snapshot>>,
}

impl Found {
    /// Takes a newer snapshot, if one came and the old one can go back. Never frees.
    fn renew(&mut self) {
        if self.spent.slots() > 0
            && let Ok(newer) = self.fresh.pop()
        {
            let old = std::mem::replace(&mut self.now, newer);
            let _ = self.spent.push(old);
        }
    }
}

impl Link for Found {
    /// Sends the ArtDmx to every node that lists its Port-Address, then reports the first
    /// failure. Nobody lists it: nothing is sent, and that is no failure.
    fn send(&mut self, packet: &[u8]) -> io::Result<usize> {
        self.renew();
        let port_address = packet
            .get(14..16)
            .and_then(|b| b.first_chunk())
            .map(|b| u16::from_le_bytes(*b))
            .ok_or(io::ErrorKind::InvalidData)?;
        let subscribers = &self.now.0;
        let start = subscribers.partition_point(|s| s.port_address < port_address);
        let mut first_failure = None;
        for subscriber in subscribers
            .get(start..)
            .unwrap_or_default()
            .iter()
            .take_while(|s| s.port_address == port_address)
        {
            // A datagram goes whole or not at all.
            if let Err(error) = self.socket.send_to(packet, subscriber.to) {
                first_failure.get_or_insert(error);
            }
        }
        first_failure.map_or(Ok(packet.len()), Err)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
mod tests {
    use super::*;

    #[test]
    fn every_state_and_event_of_a_feed() {
        use FeedEvent::{Changed, LinkDropped, Room};
        use FeedState::{Behind, Current, Dropped};
        // (state, event, new state)
        let cases = [
            (Current, Changed, Behind),
            (Current, Room, Current),
            (Current, LinkDropped, Dropped),
            (Behind, Changed, Behind),
            (Behind, Room, Current),
            (Behind, LinkDropped, Dropped),
            (Dropped, Changed, Dropped),
            (Dropped, Room, Dropped),
            (Dropped, LinkDropped, Dropped),
        ];
        for (state, event, expected) in cases {
            assert_eq!(next_feed(state, event), expected, "{state:?} {event:?}");
        }
    }

    fn snapshot(port_address: u16) -> Box<Snapshot> {
        Box::new(Snapshot(vec![Subscriber {
            port_address,
            to: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9),
        }]))
    }

    /// A link with snapshot 1, and the discovery thread's ends of its feed.
    fn link() -> (
        Found,
        rtrb::Producer<Box<Snapshot>>,
        rtrb::Consumer<Box<Snapshot>>,
    ) {
        let (give, fresh) = rtrb::RingBuffer::new(1);
        let (spent, back) = rtrb::RingBuffer::new(1);
        let found = Found {
            socket: link_socket().unwrap(),
            now: snapshot(1),
            fresh,
            spent,
        };
        (found, give, back)
    }

    #[test]
    fn a_link_takes_a_newer_snapshot_only_when_the_old_one_can_go_back() {
        // (a newer snapshot came, `spent` already holds one, snapshot after, handed back)
        let cases = [
            (false, false, 1, vec![]),
            (true, false, 2, vec![1]),
            (false, true, 1, vec![0]),
            (true, true, 1, vec![0]),
        ];
        for (newer, full, after, back) in cases {
            let (mut found, mut give, mut handed) = link();
            if newer {
                give.push(snapshot(2)).unwrap();
            }
            if full {
                found.spent.push(snapshot(0)).unwrap();
            }
            found.renew();
            assert_eq!(found.now, snapshot(after), "{newer} {full}");
            let mut returned = vec![];
            while let Ok(old) = handed.pop() {
                returned.push(old.0.first().unwrap().port_address);
            }
            assert_eq!(returned, back, "{newer} {full}");
            assert_eq!(give.slots(), usize::from(!newer || !full), "{newer} {full}");
        }
    }

    #[test]
    fn an_interface_is_polled_at_its_directed_broadcast_when_up_and_not_loopback() {
        use InterfaceFlags as F;
        let address = Some(Ipv4Addr::new(192, 168, 1, 20));
        let mask = Some(Ipv4Addr::new(255, 255, 255, 0));
        let at = Some(Ipv4Addr::new(192, 168, 1, 255));
        // (flags, address, mask, where its poll goes)
        let cases = [
            (F::IFF_UP | F::IFF_BROADCAST, address, mask, at),
            (
                F::IFF_UP | F::IFF_BROADCAST | F::IFF_RUNNING,
                address,
                mask,
                at,
            ),
            (F::IFF_UP, address, mask, None),
            (F::IFF_BROADCAST, address, mask, None),
            (F::empty(), address, mask, None),
            (
                F::IFF_UP | F::IFF_BROADCAST | F::IFF_LOOPBACK,
                address,
                mask,
                None,
            ),
            (F::IFF_UP | F::IFF_BROADCAST, None, mask, None),
            (F::IFF_UP | F::IFF_BROADCAST, address, None, None),
        ];
        for (flags, address, mask, expected) in cases {
            assert_eq!(poll_target(flags, address, mask), expected, "{flags:?}");
        }
    }

    #[test]
    fn a_link_socket_never_waits() {
        let socket = link_socket().unwrap();
        assert!(socket2::SockRef::from(&socket).nonblocking().unwrap());
    }

    #[test]
    fn the_discovery_socket_is_shared_may_broadcast_and_listens_a_while() {
        let socket = listen(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let shared = socket2::SockRef::from(&socket);
        // (option, set)
        let cases = [
            ("SO_REUSEADDR", shared.reuse_address().unwrap()),
            ("SO_REUSEPORT", shared.reuse_port().unwrap()),
            ("SO_BROADCAST", socket.broadcast().unwrap()),
        ];
        for (option, set) in cases {
            assert!(set, "{option}");
        }
        assert_eq!(socket.read_timeout().unwrap(), Some(LISTEN));
    }

    #[test]
    fn a_poll_goes_out_when_due_then_every_2_5_s() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        // (now, next poll, poll now, next poll after)
        let cases = [
            (0, 0, true, 2_500),
            (100, 0, true, 2_600),
            (1_000, 2_500, false, 2_500),
            (2_499, 2_500, false, 2_500),
            (2_500, 2_500, true, 5_000),
        ];
        for (now, next, due, after) in cases {
            assert_eq!(poll_due(at(now), at(next)), (due, at(after)), "{now}");
        }
    }

    #[test]
    fn a_link_sends_to_every_node_then_reports_the_first_failure() {
        let node = UdpSocket::bind("127.0.0.1:0").unwrap();
        node.set_nonblocking(true).unwrap();
        let ok = SocketAddrV4::new(Ipv4Addr::LOCALHOST, node.local_addr().unwrap().port());
        // Refused by the system for sure: port 0, and a broadcast without SO_BROADCAST.
        let no_port = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
        let broadcast = SocketAddrV4::new(Ipv4Addr::BROADCAST, ok.port());
        let refusal = |to| {
            link_socket()
                .unwrap()
                .send_to(&[0; 20], to)
                .unwrap_err()
                .kind()
        };
        // (nodes of Port-Address 1, sorted; the node got it; the failure reported)
        let cases = [
            (vec![ok], true, None),
            (vec![no_port, ok], true, Some(refusal(no_port))),
            (vec![ok, broadcast], true, Some(refusal(broadcast))),
            (vec![no_port, broadcast], false, Some(refusal(no_port))),
            (vec![], false, None),
        ];
        let mut packet = [0; 20];
        packet[14] = 1;
        for (to, arrived, failure) in cases {
            let (mut found, _give, _back) = link();
            *found.now = Snapshot(
                to.iter()
                    .map(|to| Subscriber {
                        port_address: 1,
                        to: *to,
                    })
                    .collect(),
            );
            let sent = found.send(&packet);
            assert_eq!(sent.as_ref().err().map(io::Error::kind), failure, "{to:?}");
            if failure.is_none() {
                assert_eq!(sent.unwrap(), 20, "{to:?}: the whole packet");
            }
            let mut buffer = [0; 64];
            let mut got = false;
            for _ in 0..200 {
                if node.recv(&mut buffer).is_ok() {
                    got = true;
                    break;
                }
                std::thread::yield_now(); // poll
            }
            assert_eq!(got, arrived, "{to:?}");
        }
    }

    #[test]
    fn a_feed_pushes_the_newest_when_behind_and_frees_what_came_back() {
        let current = [Subscriber {
            port_address: 3,
            to: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9),
        }];
        // (state, changed, `fresh` full, link dropped, new state, stays, pushed)
        let cases = [
            (
                FeedState::Current,
                false,
                false,
                false,
                FeedState::Current,
                true,
                false,
            ),
            (
                FeedState::Current,
                true,
                false,
                false,
                FeedState::Current,
                true,
                true,
            ),
            (
                FeedState::Current,
                true,
                true,
                false,
                FeedState::Behind,
                true,
                false,
            ),
            (
                FeedState::Behind,
                false,
                false,
                false,
                FeedState::Current,
                true,
                true,
            ),
            (
                FeedState::Behind,
                false,
                true,
                false,
                FeedState::Behind,
                true,
                false,
            ),
            (
                FeedState::Current,
                true,
                false,
                true,
                FeedState::Dropped,
                false,
                false,
            ),
            (
                FeedState::Behind,
                false,
                false,
                true,
                FeedState::Dropped,
                false,
                false,
            ),
        ];
        for (state, changed, full, dropped, expected, stays, pushed) in cases {
            let (fresh, mut taken) = rtrb::RingBuffer::new(1);
            let (mut handed, spent) = rtrb::RingBuffer::new(1);
            handed.push(snapshot(7)).unwrap();
            let mut feed = Feed {
                fresh,
                spent,
                state,
            };
            if full {
                feed.fresh.push(snapshot(0)).unwrap();
            }
            let row = format!("{state:?} {changed} {full} {dropped}");
            if dropped {
                drop(taken);
                assert_eq!(feed.serve(changed, &current), stays, "{row}");
                assert_eq!(feed.state, expected, "{row}");
                continue;
            }
            assert_eq!(feed.serve(changed, &current), stays, "{row}");
            assert_eq!(feed.state, expected, "{row}");
            assert_eq!(handed.slots(), 1, "{row}: what came back is freed");
            let first = taken.pop().ok().map(|s| s.0);
            let expected_first = if full {
                Some(snapshot(0).0)
            } else if pushed {
                Some(current.to_vec())
            } else {
                None
            };
            assert_eq!(first, expected_first, "{row}");
        }
    }
}
