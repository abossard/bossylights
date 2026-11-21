//! A cyclic ramp over swatches, per field. The timed sampler supplies the local position
//! for this target; the compiler gave the target's words for each swatch.

use super::{Moment, interpolate};

/// One target's words of a wave: `fields` per swatch, swatch after swatch.
pub(super) struct Ramp<'a> {
    pub words: &'a [Option<u16>],
    pub swatches: usize,
    pub fields: usize,
}

impl Ramp<'_> {
    /// The words at `phase` of a pass of `span` into `out`: between two swatches each
    /// field interpolates in 16 bits; a field with no word on either side is a gap.
    pub(super) fn sample(&self, phase: Moment, span: u128, out: &mut [Option<u16>]) {
        let count = self.swatches;
        // The whole spans select the pair. Its fractional position sets the blend.
        let position = phase.times(count as u128);
        let index = (position.count / span) as usize % count;
        let weight = Moment {
            count: position.count % span,
            ..position
        };
        let set = |swatch: usize| {
            let first = swatch * self.fields;
            self.words
                .get(first..first + self.fields)
                .unwrap_or_default()
        };
        let (a, b) = (set(index), set((index + 1) % count));
        for (field, out) in out.iter_mut().enumerate() {
            *out = match (
                a.get(field).copied().flatten(),
                b.get(field).copied().flatten(),
            ) {
                (Some(a), Some(b)) => Some(interpolate(a, b, weight, span)),
                (a, b) => a.or(b),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Binding, Elapsed, Item, MAX_DEPTH, Node, Program, Repeat, Rgb, Sampled, Speed, Use,
    };

    const BEAT: u64 = 1 << 32;
    const PERIOD: u64 = 4 * BEAT;
    const RED: Rgb = rgb(65535, 0, 0);
    const GREEN: Rgb = rgb(0, 65535, 0);
    const BLUE: Rgb = rgb(0, 0, 65535);
    const WHITE: Rgb = rgb(65535, 65535, 65535);

    const fn rgb(red: u16, green: u16, blue: u16) -> Rgb {
        Rgb { red, green, blue }
    }

    /// A wave over `colours` of four beats: its node, and its words, red, green and blue
    /// of each swatch, appended to `words`, where its bindings find them.
    fn wave(colours: &[Rgb]) -> (Node, Vec<Option<u16>>) {
        let words = colours
            .iter()
            .flat_map(|c| [Some(c.red), Some(c.green), Some(c.blue)])
            .collect();
        let node = Node::Wave {
            period: 4,
            swatches: u8::try_from(colours.len()).unwrap(),
        };
        (node, words)
    }

    /// A binding whose words start at 0: every test has one wave or one step at the
    /// start of the words.
    fn binding(node: usize, index: u16, count: u16) -> Binding {
        Binding {
            node: node as u32,
            index,
            count,
            words: 0,
        }
    }

    /// The colour of a target of three fields, red, green and blue, all fading.
    fn colour(
        program: &Program<'_>,
        bindings: &[Binding],
        at: impl Into<Elapsed>,
        settings: Use,
    ) -> Option<Rgb> {
        let target = Sampled {
            bindings,
            fades: &[true; 3],
        };
        let mut out = [None; 3];
        let mut spare = [None; 3 * MAX_DEPTH];
        program.sample(target, at, settings, &mut out, &mut spare);
        let [red, green, blue] = out;
        Some(Rgb {
            red: red?,
            green: green?,
            blue: blue?,
        })
    }

    /// A caller-built wave with no period or no swatches samples gaps, never divides by 0.
    #[test]
    fn a_wave_without_period_or_swatches_offers_nothing() {
        let (_, words) = wave(&[RED, BLUE]);
        for (period, swatches) in [(0, 2), (4, 0)] {
            let program = Program {
                nodes: &[Node::Wave { period, swatches }],
                items: &[],
                root: 0,
                words: &words,
            };
            assert_eq!(
                colour(&program, &[binding(0, 0, 1)], BEAT, Use::default()),
                None,
                "period {period}, swatches {swatches}"
            );
        }
    }

    #[test]
    fn wave_samples_a_cyclic_rgb16_palette_at_the_targets_phase() {
        let two: &[Rgb] = &[RED, BLUE];
        let three: &[Rgb] = &[RED, GREEN, BLUE];
        let four: &[Rgb] = &[RED, GREEN, BLUE, WHITE];
        let purple = rgb(32768, 0, 32768);
        // Each target is (spread percent, ordered index, target count).
        for (name, colours, at, (spread, i, n), expected) in [
            ("one swatch", &[GREEN][..], 7 * BEAT, (80, 2, 3), GREEN),
            ("two, round half up", two, BEAT, (0, 0, 1), purple),
            ("last to first", two, 3 * BEAT, (0, 0, 1), purple),
            ("exact period", two, PERIOD, (0, 0, 1), RED),
            (
                "three, first pair",
                three,
                BEAT,
                (0, 0, 1),
                rgb(16384, 49151, 0),
            ),
            (
                "three, last pair",
                three,
                3 * BEAT,
                (0, 0, 1),
                rgb(16384, 0, 49151),
            ),
            (
                "fractional offset ties exactly",
                three,
                0,
                (100, 1, 6),
                rgb(32768, 32768, 0),
            ),
            // One sixth of a three-unit ramp is exactly half a channel unit.
            (
                "the swatch weight keeps its leftover fraction",
                &[rgb(0, 3, 0), rgb(3, 0, 0)],
                0,
                (100, 1, 12),
                rgb(1, 3, 0),
            ),
            ("four, exact swatch", four, 2 * BEAT, (0, 0, 1), BLUE),
            (
                "four, last pair",
                four,
                7 * BEAT / 2,
                (0, 0, 1),
                rgb(65535, 32768, 32768),
            ),
            ("first in pair", two, 0, (100, 0, 2), RED),
            ("after reorder", two, 0, (100, 1, 2), BLUE),
            ("after palette edit", &[RED, GREEN], 0, (100, 1, 2), GREEN),
            ("partial spread", two, 0, (25, 1, 2), rgb(49151, 0, 16384)),
        ] {
            let (node, words) = wave(colours);
            let program = Program {
                nodes: &[node],
                items: &[],
                root: 0,
                words: &words,
            };
            assert_eq!(
                colour(
                    &program,
                    &[binding(0, i, n)],
                    at,
                    Use {
                        spread,
                        ..Use::default()
                    }
                ),
                Some(expected),
                "{name}",
            );
        }
    }

    #[test]
    fn linked_spreads_sample_both_sides_of_a_half_tie() {
        let spread = Use {
            spread: 100,
            ..Use::default()
        };
        let (node, words) = wave(&[rgb(1, 0, 0), rgb(0, 1, 0)]);
        let mut nodes = vec![node];
        let mut items = Vec::new();
        for child in 0..5 {
            items.push(Item {
                child,
                end: PERIOD,
                settings: spread,
                transition: 0,
            });
            nodes.push(Node::Sequence {
                first: child,
                count: 1,
                measure: PERIOD,
            });
        }
        let program = Program {
            nodes: &nodes,
            items: &items,
            root: 5,
            words: &words,
        };
        // The six signed offsets add to 120 / (16000 × 16001 × ... × 16005)
        // of a period, modulo whole loops. Either sign moves the midpoint by
        // less than one subcount, enough to decide which channel rounds up.
        for (sign, expected) in [(1, rgb(0, 1, 0)), (-1, rgb(1, 0, 0))] {
            let bindings = [1_i32, -5, 10, -10, 5, -1]
                .into_iter()
                .enumerate()
                .map(|(node, coefficient)| {
                    let count = 16_000 + node as i32;
                    let index = (sign * coefficient).rem_euclid(count);
                    binding(node, index as u16, count as u16)
                })
                .collect::<Vec<_>>();
            assert_eq!(colour(&program, &bindings, BEAT, spread), Some(expected));
        }
    }

    #[test]
    fn a_transition_freezes_the_outgoing_wave_just_before_a_rounding_edge() {
        let normal = Use::default();
        let (node, mut words) = wave(&[rgb(0, 3, 0), rgb(1, 0, 5)]);
        // the step's words after the wave's
        let step = words.len();
        words.extend([Some(WHITE.red), Some(WHITE.green), Some(WHITE.blue)]);
        let nodes = [
            node,
            Node::Step,
            Node::Sequence {
                first: 0,
                count: 2,
                measure: 2 * BEAT,
            },
        ];
        let items = [
            Item {
                child: 0,
                end: BEAT,
                settings: normal,
                transition: 0,
            },
            Item {
                child: 1,
                end: 2 * BEAT,
                settings: normal,
                transition: 100,
            },
        ];
        let bindings = [
            binding(0, 0, 1),
            Binding {
                words: step as u32,
                ..binding(1, 0, 1)
            },
            binding(2, 0, 1),
        ];
        for (root, at, expected) in [
            (0, BEAT, rgb(1, 2, 3)), // exact half ties round up
            (2, BEAT, rgb(0, 2, 2)), // red and blue rise, green falls into its tie
        ] {
            let program = Program {
                nodes: &nodes,
                items: &items,
                root,
                words: &words,
            };
            assert_eq!(colour(&program, &bindings, at, normal), Some(expected));
        }
    }

    #[test]
    fn wave_uses_the_timed_clock_at_root_and_in_a_sequence() {
        let once = Use {
            speed: Speed::Double,
            repeat: Repeat::Once,
            spread: 100,
        };
        let (node, words) = wave(&[RED, BLUE]);
        let nodes = [
            node,
            Node::Sequence {
                first: 0,
                count: 1,
                measure: 2 * PERIOD,
            },
        ];
        let items = [Item {
            child: 0,
            end: 2 * PERIOD,
            settings: once,
            transition: 0,
        }];
        let bindings = [binding(0, 1, 2), binding(1, 0, 1)];
        let parent = Program {
            nodes: &nodes,
            items: &items,
            root: 1,
            words: &words,
        };
        let child = Program {
            nodes: &nodes,
            items: &items,
            root: 0,
            words: &words,
        };
        let colour_of = colour;
        for (at, expected, finished) in [
            (0, BLUE, false),
            (BEAT / 2, rgb(32768, 0, 32768), false),
            (BEAT, RED, false), // spread clamps this light before the root ends
            (2 * BEAT - 1, RED, false),
            (2 * BEAT, RED, true),
            (3 * BEAT, RED, true),
        ] {
            assert_eq!(child.finished(at, once), finished);
            assert_eq!(colour_of(&child, &bindings, at, once), Some(expected));
            assert_eq!(
                colour_of(&parent, &bindings, at, Use::default()),
                Some(expected)
            );
        }
    }
}
