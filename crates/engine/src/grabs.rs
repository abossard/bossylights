//! The grab book: direct holds after all admitted commands, by source-table entry
//! (`docs/design/who-wins.md`). Each grab has an entry of the tick's source table and a
//! box of words on its lights; a newer grab of a property takes those lights from older
//! ones, so ending it never brings an older grab back. Each live light property reserves
//! work for an end and a release; a kept one needs only its release.
//!
//! | State \ event | Grab (newer, its lights) | Update | End | Release, Gone | ReleaseAll | Rules unkeep |
//! |---|---|---|---|---|---|---|
//! | (free) | Live | nothing | nothing | nothing | (free) | (free) |
//! | Live | Live without them, or (free) | Live, new words | Keep: Kept; Remove: (free) | Live without them, or (free) | (free) | Live |
//! | Kept | Kept without them, or (free) | nothing | nothing | Kept without them, or (free) | (free) | (free) |
//!
//! The tick never frees a box: the book keeps a clone of every box the tick may hold,
//! and lets one go only once the tick applied the command that dropped it ([`Grabs::passed`]).

use engine_core::{
    End, GRABS, GrabChange, GrabEntry, GrabId, HeldLight, LightNumber, LightRun, Lights,
    MAX_LIGHTS, Property, Refusal, Value,
};
use rig::{Patch, Words};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// The most runs the live grabs keep: every property of every light held by a grab of its
/// own fits. A grab that would keep more is refused [`Refusal::GrabFull`].
pub const RUNS: usize = MAX_LIGHTS as usize * Property::ALL.len();

/// What one grab entry takes in a frame, and the box of words it brings.
pub(crate) struct Change {
    pub entry: GrabEntry,
    pub change: GrabChange,
    pub words: Option<Arc<Words>>,
}

/// The changes of one command, all in one frame.
pub(crate) type Changes = Arc<[Change]>;

/// A grab in the book: `Some` id while live, kept after its end.
struct Grab {
    id: Option<GrabId>,
    property: Property,
    value: Value,
    /// The runs it was given; the lights it still holds are those whose holder it is.
    runs: Vec<LightRun>,
    holds: u32,
    /// The box the tick has, or will have once it applied what was sent.
    words: Arc<Words>,
}

/// What the book keeps until the tick passed a command: boxes the tick may still hold.
#[expect(
    dead_code,
    reason = "held only to be dropped once the tick passed the command, never read"
)]
enum Kept {
    Words(Arc<Words>),
    Changes(Changes),
}

pub struct Grabs {
    /// Per light and property, the entry of the grab that holds it.
    holders: Box<[[Option<GrabEntry>; Property::ALL.len()]]>,
    /// By entry.
    entries: Box<[Option<Grab>]>,
    live: HashMap<GrabId, GrabEntry>,
    /// The runs the live grabs keep, all together.
    runs: usize,
    /// Boxes let go by the changes made since the last [`Grabs::sent`].
    dropped: Vec<Kept>,
    /// Boxes the tick may still hold, with the count of commands sent once it held them
    /// last: dropped when the tick applied that many.
    retiring: VecDeque<(u64, Kept)>,
    /// One more for each command that changed the book.
    revision: u64,
}

/// The light indices of `runs`.
fn lights(runs: &[LightRun]) -> impl Iterator<Item = usize> + '_ {
    runs.iter().flat_map(|run| {
        let first = usize::from(run.first().index());
        first..first + usize::from(run.count())
    })
}

/// Visits promised to end and release a light property held live (2), kept (1), or by
/// nobody (0).
fn release_work(live: Option<bool>) -> usize {
    match live {
        Some(true) => 2,
        Some(false) => 1,
        None => 0,
    }
}

impl Grabs {
    pub fn new() -> Self {
        Self {
            holders: vec![[None; Property::ALL.len()]; usize::from(MAX_LIGHTS)].into_boxed_slice(),
            entries: std::iter::repeat_with(|| None).take(GRABS).collect(),
            live: HashMap::new(),
            runs: 0,
            dropped: Vec::new(),
            retiring: VecDeque::new(),
            revision: 0,
        }
    }

    fn holder(&self, light: usize, property: Property) -> Option<GrabEntry> {
        self.holders
            .get(light)
            .and_then(|holders| holders.get(property.index()))
            .copied()
            .flatten()
    }

