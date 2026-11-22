//! Sample a compiled look at one clock position for one light. The compiler supplies
//! its target order for each linked look, including shared children used in two places.

#[path = "wave.rs"]
mod wave;

use crate::{BeatTime, Light, Percent, Position, Repeat, Rgb, Speed, Swatches, Use};

/// The maximum number of looks from the root to a leaf, counting both.
pub const MAX_DEPTH: usize = 8;

/// A compiled look. Sequence ends and measures use the clock's Q32 beats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Node {
    Step {
        values: Light,
    },
    Wave {
        /// Whole beats. Keeping the authored period here lets every instruction
        /// fit in the same space as a v5 sequence or step.
        period: u8,
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
    pub fn finished(&self, elapsed: impl Into<Elapsed>, settings: Use) -> bool {
        let measure = match self.nodes.get(self.root) {
            Some(Node::Sequence { measure, .. }) => *measure,
            Some(Node::Wave { period, .. }) => u64::from(*period) << 32,
            Some(Node::Step { .. }) | None => return false,
        };
        let time = elapsed.into().0;
        settings.repeat == Repeat::Once && !time.at_speed(settings.speed).before(time.at(measure))
    }

    /// Sample one light at the clock's elapsed count since this play began.
    pub fn sample(
        &self,
        bindings: &[Binding],
        elapsed: impl Into<Elapsed>,
        settings: Use,
    ) -> Light {
        Sampler {
            program: self,
            bindings,
        }
        .visit(self.root, elapsed.into().0, settings, MAX_DEPTH)
    }
}

/// Time since a play started, ready for every light that uses the same clock.
#[derive(Clone, Copy)]
pub struct Elapsed(Moment);

impl From<u64> for Elapsed {
    fn from(whole: u64) -> Self {
        BeatTime::from(whole).since(0.into())
    }
}

impl BeatTime {
    /// Subtract the exact start position, even if a tap has changed the clock's span.
    pub fn since(self, start: Self) -> Elapsed {
        let (now_count, now_part, now_span) = self.parts();
        let (start_count, start_part, start_span) = start.parts();
        // One Q32 count has this many subcounts. The larger span divides the scale
        // exactly; only the smaller span can leave a fractional subcount. Keep
        // 2^MAX_DEPTH for nested half speeds and 100 for percentage offsets.
        let factor = 100 << MAX_DEPTH;
        let span = now_span.max(start_span);
        let scale = u128::from(span) * factor;
        let position = |whole: u64, part: u64, clock_span: u64| {
            let count = u128::from(whole) * scale;
            if clock_span == span {
                (count + u128::from(part) * factor, 0)
            } else {
                let fraction = u128::from(part) * scale;
                let divisor = u128::from(clock_span);
                (count + fraction / divisor, fraction % divisor)
            }
        };
        let (now, end_part) = position(now_count, now_part, now_span);
        let (from, begin_part) = position(start_count, start_part, start_span);
        let mut time = Moment {
            count: 0,
            scale,
            part: 0,
            divisor: 1,
            left: false,
        };
        if (now, end_part) > (from, begin_part) {
            let divisor = u128::from(now_span.min(start_span));
            let borrow = begin_part > end_part;
            time.count = now - from - u128::from(borrow);
            time.part = end_part + u128::from(borrow) * divisor - begin_part;
            if time.part != 0 {
                time.divisor = divisor;
            }
        }
        Elapsed(time)
    }
}

/// An exact position or its left limit. Carrying the side through nested clocks
/// makes an outgoing loop at its wrap select its final item, even after a late wake.
#[derive(Clone, Copy)]
struct Moment {
    count: u128,
    scale: u128,
    /// Fraction of the next subcount, carried through each linked target's spread.
    part: u128,
    divisor: u128,
    left: bool,
}

impl Moment {
    fn subcounts(self, clock_count: u64) -> u128 {
        u128::from(clock_count) * self.scale
    }

    fn at(self, count: u64) -> Self {
        Self {
            count: self.subcounts(count),
            scale: self.scale,
            part: 0,
            divisor: 1,
            left: false,
        }
    }

    fn left(self, count: u64) -> Self {
        Self {
            left: true,
            ..self.at(count)
        }
    }

    fn before(self, edge: Self) -> bool {
        let phase = (self.count, self.part * edge.divisor);
        let edge = (edge.count, edge.part * self.divisor);
        phase < edge || (self.left && phase == edge)
    }

