#![cfg(feature = "test-support")]

#[path = "../../outputs/tests/counting/mod.rs"]
mod counting;

use engine::{Begun, ChannelNumbers, Color, Command, Engine, Target, Text, Universes};
use engine_core::Pixels;

// Claim: An admitted grab release frees heap memory on the tick if the sender drops before the wake.
// Source: docs/design/theory.md, idea 9; docs/design/channel-model.md, "No allocation, lock or blocking on the tick or output threads".
// Severity: medium. A closing command source can leave the tick in the allocator instead of serving a frame.
#[test]
fn finding_1_01_tick_frees_grab_after_sender_drops() {
    let (mut engine, mut sender, wakes) =
        Engine::start_stepped(Universes::ONE, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    sender
        .send(Command::PatchStrip {
            name: Text::new("one pixel").unwrap(),
            pixels: Pixels::try_from(1).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    assert!(matches!(
        sender
            .begin(
                Command::SetColour {
                    target: Target::Light { light: 1 },
                    red: u16::MAX,
                    green: 0,
                    blue: 0,
                },
                &[],
            )
            .unwrap(),
        Begun::Grabbed { .. }
    ));
    engine.latest();
    wakes.wake().unwrap();
    assert_eq!(engine.latest().sent(1, 1), 255, "the grab was visible");

    sender.send(Command::ReleaseAll).unwrap();
    let applied = sender.sent();
    drop(sender);
    let before = counting::tick_frees();
    wakes.wake().unwrap();
    let picture = engine.latest();
    assert_eq!(picture.applied(), applied, "the tick applied ReleaseAll");
    assert_eq!(picture.sent(1, 1), 0, "the grab was released");
    assert_eq!(
        counting::tick_frees() - before,
        0,
        "the tick must free no heap memory when it applies ReleaseAll"
    );
}