    fn set_holder(&mut self, light: usize, property: Property, entry: Option<GrabEntry>) {
        if let Some(slot) = self
            .holders
            .get_mut(light)
            .and_then(|holders| holders.get_mut(property.index()))
        {
            *slot = entry;
        }
    }

    fn grab(&self, entry: GrabEntry) -> Option<&Grab> {
        self.entries.get(entry.index()).and_then(Option::as_ref)
    }

    /// Whether the holder of `light`'s `property` is live, `None` when nobody holds it.
    fn live_at(&self, light: usize, property: Property) -> Option<bool> {
        self.holder(light, property)
            .and_then(|entry| self.grab(entry))
            .map(|grab| grab.id.is_some())
    }

    /// Visits already promised to end and release the holds.
    pub fn reserved(&self) -> usize {
        self.entries
            .iter()
            .flatten()
            .map(|grab| grab.holds as usize * release_work(Some(grab.id.is_some())))
            .sum()
    }

    /// The entries holding `property` on a light of `runs`, and how many of those
    /// lights each holds.
    fn holding(&self, property: Property, runs: &[LightRun]) -> HashMap<GrabEntry, u32> {
        let mut holding = HashMap::new();
        for light in lights(runs) {
            if let Some(entry) = self.holder(light, property) {
                *holding.entry(entry).or_insert(0) += 1;
            }
        }
        holding
    }

    /// Checks the run limit and that an entry is free once the older grabs let the
    /// lights go, and returns the additional visits needed to end and release the grab.
    pub fn fits(&self, property: Property, runs: &[LightRun]) -> Result<usize, Refusal> {
        let taken = self.holding(property, runs);
        let emptied = |entry: &GrabEntry, count: &u32| {
            self.grab(*entry).is_some_and(|grab| grab.holds == *count)
        };
        let freed: usize = taken
            .iter()
            .filter(|(entry, count)| emptied(entry, count))
            .filter_map(|(entry, _)| self.grab(*entry))
            .filter(|grab| grab.id.is_some())
            .map(|grab| grab.runs.len())
            .sum();
        if self.runs - freed + runs.len() > RUNS {
            return Err(Refusal::GrabFull { most: RUNS as u32 });
        }
        let free = self.entries.iter().any(Option::is_none)
            || taken.iter().any(|(entry, count)| emptied(entry, count));
        if !free {
            return Err(Refusal::GrabFull { most: GRABS as u32 });
        }
        Ok(lights(runs)
            .map(|light| 2 - release_work(self.live_at(light, property)))
            .sum())
    }

    /// `id` holds `value` on the lights of `runs`, the latest: the grabs it takes the
    /// lights from no longer hold them. Call [`Grabs::fits`] first.
    pub(crate) fn hold(
        &mut self,
        id: GrabId,
        value: Value,
        runs: Vec<LightRun>,
        patch: &Patch,
    ) -> Vec<Change> {
        let property = value.property();
        let mut changes = self.let_go(Some(property), &runs, patch);
        let Some(entry) = self
            .entries
            .iter()
            .position(Option::is_none)
            .and_then(GrabEntry::new)
        else {
            return changes;
        };
        let mut holds = 0;
        for light in lights(&runs) {
            self.set_holder(light, property, Some(entry));
            holds += 1;
        }
        let words = Arc::new(patch.words(&runs, &value.light()));
        self.runs += runs.len();
        self.live.insert(id, entry);
        changes.push(Change {
            entry,
            change: GrabChange::Start,
            words: Some(Arc::clone(&words)),
        });
        if let Some(slot) = self.entries.get_mut(entry.index()) {
            *slot = Some(Grab {
                id: Some(id),
                property,
                value,
                runs,
                holds,
                words,
            });
        }
        changes
    }

    /// The property of the live grab `id`.
    pub fn property(&self, id: GrabId) -> Option<Property> {
        let entry = self.live.get(&id)?;
        self.grab(*entry).map(|grab| grab.property)
    }

    /// The lights the grab at `entry` still holds.
    fn held_by(&self, entry: GrabEntry) -> Vec<LightRun> {
        let Some(grab) = self.grab(entry) else {
            return Vec::new();
        };
        selected(&grab.runs, |light| {
            self.holder(light, grab.property) == Some(entry)
        })
    }

