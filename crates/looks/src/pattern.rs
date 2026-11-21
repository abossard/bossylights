//! Resolve a Wave's swatches and bind its spatial phase before a play reaches the tick.

use crate::{Looks, sequence};
use alloc::{boxed::Box, collections::BTreeSet, vec, vec::Vec};
use core::mem::size_of_val;
use engine_core::{
    Definition, LightNumber, LightRun, Lights, Node, Palette, Pattern, Property, Refusal, Swatches,
    Target, WaveTarget,
};
use rig::{Feed, Part, Patch};

pub(crate) fn reference(pattern: &Pattern) -> Option<u16> {
    match pattern {
        Pattern::Wave {
            palette: Palette::Step { look },
            ..
        } => Some(*look),
        Pattern::Wave {
            palette: Palette::Inline(_),
            ..
        } => None,
    }
}

pub(crate) fn compile(
    looks: &Looks,
    pattern: &Pattern,
    patch: &Patch,
) -> Result<(Node, Swatches, WaveBinding), Refusal> {
    let Pattern::Wave {
        target,
        period,
        palette,
    } = *pattern;
    if !(1..=32).contains(&period) {
        return Err(Refusal::InvalidTiming);
    }
    let swatches = match palette {
        Palette::Inline(swatches) => swatches,
        Palette::Step { look } => match &looks.get(look).ok_or(Refusal::NoSuchLook)?.definition {
            Definition::Step(step) => step.swatches,
            Definition::Pattern(_) | Definition::Sequence(_) => return Err(Refusal::WrongLookKind),
        },
    };
    if swatches.is_empty() {
        return Err(Refusal::EmptyPalette);
    }
    let count = u8::try_from(swatches.as_slice().len()).map_err(|_| Refusal::WorkLimit)?;
    Ok((
        Node::Wave {
            period,
            swatches: count,
        },
        swatches,
        WaveBinding::new(target, patch)?,
    ))
}

/// Retained samples need the same capable lights after a patch edit. The order of
/// group members can change without changing which lights the frozen colours belong to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct WaveBinding {
    target: WaveTarget,
    runs: Box<[LightRun]>,
    pub columns: Option<u16>,
}

impl WaveBinding {
    fn new(target: WaveTarget, patch: &Patch) -> Result<Self, Refusal> {
        let (runs, columns) = match target {
            WaveTarget::Group { group } => (patch.runs(Target::Group { group })?, None),
            WaveTarget::Matrix { fixture } => {
                let mode = patch.mode(fixture).ok_or(Refusal::NotPatched)?;
                let layout = mode.layout.ok_or(Refusal::NoMatrix)?;
                let cells = layout
                    .width
                    .checked_mul(layout.height)
                    .filter(|&n| n != 0 && usize::from(n) == mode.pixels.len())
                    .ok_or(Refusal::NoMatrix)?;
                // Rig places the heads last, in definition order. A fixture's own
                // light (e.g. its shared dimmer) precedes them and has no cell.
                let runs = patch.runs(Target::Fixture { fixture })?;
                let run = runs.first().ok_or(Refusal::NotPatched)?;
                let first = run.first().index()
                    + run.count().checked_sub(cells).ok_or(Refusal::NoMatrix)?;
                let heads = LightNumber::new(first, Lights::MOST)
                    .and_then(|first| LightRun::new(first, cells, Lights::MOST))
                    .ok_or(Refusal::NoMatrix)?;
                (vec![heads], Some(layout.width))
            }
        };
        if runs.is_empty() {
            return Err(Refusal::EmptyGroup);
        }
        // Rig compiles additive and subtractive mixes to continuous component fields.
        // A fixed wheel bypass has no field, a slot alone cannot blend.
        let continuous: BTreeSet<u16> = (0..)
            .zip(patch.targets())
            .filter(|(_, target)| {
                target.feeds().iter().any(|feed| {
                    matches!(feed, Feed::Takes(take)
                        if matches!(take.of, Part::Red | Part::Green | Part::Blue | Part::Shared))
                })
            })
            .map(|(light, _)| light)
            .collect();
        if let Some(index) = runs
            .iter()
            .copied()
            .flat_map(sequence::numbers)
            .find(|light| !continuous.contains(light))
        {
            return Err(Refusal::Unsupported {
                light: index + 1,
                property: Property::Colour,
            });
        }
        Ok(Self {
            target,
            runs: runs.into_boxed_slice(),
            columns,
        })
    }

    pub fn ordered(&self, patch: &Patch) -> Result<Vec<u16>, Refusal> {
        match self.target {
            WaveTarget::Group { group } => sequence::ordered_group(group, patch),
            WaveTarget::Matrix { .. } => Ok(self
                .runs
                .iter()
                .copied()
                .flat_map(sequence::numbers)
                .collect()),
        }
    }

