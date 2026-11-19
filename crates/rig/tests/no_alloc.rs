//! Proves a frame of 512 patched lights allocates and frees nothing: the rig compiles the
//! writers first (it may allocate), then every frame the light values change and `resolve`
//! writes the universes, as the tick thread does. A counting allocator sees every
//! allocation and free in this process; the window holds only frames, on this one thread. That the engine's own tick allocates nothing is proved in
//! `crates/engine/tests/no_alloc.rs`.

mod common;
#[path = "../../outputs/tests/counting/mod.rs"]
mod counting;

use common::{light, lights, mode};
use engine_core::{ChannelNumbers, Levels, Light, Position, Rgb, Universes, Writer, resolve};
use rig::compile;

const UNIVERSES: u16 = 8;
const LIGHTS: u16 = 512;
const FRAMES: u16 = 1_000;
const RED: Rgb = Rgb {
    red: u16::MAX,
    green: 0,
    blue: 0,
};

fn main() {
    let universes = Universes::new(UNIVERSES).unwrap();
    // 64 moving heads: 16-bit dimmer and shutter, colour and gobo wheels, a zoom over part
    // of its channel; then RGBW PARs, RGB PARs and dimmers
    let heads = [
        mode("Martin-Rush-MH6-Wash.qxf", "Standard"),
        mode("Shehds-LED-Spot-100W.qxf", "14 Channel"),
        mode("Martin-MAC-2000-Wash.qxf", "8-bit with Color Wheel 2"),
        mode("Elation-Platinum-Spot-5R.qxf", "Basic"),
    ];
    let modes = [
        mode("Generic-Generic-RGBW.qxf", "Dimmer RGBW"),
        mode("Generic-Generic-RGB.qxf", "RGB"),
        mode("Showtec-Par-64-100W-COB-UV.qxf", "8 Bit"),
    ];
    let patch = heads.iter().cycle().take(64).chain(modes.iter().cycle());
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
    assert!(
        writers.iter().any(|w| matches!(w, Writer::Picks { .. })),
        "wheels are written"
    );
    let mut values = vec![Light::DARK; usize::from(LIGHTS)];
    let mut out = vec![Levels::ZERO; usize::from(UNIVERSES)];

    let (allocations, frees) = (counting::allocations(), counting::frees());
    for frame in 0..FRAMES {
        for (n, value) in (0..).zip(values.iter_mut()) {
            let step = frame.wrapping_mul(97).wrapping_add(n);
            // every other frame a colour on the wheels, so a wheel changes its slot
            let colour = if frame % 2 == 0 {
                Rgb {
                    red: step.wrapping_mul(3),
                    green: step.wrapping_mul(5),
                    blue: step.wrapping_mul(7),
                }
            } else {
                RED
            };
            *value = Light {
                intensity: Some(step),
                colour: Some(colour),
                position: Some(Position {
                    pan: step,
                    tilt: step.wrapping_mul(11),
                }),
                zoom: Some(step.wrapping_mul(13)),
                // every other frame the other wheel
                gobos: if frame % 2 == 0 {
                    [Some(1 + u8::try_from(step % 8).unwrap()), None, None]
                } else {
                    [None, Some(1 + u8::try_from(step % 8).unwrap()), None]
                },
            };
        }
        resolve(&values, &writers, &mut out);
    }
    let new_allocations = counting::allocations() - allocations;
    let new_frees = counting::frees() - frees;

    let written = out.iter().flat_map(|u| u.0).filter(|&b| b != 0).count();
    assert!(written > 1_000, "the frames wrote bytes: {written}");
    assert_eq!(
        (new_allocations, new_frees),
        (0, 0),
        "{FRAMES} frames of {LIGHTS} lights: allocations and frees"
    );
}
