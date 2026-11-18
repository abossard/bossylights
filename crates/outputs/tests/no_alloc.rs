//! Proves that `take` allocates nothing, for every device kind. A counting allocator
//! sees every allocation in this process; each device is opened before the window.

#![expect(
    unsafe_code,
    reason = "a global allocator can only be written with unsafe"
)]

use outputs::{Color, Levels, Outcome, OutputKind};
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
    // (kind, the device)
    let devices = [(
        "DDP",
        outputs::open(OutputKind::Ddp {
            address,
            color: Color::Rgbw,
        })
        .unwrap(),
    )];
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
}
