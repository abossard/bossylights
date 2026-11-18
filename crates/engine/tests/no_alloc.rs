//! Proves the tick thread allocates nothing per tick, also while it takes commands.
//! A counting allocator sees every allocation in this process. While the engine
//! ticks, the main thread only sends commands, which allocates nothing, and sleeps,
//! so any allocation is the tick's.

#![expect(
    unsafe_code,
    reason = "a global allocator can only be written with unsafe"
)]

use engine::{ChannelNumbers, Command, Engine, Universes};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

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
    let (engine, mut sender) =
        Engine::start(Universes::ONE, |_| {}).expect("the tick thread starts");
    thread::sleep(Duration::from_millis(100)); // thread start-up may allocate

    let allocations = ALLOCATIONS.load(Ordering::SeqCst);
    let frame = engine.query().frame;
    for value in 0..=255 {
        let channel = ChannelNumbers {
            universe: 1,
            address: 1 + u16::from(value),
        };
        let _ = sender.send(Command::SetChannel { channel, value });
        let _ = sender.send(Command::Blackout { on: value % 2 == 0 });
        thread::sleep(Duration::from_millis(2));
    }
    let ticks = engine.query().frame - frame;
    let new_allocations = ALLOCATIONS.load(Ordering::SeqCst) - allocations;
    drop(engine);

    assert!(ticks >= 5, "too few ticks to prove anything: {ticks}");
    assert_eq!(
        new_allocations, 0,
        "{ticks} ticks made {new_allocations} allocations"
    );
}
