//! Proves that `take` allocates and frees nothing, for every device kind, running and
//! refused. A counting allocator sees every allocation and free in this process; each
//! device is opened before the window.

#[path = "counting/mod.rs"]
mod counting;

use outputs::dmx_usb_pro::{DmxUsbPro, never_wait};
use outputs::{Color, Device, Levels, Outcome, OutputKind};
use serialport::TTYPort;
use std::net::{SocketAddr, UdpSocket};

fn main() {
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    let SocketAddr::V4(address) = receiver.local_addr().unwrap() else {
        panic!("IPv4");
    };
    let frame = [Levels([7; 512]), Levels([9; 512])];
    // A fake serial port: the other end stays open and unread.
    let (end, port) = TTYPort::pair().unwrap();
    never_wait(&port).unwrap();
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
    // (kind, the device, an outcome it must give)
    // All stay open until every one was measured: a dropped DDP output's socket thread
    // frees as it ends.
    let mut devices: [(&str, Box<dyn Device>, Outcome); 3] = [
        ("DDP", ddp, Outcome::Accepted),
        ("DDP nobody listens to", refused, Outcome::Failed),
        ("DMX USB Pro", Box::new(pro), Outcome::Accepted),
    ];
    for (kind, device, expected) in &mut devices {
        let expected = *expected;
        let (allocated, freed) = (counting::allocations(), counting::frees());
        let given = (0..200).filter(|_| device.take(&frame) == expected).count();
        let allocations = counting::allocations() - allocated;
        let frees = counting::frees() - freed;
        assert!(given > 0, "{kind}: never {expected:?}");
        assert_eq!(
            (allocations, frees),
            (0, 0),
            "{kind}: allocations and frees in 200 frames"
        );
    }
    drop(end);
}
