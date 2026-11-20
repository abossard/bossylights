//! A cyclic RGB16 ramp. The timed sampler supplies the local position for this light.

use super::{Moment, interpolate};
use crate::Rgb;

pub(super) fn colour(swatches: &[Rgb], phase: Moment, span: u128) -> Option<Rgb> {
    let count = swatches.len();
    if count == 0 {
        return None;
    }
    // The whole spans select the pair. Its fractional position sets the blend.
    let position = phase.times(count as u128);
    let index = (position.count / span) as usize % count;
    let a = swatches.get(index)?;
    let b = swatches.get((index + 1) % count)?;
    let weight = Moment {
        count: position.count % span,
        ..position
    };
    Some(Rgb {
        red: interpolate(a.red, b.red, weight, span),
        green: interpolate(a.green, b.green, weight, span),
        blue: interpolate(a.blue, b.blue, weight, span),
    })
}

#[cfg(test)]
mod tests {
    use crate::{Binding, Item, Light, Node, Program, Repeat, Rgb, Speed, Swatches, Use};

    const BEAT: u64 = 1 << 32;
    const PERIOD: u64 = 4 * BEAT;
    const RED: Rgb = rgb(65535, 0, 0);
    const GREEN: Rgb = rgb(0, 65535, 0);
    const BLUE: Rgb = rgb(0, 0, 65535);
    const WHITE: Rgb = rgb(65535, 65535, 65535);

    const fn rgb(red: u16, green: u16, blue: u16) -> Rgb {
        Rgb { red, green, blue }
    }

    fn wave(colours: &[Rgb]) -> Node {
        Node::Wave {
            period: 4,
            swatches: Swatches::new(colours).unwrap(),
        }
    }

    fn binding(node: usize, index: u16, count: u16) -> Binding {
        Binding {
            node,
            index,
            count,
            discrete_colour: false,
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
            let program = Program {
                nodes: &[wave(colours)],
                items: &[],
                root: 0,
            };
            assert_eq!(
                program.sample(
                    &[binding(0, i, n)],
                    at,
                    Use {
                        spread,
                        ..Use::default()
                    }
                ),
                Light {
                    colour: Some(expected),
                    ..Light::DARK
                },
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
        let mut nodes = vec![wave(&[rgb(1, 0, 0), rgb(0, 1, 0)])];
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
            assert_eq!(
                program.sample(&bindings, BEAT, spread).colour,
                Some(expected)
            );
        }
    }

    #[test]
    fn a_transition_freezes_the_outgoing_wave_just_before_a_rounding_edge() {
        let normal = Use::default();
        let nodes = [
            wave(&[rgb(0, 3, 0), rgb(1, 0, 5)]),
            Node::Step {
                values: Light {
                    colour: Some(WHITE),
                    ..Light::DARK
                },
            },
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
        let bindings = [binding(0, 0, 1), binding(1, 0, 1), binding(2, 0, 1)];
        for (root, at, expected) in [
            (0, BEAT, rgb(1, 2, 3)), // exact half ties round up
            (2, BEAT, rgb(0, 2, 2)), // red and blue rise, green falls into its tie
        ] {
            let program = Program {
                nodes: &nodes,
                items: &items,
                root,
            };
            assert_eq!(program.sample(&bindings, at, normal).colour, Some(expected));
        }
    }

    #[test]
    fn wave_uses_the_timed_clock_at_root_and_in_a_sequence() {
        let once = Use {
            speed: Speed::Double,
            repeat: Repeat::Once,
            spread: 100,
        };
        let nodes = [
            wave(&[RED, BLUE]),
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
        };
        let child = Program {
            nodes: &nodes,
            items: &items,
            root: 0,
        };
        for (at, colour, finished) in [
            (0, BLUE, false),
            (BEAT / 2, rgb(32768, 0, 32768), false),
            (BEAT, RED, false), // spread clamps this light before the root ends
            (2 * BEAT - 1, RED, false),
            (2 * BEAT, RED, true),
            (3 * BEAT, RED, true),
        ] {
            assert_eq!(child.finished(at, once), finished);
            assert_eq!(child.sample(&bindings, at, once).colour, Some(colour));
            assert_eq!(
                parent.sample(&bindings, at, Use::default()).colour,
                Some(colour)
            );
        }
    }
}