    /// The lights the live grab `id` can still update or end.
    pub fn held(&self, id: GrabId) -> Vec<LightRun> {
        self.live
            .get(&id)
            .map_or_else(Vec::new, |entry| self.held_by(*entry))
    }

    /// The live grab `id` takes `value` on the lights it still holds.
    pub(crate) fn update(&mut self, id: GrabId, value: Value, patch: &Patch) -> Vec<Change> {
        let Some(&entry) = self.live.get(&id) else {
            return Vec::new();
        };
        let held = self.held_by(entry);
        let words = Arc::new(patch.words(&held, &value.light()));
        let Some(grab) = self.entries.get_mut(entry.index()).and_then(Option::as_mut) else {
            return Vec::new();
        };
        grab.value = value;
        let old = std::mem::replace(&mut grab.words, Arc::clone(&words));
        self.dropped.push(Kept::Words(old));
        vec![Change {
            entry,
            change: GrabChange::Offers,
            words: Some(words),
        }]
    }

    /// The live grab `id` ends: kept on the lights it still holds, or let go.
    pub(crate) fn end(&mut self, id: GrabId, end: End) -> Vec<Change> {
        let Some(entry) = self.live.remove(&id) else {
            return Vec::new();
        };
        if let Some(grab) = self.entries.get_mut(entry.index()).and_then(Option::as_mut) {
            self.runs -= grab.runs.len();
            grab.id = None;
        }
        match end {
            End::Keep => vec![Change {
                entry,
                change: GrabChange::Keep,
                words: None,
            }],
            End::Remove => {
                for light in lights(&self.held_by(entry)) {
                    if let Some(property) = self.grab(entry).map(|grab| grab.property) {
                        self.set_holder(light, property, None);
                    }
                }
                vec![self.free(entry)]
            }
        }
    }

    /// The entry is free: its box goes once the tick passed the command.
    fn free(&mut self, entry: GrabEntry) -> Change {
        if let Some(grab) = self.entries.get_mut(entry.index()).and_then(Option::take) {
            if let Some(id) = grab.id {
                self.live.remove(&id);
                self.runs -= grab.runs.len();
            }
            self.dropped.push(Kept::Words(grab.words));
        }
        Change {
            entry,
            change: GrabChange::Free,
            words: None,
        }
    }

    /// The lights of `runs` let `property` go, or every property when `None` (an
    /// unpatched fixture's lights): each grab that held one keeps the others, or is
    /// free.
    pub(crate) fn let_go(
        &mut self,
        property: Option<Property>,
        runs: &[LightRun],
        patch: &Patch,
    ) -> Vec<Change> {
        let mut touched = Vec::new();
        for property in Property::ALL
            .into_iter()
            .filter(|p| property.is_none_or(|property| property == *p))
        {
            for (entry, taken) in self.holding(property, runs) {
                if let Some(grab) = self.entries.get_mut(entry.index()).and_then(Option::as_mut) {
                    grab.holds = grab.holds.saturating_sub(taken);
                }
                touched.push(entry);
            }
            for light in lights(runs) {
                self.set_holder(light, property, None);
            }
        }
        touched.sort_unstable();
        touched.dedup();
        touched
            .into_iter()
            .map(|entry| {
                let left = self.grab(entry).map_or(0, |grab| grab.holds);
                if left == 0 {
                    return self.free(entry);
                }
                let held = self.held_by(entry);
                let value = self.grab(entry).map(|grab| grab.value.light());
                let words = Arc::new(value.map(|v| patch.words(&held, &v)).unwrap_or_default());
                if let Some(grab) = self.entries.get_mut(entry.index()).and_then(Option::as_mut) {
                    let old = std::mem::replace(&mut grab.words, Arc::clone(&words));
                    self.dropped.push(Kept::Words(old));
                }
                Change {
                    entry,
                    change: GrabChange::Offers,
                    words: Some(words),
                }
            })
            .collect()
    }

    /// Whether a grab holds `property` on a light of `runs`: a release has work.
    pub fn releasing(&self, property: Property, runs: &[LightRun]) -> Vec<LightRun> {
        selected(runs, |light| self.holder(light, property).is_some())
    }

