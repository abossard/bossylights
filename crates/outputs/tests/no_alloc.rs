//! Proves that `take` allocates and frees nothing, for every device kind, running and
//! refused. A counting allocator sees every allocation and free in this process; each
//! device is opened before the window.

#![expect(
    clippy::disallowed_methods,
    reason = "the test waits between frames; it is not the output thread"
)]

#[path = "common/artnet.rs"]
mod artnet;
mod common;
#[path = "counting/mod.rs"]
mod counting;

use common::NEVER;

use nix::errno::Errno;
use nix::unistd::read;
use outputs::artnet::{Discovery, PortAddress, Presence};
use outputs::dmx_usb_pro::{DmxUsbPro, never_wait};
use outputs::{Color, Device, Outcome, OutputKind};
use serialport::TTYPort;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
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

/// Takes frames on `device` and fails on any allocation or free in the process meanwhile.
fn measure(kind: &str, device: &mut dyn Device, expected: Outcome, frame: &[u8]) {
    let (allocated, freed) = (counting::allocations(), counting::frees());
    // At least 200 frames, and on until the device gave its outcome: the host answers a
    // send to nobody a little later, later still under load (#175).
    let started = Instant::now();
    let (mut frames, mut given) = (0, 0);
    while frames < 200 || (given == 0 && started.elapsed() < Duration::from_secs(10)) {
        given += usize::from(device.take(frame) == expected);
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

/// An Art-Net output to the nodes discovery found, one of them listing Port-Address 1.
/// It runs alone, before the other rows; the counter leaves out the discovery thread.
fn found_nodes(frame: &[u8]) {
    let node = UdpSocket::bind("127.0.0.1:0").unwrap();
    node.set_read_timeout(Some(NEVER)).unwrap();
    let port = node.local_addr().unwrap().port();
    let on = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
    let discovery = Discovery::start(on, port, vec![Ipv4Addr::LOCALHOST]).unwrap();
    let mut poll = [0; 64];
    let (_, from) = node.recv_from(&mut poll).unwrap();
    node.send_to(&artnet::node_reply(Ipv4Addr::LOCALHOST, &[1]), from)
        .unwrap();
    let started = Instant::now();
    while discovery.nodes().first().map(|n| n.presence) != Some(Presence::Seen) {
        assert!(started.elapsed() < NEVER, "the node was never found"); // guard
        thread::sleep(Duration::from_millis(5)); // poll
    }
    let mut device = discovery.open(PortAddress::new(1).unwrap()).unwrap();
    // The node now lists 1 and 2: the discovery thread hands the link a new snapshot, and
    // the link takes it in its first frame of the window.
    node.send_to(&artnet::node_reply(Ipv4Addr::LOCALHOST, &[1, 2]), from)
        .unwrap();
    let started = Instant::now();
    while discovery.nodes().first().map(|n| n.outputs.len()) != Some(2) {
        assert!(started.elapsed() < NEVER, "the new ports never came"); // guard
        thread::sleep(Duration::from_millis(5)); // poll
    }
    measure(
        "Art-Net found nodes, a new snapshot taken",
        device.as_mut(),
        Outcome::Accepted,
        frame,
    );
    // Universe 2 reached the node: the link swapped its snapshot in the window.
    let mut packet = [0; 600];
    let started = Instant::now();
    loop {
        assert!(started.elapsed() < NEVER, "universe 2 never came"); // guard
        let (size, _) = node.recv_from(&mut packet).unwrap();
        if packet[8..10] == [0x00, 0x50] && packet[14..16] == [2, 0] {
            assert_eq!(size, 18 + 512, "one universe");
            break;
        }
    }
}

fn main() {
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    let SocketAddr::V4(address) = receiver.local_addr().unwrap() else {
        panic!("IPv4");
    };
    let frame = [[7; 512], [9; 512]].concat();
    found_nodes(&frame);
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
    let sacn = |to| {
        let first = outputs::sacn::Universe::new(1).unwrap();
        let output = engine_core::OutputNumber::try_from(1).unwrap();
        outputs::sacn::open(to, first, [7; 16], output).unwrap()
    };
    let sacn_unicast = sacn(outputs::sacn::Destination::Unicast(address));
    // A machine that sends no multicast (no route, or a CI Mac) checks unicast only.
    let multicast_route = UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.send_to(
                b"",
                (std::net::Ipv4Addr::new(239, 255, 0, 1), address.port()),
            )
        })
        .is_ok();
    // (kind, the device, an outcome it must give)
    // All stay open until every one was measured: a dropped DDP output's socket thread
    // frees as it ends.
    let mut devices: Vec<(&str, Box<dyn Device>, Outcome)> = vec![
        ("DDP", ddp, Outcome::Accepted),
        ("Art-Net", art_net, Outcome::Accepted),
        ("sACN unicast", sacn_unicast, Outcome::Accepted),
        ("DDP nobody listens to", refused, Outcome::Failed),
        ("DMX USB Pro", Box::new(pro), Outcome::Accepted),
    ];
    // An announced output: discovery lists to the receiver, beside the frames.
    let discovery = outputs::sacn::Discovery::start([7; 16], address).unwrap();
    if multicast_route {
        let to = outputs::sacn::Destination::Multicast(address.port());
        devices.push(("sACN multicast", sacn(to), Outcome::Accepted));
        let first = outputs::sacn::Universe::new(1).unwrap();
        let output = engine_core::OutputNumber::try_from(2).unwrap();
        let announced = discovery.open(to, first, output).unwrap();
        devices.push(("sACN multicast, announced", announced, Outcome::Accepted));
    }
    for (kind, device, expected) in &mut devices {
        measure(kind, device.as_mut(), *expected, &frame);
    }
    drop(devices);
    drop(discovery);
    let started = Instant::now();
    while !reader.is_finished() {
        assert!(started.elapsed() < NEVER, "the fake port was never closed"); // guard
        thread::sleep(Duration::from_millis(5)); // poll
    }
    reader.join().unwrap();
}
