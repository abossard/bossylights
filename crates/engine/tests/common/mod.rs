//! A DDP receiver on this machine: keeps the last frame it got, in atomics.

#![expect(
    clippy::disallowed_types,
    reason = "a test thread listens like a DDP device; the ban is for the tick thread"
)]

use engine::{ADDRESSES, Color, Command, OutputKind, OutputNumber, RunNumbers, Sender};
use std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub struct Receiver {
    pub address: SocketAddrV4,
    heard: Arc<Heard>,
}

/// Shared with the listening thread.
struct Heard {
    frames: AtomicU64,
    levels: [AtomicU8; 2 * ADDRESSES],
    stop: AtomicBool,
}

impl Receiver {
    /// Listens on a free port until dropped.
    pub fn start() -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
            panic!("IPv4");
        };
        let heard = Arc::new(Heard {
            frames: AtomicU64::new(0),
            levels: [const { AtomicU8::new(0) }; 2 * ADDRESSES],
            stop: AtomicBool::new(false),
        });
        let listening = Arc::clone(&heard);
        thread::spawn(move || {
            let mut packet = [0; 1500];
            while !listening.stop.load(Ordering::Relaxed) {
                let Ok(size) = socket.recv(&mut packet) else {
                    continue;
                };
                let offset = u32::from_be_bytes(packet[4..8].try_into().unwrap()) as usize;
                let data = packet[10..size].iter();
                for (cell, level) in listening.levels.iter().skip(offset).zip(data) {
                    cell.store(*level, Ordering::Relaxed);
                }
                if packet[0] & 0x01 == 0x01 {
                    listening.frames.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
        Self { address, heard }
    }

    /// Output 1, RGBW (so all 1024 channels of two universes are pixels), sending here.
    pub fn add(&self, sender: &mut Sender, universes: u16) {
        let command = Command::SetOutput {
            output: OutputNumber::try_from(1).unwrap(),
            kind: OutputKind::Ddp {
                address: self.address,
                color: Color::Rgbw,
            },
            universes: RunNumbers {
                first: 1,
                count: universes,
            },
            rate: None,
        };
        assert_eq!(sender.send(command), Ok(()));
        self.two_more_frames();
    }

    pub fn level(&self, universe: u16, address: u16) -> u8 {
        let at = usize::from(universe - 1) * ADDRESSES + usize::from(address - 1);
        self.heard.levels[at].load(Ordering::Relaxed)
    }

    /// Waits until two more frames arrived: the one that may have been computed while
    /// the commands were sent, and the next one.
    pub fn two_more_frames(&self) {
        let before = self.heard.frames.load(Ordering::Relaxed);
        let started = Instant::now();
        while self.heard.frames.load(Ordering::Relaxed) < before + 2 {
            assert!(started.elapsed() < Duration::from_secs(2), "no frames");
            thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.heard.stop.store(true, Ordering::Relaxed);
    }
}
