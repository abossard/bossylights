//! Proves that `take` allocates nothing, for every device kind. A counting allocator
//! sees every allocation in this process; each device is opened before the window.

#![expect(
    unsafe_code,
    reason = "a global allocator can only be written with unsafe"
)]

use outputs::dmx_usb_pro::{DmxUsbPro, never_wait};
use outputs::{Color, Device, Levels, Outcome, OutputKind};
use serialport::TTYPort;
use std::alloc::{GlobalAlloc, Layout, System};
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

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
    // (kind, the device)
    let devices: [(&str, Box<dyn Device>); 2] = [("DDP", ddp), ("DMX USB Pro", Box::new(pro))];
    for (kind, mut device) in devices {
        let before = ALLOCATIONS.load(Ordering::SeqCst);
        let accepted = (0..200)
            .filter(|_| device.take(&frame) == Outcome::Accepted)
            .count();
        let allocations = ALLOCATIONS.load(Ordering::SeqCst) - before;
        assert!(accepted > 0, "{kind}: nothing was accepted");
        assert_eq!(
            allocations, 0,
            "{kind}: {allocations} allocations in 200 frames"
        );
    }
    drop(end);
}
