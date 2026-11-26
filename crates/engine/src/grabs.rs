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

/// The boxes the tick may still hold when the front goes away ([`Grabs::in_flight`]).
#[expect(
    dead_code,
    reason = "held only to be dropped once the tick thread ended, never read"
)]
pub(crate) struct InFlight(VecDeque<(u64, Kept)>);

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

    /// The boxes the tick may still hold, for a front that goes away: whoever keeps them
    /// until the tick thread ended keeps the rule. The others can go with the book: the
    /// tick lets a box go only by a command, and none comes after the last one sent.
    pub(crate) fn in_flight(&mut self) -> InFlight {
        InFlight(std::mem::take(&mut self.retiring))
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
    use engine_core::{End, Gobo, Position, Rgb, Universes};
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

    #[test]
    fn a_grab_books_an_end_and_a_release_per_light_it_holds() {
        let lights = Lights::new(3).unwrap();
        let patch = Patch::new(Universes::ONE, lights, Whole(|_| NonZeroU16::MIN));
        let run = |first, count| {
            vec![LightRun::new(LightNumber::new(first, lights).unwrap(), count, lights).unwrap()]
        };
        let intensity = Value::Intensity(u16::MAX);
        let colour = Value::Colour(Rgb {
            red: u16::MAX,
            green: 0,
            blue: 0,
        });
        // docs/engine.md, "Booking by lights": a live light property books an end and a
        // release (2), a kept one only its release (1). (the book: each grab's value, first
        // light, light count, kept; what it books; what a new intensity grab on all three
        // lights adds: 2 a light, less what the light books already)
        let cases = [
            ("nobody holds them", vec![], 0, 6),
            (
                "a live grab holds them",
                vec![(intensity, 0, 3, false)],
                6,
                0,
            ),
            (
                "a kept grab holds them",
                vec![(intensity, 0, 3, true)],
                3,
                3,
            ),
            (
                "a live grab holds one",
                vec![(intensity, 0, 1, false)],
                2,
                4,
            ),
            (
                "a kept grab holds one, a live grab two",
                vec![(intensity, 0, 1, true), (intensity, 1, 2, false)],
                5,
                1,
            ),
            (
                "a live grab of colour holds them",
                vec![(colour, 0, 3, false)],
                6,
                6,
            ),
        ];
        for (case, book, booked, adds) in cases {
            let mut grabs = Grabs::new();
            for (generation, (value, first, count, kept)) in (0..).zip(book) {
                let id = GrabId { generation };
                grabs.hold(id, value, run(first, count), &patch);
                if kept {
                    grabs.end(id, End::Keep);
                }
            }
            assert_eq!(grabs.reserved(), booked, "{case}: booked");
            assert_eq!(
                grabs.fits(Property::Intensity, &run(0, 3)),
                Ok(adds),
                "{case}: a new grab adds"
            );
        }
    }

    /// A step on a full book: an intensity grab on lights (indices), or a grab's end.
    #[derive(Clone, Copy, Debug)]
    enum Step {
        Grab(u32, &'static [u16]),
        End(u32, End),
    }

    #[test]
    fn a_grab_fits_while_the_live_grabs_keep_room_for_its_runs() {
        let lights = Lights::new(MAX_LIGHTS).unwrap();
        let patch = Patch::new(Universes::ONE, lights, Whole(|_| NonZeroU16::MIN));
        let runs = |indices: &mut dyn Iterator<Item = u16>| -> Vec<LightRun> {
            indices
                .map(|at| LightRun::new(LightNumber::new(at, lights).unwrap(), 1, lights).unwrap())
                .collect()
        };
        let value = |property| match property {
            Property::Intensity => Value::Intensity(u16::MAX),
            Property::Colour => Value::Colour(Rgb {
                red: u16::MAX,
                green: 0,
                blue: 0,
            }),
            Property::Position => Value::Position(Position { pan: 0, tilt: 0 }),
            Property::Zoom => Value::Zoom(0),
            Property::Gobo(wheel) => Value::Gobo(Gobo { wheel, slot: 1 }),
        };
        let full = || {
            // Every light a run of its own, per property one live grab on the odd lights and
            // one on the even ones: 16 384 lights x 7 properties = 114 688 runs, the most.
            // The even intensity lights are two grabs: 0 on lights 0 and 2, 1 on the rest.
            let mut grabs = Grabs::new();
            let mut books: Vec<(Property, Vec<LightRun>)> = vec![
                (Property::Intensity, runs(&mut [0, 2].into_iter())),
                (Property::Intensity, runs(&mut (4..MAX_LIGHTS).step_by(2))),
            ];
            for property in Property::ALL {
                books.push((property, runs(&mut (1..MAX_LIGHTS).step_by(2))));
                if property != Property::Intensity {
                    books.push((property, runs(&mut (0..MAX_LIGHTS).step_by(2))));
                }
            }
            for (generation, (property, runs)) in (0..).zip(books) {
                grabs.hold(GrabId { generation }, value(property), runs, &patch);
            }
            assert_eq!(grabs.runs, 114_688, "the book is full");
            grabs
        };
        let most = Err(Refusal::GrabFull { most: 114_688 });
        // (steps, the lights a new intensity grab asks for, the answer: refused, or the
        // visits it adds, 2 a free light, 1 a kept one, 0 a live one)
        let cases = [
            (vec![], &[4][..], most),
            (vec![], &[0, 2][..], Ok(0)),
            (vec![Step::End(0, End::Keep)], &[0, 2][..], Ok(2)),
            (vec![Step::End(0, End::Keep)], &[0, 2, 4][..], most),
            (
                vec![Step::Grab(100, &[0, 2]), Step::End(100, End::Remove)],
                &[0, 2][..],
                Ok(4),
            ),
            (
                vec![Step::Grab(100, &[0, 2]), Step::End(100, End::Remove)],
                &[0, 2, 4][..],
                most,
            ),
        ];
        for (steps, asked, answer) in cases {
            let mut grabs = full();
            for step in &steps {
                match *step {
                    Step::Grab(generation, at) => {
                        let id = GrabId { generation };
                        grabs.hold(
                            id,
                            value(Property::Intensity),
                            runs(&mut at.iter().copied()),
                            &patch,
                        );
                    }
                    Step::End(generation, end) => drop(grabs.end(GrabId { generation }, end)),
                }
            }
            let asked = runs(&mut asked.iter().copied());
            assert_eq!(
                grabs.fits(Property::Intensity, &asked),
                answer,
                "{steps:?}, then {} runs",
                asked.len()
            );
        }
    }

    #[test]
    fn a_grab_fits_while_the_book_has_an_entry_for_it() {
        let lights = Lights::new(1024).unwrap();
        let patch = Patch::new(Universes::ONE, lights, Whole(|_| NonZeroU16::MIN));
        let light = |at| LightRun::new(LightNumber::new(at, lights).unwrap(), 1, lights).unwrap();
        let value = Value::Intensity(u16::MAX);
        // docs/engine.md: 1 024 sources, 32 plays, then 992 grabs. Each grab on a light of
        // its own; (the lights grab 0 holds, the light a new grab asks for, the answer)
        let most = Err(Refusal::GrabFull { most: 992 });
        let cases = [
            ("every entry is taken", vec![light(0)], 1000, most),
            ("it takes grab 0 whole", vec![light(0)], 0, Ok(0)),
            (
                "it takes one of grab 0's two",
                vec![light(0), light(992)],
                0,
                most,
            ),
        ];
        for (case, first, asked, answer) in cases {
            let mut grabs = Grabs::new();
            grabs.hold(GrabId { generation: 0 }, value, first, &patch);
            for generation in 1..992 {
                grabs.hold(
                    GrabId { generation },
                    value,
                    vec![light(u16::try_from(generation).unwrap())],
                    &patch,
                );
            }
            assert_eq!(
                grabs.fits(Property::Intensity, &[light(asked)]),
                answer,
                "{case}"
            );
        }
    }

    /// What the book gets from an event: a newer grab, an update or the end of grab 0, a
    /// release or an unpatch of the first lights, every grab let go, rules that no longer
    /// keep.
    #[derive(Clone, Copy, Debug)]
    enum Event {
        Grab(u16),
        Update,
        EndGrab(End),
        Release(u16),
        ReleaseAll,
        Unkeep,
        Unpatch(u16),
    }

    impl Event {
        /// The event on a book of intensity grabs on two lights: what it tells the tick.
        fn reach(self, grabs: &mut Grabs, patch: &Patch) -> Vec<Change> {
            let lights = Lights::new(2).unwrap();
            let first = |count| {
                vec![LightRun::new(LightNumber::new(0, lights).unwrap(), count, lights).unwrap()]
            };
            let (id, newer) = (GrabId { generation: 0 }, GrabId { generation: 1 });
            let intensity = Property::Intensity;
            match self {
                Event::Grab(count) => grabs.hold(newer, Value::Intensity(1), first(count), patch),
                Event::Update => grabs.update(id, Value::Intensity(1), patch),
                Event::EndGrab(end) => grabs.end(id, end),
                Event::Release(count) => grabs.let_go(Some(intensity), &first(count), patch),
                Event::ReleaseAll => {
                    grabs.clear();
                    Vec::new()
                }
                Event::Unkeep => {
                    grabs.clear_kept();
                    Vec::new()
                }
                Event::Unpatch(count) => grabs.let_go(None, &first(count), patch),
            }
        }
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
            (false, Event::EndGrab(End::Keep), vec![]),
            (false, Event::Release(2), vec![]),
            (false, Event::ReleaseAll, vec![]),
            (false, Event::Unkeep, vec![]),
            (false, Event::Unpatch(2), vec![]),
            (true, Event::EndGrab(End::Keep), vec![run(0, 2)]),
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
            drop(event.reach(&mut grabs, &patch));
            let book: Vec<_> = (0..GRABS)
                .filter_map(GrabEntry::new)
                .filter(|entry| grabs.grab(*entry).is_some_and(|grab| grab.id.is_none()))
                .map(|entry| grabs.held_by(entry))
                .collect();
            assert_eq!(book, left, "kept {kept}, {event:?}");
            assert_eq!(grabs.live.len(), 0, "kept {kept}, {event:?}: nothing live");
        }
    }

    #[test]
    fn the_book_walks_its_live_row() {
        let lights = Lights::new(2).unwrap();
        let patch = Patch::new(Universes::ONE, lights, Whole(|_| NonZeroU16::MIN));
        let run = |first, count| {
            vec![LightRun::new(LightNumber::new(first, lights).unwrap(), count, lights).unwrap()]
        };
        let id = GrabId { generation: 0 };
        use GrabChange::{Free, Keep, Offers, Start};
        // docs/engine.md, "A grab on the tick", row Live. (event, what the tick is told by
        // entry, the lights grab 0 holds live, the lights each kept grab holds, the lights
        // any grab holds)
        let cases = [
            (
                Event::Grab(2),
                vec![(0, Free), (0, Start)],
                vec![],
                vec![],
                run(0, 2),
            ),
            (
                Event::Grab(1),
                vec![(0, Offers), (1, Start)],
                run(1, 1),
                vec![],
                run(0, 2),
            ),
            (
                Event::Update,
                vec![(0, Offers)],
                run(0, 2),
                vec![],
                run(0, 2),
            ),
            (
                Event::EndGrab(End::Keep),
                vec![(0, Keep)],
                vec![],
                vec![run(0, 2)],
                run(0, 2),
            ),
            (
                Event::EndGrab(End::Remove),
                vec![(0, Free)],
                vec![],
                vec![],
                vec![],
            ),
            (
                Event::Release(1),
                vec![(0, Offers)],
                run(1, 1),
                vec![],
                run(1, 1),
            ),
            (Event::Release(2), vec![(0, Free)], vec![], vec![], vec![]),
            (Event::ReleaseAll, vec![], vec![], vec![], vec![]),
            (Event::Unkeep, vec![], run(0, 2), vec![], run(0, 2)),
            (
                Event::Unpatch(1),
                vec![(0, Offers)],
                run(1, 1),
                vec![],
                run(1, 1),
            ),
            (Event::Unpatch(2), vec![(0, Free)], vec![], vec![], vec![]),
        ];
        for (event, told, live, kept, held) in cases {
            let mut grabs = Grabs::new();
            grabs.hold(id, Value::Intensity(u16::MAX), run(0, 2), &patch);
            let changes: Vec<_> = event
                .reach(&mut grabs, &patch)
                .iter()
                .map(|change| (change.entry.index(), change.change))
                .collect();
            assert_eq!(changes, told, "{event:?}: the tick is told");
            assert_eq!(grabs.held(id), live, "{event:?}: grab 0 holds");
            assert_eq!(
                grabs.live.contains_key(&id),
                !live.is_empty(),
                "{event:?}: grab 0 is live"
            );
            let book: Vec<_> = (0..GRABS)
                .filter_map(GrabEntry::new)
                .filter(|entry| grabs.grab(*entry).is_some_and(|grab| grab.id.is_none()))
                .map(|entry| grabs.held_by(entry))
                .collect();
            assert_eq!(book, kept, "{event:?}: kept");
            assert_eq!(
                grabs.releasing(Property::Intensity, &run(0, 2)),
                held,
                "{event:?}: held by any grab"
            );
        }
    }

    #[test]
    fn a_box_let_go_stays_until_the_tick_applied_the_command() {
        let lights = Lights::new(2).unwrap();
        let patch = Patch::new(Universes::ONE, lights, Whole(|_| NonZeroU16::MIN));
        let run = vec![LightRun::new(LightNumber::new(0, lights).unwrap(), 2, lights).unwrap()];
        let id = GrabId { generation: 0 };
        let mut grabs = Grabs::new();
        let start = grabs.hold(id, Value::Intensity(u16::MAX), run, &patch);
        let words = start
            .first()
            .and_then(|change| change.words.clone())
            .unwrap();
        grabs.sent(1, Some(start.into()));
        let free = grabs.end(id, End::Remove);
        grabs.sent(2, Some(free.into()));
        assert_eq!(grabs.revision(), 2, "one more for each command");
        // docs/engine.md, "The tick never frees a box": the front keeps the change list and
        // the box until the tick applied the command. (commands the tick applied, who holds
        // the box: this test, command 1's change list, and the book until command 2)
        for (applied, holders) in [(0, 3), (1, 2), (2, 1)] {
            grabs.passed(applied);
            assert_eq!(Arc::strong_count(&words), holders, "{applied} applied");
        }
    }

    #[test]
    fn the_picture_shows_each_held_light_with_its_intensity_and_colour() {
        let lights = Lights::new(3).unwrap();
        let patch = Patch::new(Universes::ONE, lights, Whole(|_| NonZeroU16::MIN));
        let run = |first, count| {
            vec![LightRun::new(LightNumber::new(first, lights).unwrap(), count, lights).unwrap()]
        };
        let red = Rgb {
            red: u16::MAX,
            green: 0,
            blue: 0,
        };
        let mut grabs = Grabs::new();
        grabs.hold(
            GrabId { generation: 0 },
            Value::Intensity(1000),
            run(0, 2),
            &patch,
        );
        grabs.hold(
            GrabId { generation: 1 },
            Value::Colour(red),
            run(1, 2),
            &patch,
        );
        // docs/engine.md, `lights` in a Picture: each held light from 1, `None` where a
        // part is not held
        let held = |light, intensity, colour| HeldLight {
            light,
            intensity,
            colour,
        };
        assert_eq!(
            grabs.held_lights(),
            [
                held(1, Some(1000), None),
                held(2, Some(1000), Some(red)),
                held(3, None, Some(red)),
            ]
        );
    }
}