    pub fn matches(&self, patch: &Patch) -> bool {
        Self::new(self.target, patch).is_ok_and(|now| now == *self)
    }

    pub fn storage(all: &[Self]) -> usize {
        size_of_val(all)
            + all
                .iter()
                .map(|wave| size_of_val(wave.runs.as_ref()))
                .sum::<usize>()
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "the fixture supplies the named looks, groups and light runs"
)]
mod tests {
    use super::*;
    use crate::tests::{item, leaf, offered, patch, sequence};
    use crate::{WORK_LIMIT, play_work};
    use core::num::NonZeroU16;
    use engine_core::{
        ChannelNumbers, Color, PatchCommand, Pixels, SavedLook, SequenceItem, Text, Universes, Use,
    };
    use rig::Whole;

    fn wave(
        look: u16,
        target: engine_core::WaveTarget,
        period: u8,
        palette: engine_core::Palette,
    ) -> SavedLook {
        SavedLook {
            look,
            name: Text::new("wave").unwrap(),
            definition: Definition::Pattern(engine_core::Pattern::Wave {
                target,
                period,
                palette,
            }),
        }
    }

    #[test]
    fn wave_rebinds_group_order_and_every_palette_reference_in_one_revision() {
        use engine_core::{AuthoredStep, Member, Members, Palette, Rgb, Swatches, WaveTarget};

        const BEAT: u64 = 1 << 32;
        let row = Text::new("row").unwrap();
        let group = WaveTarget::Group { group: row };
        let grouped = |patch: &Patch, order: [u16; 3]| {
            patch
                .update(
                    PatchCommand::SetGroup {
                        name: row,
                        members: Members::new(&order.map(|light| Member::Light { light })).unwrap(),
                    },
                    &[],
                )
                .unwrap()
                .0
                .unwrap()
        };
        let original = grouped(&patch(), [3, 1, 3]);
        let reversed = grouped(&original, [1, 3, 1]);
        let colour = |red| Rgb {
            red,
            green: 0,
            blue: 0,
        };
        let palette = |swatches: &[u16]| {
            let mut saved = SavedLook::from(leaf(1));
            saved.definition = Definition::Step(AuthoredStep {
                swatches: Swatches::new(&swatches.iter().copied().map(colour).collect::<Vec<_>>())
                    .unwrap(),
                ..AuthoredStep::from(leaf(1).step)
            });
            saved
        };
        let spread = Use {
            spread: 100,
            ..Use::default()
        };
        let looks = Looks::new()
            .put(palette(&[0, 65535]), &original)
            .unwrap()
            .put(wave(2, group, 2, Palette::Step { look: 1 }), &original)
            .unwrap()
            .put(
                sequence(
                    3,
                    &[SequenceItem {
                        usage: spread,
                        width: 32,
                        ..item(2)
                    }],
                ),
                &original,
            )
            .unwrap();
        let reordered = looks.rebind(&reversed).unwrap();
        let edited = reordered.put(palette(&[3000, 2000]), &reversed).unwrap();
        assert_eq!(
            edited.put(palette(&[]), &reversed),
            Err(Refusal::EmptyPalette)
        );
        assert_eq!(edited.remove(1), Err(Refusal::LookInUse { look: 2 }));
        for (case, revision, elapsed, expected) in [
            ("first distinct member", &looks, 0, [65535, 0]),
            ("wrap", &looks, 2 * BEAT, [65535, 0]),
            (
                "half-way, round to nearest",
                &looks,
                BEAT / 2,
                [32768, 32768],
            ),
            ("reverse", &reordered, 0, [0, 65535]),
            ("replace the palette", &edited, 0, [3000, 2000]),
        ] {
            for (root, usage) in [(2, spread), (3, Use::default())] {
                let plan = revision.plan(root).unwrap();
                assert!(plan.uses(1), "{case}, root {root} retains the palette id");
                assert_eq!(
                    offered(&plan, elapsed, usage),
                    [
                        (0, expected[0]),
                        (1, 0),
                        (2, 0),
                        (6, expected[1]),
                        (7, 0),
                        (8, 0)
                    ],
                    "{case} root {root}"
                );
            }
        }
        let changed_members = grouped(&original, [3, 2, 3]);
        let retained = looks.plan(3).unwrap();
        assert!(retained.bindings_match(&reversed));
        assert!(!retained.bindings_match(&changed_members));
    }

