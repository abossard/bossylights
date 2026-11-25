//! The counting allocator of the `no_alloc` tests of `outputs`, `engine` and `rig` (the
//! others take this file by its path). It counts every allocation and every free in the
//! process, except the allocations of a thread inside `uncounted` and the work of the
//! Art-Net discovery thread (it never runs a `take`, and allocates when a node comes or
//! goes), and apart the frees of the tick thread. Threads are told by their name.

#![allow(dead_code, reason = "each test file uses its own part")]
#![expect(
    unsafe_code,
    reason = "a global allocator can only be written with unsafe"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

/// The name `engine` gives the thread `rt-thread` starts.
const TICK: &str = "tick";
/// The name of the thread that finds Art-Net nodes.
const DISCOVERY: &str = "artnet-discovery";

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static FREES: AtomicUsize = AtomicUsize::new(0);
static TICK_FREES: AtomicUsize = AtomicUsize::new(0);

/// Whether this thread is the tick thread, once known.
#[derive(Clone, Copy)]
enum Thread {
    Unknown,
    Tick,
    Discovery,
    Other,
}

thread_local! {
    static UNCOUNTED: Cell<bool> = const { Cell::new(false) };
    static THIS: Cell<Thread> = const { Cell::new(Thread::Unknown) };
}

/// Which thread this is. Asks for its name once per thread.
fn this_thread() -> Thread {
    THIS.try_with(|this| match this.get() {
        Thread::Unknown => {
            // Other first: asking for the name may allocate and free on this thread.
            this.set(Thread::Other);
            let found = match thread::current().name() {
                Some(TICK) => Thread::Tick,
                Some(DISCOVERY) => Thread::Discovery,
                _ => Thread::Other,
            };
            this.set(found);
            found
        }
        known @ (Thread::Tick | Thread::Discovery | Thread::Other) => known,
    })
    .unwrap_or(Thread::Other)
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let uncounted = UNCOUNTED.try_with(Cell::get).unwrap_or(false);
        if !uncounted && !matches!(this_thread(), Thread::Discovery) {
            ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        match this_thread() {
            Thread::Discovery => {}
            Thread::Tick => {
                FREES.fetch_add(1, Ordering::SeqCst);
                TICK_FREES.fetch_add(1, Ordering::SeqCst);
            }
            Thread::Unknown | Thread::Other => {
                FREES.fetch_add(1, Ordering::SeqCst);
            }
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Allocations so far.
pub fn allocations() -> usize {
    ALLOCATIONS.load(Ordering::SeqCst)
}

/// Frees so far, on every thread.
pub fn frees() -> usize {
    FREES.load(Ordering::SeqCst)
}

/// Frees so far on the tick thread.
pub fn tick_frees() -> usize {
    TICK_FREES.load(Ordering::SeqCst)
}

/// Runs `work` with this thread's allocations not counted.
pub fn uncounted<T>(work: impl FnOnce() -> T) -> T {
    UNCOUNTED.with(|flag| flag.set(true));
    let result = work();
    UNCOUNTED.with(|flag| flag.set(false));
    result
}
