//! The saved look graph and its immutable playback plans. Edits compile off the tick
//! thread. A new revision shares every plan outside the edited look's parent chain.

#![no_std]
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

extern crate alloc;

mod compile;
mod pattern;
mod sequence;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::sync::Arc;
use alloc::vec::Vec;
use engine_core::{Definition, LOOKS, MixRules, Refusal, SavedLook};
use rig::Patch;

pub use compile::{PLAN_BYTES, Plan, WORK_LIMIT, compile, play_work};

/// Edges of the authored graph, including a Wave that reads a step's swatches.
fn references(definition: &Definition, mut uses: impl FnMut(u16) -> bool) -> bool {
    match definition {
        Definition::Step(_) => false,
        Definition::Sequence(sequence) => sequence.as_slice().iter().any(|item| uses(item.look)),
        Definition::Pattern(pattern) => pattern::reference(pattern).is_some_and(uses),
    }
}

/// Definitions and plans belong to one revision. The sender can start any look by
/// sharing its precompiled plan, even while another revision is being prepared.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Looks {
    saved: BTreeMap<u16, Arc<SavedLook>>,
    plans: BTreeMap<u16, Arc<Plan>>,
    rules: MixRules,
}

impl Looks {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn rules(&self) -> MixRules {
        self.rules
    }

    pub fn with_rules(&self, rules: MixRules) -> Self {
        Self {
            rules,
            ..self.clone()
        }
    }

    pub fn all(&self) -> Vec<SavedLook> {
        self.rows().copied().collect()
    }

    /// Authored rows in file order, borrowed while preparing a saved edit.
    pub fn rows(&self) -> impl Iterator<Item = &SavedLook> {
        self.saved.values().map(Arc::as_ref)
    }

    pub fn get(&self, look: u16) -> Option<&SavedLook> {
        self.saved.get(&look).map(Arc::as_ref)
    }

    /// Already compiled against this revision's patch.
    pub fn plan(&self, look: u16) -> Result<Arc<Plan>, Refusal> {
        self.plans.get(&look).cloned().ok_or(Refusal::NoSuchLook)
    }

    /// Bytes of compiled plans kept solely by older revisions after this edit.
    pub fn replaced_bytes(&self, next: &Self) -> usize {
        self.plans
            .iter()
            .filter(|(id, old)| next.plans.get(id).is_none_or(|new| !Arc::ptr_eq(old, new)))
            .map(|(_, old)| old.bytes())
            .sum()
    }

    /// A replacement is atomic across every parent, even when the change makes a
    /// parent's transitive expansion too large.
    pub fn put(&self, saved: impl Into<SavedLook>, patch: &Patch) -> Result<Self, Refusal> {
        let saved = saved.into();
        if let Some(previous) = self.get(saved.look) {
            if core::mem::discriminant(&previous.definition)
                != core::mem::discriminant(&saved.definition)
            {
                return Err(Refusal::WrongLookKind);
            }
        } else {
            Self::room(self.saved.len())?;
        }
        let look = saved.look;
        let mut next = self.clone();
        next.saved.insert(look, Arc::new(saved));
        let mut parents = BTreeSet::from([look]);
        loop {
            let before = parents.len();
            for saved in next.saved.values() {
                if references(&saved.definition, |id| parents.contains(&id)) {
                    parents.insert(saved.look);
                }
            }
            if parents.len() == before {
                break;
            }
        }
        next.recompile(patch, &parents, false)?;
        Ok(next)
    }

    fn room(count: usize) -> Result<(), Refusal> {
        if count >= LOOKS {
            Err(Refusal::TooManyLooks { most: LOOKS as u16 })
        } else {
            Ok(())
        }
    }

    /// Restore the complete graph before following references. File order is by ID,
    /// so a child can occur after its parents.
    pub fn restore(
        saved: impl IntoIterator<Item = SavedLook>,
        rules: MixRules,
        patch: &Patch,
    ) -> Result<Self, Refusal> {
        let mut all = Self {
            rules,
            ..Self::default()
        };
        for saved in saved {
            if all.saved.contains_key(&saved.look) {
                return Err(Refusal::LookInUse { look: saved.look });
            }
            Self::room(all.saved.len())?;
            all.saved.insert(saved.look, Arc::new(saved));
        }
        let ids = all.saved.keys().copied().collect();
        all.recompile(patch, &ids, false)?;
        Ok(all)
    }

