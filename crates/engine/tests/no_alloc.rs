//! Proves the tick thread allocates nothing per wake, also while it serves two outputs at
//! different rates, takes commands and hands a reader a snapshot on every frame. A
//! counting allocator sees every allocation in this process. The outputs are attached
//! before the window (a device is boxed on the sending thread). In the window the main
//! thread only sends channel commands and reads snapshots, which allocates nothing, and
//! sleeps, so any allocation is the tick's or the reader's. That each real device's `take`
//! allocates nothing is proved in `crates/outputs/tests/no_alloc.rs`.

#![expect(
    unsafe_code,
    reason = "a global allocator can only be written with unsafe"
)]

mod mock;

use engine::{ChannelNumbers, Command, Engine, Rate, RunNumbers, Universes};
use std::alloc::{GlobalAlloc, Layout, System};
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
    let (mut engine, mut sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
    let run = RunNumbers { first: 1, count: 1 };
    mock::attach(&mut sender, Universes::ONE, 1, run, Rate::Sixty);
    mock::attach(&mut sender, Universes::ONE, 2, run, Rate::Thirty);
    // Thread start-up may allocate: wait until both outputs were served.
    mock::wait_until("the outputs were never served", || {
        let outputs = engine.latest().state.outputs;
        outputs[..2]
            .iter()
            .all(|o| o.is_some_and(|o| o.counters.accepted > 0))
    });

    let allocations = ALLOCATIONS.load(Ordering::SeqCst);
    let frame = engine.latest().state.frame;
    let mut pictures = 0;
    for value in 0..=255 {
        let channel = ChannelNumbers {
            universe: 1,
            address: 1 + u16::from(value),
        };
        let _ = sender.send(Command::SetChannel { channel, value });
        let _ = sender.send(Command::Blackout { on: value % 2 == 0 });
        // Pace the commands by the tick: a reader on every frame waits for the next picture.
        let seen = engine.latest().state.frame;
        mock::wait_until("no frame went by", || engine.latest().state.frame > seen);
        pictures += 1;
    }
    let state = engine.latest().state;
    let ticks = state.frame - frame;
    let new_allocations = ALLOCATIONS.load(Ordering::SeqCst) - allocations;
    drop(engine);

    assert!(ticks >= 5, "too few ticks to prove anything: {ticks}");
    assert_eq!(pictures, 256, "the reader saw every frame it waited for");
    let accepted = state.outputs.map(|o| o.map(|o| o.counters.accepted));
    assert!(
        accepted[..2].iter().all(|n| n.is_some_and(|n| n > 5)),
        "both outputs were served: {accepted:?}"
    );
    assert_eq!(
        new_allocations, 0,
        "{ticks} ticks made {new_allocations} allocations"
    );
}
