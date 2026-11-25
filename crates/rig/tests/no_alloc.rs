//! Proves a frame of 512 patched lights allocates and frees nothing: the patch makes the
//! field table and each light's words first (off the tick, it may allocate), then every
//! frame each light's words offer on its fields through the table and one mixer decides
//! each channel, as the tick thread does. A counting allocator sees every allocation and
//! free in this process; the window holds only frames, on this one thread. That the
//! engine's own tick allocates nothing is proved in `crates/engine/tests/no_alloc.rs`.

mod common;
#[path = "../../outputs/tests/counting/mod.rs"]
mod counting;

use common::{fixture, light, lights};
use engine_core::{
    ChannelNumbers, ChannelSet, Channels, EngineCommand, GrabChange, GrabEntry, Grabs, Levels,
    Light, LightRun, PatchCommand, Position, Priority, Rgb, State, Text, Universes, frame, update,
};
use rig::{Patch, Words};

const UNIVERSES: u16 = 8;
const LIGHTS: u16 = 512;
const FRAMES: u16 = 1_000;
/// Values the lights take in turn, one set per frame.
const KINDS: u16 = 6;
const RED: Rgb = Rgb {
    red: u16::MAX,
    green: 0,
    blue: 0,
};

fn text(text: &str) -> Text {
    Text::new(text).unwrap()
}

/// The value of light `n` in a frame of kind `kind`: every other kind a colour on the
/// wheels, so a wheel changes its slot; gobos on wheel 1, wheel 2, then both.
fn value(kind: u16, n: u16) -> Light {
    let step = kind.wrapping_mul(97).wrapping_add(n);
    let colour = if kind.is_multiple_of(2) {
        Rgb {
            red: step.wrapping_mul(3),
            green: step.wrapping_mul(5),
            blue: step.wrapping_mul(7),
        }
    } else {
        RED
    };
    let slot = Some(1 + u8::try_from(step % 8).unwrap());
    Light {
        intensity: Some(step),
        colour: Some(colour),
        position: Some(Position {
            pan: step,
            tilt: step.wrapping_mul(11),
        }),
        zoom: Some(step.wrapping_mul(13)),
        gobos: match kind % 3 {
            0 => [slot, None, None],
            1 => [None, slot, None],
            _ => [slot, slot, None],
        },
    }
}

fn main() {
    let universes = Universes::new(UNIVERSES).unwrap();
    // 64 moving heads: 16-bit dimmer and shutter, colour and gobo wheels, a zoom over part
    // of its channel; then RGBW PARs, RGB PARs and dimmers
    let heads = [
        ("Martin-Rush-MH6-Wash.qxf", "Standard"),
        ("Shehds-LED-Spot-100W.qxf", "14 Channel"),
        ("Martin-MAC-2000-Wash.qxf", "8-bit with Color Wheel 2"),
        ("Elation-Platinum-Spot-5R.qxf", "Basic"),
    ];
    let modes = [
        ("Generic-Generic-RGBW.qxf", "Dimmer RGBW"),
        ("Generic-Generic-RGB.qxf", "RGB"),
        ("Showtec-Par-64-100W-COB-UV.qxf", "8 Bit"),
    ];
    let library: Vec<_> = heads
        .iter()
        .chain(&modes)
        .map(|(file, _)| fixture(file))
        .collect();
    let rig = heads.iter().cycle().take(64).chain(modes.iter().cycle());
    let (mut patch, mut at) = (Patch::new(universes, lights(), common::whole()), (1, 1));
    for (file, mode) in rig.take(usize::from(LIGHTS)) {
        let fixture = &library[heads
            .iter()
            .chain(&modes)
            .position(|f| f.0 == *file)
            .unwrap()];
        let size = fixture
            .modes
            .iter()
            .find(|m| m.name == *mode)
            .unwrap()
            .channels
            .len();
        let size = u16::try_from(size).unwrap();
        if at.1 + size > 513 {
            at = (at.0 + 1, 1);
        }
        let command = PatchCommand::Patch {
            name: text(file),
            maker: text(&fixture.maker),
            model: text(&fixture.model),
            mode: text(mode),
            at: ChannelNumbers {
                universe: at.0,
                address: at.1,
            },
        };
        patch = patch.update(command, &library).unwrap().0.unwrap();
        at.1 += size;
    }
    let plan = patch.prepare();
    let lit = (0..LIGHTS)
        .filter(|&n| !plan.targets().of(n).0.is_empty())
        .count();
    assert_eq!(lit, usize::from(LIGHTS), "every fixture here is one light");
    // each light's words in each kind of frame, and a grab entry per light
    let words: Vec<Vec<Words>> = (0..KINDS)
        .map(|kind| {
            (0..LIGHTS)
                .map(|n| {
                    let run = LightRun::new(light(n), 1, lights()).unwrap();
                    patch.words(&[run], &value(kind, n))
                })
                .collect()
        })
        .collect();
    let (mut raw, mut holds) = (
        vec![Levels::ZERO; usize::from(UNIVERSES)],
        vec![ChannelSet::NONE; usize::from(UNIVERSES)],
    );
    let mut grabs = Box::new(Grabs::NONE);
    let mut state = State::new(Priority::RealTime);
    let entries: Vec<GrabEntry> = (0..usize::from(LIGHTS))
        .map(|n| GrabEntry::new(n).unwrap())
        .collect();
    for &entry in &entries {
        let start = EngineCommand::Grab {
            entry,
            change: GrabChange::Start,
        };
        state = update(state, &mut raw, &mut holds, &mut grabs, start);
    }
    let mut channels = Box::new(Channels::NEW);
    channels.configure(&[], plan.configured());
    let targets = plan.targets();

    let (allocations, frees) = (counting::allocations(), counting::frees());
    for at in 0..FRAMES {
        let kind = &words[usize::from(at % KINDS)];
        frame(
            &state,
            &grabs,
            (&raw, &holds),
            plan.configured(),
            &mut channels,
            |offers| {
                for (words, &entry) in kind.iter().zip(&entries) {
                    words.offer(targets, |field, word| offers.grab(entry, field, word));
                }
            },
        );
    }
    let new_allocations = counting::allocations() - allocations;
    let new_frees = counting::frees() - frees;

    let written = channels
        .values()
        .iter()
        .flat_map(|u| u.0)
        .filter(|&b| b != 0)
        .count();
    assert!(written > 1_000, "the frames wrote bytes: {written}");
    assert_eq!(
        (new_allocations, new_frees),
        (0, 0),
        "{FRAMES} frames of {LIGHTS} lights: allocations and frees"
    );
}
