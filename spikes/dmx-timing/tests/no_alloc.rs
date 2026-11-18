//! The output loop allocates nothing per frame. A counting allocator counts the
//! allocations made on the output thread until the loop sets `looped`; the report
//! built after that may allocate.

#![allow(
    unsafe_code,
    reason = "a global allocator can only be written with unsafe"
)]

use dmx_timing::sink::Sink;
use dmx_timing::{Config, Priority, Wait, start};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::time::Duration;

struct Counting;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
/// The output thread's OS id; 0 counts nothing.
static WATCHED: AtomicUsize = AtomicUsize::new(0);
/// The watched run's `looped` flag; null counts nothing.
static LOOPED: AtomicPtr<AtomicBool> = AtomicPtr::new(std::ptr::null_mut());

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: pthread_self has no preconditions.
        if unsafe { libc::pthread_self() } as usize == WATCHED.load(Ordering::SeqCst) {
            let looped = LOOPED.load(Ordering::SeqCst);
            // SAFETY: the test clears LOOPED before it drops the run that owns the flag.
            if !looped.is_null() && !unsafe { &*looped }.load(Ordering::SeqCst) {
                ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
            }
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn main() {
    // DDP needs someone listening, or every send is refused
    let receiver = std::net::UdpSocket::bind("127.0.0.1:0").expect("a local UDP port");
    let ddp = receiver.local_addr().expect("its address").to_string();
    // every sink without hardware, every thread setup and both ways of waiting
    for ddp_sink in [false, true] {
        for priority in [Priority::Default, Priority::Qos, Priority::Realtime] {
            for wait in [Wait::Sleep, Wait::Spin] {
                let sink = if ddp_sink {
                    Sink::ddp(&ddp).expect("a DDP sink")
                } else {
                    Sink::mock()
                };
                let name = if ddp_sink { "ddp" } else { "mock" };
                let config = Config {
                    priority,
                    wait,
                    duration: Duration::from_millis(500),
                };
                let running = start(config, sink).expect("the output thread starts");
                LOOPED.store(
                    std::sync::Arc::as_ptr(&running.looped).cast_mut(),
                    Ordering::SeqCst,
                );
                WATCHED.store(running.os_thread(), Ordering::SeqCst);
                while !running.ready.load(Ordering::SeqCst) {
                    std::hint::spin_loop();
                }
                let before = ALLOCATIONS.load(Ordering::SeqCst);
                while !running.looped.load(Ordering::SeqCst) {
                    std::hint::spin_loop();
                }
                let during = ALLOCATIONS.load(Ordering::SeqCst) - before;
                WATCHED.store(0, Ordering::SeqCst);
                LOOPED.store(std::ptr::null_mut(), Ordering::SeqCst);
                let report = running.finish();
                assert!(
                    report.frames > 1,
                    "{name} {priority:?} {wait:?}: no loop ran: {} frames",
                    report.frames
                );
                assert_eq!(
                    during, 0,
                    "{name} {priority:?} {wait:?}: {} frames made {during} allocations",
                    report.frames
                );
                println!(
                    "{name} {priority:?} {wait:?}: {} frames, no allocation",
                    report.frames
                );
            }
        }
    }
}