    /// Remove an unreferenced definition. The sender also checks play reservations.
    pub fn remove(&self, look: u16) -> Result<Self, Refusal> {
        if let Some(parent) = self
            .saved
            .values()
            .find(|saved| references(&saved.definition, |id| id == look))
        {
            return Err(Refusal::LookInUse { look: parent.look });
        }
        let mut next = self.clone();
        next.saved.remove(&look);
        next.plans.remove(&look);
        Ok(next)
    }

    /// Compile the whole graph against a prospective patch. A failed rebind names
    /// the definition that would no longer play.
    pub fn rebind(&self, patch: &Patch) -> Result<Self, Refusal> {
        let mut next = self.clone();
        next.recompile(patch, &self.saved.keys().copied().collect(), true)?;
        Ok(next)
    }

    pub fn check(&self, patch: &Patch) -> Result<(), Refusal> {
        self.rebind(patch).map(drop)
    }

    fn recompile(
        &mut self,
        patch: &Patch,
        roots: &BTreeSet<u16>,
        rebinding: bool,
    ) -> Result<(), Refusal> {
        // Count unaffected roots first. A valid edit may shrink one parent and grow
        // another. Never charge the replaced roots to the prospective revision.
        let mut bytes = self
            .plans
            .iter()
            .filter(|(look, _)| !roots.contains(look))
            .map(|(_, plan)| plan.bytes())
            .sum::<usize>();
        for &look in roots {
            let plan = Plan::build(self, look, patch, PLAN_BYTES - bytes).map_err(|reason| {
                if rebinding && reason != Refusal::WorkLimit {
                    Refusal::LookInUse { look }
                } else {
                    reason
                }
            })?;
            bytes += plan.bytes();
            if self.plans.get(&look).is_none_or(|prior| **prior != plan) {
                self.plans.insert(look, Arc::new(plan));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "the test fixture supplies the named looks and bound lights"
)]
mod tests {
    use super::*;
    use core::num::NonZeroU16;
    use engine_core::{
        ChannelNumbers, Color, Definition, Light, Lights, PatchCommand, Pixels, SavedLook,
        Sequence, SequenceItem, Target, Text, Universes, Use,
    };
    use engine_core::{Saved, Step};
    use rig::Whole;

    pub(super) fn patch() -> Patch {
        let empty = Patch::new(
            Universes::new(1).unwrap(),
            Lights::new(32).unwrap(),
            Whole(|_| NonZeroU16::MIN),
        );
        empty
            .update(
                PatchCommand::PatchStrip {
                    name: Text::new("strip").unwrap(),
                    pixels: Pixels::try_from(3).unwrap(),
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
            .unwrap()
    }

    pub(super) fn leaf(look: u16) -> Saved {
        Saved {
            look,
            name: Text::new("rest").unwrap(),
            step: Step {
                target: Target::Fixture { fixture: 1 },
                values: Light::DARK,
            },
        }
    }

    pub(super) fn item(look: u16) -> SequenceItem {
        SequenceItem {
            look,
            width: 16,
            transition: 0,
            usage: Use::default(),
        }
    }

    pub(super) fn sequence(look: u16, items: &[SequenceItem]) -> SavedLook {
        SavedLook {
            look,
            name: Text::new("sequence").unwrap(),
            definition: Definition::Sequence(Sequence::new(items).unwrap()),
        }
    }

    #[test]
    fn an_invalid_edit_is_refused_as_a_whole() {
        let patch = patch();
        let looks = Looks::new().put(leaf(1), &patch).unwrap();
        let looks = looks.put(sequence(2, &[item(1)]), &patch).unwrap();
        let looks = looks.put(sequence(3, &[item(2); 64]), &patch).unwrap();
        let looks = looks.put(sequence(4, &[item(3); 64]), &patch).unwrap();
        for (name, invalid, reason) in [
            (
                "cycle through a parent",
                sequence(2, &[item(4)]),
                Refusal::LookCycle,
            ),
            (
                "step to sequence",
                sequence(1, &[item(2)]),
                Refusal::WrongLookKind,
            ),
            ("sequence to step", leaf(2).into(), Refusal::WrongLookKind),
            ("absent child", sequence(2, &[item(9)]), Refusal::NoSuchLook),
            (
                "zero width",
                sequence(
                    2,
                    &[SequenceItem {
                        width: 0,
                        ..item(1)
                    }],
                ),
                Refusal::InvalidTiming,
            ),
            (
                "transition past 100",
                sequence(
                    2,
                    &[SequenceItem {
                        transition: 101,
                        ..item(1)
                    }],
                ),
                Refusal::InvalidTiming,
            ),
            (
                "spread past 100",
                sequence(
                    2,
                    &[SequenceItem {
                        usage: Use {
                            spread: 101,
                            ..Use::default()
                        },
                        ..item(1)
                    }],
                ),
                Refusal::InvalidTiming,
            ),
            (
                "reverse expansion past limit",
                sequence(2, &[item(1); 64]),
                Refusal::WorkLimit,
            ),
        ] {
            let before = looks.clone();
            assert_eq!(looks.put(invalid, &patch), Err(reason), "{name}");
            assert_eq!(before, looks, "{name}");
        }
        assert_eq!(looks.remove(1), Err(Refusal::LookInUse { look: 2 }));
    }

    #[test]
    fn compiled_spread_uses_each_looks_own_order_and_skips_repeated_lights() {
        use engine_core::{Entry, Member, Members};
        const BEAT: u64 = 1 << 32;
        let group = Text::new("front").unwrap();
        let spread = Use {
            spread: 100,
            ..Use::default()
        };
        for (members, sequence_phase, step_entry) in [
            ([3, 1, 3, 2], [22, 22, 11], [11, 11, 22]),
            ([1, 2, 2, 3], [11, 22, 22], [22, 11, 11]),
        ] {
            let patch = patch()
                .update(
                    PatchCommand::SetGroup {
                        name: group,
                        members: Members::new(&members.map(|light| Member::Light { light }))
                            .unwrap(),
                    },
                    &[],
                )
                .unwrap()
                .0
                .unwrap();
            let mut looks = Looks::new();
            for (id, value, target) in [
                (1, 11, Target::Group { group }),
                (2, 22, Target::Group { group }),
                (3, 11, Target::Fixture { fixture: 1 }),
            ] {
                let mut saved = leaf(id);
                saved.step.target = target;
                saved.step.values.intensity = Some(value);
                looks = looks.put(saved, &patch).unwrap();
            }
            let initial = Entry {
                lights: patch.runs(Target::Light { light: 1 }).unwrap()[0],
                values: Light::DARK,
            };
            let delayed = SequenceItem {
                usage: spread,
                ..item(2)
            };
            for (items, at, usage, expected) in [
                ([item(1), item(2)], BEAT / 2, spread, sequence_phase),
                // The fixture gives the parent order 1, 2, 3. A quarter into the
                // group step, only its first distinct light should have entered.
                (
                    [item(3), delayed],
                    BEAT + BEAT / 4,
                    Use::default(),
                    step_entry,
                ),
            ] {
                let looks = looks.put(sequence(4, &items), &patch).unwrap();
                let plan = looks.plan(4).unwrap();
                let mut scratch = alloc::vec![initial; plan.lights()];
                assert!(plan.sample(at, usage, &mut scratch));
                assert_eq!(
                    plan.entries(&scratch)
                        .iter()
                        .map(|e| e.values.intensity)
                        .collect::<Vec<_>>(),
                    expected.map(Some),
                    "members {members:?} at {at}"
                );
            }
        }
    }

    #[test]
    fn a_shared_subgraph_cannot_hide_a_path_past_eight_looks() {
        let patch = patch();
        let mut looks = Looks::new().put(leaf(1), &patch).unwrap();
        for id in 2..=8 {
            looks = looks.put(sequence(id, &[item(id - 1)]), &patch).unwrap();
        }
        for children in [[item(2), item(8)], [item(8), item(2)]] {
            assert_eq!(
                looks.put(sequence(9, &children), &patch),
                Err(Refusal::WorkLimit)
            );
        }
    }
}
