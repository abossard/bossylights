use engine_core::{ChannelNumbers, Color, Lights, PatchCommand, Pixels, Text, Universes};
use looks::Looks;
use rig::{Patch, Whole};
use serde_json::{Value, json};
use std::num::NonZeroU16;

#[test]
fn a_v5_show_near_its_compiled_plan_limit_still_opens() {
    // With this strip width these sequence plans fit in v5's 64 MiB limit.
    const SEQUENCES: u16 = 1018;
    let empty = Patch::new(
        Universes::new(8).unwrap(),
        Lights::MOST,
        Whole(|_| NonZeroU16::MIN),
    );
    let (patch, _) = empty
        .update(
            PatchCommand::PatchStrip {
                name: Text::new("Wall").unwrap(),
                pixels: Pixels::try_from(1168).unwrap(),
                color: Color::Rgb,
                at: ChannelNumbers {
                    universe: 1,
                    address: 1,
                },
            },
            &[],
        )
        .unwrap();
    let text = show_file::write(&patch.unwrap(), &Looks::new()).unwrap();
    let mut v5: Value = serde_json::from_str(&text).unwrap();
    v5["version"] = json!(5);
    let step = json!({"look": 1, "name": "Red", "step": {
        "target": {"Fixture": {"fixture": 1}},
        "values": {"colour": {"red": 65535, "green": 0, "blue": 0}}
    }});
    let sequence =
        |look| json!({"look": look, "name": "Sequence", "sequence": {"items": [{"look": 1}]}});
    let mut rows = vec![step];
    rows.extend((2..=SEQUENCES + 1).map(sequence));
    v5["looks"] = json!(rows);

    let (_, restored) = show_file::read(&v5.to_string(), &empty).unwrap();
    assert_eq!(restored.rows().count(), usize::from(SEQUENCES) + 1);
}
