//! A cyclic RGB16 ramp. The timed sampler supplies the local position for this light.

use super::Moment;
use crate::Rgb;

/// Round the exact weighted channel. The position may lie a fraction of a subcount
/// on either side of a half tie. At a rising tie the outgoing left limit rounds down.
fn lerp(from: u16, to: u16, weight: Moment, span: u128) -> u16 {
    let rounded =
        u128::from(from) * (span - weight.count) + u128::from(to) * weight.count + span / 2;
    let fraction = weight.part * u128::from(to.abs_diff(from));
    let numerator = if from < to {
        let left_of_integer = weight.left && fraction.is_multiple_of(weight.divisor);
        rounded + fraction / weight.divisor - u128::from(left_of_integer)
    } else {
        rounded - fraction.div_ceil(weight.divisor)
    };
    (numerator / span) as u16
}

pub(super) fn colour(swatches: &[Rgb], phase: Moment, span: u128) -> Option<Rgb> {
    let count = swatches.len();
    if count == 0 {
        return None;
    }
    // Multiply the phase by the swatch count, then split off the swatch index.
    // Keeping the remainder below the 112-bit divisor leaves room for an RGB16 product.
    let fraction = phase.part * count as u128;
    let position = phase.count * count as u128 + fraction / phase.divisor;
    let index = (position / span) as usize % count;
    let a = swatches.get(index)?;
    let b = swatches.get((index + 1) % count)?;
    let weight = Moment {
        count: position % span,
        part: fraction % phase.divisor,
        ..phase
    };
    Some(Rgb {
        red: lerp(a.red, b.red, weight, span),
        green: lerp(a.green, b.green, weight, span),
        blue: lerp(a.blue, b.blue, weight, span),
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
            measure: PERIOD,
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
