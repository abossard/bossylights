//! Proves turning the values of 512 patched lights into bytes allocates and frees nothing:
//! the rig compiles the fields of each light first (it may allocate), then every frame the
//! light values change and each light's offers are laid on its channels. Nothing of the rig
//! runs on the tick thread any more: a step, a grab and a swatch become words off the
//! tick, and the tick mixes words (`crates/engine/tests/no_alloc.rs` proves it allocates
//! nothing). This keeps the words of a light free of the heap, so a reused buffer holds
//! them. A counting allocator sees every allocation and free in this process; the window
//! holds only frames, on this one thread.

mod common;
#[path = "../../outputs/tests/counting/mod.rs"]
mod counting;

use common::{lay, light, lights, mode};
use engine_core::{ADDRESSES, ChannelNumbers, Light, Position, Rgb, Universes};
use rig::{Feed, LightFields, compile};

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
    let (mut targets, mut next, mut at) = (Vec::<LightFields>::new(), 0, (1, 1));
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
        next += u16::try_from(rigged.targets.len()).unwrap();
        targets.extend(rigged.targets);
        at.1 += size;
    }
    assert_eq!(next, LIGHTS, "every fixture here is one light");
    assert!(
        targets
            .iter()
            .flat_map(LightFields::feeds)
            .any(|feed| matches!(feed, Feed::Picks(_))),
        "wheels are fed"
    );
    let mut values = vec![Light::DARK; usize::from(LIGHTS)];
    let mut out = vec![0_u8; usize::from(UNIVERSES) * ADDRESSES];

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
                // wheel 1, wheel 2, then both at once
                gobos: {
                    let slot = Some(1 + u8::try_from(step % 8).unwrap());
                    match frame % 3 {
                        0 => [slot, None, None],
                        1 => [None, slot, None],
                        _ => [slot, slot, None],
                    }
                },
            };
        }
        for (target, value) in targets.iter().zip(&values) {
            lay(&mut out, 0, target.offers(value));
        }
    }
    let new_allocations = counting::allocations() - allocations;
    let new_frees = counting::frees() - frees;

    let written = out.iter().filter(|&&b| b != 0).count();
    assert!(written > 1_000, "the frames wrote bytes: {written}");
    assert_eq!(
        (new_allocations, new_frees),
        (0, 0),
        "{FRAMES} frames of {LIGHTS} lights: allocations and frees"
    );
}
