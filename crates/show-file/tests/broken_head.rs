use engine_core::{ChannelNumbers, Color, Lights, PatchCommand, Pixels, Text, Universes, Unopened};
use looks::Looks;
use rig::{Patch, Whole};
use serde_json::{Value, json};
use std::num::NonZeroU16;

// Opening a show must refuse a pinned mode whose head points outside the mode.
// Source: docs/design/theory.md, owner decision Q18 ("A mode with a broken head is refused").
// Medium: reopening a damaged show can leave a fixture with incomplete pixel controls.
// Finding 4.01 of #349.
#[test]
fn a_show_whose_head_names_a_channel_past_its_mode_is_refused() {
    let cases = [
        (
            "the last channel of the only head",
            1,
            json!([[0, 1, 2]]),
            json!([[0, 1, 3]]),
        ),
        (
            "a middle channel of the second head",
            2,
            json!([[0, 1, 2], [3, 4, 5]]),
            json!([[0, 1, 2], [3, 6, 5]]),
        ),
    ];
    for (case, pixels, written, broken) in cases {
        let empty = Patch::new(
            Universes::ONE,
            Lights::new(8).unwrap(),
            Whole(|_| NonZeroU16::MIN),
        );
        let patch = empty
            .update(
                PatchCommand::PatchStrip {
                    name: Text::new("Strip").unwrap(),
                    pixels: Pixels::try_from(pixels).unwrap(),
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
        assert_eq!(modes.len(), 1, "{case}");
        let mode = &mut modes.values_mut().next().unwrap()["mode"];
        assert_eq!(
            mode["channels"].as_array().unwrap().len(),
            3 * usize::from(pixels),
            "{case}"
        );
        assert_eq!(mode["pixels"], written, "{case}");
        mode["pixels"] = broken;

        let opened = show_file::read(&file.to_string(), &empty).map(|_| "opened");
        assert!(
            matches!(opened, Err(Unopened::Broken { .. })),
            "{case}: expected the show to be refused, a head names a channel past the mode; got {opened:?}"
        );
    }
}
