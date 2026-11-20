//! Sample a compiled look at one clock position for one light. The compiler supplies
//! its target order for each linked look, including shared children used in two places.

#[path = "wave.rs"]
mod wave;

use crate::{Light, Position, Repeat, Rgb, Speed, Swatches, Use};

/// The maximum number of looks from the root to a leaf, counting both.
pub const MAX_DEPTH: usize = 8;

/// A compiled look. Sequence ends and timed measures use the clock's Q32 beats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Node {
    Step {
        values: Light,
    },
    Wave {
        measure: u64,
        swatches: Swatches,
    },
    Sequence {
        first: usize,
        count: usize,
        measure: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub child: usize,
    pub end: u64,
    pub settings: Use,
    pub transition: u8,
}

/// A light's place in the ordered target, or its column and width for a matrix Wave.
/// A wheel cuts at entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub node: usize,
    pub index: u16,
    pub count: u16,
    pub discrete_colour: bool,
}

/// The nodes reachable from one playing look. Each light's bindings are sorted by node.
pub struct Program<'a> {
    pub nodes: &'a [Node],
    pub items: &'a [Item],
    pub root: usize,
}

impl Program<'_> {
    /// A once-use ends when the *unspread* clock reaches its measure. Its final
    /// sample still exists: the Mixer decides whether to keep or remove it.
    pub fn finished(&self, elapsed: u64, settings: Use) -> bool {
        match self.nodes.get(self.root) {
            Some(Node::Sequence { measure, .. } | Node::Wave { measure, .. }) => {
                settings.repeat == Repeat::Once
                    && scaled(u128::from(elapsed), settings.speed) >= u128::from(*measure)
            }
            Some(Node::Step { .. }) | None => false,
        }
    }

    /// Sample one light at the clock's elapsed count since this play began.
    pub fn sample(&self, bindings: &[Binding], elapsed: u64, settings: Use) -> Light {
        Sampler {
            program: self,
            bindings,
        }
        .visit(self.root, Moment::at(elapsed), settings, MAX_DEPTH)
    }
}

fn scaled(count: u128, speed: Speed) -> u128 {
    match speed {
        Speed::Half => count / 2,
        Speed::Normal => count,
        Speed::Double => count * 2,
    }
}

/// An exact position or its left limit. Carrying the side through nested clocks
/// makes an outgoing loop at its wrap select its final item, even after a late wake.
#[derive(Clone, Copy)]
struct Moment {
    count: u128,
    /// Fraction of the next subcount, carried through each linked target's spread.
    part: u128,
    divisor: u128,
    left: bool,
}

/// The factor of 100 makes percentages exact. The low bits leave room to halve
/// the clock and its spread offsets at every level without losing a fraction.
fn subcounts(clock_count: u64) -> u128 {
    (u128::from(clock_count) << 32) * 100
}

impl Moment {
    fn at(count: u64) -> Self {
        Self {
            count: subcounts(count),
            part: 0,
            divisor: 1,
            left: false,
        }
    }

    fn left(count: u64) -> Self {
        Self {
            left: true,
            ..Self::at(count)
        }
    }

    fn before(self, edge: Self) -> bool {
        let phase = (self.count, self.part * edge.divisor);
        let edge = (edge.count, edge.part * self.divisor);
        phase < edge || (self.left && phase == edge)
    }

    fn since(self, start: u64) -> Self {
        Self {
            count: self.count.saturating_sub(subcounts(start)),
            ..self
        }
    }

    /// Normalize after adding two proper fractions or doubling one.
    fn carry(mut self) -> Self {
        if self.part >= self.divisor {
            self.count += 1;
            self.part -= self.divisor;
        }
        self
    }

    fn at_speed(mut self, speed: Speed) -> Self {
        match speed {
            Speed::Half => {
                self.part = ((self.count % 2) * self.divisor + self.part) / 2;
                self.count /= 2;
                self
            }
            Speed::Normal => self,
            Speed::Double => {
                self.count *= 2;
                self.part *= 2;
                self.carry()
            }
        }
    }

