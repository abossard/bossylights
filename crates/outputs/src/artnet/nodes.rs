//! The nodes on the network, as their ArtPollReplies say. A calculation: the time comes
//! in as an argument.
//!
//! | node \ event | reply | expire, last reply 3 s or less ago | expire, more than 3 s |
//! |---|---|---|---|
//! | Unknown | Seen | Unknown | Unknown |
//! | Seen | Seen, its new ports | Seen | Gone |
//! | Gone | Seen, its new ports | Gone | Gone |

use super::PortAddress;
use super::poll::Reply;
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::{Duration, Instant};

/// A node that has not replied for longer than this is gone.
pub(crate) const GONE_AFTER: Duration = Duration::from_secs(3);

/// Whether a node still answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// It replied in the last 3 s: it gets the universes it lists.
    Seen,
    /// It stopped replying: it gets nothing.
    Gone,
}

/// A node, or another controller, as the network last said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub ip: Ipv4Addr,
    /// Which part of a node with more than 4 ports, 1 for the root.
    pub bind_index: u8,
    pub short_name: String,
    pub long_name: String,
    /// The Port-Addresses its ports output.
    pub outputs: Vec<PortAddress>,
    pub presence: Presence,
}

/// Where one universe goes: a Seen node that lists its Port-Address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Subscriber {
    pub port_address: u16,
    pub to: SocketAddrV4,
}

/// A node's state. Not in the table is Unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Seen { at: Instant },
    Gone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    /// It replied at this time.
    Reply(Instant),
    /// The clock says this time.
    Expire(Instant),
}

/// The one transition function of a node. `None` is Unknown.
fn next(state: Option<State>, event: Event) -> Option<State> {
    match (state, event) {
        (_, Event::Reply(at)) => Some(State::Seen { at }),
        (Some(State::Seen { at }), Event::Expire(now))
            if now.saturating_duration_since(at) > GONE_AFTER =>
        {
            Some(State::Gone)
        }
        (same, Event::Expire(_)) => same,
    }
}

/// Every node the network told of, keyed by IP and BindIndex.
#[derive(Default)]
pub(crate) struct Nodes {
    table: BTreeMap<(Ipv4Addr, u8), (Reply, State)>,
}

impl Nodes {
    /// A node replied at `now`.
    pub(crate) fn heard(&mut self, reply: Reply, now: Instant) {
        let key = (reply.ip, reply.bind_index);
        let before = self.table.get(&key).map(|(_, state)| *state);
        if let Some(state) = next(before, Event::Reply(now)) {
            self.table.insert(key, (reply, state));
        }
    }

    /// The clock says `now`: a node silent for longer than 3 s is gone. Whether one went.
    pub(crate) fn expire(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for (_, state) in self.table.values_mut() {
            if let Some(after) = next(Some(*state), Event::Expire(now)) {
                changed |= after != *state;
                *state = after;
            }
        }
        changed
    }

    /// Every node, ordered by IP and BindIndex.
    pub(crate) fn list(&self) -> Vec<Node> {
        self.table
            .values()
            .map(|(reply, state)| Node {
                ip: reply.ip,
                bind_index: reply.bind_index,
                short_name: reply.short_name.clone(),
                long_name: reply.long_name.clone(),
                outputs: reply.outputs.clone(),
                presence: match state {
                    State::Seen { .. } => Presence::Seen,
                    State::Gone => Presence::Gone,
                },
            })
            .collect()
    }