    #[test]
    fn a_three_column_one_row_matrix_uses_its_width_for_spread() {
        use engine_core::{Rgb, Swatches};
        let patch = patch();
        let colours = [
            Rgb {
                red: 65535,
                ..Rgb::BLACK
            },
            Rgb {
                green: 65535,
                ..Rgb::BLACK
            },
            Rgb {
                blue: 65535,
                ..Rgb::BLACK
            },
        ];
        let saved = wave(
            1,
            WaveTarget::Matrix { fixture: 1 },
            4,
            Palette::Inline(Swatches::new(&colours).unwrap()),
        );
        let plan = Looks::new().put(saved, &patch).unwrap().plan(1).unwrap();
        let words: Vec<_> = colours
            .iter()
            .flat_map(|colour| [colour.red, colour.green, colour.blue])
            .enumerate()
            .map(|(at, word)| (u32::try_from(at).unwrap(), word))
            .collect();
        assert_eq!(
            offered(
                &plan,
                0,
                Use {
                    spread: 100,
                    ..Use::default()
                }
            ),
            words
        );
    }

    #[test]
    fn retained_plans_distinguish_a_palette_from_an_ordinary_step_item() {
        use engine_core::{AuthoredStep, Rgb, Swatches};
        let patch = patch();
        let mut saved = SavedLook::from(leaf(1));
        saved.definition = Definition::Step(AuthoredStep {
            swatches: Swatches::new(&[Rgb::BLACK]).unwrap(),
            ..leaf(1).step.into()
        });
        let looks = Looks::new()
            .put(saved, &patch)
            .unwrap()
            .put(
                wave(
                    2,
                    WaveTarget::Matrix { fixture: 1 },
                    1,
                    Palette::Step { look: 1 },
                ),
                &patch,
            )
            .unwrap();
        for (children, uses_palette) in [
            (&[item(1)][..], false),
            (&[item(2)][..], true),
            (&[item(1), item(2)][..], true),
            (&[item(2), item(1)][..], true),
        ] {
            let revision = looks.put(sequence(3, children), &patch).unwrap();
            let play = revision.plan(3).unwrap();
            assert!(play.uses(1));
            assert_eq!(play.uses_palette(1), uses_palette);
            assert!(!play.uses_palette(2));
        }
        assert!(!looks.plan(1).unwrap().uses_palette(1));
    }

    #[test]
    fn wave_refuses_bad_timing_targets_and_palettes_before_saving() {
        use engine_core::{Palette, Rgb, Swatches, WaveTarget};

        let empty = Text::new("empty").unwrap();
        let patch = patch()
            .update(
                PatchCommand::SetGroup {
                    name: empty,
                    members: engine_core::Members::new(&[]).unwrap(),
                },
                &[],
            )
            .unwrap()
            .0
            .unwrap();
        let matrix = WaveTarget::Matrix { fixture: 1 };
        let one = Palette::Inline(
            Swatches::new(&[Rgb {
                red: 1,
                green: 2,
                blue: 3,
            }])
            .unwrap(),
        );
        let looks = Looks::new()
            .put(leaf(1), &patch)
            .unwrap()
            .put(sequence(2, &[item(1)]), &patch)
            .unwrap();
        for (target, period, palette, reason) in [
            (matrix, 0, one, Refusal::InvalidTiming),
            (matrix, 33, one, Refusal::InvalidTiming),
            (
                WaveTarget::Group { group: empty },
                1,
                one,
                Refusal::EmptyGroup,
            ),
            (
                matrix,
                1,
                Palette::Inline(Swatches::default()),
                Refusal::EmptyPalette,
            ),
            (matrix, 1, Palette::Step { look: 1 }, Refusal::EmptyPalette),
            (matrix, 1, Palette::Step { look: 2 }, Refusal::WrongLookKind),
            (matrix, 1, Palette::Step { look: 99 }, Refusal::NoSuchLook),
            (
                WaveTarget::Matrix { fixture: 99 },
                1,
                one,
                Refusal::NotPatched,
            ),
            (
                WaveTarget::Group {
                    group: Text::new("absent").unwrap(),
                },
                1,
                one,
                Refusal::NoSuchGroup,
            ),
        ] {
            assert_eq!(
                looks.put(wave(3, target, period, palette), &patch),
                Err(reason)
            );
        }
        for period in [1, 32] {
            let valid = looks.put(wave(3, matrix, period, one), &patch).unwrap();
            assert_eq!(valid.put(leaf(3), &patch), Err(Refusal::WrongLookKind));
        }
    }

