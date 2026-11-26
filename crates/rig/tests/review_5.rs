mod common;

use engine_core::{
    ChannelNumbers, Color, Lights, PatchCommand, Pixels, RunNumbers, Text, Universes,
};
use rig::Patch;
use std::panic::catch_unwind;

// A fixture wholly before an output's run must not affect its patched slot range.
// Source: Patch::patched_in ("None: no fixture has a channel there") and docs/engine.md,
// "After the handoff".
// High: asking for health can panic while reporting delay for a valid second-universe output.
#[test]
fn finding_5_01_health_range_ignores_fixtures_before_the_run() {
    let universes = Universes::new(2).unwrap();
    let run = RunNumbers { first: 2, count: 1 }.check(universes).unwrap();
    let empty = Patch::new(universes, Lights::new(8).unwrap(), common::whole());
    let patch = empty
        .update(
            PatchCommand::PatchStrip {
                name: Text::new("Strip").unwrap(),
                pixels: Pixels::try_from(1).unwrap(),
                color: Color::Rgb,
                at: ChannelNumbers {
                    universe: 1,
                    address: 1,
                },
            },
            &[],
        )
        .unwrap()
        .0
        .unwrap();

    let range = catch_unwind(|| patch.patched_in(run)).map_err(|_| "panicked");
    assert_eq!(
        range,
        Ok(None),
        "universe 2 has no patched channels; the strip occupies universe 1 slots 1 to 3"
    );
}
