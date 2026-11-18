//! Proves a frame of 512 patched lights allocates nothing: the rig compiles the writers
//! first (it may allocate), then every frame the light values change and `resolve` writes
//! the universes. A counting allocator sees every allocation in this process; the window
//! holds only frames. That the engine's own tick allocates nothing is proved in
//! `crates/engine/tests/no_alloc.rs`.

#![expect(
    unsafe_code,
    reason = "a global allocator can only be written with unsafe"
)]

mod common;

use common::{light, lights, mode};
use engine_core::{ChannelNumbers, Levels, Light, Universes, Writer, resolve};
use rig::compile;
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

const UNIVERSES: u16 = 8;
const LIGHTS: u16 = 512;
const FRAMES: u16 = 1_000;

fn main() {
    let universes = Universes::new(UNIVERSES).unwrap();
    // 64 moving heads (16-bit dimmer, shutter), then RGBW PARs, RGB PARs and dimmers
    let heads = mode("Martin-Rush-MH6-Wash.qxf", "Standard");
    let modes = [
        mode("Generic-Generic-RGBW.qxf", "Dimmer RGBW"),
        mode("Generic-Generic-RGB.qxf", "RGB"),
        mode("Showtec-Par-64-100W-COB-UV.qxf", "8 Bit"),
    ];
    let patch = std::iter::repeat_n(&heads, 64).chain(modes.iter().cycle());
    let (mut writers, mut next, mut at) = (Vec::<Writer>::new(), 0, (1, 1));
    for mode in patch.take(usize::from(LIGHTS)) {
        let size = u16::try_from(mode.channels.len()).unwrap();
        if at.1 + size > 513 {
            at = (at.0 + 1, 1);
        }
        let place = ChannelNumbers {
            universe: at.0,
            address: at.1,
        };
        let rigged = compile(mode, place, light(next), universes, lights()).unwrap();
        writers.extend(rigged.writers);
        next += rigged.lights;
        at.1 += size;
    }
    assert_eq!(next, LIGHTS, "every fixture here is one light");
    let mut values = vec![Light::DARK; usize::from(LIGHTS)];
    let mut out = vec![Levels::ZERO; usize::from(UNIVERSES)];

    let allocations = ALLOCATIONS.load(Ordering::SeqCst);
    for frame in 0..FRAMES {
        for (n, value) in (0..).zip(values.iter_mut()) {
            let step = frame.wrapping_mul(97).wrapping_add(n);
            *value = Light {
                intensity: step,
                red: step.wrapping_mul(3),
                green: step.wrapping_mul(5),
                blue: step.wrapping_mul(7),
            };
        }
        resolve(&values, &writers, &mut out);
    }
    let new_allocations = ALLOCATIONS.load(Ordering::SeqCst) - allocations;

    let written = out.iter().flat_map(|u| u.0).filter(|&b| b != 0).count();
    assert!(written > 1_000, "the frames wrote bytes: {written}");
    assert_eq!(
        new_allocations, 0,
        "{FRAMES} frames of {LIGHTS} lights made {new_allocations} allocations"
    );
}