    /// The kept grabs let go: rules that no longer keep them (the tick does the same
    /// with the `Rules` command).
    pub fn clear_kept(&mut self) {
        let kept: Vec<GrabEntry> = (0..GRABS)
            .filter_map(GrabEntry::new)
            .filter(|entry| self.grab(*entry).is_some_and(|grab| grab.id.is_none()))
            .collect();
        for entry in kept {
            for light in lights(&self.held_by(entry)) {
                if let Some(property) = self.grab(entry).map(|grab| grab.property) {
                    self.set_holder(light, property, None);
                }
            }
            self.free(entry);
        }
    }

    /// Every grab ends (the tick does the same with `ReleaseAll`).
    pub fn clear(&mut self) {
        self.holders.fill([None; Property::ALL.len()]);
        for grab in self.entries.iter_mut().filter_map(Option::take) {
            self.dropped.push(Kept::Words(grab.words));
        }
        self.live.clear();
        self.runs = 0;
    }

    /// The changes just made went with the command that makes `sent` commands sent: the
    /// boxes they let go, and `changes` itself, wait until the tick applied it.
    pub(crate) fn sent(&mut self, sent: u64, changes: Option<Changes>) {
        for kept in self.dropped.drain(..).chain(changes.map(Kept::Changes)) {
            self.retiring.push_back((sent, kept));
        }
        self.revision += 1;
    }

    /// One more for each command that changed the book: the held lights change only with it.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The tick applied `applied` commands: the boxes it let go by then are dropped here.
    pub fn passed(&mut self, applied: u64) {
        while self
            .retiring
            .front()
            .is_some_and(|(sent, _)| *sent <= applied)
        {
            self.retiring.pop_front();
        }
    }

    /// Boxes waiting for the tick, for tests.
    #[cfg(all(test, feature = "test-support"))]
    pub fn retiring(&self) -> usize {
        self.retiring.len()
    }

    /// Every light a grab holds, live or kept, in light order: what the picture shows.
    pub fn held_lights(&self) -> Vec<HeldLight> {
        let value = |light: usize, property: Property| {
            self.holder(light, property)
                .and_then(|entry| self.grab(entry))
                .map(|grab| grab.value)
        };
        (0..self.holders.len())
            .filter(|&light| {
                self.holders
                    .get(light)
                    .is_some_and(|h| h.iter().any(Option::is_some))
            })
            .filter_map(|light| {
                let intensity = match value(light, Property::Intensity) {
                    Some(Value::Intensity(level)) => Some(level),
                    _ => None,
                };
                let colour = match value(light, Property::Colour) {
                    Some(Value::Colour(colour)) => Some(colour),
                    _ => None,
                };
                Some(HeldLight {
                    light: u16::try_from(light + 1).ok()?,
                    intensity,
                    colour,
                })
            })
            .collect()
    }
}

