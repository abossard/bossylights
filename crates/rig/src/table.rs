//! The field table: the one place a light becomes channels on the tick (#250). A light is
//! an index; the table gives its fields, and which of them fade. Plans and grabs carry
//! words per field of a light, never an address, so a Move swaps the table and every
//! playing look and grab follows its fixture. The interim step from a light index to
//! channels, until looks name fixtures (#253).
//!
//! ```text
//! light index --FieldPlan--> its fields (a channel, or a pair as one word), fade or jump
//! Words: (lights, words index) runs + a pool --offer--> (field, word) for the Mixer
//! ```

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::fmt;
use engine_core::{Configured, Field, LightRun, MAX_DEPTH};

/// The patch as the tick needs it, made off the tick for each patch edit and swapped in
/// whole: every patched channel as its file says, the dimmer channels (`highest` sets
/// them), each light's fields and fades, and the scratch a timed plan samples into.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct FieldPlan {
    configured: Vec<Configured>,
    dimmers: Vec<u32>,
    /// By light index: where its fields start, and how many.
    spans: Vec<(u32, u16)>,
    fields: Vec<Field>,
    fades: Vec<bool>,
    /// Room to sample the light with the most fields: [`MAX_DEPTH`] + 1 times them.
    scratch: Vec<Option<u16>>,
}

impl fmt::Debug for FieldPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FieldPlan")
    }
}

/// Each light's fields and fades, borrowed from a [`FieldPlan`].
#[derive(Clone, Copy)]
pub struct Targets<'a> {
    spans: &'a [(u32, u16)],
    fields: &'a [Field],
    fades: &'a [bool],
}

impl<'a> Targets<'a> {
    /// The fields of light `light` (an index) and whether each fades. Empty for a light
    /// nothing patched.
    pub fn of(&self, light: u16) -> (&'a [Field], &'a [bool]) {
        let Some(&(start, count)) = self.spans.get(usize::from(light)) else {
            return (&[], &[]);
        };
        let range = start as usize..start as usize + usize::from(count);
        (
            self.fields.get(range.clone()).unwrap_or_default(),
            self.fades.get(range).unwrap_or_default(),
        )
    }
}

impl FieldPlan {
    /// The table of `lights`, by index: each light's fields, its fades (from the flags of
    /// `configured`) and which are dimmers.
    pub(crate) fn new<'a>(
        configured: Vec<Configured>,
        lights: impl Iterator<Item = (u16, &'a crate::LightFields)>,
    ) -> Self {
        let fades_at: BTreeMap<u32, bool> =
            configured.iter().map(|c| (c.at, c.flags.fades())).collect();
        let mut plan = Self {
            configured,
            ..Self::default()
        };
        let mut most = 0;
        for (light, fields) in lights {
            let index = usize::from(light);
            if plan.spans.len() <= index {
                plan.spans.resize(index + 1, (0, 0));
            }
            let start = u32::try_from(plan.fields.len()).unwrap_or(u32::MAX);
            let count = fields.fields().count();
            if let Some(span) = plan.spans.get_mut(index) {
                *span = (start, u16::try_from(count).unwrap_or(u16::MAX));
            }
            most = most.max(count);
            for field in fields.fields() {
                let coarse = match field {
                    Field::Byte(at) => at,
                    Field::Word(pair) => pair.coarse,
                };
                plan.fields.push(field);
                plan.fades
                    .push(fades_at.get(&coarse).copied().unwrap_or(false));
            }
            plan.dimmers.extend(
                fields
                    .dimmers()
                    .flat_map(|field| match field {
                        Field::Byte(at) => [Some(at), None],
                        Field::Word(pair) => [Some(pair.coarse), Some(pair.fine)],
                    })
                    .flatten(),
            );
        }
        plan.scratch = alloc::vec![None; most * (MAX_DEPTH + 1)];
        plan
    }

    /// Every patched channel as its file says, for [`engine_core::Channels::configure`].
    pub fn configured(&self) -> &[Configured] {
        &self.configured
    }

    /// The dimmer channels, for [`engine_core::Channels::highest`].
    pub fn dimmers(&self) -> &[u32] {
        &self.dimmers
    }