    fn since(self, start: u64) -> Self {
        Self {
            count: self.count.saturating_sub(self.subcounts(start)),
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

    fn times(mut self, factor: u128) -> Self {
        let fraction = self.part * factor;
        self.count = self.count * factor + fraction / self.divisor;
        self.part = fraction % self.divisor;
        self
    }

    /// Progress through the part of an item left after its step-entry delay.
    /// Keep both weights in the same units so a fractional delay cancels exactly.
    fn fade_weight(mut self, delay: Self, width: u128, percent: u8) -> Option<(Self, u128)> {
        if percent == 0 {
            return None;
        }
        self.count -= delay.count;
        let mut remaining = width - delay.count;
        if delay.part != 0 {
            self = self.times(delay.divisor);
            self.count -= delay.part;
            remaining = remaining * delay.divisor - delay.part;
        }
        let span = if remaining.is_multiple_of(100) {
            remaining / 100 * u128::from(percent)
        } else {
            // A tapped tempo and fractional delay can leave a non-integral
            // percentage. Keep the factor of 100 on the progress in that case.
            self = self.times(100);
            remaining * u128::from(percent)
        };
        (self.count < span).then_some((self, span))
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

    /// Add span × spread/100 × index/count. Leave 16 bits for an RGB16
    /// product. Compiled plans fit: at depth eight their per-light visit cost
    /// caps the root at 425 lights. Eight such divisors and the smaller tap
    /// span take under 101 bits. Reject oversized caller-built programs.
    fn shifted(mut self, span: u128, spread: Percent, binding: &Binding) -> Option<Self> {
        let spread = spread.get();
        if spread == 0 || binding.index == 0 || binding.count == 0 {
            return Some(self);
        }
        let offset = (span / 100) * u128::from(spread) * u128::from(binding.index);
        let lights = u128::from(binding.count);
        self.count += offset / lights;
        let remainder = offset % lights;
        if remainder != 0 {
            let divisor = self.divisor * lights;
            if divisor > 1u128 << 112 {
                return None;
            }
            self.part = self.part * lights + remainder * self.divisor;
            self.divisor = divisor;
            self = self.carry();
        }
        Some(self)
    }

    /// Phase in this use and whether it has a predecessor across a loop boundary.
    fn phase(self, measure: u64, settings: Use, binding: &Binding) -> Option<(Self, bool)> {
        let end = self.at(measure);
        let mut phase =
            self.at_speed(settings.speed)
                .shifted(end.count, settings.spread, binding)?;
        if phase.before(end) {
            return Some((phase, false));
        }
        if settings.repeat == Repeat::Once {
            return Some((self.left(measure), false));
        }
        phase.count %= end.count;
        if phase.left && phase.count == 0 && phase.part == 0 {
            Some((self.left(measure), true))
        } else {
            Some((phase, true))
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
            Some(Node::Wave { period, swatches }) if *period > 0 => {
                let measure = u64::from(*period) << 32;
                let Some((phase, _)) = time.phase(measure, settings, binding) else {
                    return Light::DARK;
                };
                return Light {
                    colour: wave::colour(swatches.as_slice(), phase, time.subcounts(measure)),
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
        let Some((phase, wrapped)) = time.phase(measure, settings, binding) else {
            return Light::DARK;
        };
        let index = items.partition_point(|item| !phase.before(time.at(item.end)));
        let Some((incoming, start)) = item_at(items, index) else {
            return Light::DARK;
        };
        let age = phase.since(start);
        let width = time.subcounts(incoming.end.saturating_sub(start));
        let delay = if incoming.settings.spread != Percent::LEAST
            && matches!(
                self.program.nodes.get(incoming.child),
                Some(Node::Step { .. })
            ) {
            self.binding(incoming.child)
                .and_then(|b| time.at(0).shifted(width, incoming.settings.spread, b))
                .unwrap_or(time.at(0))
        } else {
            time.at(0)
        };
        let previous = || {
            let index = index
                .checked_sub(1)
                .or_else(|| wrapped.then(|| items.len().saturating_sub(1)))?;
            let (outgoing, start) = item_at(items, index)?;
            Some(self.visit(
                outgoing.child,
                time.left(outgoing.end.saturating_sub(start)),
                outgoing.settings,
                depth,
            ))
        };
        if age.before(delay) {
            return previous().unwrap_or(Light::DARK);
        }
        let now = self.visit(incoming.child, age, incoming.settings, depth);
        let Some((progress, fade)) = age.fade_weight(delay, width, incoming.transition) else {
            return now;
        };
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

/// Round the exact weighted channel. The position may lie a fraction of a subcount
/// on either side of a half tie. At a rising tie the outgoing left limit rounds down.
fn interpolate(from: u16, to: u16, weight: Moment, span: u128) -> u16 {
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

/// Only the incoming sample can claim a property. A common continuous value
/// interpolates in 16 bits, rounded to nearest. Gobo and wheel colour cut at entry.
fn blend(from: Light, mut to: Light, progress: Moment, fade: u128, discrete_colour: bool) -> Light {
    let lerp = |a, b| interpolate(a, b, progress, fade);
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
            spread: Percent::MOST,
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
    fn a_spread_step_fade_rounds_at_the_midpoint_and_at_its_left_limit() {
        let mut graph = Graph::default();
        let dark = graph.step(Some(0));
        let lit = graph.step(Some(1));
        let normal = Use::default();
        let once = Use {
            repeat: Repeat::Once,
            ..normal
        };
        for (percent, transition, count, width) in [
            // Offset 2/3, delay 1/3. At beat 1 the full fade is halfway.
            (100, 100, 3, 16),
            // Offset 1/400, delay 1/300. At beat 1.25 the 50% fade is halfway.
            (1, 50, 8, 20),
        ] {
            let spread = Use {
                spread: Percent::new(percent).unwrap(),
                ..normal
            };
            let fade = graph.sequence(&[(dark, 16, normal, 0), (lit, 16, spread, transition)]);
            let parent = graph.sequence(&[(fade, width, spread, 0)]);
            let mut bindings = graph.bindings(1, 3);
            bindings[fade].count = count;
            for (root, settings, expected) in [(fade, spread, 1), (parent, once, 0)] {
                assert_eq!(
                    graph
                        .program(root)
                        .sample(&bindings, u64::from(width) * UNIT, settings)
                        .intensity,
                    Some(expected)
                );
            }
        }
    }

    #[test]
    fn sequence_spread_cuts_on_the_first_clock_count_that_reaches_the_delay() {
        let mut graph = Graph::default();
        let lit = graph.step(Some(1));
        let normal = Use::default();
        let spread = Use {
            spread: Percent::MOST,
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
            spread: Percent::MOST,
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
            spread: Percent::MOST,
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