    /// Add span × spread/100 × index/count. At most eight nodes multiply
    /// divisors, each at most MAX_LIGHTS (2¹⁴). Their product fits in 112 bits,
    /// including the binding of a Wave at maximum depth.
    fn shifted(mut self, span: u128, spread: u8, binding: &Binding) -> Self {
        if spread == 0 || binding.index == 0 || binding.count == 0 {
            return self;
        }
        let offset = (span / 100) * u128::from(spread) * u128::from(binding.index);
        let lights = u128::from(binding.count);
        self.count += offset / lights;
        let remainder = offset % lights;
        if remainder != 0 {
            self.part = self.part * lights + remainder * self.divisor;
            self.divisor *= lights;
            self = self.carry();
        }
        self
    }

    /// Phase in this use and whether it has a predecessor across a loop boundary.
    fn phase(self, measure: u64, settings: Use, binding: &Binding) -> (Self, bool) {
        let end = Self::at(measure);
        let mut phase = self
            .at_speed(settings.speed)
            .shifted(end.count, settings.spread, binding);
        if phase.before(end) {
            return (phase, false);
        }
        if settings.repeat == Repeat::Once {
            return (Self::left(measure), false);
        }
        phase.count %= end.count;
        if phase.left && phase.count == 0 && phase.part == 0 {
            (Self::left(measure), true)
        } else {
            (phase, true)
        }
    }
}

struct Sampler<'a> {
    program: &'a Program<'a>,
    bindings: &'a [Binding],
}

impl Sampler<'_> {
    fn binding(&self, node: usize) -> Option<&Binding> {
        let index = self.bindings.binary_search_by_key(&node, |b| b.node).ok()?;
        self.bindings.get(index)
    }

    /// Descend at most MAX_DEPTH nodes, including the terminal step or Wave.
    fn visit(&self, node: usize, time: Moment, settings: Use, depth: usize) -> Light {
        let (Some(depth), Some(binding)) = (depth.checked_sub(1), self.binding(node)) else {
            return Light::DARK;
        };
        let (items, measure) = match self.program.nodes.get(node) {
            Some(Node::Step { values }) => return *values,
            Some(Node::Wave { measure, swatches }) if *measure > 0 => {
                let (phase, _) = time.phase(*measure, settings, binding);
                return Light {
                    colour: wave::colour(swatches.as_slice(), phase, subcounts(*measure)),
                    ..Light::DARK
                };
            }
            Some(Node::Sequence {
                first,
                count,
                measure,
            }) if *measure > 0 && depth != 0 => {
                let Some(items) = self.program.items.get(*first..first.saturating_add(*count))
                else {
                    return Light::DARK;
                };
                (items, *measure)
            }
            Some(Node::Sequence { .. } | Node::Wave { .. }) | None => return Light::DARK,
        };
        let (phase, wrapped) = time.phase(measure, settings, binding);
        let index = items.partition_point(|item| !phase.before(Moment::at(item.end)));
        let Some((incoming, start)) = item_at(items, index) else {
            return Light::DARK;
        };
        let age = phase.since(start);
        let width = subcounts(incoming.end.saturating_sub(start));
        let delay = if incoming.settings.spread != 0
            && matches!(
                self.program.nodes.get(incoming.child),
                Some(Node::Step { .. })
            ) {
            self.binding(incoming.child).map_or(Moment::at(0), |b| {
                Moment::at(0).shifted(width, incoming.settings.spread, b)
            })
        } else {
            Moment::at(0)
        };
        let previous = || {
            let index = index
                .checked_sub(1)
                .or_else(|| wrapped.then(|| items.len().saturating_sub(1)))?;
            let (outgoing, start) = item_at(items, index)?;
            Some(self.visit(
                outgoing.child,
                Moment::left(outgoing.end.saturating_sub(start)),
                outgoing.settings,
                depth,
            ))
        };
        if age.before(delay) {
            return previous().unwrap_or(Light::DARK);
        }
        let now = self.visit(incoming.child, age, incoming.settings, depth);
        // Convert to interpolation weights only after deciding the exact item and entry.
        let fade = width.saturating_sub(delay.count) * u128::from(incoming.transition) / 100;
        let progress = age.count.saturating_sub(delay.count);
        if progress >= fade {
            return now;
        }
        match previous() {
            Some(from) => blend(from, now, progress, fade, binding.discrete_colour),
            None => now,
        }
    }
}

