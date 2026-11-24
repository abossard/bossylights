//! Proves that `take` allocates and frees nothing, for every device kind, running and
//! refused. A counting allocator sees every allocation and free in this process; each
//! device is opened before the window.

#![expect(
    clippy::disallowed_methods,
    reason = "the test waits between frames; it is not the output thread"
)]

mod common;
#[path = "counting/mod.rs"]
mod counting;

use common::NEVER;

use nix::errno::Errno;
use nix::unistd::read;
use outputs::dmx_usb_pro::{DmxUsbPro, never_wait};
use outputs::{Color, Device, Outcome, OutputKind};
use serialport::TTYPort;
use std::io::{Read, Write};
use std::net::{SocketAddr, UdpSocket};
use std::os::fd::AsRawFd;
use std::sync::mpsc::sync_channel;
use std::thread;
use std::time::{Duration, Instant};

// Drain the fake port so a full queue never sends the port thread looking for a lost adapter.
fn fake_port() -> (TTYPort, thread::JoinHandle<()>) {
    let (mut end, mut port) = TTYPort::pair().unwrap();
    port.write_all(&[0]).unwrap();
    let (ready, first_read) = sync_channel(1);
    let reader = thread::spawn(move || {
        let mut bytes = [0; 4096];
        end.read_exact(&mut bytes[..1]).unwrap();
        ready.send(()).unwrap();
        // Read the blocking descriptor directly. TTYPort's idle timeouts allocate.
        loop {
            match read(end.as_raw_fd(), &mut bytes) {
                Ok(0) | Err(Errno::EIO) => break, // the writer closed
                Ok(_) | Err(Errno::EINTR) => {}
                Err(error) => panic!("read fake port: {error}"),
            }
        }
    });
    first_read.recv().unwrap();
    never_wait(&port).unwrap();
    (port, reader)
}

fn main() {
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    let SocketAddr::V4(address) = receiver.local_addr().unwrap() else {
        panic!("IPv4");
    };
    let frame = [[7; 512], [9; 512]].concat();
    let (port, reader) = fake_port();
    let mut port = Some(port);
    let pro = DmxUsbPro::open(Box::new(move || {
        port.take()
            .ok_or_else(|| std::io::Error::other("one port only"))
    }))
    .unwrap();
    let ddp = outputs::open(OutputKind::Ddp {
        address,
        color: Color::Rgbw,
    })
    .unwrap();
    let nobody = UdpSocket::bind("127.0.0.1:0").unwrap();
    let SocketAddr::V4(nobody_listens) = nobody.local_addr().unwrap() else {
        panic!("IPv4");
    };
    drop(nobody);
    let refused = outputs::open(OutputKind::Ddp {
        address: nobody_listens,
        color: Color::Rgbw,
    })
    .unwrap();
    let art_net =
        outputs::artnet::open(address, outputs::artnet::PortAddress::new(1).unwrap()).unwrap();
    // (kind, the device, an outcome it must give)
    // All stay open until every one was measured: a dropped DDP output's socket thread
    // frees as it ends.
    let mut devices: [(&str, Box<dyn Device>, Outcome); 4] = [
        ("DDP", ddp, Outcome::Accepted),
        ("Art-Net", art_net, Outcome::Accepted),
        ("DDP nobody listens to", refused, Outcome::Failed),
        ("DMX USB Pro", Box::new(pro), Outcome::Accepted),
    ];
    for (kind, device, expected) in &mut devices {
        let expected = *expected;
        let (allocated, freed) = (counting::allocations(), counting::frees());
        // At least 200 frames, and on until the device gave its outcome: the host answers a
        // send to nobody a little later, later still under load (#175).
        let started = Instant::now();
        let (mut frames, mut given) = (0, 0);
        while frames < 200 || (given == 0 && started.elapsed() < Duration::from_secs(10)) {
            given += usize::from(device.take(&frame) == expected);
            frames += 1;
            thread::sleep(Duration::from_millis(1)); // poll
        }
        let allocations = counting::allocations() - allocated;
        let frees = counting::frees() - freed;
        assert!(given > 0, "{kind}: never {expected:?}");
        assert_eq!(
            (allocations, frees),
            (0, 0),
            "{kind}: allocations and frees in {frames} frames"
        );
    }
    drop(devices);
    let started = Instant::now();
    while !reader.is_finished() {
        assert!(started.elapsed() < NEVER, "the fake port was never closed"); // guard
        thread::sleep(Duration::from_millis(5)); // poll
    }
    reader.join().unwrap();
}