    #[test]
    fn wave_checks_every_target_even_with_a_single_swatch() {
        use engine_core::{Member, Members, Palette, Property, Rgb, Swatches, WaveTarget};
        let definition = br##"<FixtureDefinition><Manufacturer>Desk</Manufacturer><Model>Wash</Model><Type>Other</Type>
            <Channel Name="Wheel"><Group Byte="0">Colour</Group><Capability Min="0" Max="255" Preset="ColorMacro" Res1="#ff0000">Red</Capability></Channel>
            <Channel Name="Dimmer" Preset="IntensityDimmer"/>
            <Mode Name="Wheel"><Channel Number="0">Wheel</Channel></Mode>
            <Mode Name="Dimmer"><Channel Number="0">Dimmer</Channel></Mode>
            </FixtureDefinition>"##;
        let fixture = fixture_library::parse("desk.qxf", definition).unwrap();
        let name = Text::new("wash").unwrap();
        let palette = Palette::Inline(
            Swatches::new(&[Rgb {
                red: 65535,
                green: 0,
                blue: 0,
            }])
            .unwrap(),
        );
        for mode in ["Wheel", "Dimmer"] {
            let with_wash = patch()
                .update(
                    PatchCommand::Patch {
                        name,
                        maker: Text::new("Desk").unwrap(),
                        model: Text::new("Wash").unwrap(),
                        mode: Text::new(mode).unwrap(),
                        at: ChannelNumbers {
                            universe: 1,
                            address: 44,
                        },
                    },
                    core::slice::from_ref(&fixture),
                )
                .unwrap()
                .0
                .unwrap();
            let combined = with_wash
                .update(
                    PatchCommand::SetGroup {
                        name,
                        members: Members::new(&[
                            Member::Light { light: 1 },
                            Member::Fixture { fixture: 2 },
                        ])
                        .unwrap(),
                    },
                    &[],
                )
                .unwrap()
                .0
                .unwrap();
            for (target, reason) in [
                (WaveTarget::Matrix { fixture: 2 }, Refusal::NoMatrix),
                (
                    WaveTarget::Group { group: name },
                    Refusal::Unsupported {
                        light: 4,
                        property: Property::Colour,
                    },
                ),
            ] {
                assert_eq!(
                    Looks::new().put(wave(8, target, 4, palette), &combined),
                    Err(reason),
                    "{mode}"
                );
            }
        }
    }

    #[test]
    fn a_linked_palette_counts_toward_the_authored_depth_limit() {
        use engine_core::{AuthoredStep, Rgb, Swatches};
        let patch = patch();
        let swatches = Swatches::new(&[Rgb::BLACK]).unwrap();
        let mut saved = SavedLook::from(leaf(1));
        saved.definition = Definition::Step(AuthoredStep {
            swatches,
            ..leaf(1).step.into()
        });
        // Look 2 is one timed instruction. Reading look 1 adds an authored edge.
        // The inline control runs at the same cost with one fewer authored look.
        for palette in [Palette::Step { look: 1 }, Palette::Inline(swatches)] {
            let mut looks = Looks::new()
                .put(saved, &patch)
                .unwrap()
                .put(
                    wave(2, WaveTarget::Matrix { fixture: 1 }, 1, palette),
                    &patch,
                )
                .unwrap();
            for id in 3..=8 {
                looks = looks.put(sequence(id, &[item(id - 1)]), &patch).unwrap();
            }
            for children in [&[item(8)][..], &[item(2), item(8)], &[item(8), item(2)]] {
                let ninth = looks.put(sequence(9, children), &patch).map(drop);
                if matches!(palette, Palette::Step { .. }) {
                    assert_eq!(ninth, Err(Refusal::WorkLimit), "{children:?}");
                } else {
                    assert!(ninth.is_ok(), "{children:?}");
                }
            }
        }
    }

    #[test]
    fn wave_books_both_swatch_reads_and_its_terminal_sample_before_saving() {
        use engine_core::{Palette, Rgb, Swatches, WaveTarget};
        let one = Swatches::new(&[Rgb {
            red: 1,
            green: 2,
            blue: 3,
        }])
        .unwrap();
        let wave = wave(
            1,
            WaveTarget::Matrix { fixture: 1 },
            1,
            Palette::Inline(one),
        );
        for (pixels, expected) in [(4096, Ok(WORK_LIMIT)), (4097, Err(Refusal::WorkLimit))] {
            let patch = Patch::new(
                Universes::new(32).unwrap(),
                Lights::MOST,
                Whole(|_| NonZeroU16::MIN),
            )
            .update(
                PatchCommand::PatchStrip {
                    name: Text::new("panel").unwrap(),
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
            let result = Looks::new()
                .put(wave, &patch)
                .and_then(|looks| looks.plan(1))
                .map(|plan| play_work(plan.work()));
            assert_eq!(result, expected, "{pixels} pixels");
        }
    }
}