/// The item and its inclusive start. Only a left-limit sample can reach its end.
fn item_at(items: &[Item], index: usize) -> Option<(&Item, u64)> {
    let start = index
        .checked_sub(1)
        .and_then(|i| items.get(i))
        .map_or(0, |i| i.end);
    Some((items.get(index)?, start))
}

/// Only the incoming sample can claim a property. A common continuous value
/// interpolates in 16 bits, rounded to nearest. Gobo and wheel colour cut at entry.
fn blend(from: Light, mut to: Light, progress: u128, fade: u128, discrete_colour: bool) -> Light {
    let lerp = |a: u16, b: u16| {
        ((u128::from(a) * (fade - progress) + u128::from(b) * progress + fade / 2) / fade) as u16
    };
    if let (Some(a), Some(b)) = (from.intensity, to.intensity) {
        to.intensity = Some(lerp(a, b));
    }
    if let (Some(a), Some(b)) = (from.zoom, to.zoom) {
        to.zoom = Some(lerp(a, b));
    }
    if let (Some(a), Some(b)) = (from.position, to.position) {
        to.position = Some(Position {
            pan: lerp(a.pan, b.pan),
            tilt: lerp(a.tilt, b.tilt),
        });
    }
    if !discrete_colour && let (Some(a), Some(b)) = (from.colour, to.colour) {
        to.colour = Some(Rgb {
            red: lerp(a.red, b.red),
            green: lerp(a.green, b.green),
            blue: lerp(a.blue, b.blue),
        });
    }
    to
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEAT: u64 = 1 << 32;
    const UNIT: u64 = BEAT / 16;

    #[derive(Default)]
    struct Graph {
        nodes: Vec<Node>,
        items: Vec<Item>,
    }

    impl Graph {
        fn step(&mut self, level: Option<u16>) -> usize {
            self.light(Light {
                intensity: level,
                ..Light::DARK
            })
        }

        fn light(&mut self, values: Light) -> usize {
            self.nodes.push(Node::Step { values });
            self.nodes.len() - 1
        }

        // Each row: referenced node, width in sixteenths, child use, transition percent.
        fn sequence(&mut self, rows: &[(usize, u16, Use, u8)]) -> usize {
            let first = self.items.len();
            let mut measure = 0;
            for &(child, width, settings, transition) in rows {
                measure += u64::from(width) * UNIT;
                self.items.push(Item {
                    child,
                    end: measure,
                    settings,
                    transition,
                });
            }
            self.nodes.push(Node::Sequence {
                first,
                count: rows.len(),
                measure,
            });
            self.nodes.len() - 1
        }

        fn program(&self, root: usize) -> Program<'_> {
            Program {
                nodes: &self.nodes,
                items: &self.items,
                root,
            }
        }

        fn bindings(&self, index: u16, count: u16) -> Vec<Binding> {
            (0..self.nodes.len())
                .map(|node| Binding {
                    node,
                    index,
                    count,
                    discrete_colour: false,
                })
                .collect()
        }
    }

    #[test]
    fn sequence_edges_use_the_current_item_and_once_holds_its_left_limit() {
        let mut graph = Graph::default();
        let (a, b, rest) = (
            graph.step(Some(100)),
            graph.step(Some(200)),
            graph.step(None),
        );
        let normal = Use::default();
        let once = Use {
            repeat: Repeat::Once,
            ..normal
        };
        let double = Use {
            speed: Speed::Double,
            ..normal
        };
        let short = graph.sequence(&[(a, 1, normal, 0), (b, 32, normal, 0)]);
        let with_rest = graph.sequence(&[(a, 1, normal, 0), (rest, 32, normal, 0)]);
        let nested = graph.sequence(&[(short, 48, double, 0)]);
        let nested_once = graph.sequence(&[(
            short,
            48,
            Use {
                speed: Speed::Double,
                ..once
            },
            0,
        )]);
        let bindings = graph.bindings(0, 1);
        // Wakes deliberately skip the whole last item in the once/rest rows.
        for (name, root, settings, at, expected, ended) in [
            ("first sixteenth", short, normal, UNIT - 1, Some(100), false),
            ("two beats start", short, normal, UNIT, Some(200), false),
            (
                "two beats end",
                short,
                normal,
                33 * UNIT - 1,
                Some(200),
                false,
            ),
            ("loop edge", short, normal, 33 * UNIT, Some(100), false),
            (
                "once before end",
                short,
                once,
                33 * UNIT - 1,
                Some(200),
                false,
            ),
            ("once at end", short, once, 33 * UNIT, Some(200), true),
            (
                "once ended between wakes",
                short,
                once,
                8 * BEAT,
                Some(200),
                true,
            ),
            ("stop during rest", with_rest, normal, 2 * BEAT, None, false),
            (
                "once ends during rest",
                with_rest,
                once,
                8 * BEAT,
                None,
                true,
            ),
            (
                "double before edge",
                nested,
                normal,
                UNIT / 2 - 1,
                Some(100),
                false,
            ),
            ("double at edge", nested, normal, UNIT / 2, Some(200), false),
            (
                "child wraps within parent item",
                nested,
                normal,
                33 * UNIT / 2,
                Some(100),
                false,
            ),
            (
                "nested once holds through parent item",
                nested_once,
                normal,
                33 * UNIT / 2,
                Some(200),
                false,
            ),
            (
                "half speed root",
                short,
                Use {
                    speed: Speed::Half,
                    ..normal
                },
                UNIT,
                Some(100),
                false,
            ),
        ] {
            let p = graph.program(root);
            assert_eq!(
                (
                    p.sample(&bindings, at, settings).intensity,
                    p.finished(at, settings)
                ),
                (expected, ended),
                "{name}"
            );
        }
    }

    #[test]
    fn sequence_spread_uses_saved_order_for_steps_and_timed_uses() {
        let mut graph = Graph::default();
        let (a, b) = (graph.step(Some(0)), graph.step(Some(1000)));
        let normal = Use::default();
        let spread = Use {
            spread: 100,
            ..normal
        };
        let steps = graph.sequence(&[(a, 16, normal, 0), (b, 32, spread, 100)]);
        let timed = graph.sequence(&[(a, 16, normal, 0), (b, 16, normal, 0)]);
        let nested = graph.sequence(&[(timed, 64, spread, 0)]);
        let first = graph.sequence(&[(b, 16, spread, 0)]);
        for (name, root, settings, index, at, expected) in [
            (
                "first light starts fading on edge",
                steps,
                normal,
                0,
                24 * UNIT,
                Some(250),
            ),
            (
                "second holds through delay",
                steps,
                normal,
                1,
                24 * UNIT,
                Some(0),
            ),
            (
                "second fits fade into remaining beat",
                steps,
                normal,
                1,
                40 * UNIT,
                Some(500),
            ),
            (
                "first item has no predecessor during delay",
                first,
                normal,
                1,
                UNIT,
                None,
            ),
            ("first light at its own phase", timed, spread, 0, 0, Some(0)),
            (
                "second light offset by half a pass",
                timed,
                spread,
                1,
                0,
                Some(1000),
            ),
            ("offset loops at measure", timed, spread, 1, BEAT, Some(0)),
            (
                "offset once clamps at measure",
                timed,
                Use {
                    repeat: Repeat::Once,
                    ..spread
                },
                1,
                BEAT,
                Some(1000),
            ),
            (
                "nested use has its own spread",
                nested,
                normal,
                1,
                BEAT,
                Some(0),
            ),
        ] {
            assert_eq!(
                graph
                    .program(root)
                    .sample(&graph.bindings(index, 2), at, settings)
                    .intensity,
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn sequence_spread_cuts_on_the_first_clock_count_that_reaches_the_delay() {
        let mut graph = Graph::default();
        let lit = graph.step(Some(1));
        let normal = Use::default();
        let spread = Use {
            spread: 100,
            ..normal
        };
        let root = graph.sequence(&[(lit, 16, spread, 0)]);
        for (index, count, expected) in [
            (1, BEAT / 3, None),
            (1, BEAT / 3 + 1, Some(1)),
            (2, 2 * BEAT / 3, None),
            (2, 2 * BEAT / 3 + 1, Some(1)),
        ] {
            assert_eq!(
                graph
                    .program(root)
                    .sample(&graph.bindings(index, 3), count, normal)
                    .intensity,
                expected
            );
        }
    }

    #[test]
    fn combined_spread_wraps_at_an_exact_child_edge() {
        let mut graph = Graph::default();
        let first = graph.step(Some(100));
        let last = graph.step(Some(200));
        let normal = Use::default();
        let spread = Use {
            spread: 100,
            ..normal
        };
        let child = graph.sequence(&[(first, 8, normal, 0), (last, 8, normal, 0)]);
        let parent = graph.sequence(&[(child, 32, spread, 0)]);

        // Parent and child offsets combine to zero, one and two beats.
        // All three lights therefore start in the child's first item.
        let levels = [0, 1, 2].map(|light| {
            graph
                .program(parent)
                .sample(&graph.bindings(light, 3), 0, spread)
                .intensity
        });
        assert_eq!(levels, [Some(100); 3]);
    }

    #[test]
    fn linked_spread_carries_between_different_targets_at_each_speed() {
        let mut graph = Graph::default();
        let first = graph.step(Some(100));
        let last = graph.step(Some(200));
        let normal = Use::default();
        let spread = Use {
            spread: 100,
            ..normal
        };
        let child = graph.sequence(&[(first, 8, normal, 0), (last, 8, normal, 0)]);

        // Parent target 1/9 and child target 1/3: after scaling the parent
        // offset, 2/3 + 1/3 wraps the child exactly on the starting frame.
        for (speed, parent_width, half_beat) in [
            (Speed::Half, 192, 16),
            (Speed::Normal, 96, 8),
            (Speed::Double, 48, 4),
        ] {
            let parent = graph.sequence(&[(child, parent_width, Use { speed, ..spread }, 0)]);
            let fade = graph.sequence(&[(parent, half_beat, spread, 0), (last, 16, normal, 100)]);
            let mut bindings = graph.bindings(1, 3);
            bindings[parent].count = 9;

            let edge = u64::from(half_beat) * UNIT;
            for (root, settings, at, expected) in [
                (parent, spread, 0, 100),
                (parent, spread, edge - 1, 100),
                (parent, spread, edge, 200),
                // The outgoing parent freezes the first item just before the cut.
                (fade, normal, edge, 100),
            ] {
                assert_eq!(
                    graph
                        .program(root)
                        .sample(&bindings, at, settings)
                        .intensity,
                    Some(expected),
                    "{speed:?} at {at}"
                );
            }
        }
    }

    #[test]
    fn a_tempo_change_preserves_the_exact_sixteenth_edge() {
        use crate::{Bpm, Clock};
        use core::time::Duration;

        let mut graph = Graph::default();
        let first = graph.step(Some(100));
        let second = graph.step(Some(200));
        let normal = Use::default();
        let root = graph.sequence(&[(first, 1, normal, 0), (second, 16, normal, 0)]);
        let program = graph.program(root);
        let bindings = graph.bindings(0, 1);

        // The first 12.5 ms at 120 BPM provide 0.025 beat. Another 37.5 ms at
        // 60 BPM adds 0.0375 beat, so the item changes at exactly 50 ms.
        let clock = Clock::START
            .set_feed(Bpm::new(120).unwrap())
            .settle(Duration::from_micros(12_500))
            .set_feed(Bpm::new(60).unwrap());
        let actual = [49_999, 50_000, 50_001].map(|micros| {
            let elapsed = clock.elapsed_at(Duration::from_micros(micros));
            (
                elapsed >= UNIT,
                program.sample(&bindings, elapsed, normal).intensity,
            )
        });
        assert_eq!(
            actual,
            [(false, Some(100)), (true, Some(200)), (true, Some(200))]
        );
    }

    #[test]
    fn sequence_transitions_freeze_the_outgoing_child_and_follow_the_incoming_child() {
        let mut graph = Graph::default();
        let normal = Use::default();
        let (a, b, c, d) = (
            graph.step(Some(0)),
            graph.step(Some(24000)),
            graph.step(Some(36000)),
            graph.step(Some(52000)),
        );
        let outgoing = graph.sequence(&[(a, 16, normal, 0), (b, 16, normal, 100)]);
        let incoming = graph.sequence(&[(c, 1, normal, 0), (d, 16, normal, 100)]);
        let parent = graph.sequence(&[(outgoing, 24, normal, 0), (incoming, 16, normal, 100)]);
        let exactly = graph.sequence(&[(outgoing, 32, normal, 0), (c, 16, normal, 100)]);
        let wraps = graph.sequence(&[(a, 16, normal, 100), (b, 16, normal, 0)]);
        let bindings = graph.bindings(0, 1);
        for (name, root, at, expect) in [
            ("outgoing moves", parent, 20 * UNIT, 6000),
            ("outgoing at edge inside its fade", parent, 24 * UNIT, 12000),
            (
                "frozen 12000 blended halfway to current 43000",
                parent,
                32 * UNIT,
                27500,
            ),
            (
                "both children at transition end",
                parent,
                40 * UNIT - 1,
                51000,
            ),
            (
                "child ends exactly on wrap, left limit is 24000",
                exactly,
                32 * UNIT,
                24000,
            ),
            ("no predecessor on first pass", wraps, 8 * UNIT, 0),
            ("loop uses last item at edge", wraps, 32 * UNIT, 24000),
            ("loop transition progresses", wraps, 40 * UNIT, 12000),
        ] {
            assert_eq!(
                graph.program(root).sample(&bindings, at, normal).intensity,
                Some(expect),
                "{name}"
            );
        }
    }

    #[test]
    fn sequence_falls_through_outside_a_childs_targets() {
        let mut graph = Graph::default();
        let normal = Use::default();
        let (a, b) = (graph.step(Some(40)), graph.step(Some(70)));
        let root = graph.sequence(&[(a, 16, normal, 0), (b, 16, normal, 100)]);
        for (missing, at, expected) in [
            (a, 0, None),
            (a, BEAT, Some(70)), // a new property enters immediately
            (b, BEAT, None),     // a property omitted by the incoming look falls through
            (root, 0, None),
        ] {
            let mut bindings = graph.bindings(0, 1);
            bindings.retain(|b| b.node != missing);
            assert_eq!(
                graph.program(root).sample(&bindings, at, normal).intensity,
                expected
            );
        }
    }

    #[test]
    fn sequence_only_blends_common_continuous_properties() {
        let mut graph = Graph::default();
        let normal = Use::default();
        let rgb = |red| Rgb {
            red,
            green: 0,
            blue: 0,
        };
        let start = Light {
            intensity: Some(50),
            colour: Some(rgb(300)),
            position: Some(Position { pan: 20, tilt: 40 }),
            gobos: [Some(1), None, None],
            ..Light::DARK
        };
        let end = Light {
            colour: Some(rgb(600)),
            zoom: Some(0),
            position: Some(Position { pan: 40, tilt: 10 }),
            gobos: [Some(3), None, None],
            ..Light::DARK
        };
        let a = graph.light(start);
        let b = graph.light(end);
        let root = graph.sequence(&[(a, 16, normal, 0), (b, 16, normal, 100)]);
        for (wheel, at, red, pan, tilt) in [
            (false, BEAT, 300, 20, 40),
            (true, BEAT, 600, 20, 40),
            (false, BEAT + BEAT / 2, 450, 30, 25),
            (true, BEAT + BEAT / 2, 600, 30, 25),
        ] {
            let mut bindings = graph.bindings(0, 1);
            for binding in &mut bindings {
                binding.discrete_colour = wheel;
            }
            assert_eq!(
                graph.program(root).sample(&bindings, at, normal),
                Light {
                    colour: Some(rgb(red)),
                    position: Some(Position { pan, tilt }),
                    ..end
                },
                "wheel {wheel}, at {at}"
            );
        }
    }
}