/// Compact the matching lights of the Rig's sorted, disjoint runs.
fn selected(runs: &[LightRun], matches: impl Fn(usize) -> bool) -> Vec<LightRun> {
    let mut result = Vec::new();
    let mut indices = lights(runs).filter(|&light| matches(light)).peekable();
    while let Some(index) = indices.next() {
        let mut end = index + 1;
        while indices.next_if_eq(&end).is_some() {
            end += 1;
        }
        if let Some(first) = LightNumber::new(index as u16, Lights::MOST)
            && let Some(run) = LightRun::new(first, (end - index) as u16, Lights::MOST)
        {
            result.push(run);
        }
    }
    result
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
mod tests {
    use super::*;
    use engine_core::{End, Universes};
    use rig::Whole;
    use std::num::NonZeroU16;

    #[test]
    fn a_kept_grab_gives_its_runs_back() {
        let lights = Lights::new(MAX_LIGHTS).unwrap();
        let patch = Patch::new(Universes::ONE, lights, Whole(|_| NonZeroU16::MIN));
        // every other light: the most runs one grab brings
        let runs: Vec<LightRun> = (0..MAX_LIGHTS / 2)
            .map(|at| LightRun::new(LightNumber::new(2 * at, lights).unwrap(), 1, lights).unwrap())
            .collect();
        let value = Value::Intensity(u16::MAX);
        let mut grabs = Grabs::new();
        // grabbed and kept one after the other, more runs than the live grabs may keep
        for generation in 0..=u32::try_from(RUNS / runs.len()).unwrap() {
            let id = GrabId { generation };
            assert!(
                grabs.fits(value.property(), &runs).is_ok(),
                "grab {generation}"
            );
            grabs.hold(id, value, runs.clone(), &patch);
            grabs.end(id, End::Keep);
        }
    }

    #[test]
    fn a_newer_grab_takes_its_lights_from_a_kept_grab() {
        let lights = Lights::new(2).unwrap();
        let patch = Patch::new(Universes::ONE, lights, Whole(|_| NonZeroU16::MIN));
        let run = |first, count| {
            vec![LightRun::new(LightNumber::new(first, lights).unwrap(), count, lights).unwrap()]
        };
        let value = Value::Intensity(u16::MAX);
        let (kept, newer) = (GrabId { generation: 0 }, GrabId { generation: 1 });
        // (the newer grab's lights, what the tick is told by entry, what the book holds
        // once the newer one is let go)
        let cases = [
            (
                "all of them",
                run(0, 2),
                [(0, GrabChange::Free), (0, GrabChange::Start)],
                vec![],
            ),
            (
                "one of two",
                run(0, 1),
                [(0, GrabChange::Offers), (1, GrabChange::Start)],
                vec![run(1, 1)],
            ),
        ];
        for (case, taken, told, left) in cases {
            let mut grabs = Grabs::new();
            grabs.hold(kept, value, run(0, 2), &patch);
            grabs.end(kept, End::Keep);
            let changes = grabs.hold(newer, value, taken.clone(), &patch);
            let changes: Vec<_> = changes
                .iter()
                .map(|change| (change.entry.index(), change.change))
                .collect();
            assert_eq!(changes, told, "{case}: the tick is told");
            grabs.let_go(Some(value.property()), &taken, &patch);
            let book: Vec<_> = (0..GRABS)
                .filter_map(GrabEntry::new)
                .filter(|entry| grabs.grab(*entry).is_some())
                .map(|entry| grabs.held_by(entry))
                .collect();
            assert_eq!(book, left, "{case}");
        }
    }

    /// What the book gets from an event: the grab's end, a release or an unpatch of the
    /// first lights, every grab let go, rules that no longer keep.
    #[derive(Clone, Copy, Debug)]
    enum Event {
        EndGrab,
        Release(u16),
        ReleaseAll,
        Unkeep,
        Unpatch(u16),
    }

    #[test]
    fn the_book_walks_its_free_and_kept_rows() {
        let lights = Lights::new(2).unwrap();
        let patch = Patch::new(Universes::ONE, lights, Whole(|_| NonZeroU16::MIN));
        let run = |first, count| {
            vec![LightRun::new(LightNumber::new(first, lights).unwrap(), count, lights).unwrap()]
        };
        let value = Value::Intensity(u16::MAX);
        let id = GrabId { generation: 0 };
        // (kept on both lights, or (free), event, the lights each grab of the book holds)
        let cases = [
            (false, Event::EndGrab, vec![]),
            (false, Event::Release(2), vec![]),
            (false, Event::ReleaseAll, vec![]),
            (false, Event::Unkeep, vec![]),
            (false, Event::Unpatch(2), vec![]),
            (true, Event::EndGrab, vec![run(0, 2)]),
            (true, Event::Release(1), vec![run(1, 1)]),
            (true, Event::Release(2), vec![]),
            (true, Event::ReleaseAll, vec![]),
            (true, Event::Unkeep, vec![]),
            (true, Event::Unpatch(1), vec![run(1, 1)]),
            (true, Event::Unpatch(2), vec![]),
        ];
        for (kept, event, left) in cases {
            let mut grabs = Grabs::new();
            if kept {
                grabs.hold(id, value, run(0, 2), &patch);
                grabs.end(id, End::Keep);
            }
            match event {
                Event::EndGrab => drop(grabs.end(id, End::Keep)),
                Event::Release(count) => {
                    drop(grabs.let_go(Some(value.property()), &run(0, count), &patch));
                }
                Event::ReleaseAll => grabs.clear(),
                Event::Unkeep => grabs.clear_kept(),
                Event::Unpatch(count) => drop(grabs.let_go(None, &run(0, count), &patch)),
            }
            let book: Vec<_> = (0..GRABS)
                .filter_map(GrabEntry::new)
                .filter(|entry| grabs.grab(*entry).is_some_and(|grab| grab.id.is_none()))
                .map(|entry| grabs.held_by(entry))
                .collect();
            assert_eq!(book, left, "kept {kept}, {event:?}");
            assert_eq!(grabs.live.len(), 0, "kept {kept}, {event:?}: nothing live");
        }
    }
}