    /// Each light's fields, and the scratch to sample one light into.
    pub fn split(&mut self) -> (Targets<'_>, &mut [Option<u16>]) {
        let (_, targets, scratch) = self.parts();
        (targets, scratch)
    }

    /// Every patched channel, each light's fields, and the scratch to sample one light
    /// into: what a frame needs at once.
    pub fn parts(&mut self) -> (&[Configured], Targets<'_>, &mut [Option<u16>]) {
        (
            &self.configured,
            Targets {
                spans: &self.spans,
                fields: &self.fields,
                fades: &self.fades,
            },
            &mut self.scratch,
        )
    }

    /// Each light's fields.
    pub fn targets(&self) -> Targets<'_> {
        Targets {
            spans: &self.spans,
            fields: &self.fields,
            fades: &self.fades,
        }
    }
}

/// Words on runs of lights: what a step or a grab offers. Each run names where its words
/// start in the pool: one word per field of each of its lights, `None` where it offers
/// nothing. Equal words are kept once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Words {
    runs: Vec<(LightRun, u32)>,
    pool: Vec<Option<u16>>,
}

impl Words {
    /// Each light's words per field, in light order, made into runs: lights one after
    /// the other with equal words share one run.
    pub(crate) fn new(lights: impl Iterator<Item = (LightRun, Vec<Option<u16>>)>) -> Self {
        let mut kept: BTreeMap<Vec<Option<u16>>, u32> = BTreeMap::new();
        let mut words = Self::default();
        for (light, each) in lights {
            let pool = &mut words.pool;
            let start = *kept.entry(each).or_insert_with_key(|each| {
                let start = u32::try_from(pool.len()).unwrap_or(u32::MAX);
                pool.extend_from_slice(each);
                start
            });
            match words.runs.last_mut() {
                Some((run, at))
                    if *at == start
                        && run.first().index() + run.count() == light.first().index() =>
                {
                    *run = run.joined(light.count());
                }
                _ => words.runs.push((light, start)),
            }
        }
        words
    }

    /// Each word on its field, through `targets`: `emit` takes it.
    pub fn offer(&self, targets: Targets<'_>, mut emit: impl FnMut(Field, u16)) {
        for (run, start) in &self.runs {
            let first = run.first().index();
            for light in first..first.saturating_add(run.count()) {
                let (fields, _) = targets.of(light);
                let words = self.pool.get(*start as usize..).unwrap_or_default();
                for (field, word) in fields.iter().zip(words) {
                    if let Some(word) = word {
                        emit(*field, *word);
                    }
                }
            }
        }
    }

    /// The lights it names.
    pub fn lights(&self) -> usize {
        self.runs
            .iter()
            .map(|(run, _)| usize::from(run.count()))
            .sum()
    }

    /// What it keeps in memory.
    pub fn bytes(&self) -> usize {
        core::mem::size_of_val(self.runs.as_slice()) + core::mem::size_of_val(self.pool.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use engine_core::{LightNumber, Lights};

    #[test]
    fn words_name_their_lights_and_keep_each_run_and_word_once() {
        let lights = Lights::new(8).unwrap();
        let light = |at| LightRun::new(LightNumber::new(at, lights).unwrap(), 1, lights).unwrap();
        // A run is a LightRun (two u16) and its u32 start: 8 bytes. A word is an
        // Option<u16>: 4 bytes. (case, each light's words, lights, bytes by hand)
        let cases = [
            ("none", vec![], 0, 0),
            (
                "three lights side by side, equal words: one run, two words",
                (0..3).map(|at| (at, vec![Some(1), None])).collect(),
                3,
                8 + 2 * 4,
            ),
            (
                "two lights apart, other words: two runs, two words",
                vec![(0, vec![Some(1)]), (5, vec![Some(2)])],
                2,
                2 * 8 + 2 * 4,
            ),
            (
                "two lights apart, equal words: two runs, one word",
                vec![(0, vec![Some(1)]), (2, vec![Some(1)])],
                2,
                2 * 8 + 4,
            ),
        ];
        for (case, each, named, bytes) in cases {
            let words = Words::new(each.into_iter().map(|(at, words)| (light(at), words)));
            assert_eq!(words.lights(), named, "{case}: lights");
            assert_eq!(words.bytes(), bytes, "{case}: bytes");
        }
    }
}
