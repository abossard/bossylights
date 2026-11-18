//! A query over a whole library allocates a handful of times, not once per letter of every name.

#![expect(
    unsafe_code,
    reason = "a global allocator can only be written with unsafe"
)]

use fixture_library::{Fixture, parse, search};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn library() -> Vec<Fixture> {
    let files = [
        "Cameo-CLBARL10RGBA.qxf",
        "Lanta-Fireball-P64s.qxf",
        "American-DJ-Ultra-Bar-6.qxf",
        "Showtec-Par-64-100W-COB-UV.qxf",
        "Elation-EVC-MH.qxf",
        "JB-Systems-Lounge-Laser-DMX.qxf",
    ];
    let real: Vec<Fixture> = files
        .iter()
        .map(|file| {
            let path = format!("{}/tests/definitions/{file}", env!("CARGO_MANIFEST_DIR"));
            parse(file, &std::fs::read(path).expect("in the repository")).expect("reads")
        })
        .collect();
    real.iter().cycle().take(1782).cloned().collect()
}

// query
#[test]
fn a_query_over_1782_fixtures_allocates_under_a_thousand_times() {
    let library = library();
    for query in ["p", "par", "led bar", "clbar10rgba", "rgb", "lounge lazer"] {
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let found = search(&library, query);
        let used = ALLOCATIONS.load(Ordering::Relaxed) - before;
        std::hint::black_box(&found);
        assert!(used < 1000, "{query:?}: {used} allocations");
    }
}