    /// Where each Port-Address goes: every Seen node that lists it, at `port`. Sorted, once
    /// each.
    pub(crate) fn subscribers(&self, port: u16) -> Vec<Subscriber> {
        let mut subscribers: Vec<_> = self
            .table
            .values()
            .filter(|(_, state)| matches!(state, State::Seen { .. }))
            .flat_map(|(reply, _)| {
                reply.outputs.iter().map(|output| Subscriber {
                    port_address: output.get(),
                    to: SocketAddrV4::new(reply.ip, port),
                })
            })
            .collect();
        subscribers.sort_unstable();
        subscribers.dedup();
        subscribers
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
mod tests {
    use super::*;

    #[test]
    fn every_state_and_event_of_a_node() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let seen = State::Seen { at: at(0) };
        // (state, event, new state)
        let cases = [
            (None, Event::Reply(at(0)), Some(seen)),
            (None, Event::Expire(at(5_000)), None),
            (None, Event::Expire(at(0)), None),
            (
                Some(seen),
                Event::Reply(at(2_500)),
                Some(State::Seen { at: at(2_500) }),
            ),
            (Some(seen), Event::Expire(at(0)), Some(seen)),
            (Some(seen), Event::Expire(at(2_999)), Some(seen)),
            (Some(seen), Event::Expire(at(3_000)), Some(seen)),
            (Some(seen), Event::Expire(at(3_001)), Some(State::Gone)),
            (Some(seen), Event::Expire(at(60_000)), Some(State::Gone)),
            (
                Some(State::Seen { at: at(1_000) }),
                Event::Expire(at(0)),
                Some(State::Seen { at: at(1_000) }),
            ),
            (
                Some(State::Gone),
                Event::Reply(at(9_000)),
                Some(State::Seen { at: at(9_000) }),
            ),
            (
                Some(State::Gone),
                Event::Expire(at(9_000)),
                Some(State::Gone),
            ),
        ];
        for (state, event, expected) in cases {
            assert_eq!(next(state, event), expected, "{state:?} {event:?}");
        }
    }

    fn reply(ip: [u8; 4], bind_index: u8, outputs: &[u16]) -> Reply {
        Reply {
            ip: Ipv4Addr::from(ip),
            bind_index,
            short_name: format!("node {}", ip[3]),
            long_name: String::new(),
            outputs: outputs
                .iter()
                .map(|n| PortAddress::new(*n).unwrap())
                .collect(),
        }
    }

    /// (IP's last byte, BindIndex, presence) of every node in the list.
    fn listed(nodes: &Nodes) -> Vec<(u8, u8, Presence)> {
        nodes
            .list()
            .iter()
            .map(|node| (node.ip.octets()[3], node.bind_index, node.presence))
            .collect()
    }

    /// (Port-Address, IP's last byte, port) of every subscriber.
    fn subscribed(nodes: &Nodes) -> Vec<(u16, u8, u16)> {
        nodes
            .subscribers(6454)
            .iter()
            .map(|s| (s.port_address, s.to.ip().octets()[3], s.to.port()))
            .collect()
    }

    #[test]
    fn the_table_lists_each_node_and_where_each_universe_goes() {
        use Presence::{Gone, Seen};
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        // (replies: IP, BindIndex, outputs, at ms; expire at ms; listed; subscribers)
        type Replies = Vec<([u8; 4], u8, Vec<u16>, u64)>;
        type Row = (Replies, u64, Vec<(u8, u8, Presence)>, Vec<(u16, u8, u16)>);
        let cases: [Row; 9] = [
            (vec![], 0, vec![], vec![]),
            (
                vec![([2, 0, 0, 9], 1, vec![5], 0)],
                0,
                vec![(9, 1, Seen)],
                vec![(5, 9, 6454)],
            ),
            (
                vec![([2, 0, 0, 9], 1, vec![5, 6, 5], 0)],
                3_000,
                vec![(9, 1, Seen)],
                vec![(5, 9, 6454), (6, 9, 6454)],
            ),
            (
                vec![([2, 0, 0, 9], 1, vec![5], 0)],
                3_001,
                vec![(9, 1, Gone)],
                vec![],
            ),
            (
                vec![
                    ([2, 0, 0, 9], 1, vec![5], 0),
                    ([2, 0, 0, 9], 1, vec![7], 2_500),
                ],
                5_000,
                vec![(9, 1, Seen)],
                vec![(7, 9, 6454)],
            ),
            (
                vec![([2, 0, 0, 9], 1, vec![5], 0), ([2, 0, 0, 3], 1, vec![5], 0)],
                0,
                vec![(3, 1, Seen), (9, 1, Seen)],
                vec![(5, 3, 6454), (5, 9, 6454)],
            ),
            (
                vec![
                    ([2, 0, 0, 9], 1, vec![1], 0),
                    ([2, 0, 0, 9], 2, vec![2], 2_000),
                ],
                4_000,
                vec![(9, 1, Gone), (9, 2, Seen)],
                vec![(2, 9, 6454)],
            ),
            (
                vec![([2, 0, 0, 9], 1, vec![], 0)],
                0,
                vec![(9, 1, Seen)],
                vec![],
            ),
            (
                vec![
                    ([2, 0, 0, 9], 1, vec![4, 3], 0),
                    ([2, 0, 0, 1], 1, vec![3], 0),
                ],
                0,
                vec![(1, 1, Seen), (9, 1, Seen)],
                vec![(3, 1, 6454), (3, 9, 6454), (4, 9, 6454)],
            ),
        ];
        for (replies, expire, list, subscribers) in cases {
            let mut nodes = Nodes::default();
            for (ip, bind_index, outputs, ms) in &replies {
                nodes.heard(reply(*ip, *bind_index, outputs), at(*ms));
            }
            nodes.expire(at(expire));
            assert_eq!(listed(&nodes), list, "{replies:?}");
            assert_eq!(subscribed(&nodes), subscribers, "{replies:?}");
        }
    }

    #[test]
    fn expire_says_whether_a_node_went() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut nodes = Nodes::default();
        nodes.heard(reply([2, 0, 0, 9], 1, &[5]), at(0));
        nodes.heard(reply([2, 0, 0, 10], 1, &[6]), at(2_000));
        // (expire at ms, a node went): the first goes while the second stays, then the second
        let cases = [
            (1_000, false),
            (3_001, true),
            (4_000, false),
            (5_001, true),
            (6_000, false),
        ];
        for (ms, went) in cases {
            assert_eq!(nodes.expire(at(ms)), went, "{ms}");
        }
    }

    #[test]
    fn a_node_keeps_its_names_from_its_last_reply() {
        let mut nodes = Nodes::default();
        let mut later = reply([2, 0, 0, 9], 1, &[5]);
        later.long_name = "renamed".into();
        nodes.heard(reply([2, 0, 0, 9], 1, &[5]), Instant::now());
        nodes.heard(later, Instant::now());
        let list = nodes.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list.first().unwrap().long_name, "renamed");
        assert_eq!(list.first().unwrap().short_name, "node 9");
    }
}
