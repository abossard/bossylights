use engine_core::{ChannelNumbers, Color, Lights, PatchCommand, Pixels, Text, Universes};
use looks::Looks;
use rig::{Patch, Whole};
use serde_json::{Value, json};
use std::num::NonZeroU16;

// Opening a show must refuse a pinned mode whose head points outside the mode.
// Source: docs/design/theory.md, owner decision Q18 ("A mode with a broken head is refused").
// Medium: reopening a damaged show can leave a fixture with incomplete pixel controls.
#[test]
fn finding_4_01_show_refuses_an_out_of_range_head_channel() {
    let empty = Patch::new(
        Universes::ONE,
        Lights::new(8).unwrap(),
        Whole(|_| NonZeroU16::MIN),
    );
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
    let valid = show_file::write(&patch, &Looks::new()).unwrap();
    show_file::read(&valid, &empty).expect("the original show opens");
    let mut file: Value = serde_json::from_str(&valid).unwrap();
    let modes = file["modes"].as_object_mut().unwrap();
    assert_eq!(modes.len(), 1);
    let mode = &mut modes.values_mut().next().unwrap()["mode"];
    assert_eq!(mode["channels"].as_array().unwrap().len(), 3);
    assert_eq!(mode["pixels"], json!([[0, 1, 2]]));
    mode["pixels"] = json!([[0, 1, 3]]);

    let opened = show_file::read(&file.to_string(), &empty).map(|_| "opened");
    assert!(
        opened.is_err(),
        "expected the show to be refused: head index 3 is outside its three channels; got {opened:?}"
    );
}
