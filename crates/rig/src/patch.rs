//! The patch: which fixture of the library, in which mode, where, under what name, with
//! which lights. Owned by the engine front, changed only by [`Patch::update`]: a
//! calculation, project and command in, new project and effects out.
//!
//! | Command | Does | Refused when |
//! |---|---|---|
//! | patch | adds a fixture: the lowest free number, the first free lights from the lowest | no such fixture, no such mode, past the end of the universe, on another fixture's channels, no lights left |
//! | patch strip | adds a generic strip of N pixels, RGB or RGBW, as patch does | as patch, but never no such fixture or mode |
//! | move | new address, the same lights: their held values stay | not patched, past the end, on another fixture's channels |
//! | unpatch | removes it, its lights' held values go, and it leaves its groups | not patched |
//! | rename | | not patched |
//! | set group | makes a group or replaces the one of that name: members in order | a member not patched |
//! | remove group | | |
//! | set, release intensity or colour | sent for one light, the lights of a fixture, or of every member of a group: a group as one effect, all its runs in one frame | not patched, no such light, no such group |
//!
//! No two fixtures share a channel: a patch, move or show file that would make two is
//! refused ([`Refusal::Overlaps`]), so every channel has one writer.
//!
//! A show file's fixtures come back with [`Patch::restore`]: each at its own number and
//! lights, checked as patch checks them, and refused when it has another's number or light.
//!
//! A patch kept in a show file ([`Patch::kept_in`]) refuses every change whose file would
//! be over the file's most bytes ([`Refusal::ShowFileFull`]): the file always holds the
//! patch. The file's text is the show file's to write (`crates/show-file`), so its
//! [`Limit`] brings the measure.
//!
//! No fixture loses a channel ([`Patch::unstreamed`]): a fixture over more than one
//! universe needs one output streaming them all, and every channel of a fixture an
//! output streams is one it sends. What an output sends is the outputs' to know
//! (`crates/outputs`), so [`Whole`] brings it in.

use crate::{ADDRESSES, Unfit, compile, flat, lights_of, strip, supports, unflat};
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::num::NonZeroU16;
use engine_core::{
    ChannelNumbers, EngineCommand, Grouped, LightChange, LightNumber, LightRun, Lights, Member,
    OutputKind, PatchCommand, Patched, Project, Refusal, Target, Universes, Value, Writer,
};
use fixture_library::{Fixture, Mode};

/// Every patched fixture, and the writers they make together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch {
    universes: Universes,
    lights: Lights,
    /// By number, lowest first. Shared with the patch before: a change copies only the
    /// fixtures it changes.
    fixtures: Vec<Arc<Entry>>,
    /// The writers of every fixture, in the order of their numbers.
    writers: Vec<Writer>,
    /// In the order they were made. Every member is patched.
    groups: Vec<Group>,
    /// The show file it is kept in, `None` when it is kept in none.
    limit: Option<Limit>,
    /// What each kind of output sends of its run.
    whole: Whole,
}

/// What a kind of output sends of its run: whole units of this many channels, the last
/// channels that make no whole unit never (a DDP RGB pixel is 3). A calculation of the
/// outputs', brought in so the patch checks every channel is sent without knowing bytes.
#[derive(Clone, Copy, Debug)]
pub struct Whole(pub fn(OutputKind) -> NonZeroU16);

impl PartialEq for Whole {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::fn_addr_eq(self.0, other.0)
    }
}

impl Eq for Whole {}

/// The show file a patch is kept in: its most bytes, and whether a patch's file is at
/// most that long. A calculation of the show file's, brought in so the patch measures
/// every change without knowing the file's text.
#[derive(Clone, Copy, Debug)]
pub struct Limit {
    pub most: usize,
    pub fits: fn(&Patch) -> bool,
    /// The file's text of a fixture's definition (maker, model, mode), made once when the
    /// fixture is patched and kept with it: a change of anything else does not encode it
    /// again, so `fits` sums what is kept and never encodes a mode.
    pub encode: fn(&str, &str, &Mode) -> Option<Arc<str>>,
}

impl PartialEq for Limit {
    fn eq(&self, other: &Self) -> bool {
        self.most == other.most
            && core::ptr::fn_addr_eq(self.fits, other.fits)
            && core::ptr::fn_addr_eq(self.encode, other.encode)
    }
}

impl Eq for Limit {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Group {
    name: String,
    /// In the order they were set.
    members: Vec<Member>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    number: u16,
    name: String,
    maker: String,
    model: String,
    mode: Mode,
    /// The file's text of its definition, once the patch is kept in a show file.
    text: Option<Arc<str>>,
    at: ChannelNumbers,
    first: LightNumber,
    lights: u16,
    writers: Vec<Writer>,
}

/// A fixture as a show file keeps it: its number and its lights as they were.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub fixture: NonZeroU16,
    pub name: String,
    pub maker: String,
    pub model: String,
    pub mode: Mode,
    pub at: ChannelNumbers,
    /// Its first light, counted from 1, as the picture shows it.
    pub first: u16,
}

/// Why a fixture of a show file is not restored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unplaced {
    /// Its number, or one of its lights, is another fixture's.
    Taken,
    /// Patched there, it would be refused for this reason.
    Refused(Refusal),
}

/// What follows a change, as data, in the order the front does it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchEffect {
    /// The fixture was patched with this number: the reply says it.
    Patched { fixture: u16 },
    /// The writers of the whole patch, to swap in on the tick thread.
    Writers(Vec<Writer>),
    /// Send this to the tick thread.
    Send(EngineCommand),
    /// Make `change` to each of these runs, all in one frame: a group of more than one run.
    Runs {
        change: LightChange,
        runs: Vec<LightRun>,
    },
    /// A new grab of `value` on these runs, lowest first: the front gives it its ID.
    Grab { value: Value, runs: Vec<LightRun> },
}

impl Patch {
    /// An empty patch for an engine of `universes` and `lights`, whose outputs send what
    /// `whole` says.
    pub const fn new(universes: Universes, lights: Lights, whole: Whole) -> Self {
        Self {
            universes,
            lights,
            fixtures: Vec::new(),
            writers: Vec::new(),
            groups: Vec::new(),
            limit: None,
            whole,
        }
    }

    /// This patch, kept in the show file of `limit`: from now on a change whose file
    /// would be over its most bytes is refused.
    #[must_use]
    pub fn kept_in(mut self, limit: Limit) -> Self {
        for entry in &mut self.fixtures {
            if entry.text.is_none() {
                let text = (limit.encode)(&entry.maker, &entry.model, &entry.mode);
                Arc::make_mut(entry).text = text;
            }
        }
        self.limit = Some(limit);
        self
    }

    /// `ShowFileFull` when this patch is over the most bytes the show file it is kept in
    /// gives it ([`Limit::most`]), so the looks have less than their room: one opened from
    /// a file at its most bytes. A patch kept in no file fits.
    pub fn fits(&self) -> Result<(), Refusal> {
        match self.limit {
            Some(limit) if !(limit.fits)(self) => Err(Refusal::ShowFileFull {
                most: u32::try_from(limit.most).unwrap_or(u32::MAX),
            }),
            Some(_) | None => Ok(()),
        }
    }

    /// The file's text of the definition fixture `fixture` is patched from, when it was
    /// made ([`Limit::encode`]).
    pub fn definition_text(&self, fixture: u16) -> Option<&Arc<str>> {
        self.entry(fixture).ok().and_then(|e| e.text.as_ref())
    }

    /// The only place the patch changes. `library` is where a patched fixture is looked
    /// up. The new patch is `None` when it stays as it is. On a refusal nothing changed:
    /// also when the new patch would not fit in the show file it is kept in.
    pub fn update(
        &self,
        command: PatchCommand,
        library: &[Fixture],
    ) -> Result<(Option<Self>, Vec<PatchEffect>), Refusal> {
        let (patch, effects) = self.changed_by(command, library)?;
        if let (Some(limit), Some(new)) = (self.limit, &patch)
            && !(limit.fits)(new)
        {
            return Err(Refusal::ShowFileFull {
                most: u32::try_from(limit.most).unwrap_or(u32::MAX),
            });
        }
        Ok((patch, effects))
    }

    /// What `command` makes of this patch, before the show file is asked.
    fn changed_by(
        &self,
        command: PatchCommand,
        library: &[Fixture],
    ) -> Result<(Option<Self>, Vec<PatchEffect>), Refusal> {
        match command {
            PatchCommand::Patch {
                name,
                maker,
                model,
                mode,
                at,
            } => {
                let fixture = library
                    .iter()
                    .find(|f| f.maker == maker.as_str() && f.model == model.as_str())
                    .ok_or(Refusal::NoSuchFixture)?;
                let mode = fixture
                    .modes
                    .iter()
                    .find(|m| m.name == mode.as_str())
                    .ok_or(Refusal::NoSuchMode)?;
                self.add(
                    name.as_str(),
                    &fixture.maker,
                    &fixture.model,
                    mode.clone(),
                    at,
                )
            }
            PatchCommand::PatchStrip {
                name,
                pixels,
                color,
                at,
            } => {
                // a strip longer than the lights left is refused before it is built
                self.free_lights(usize::from(pixels.count()))?;
                let (model, mode) = strip(pixels, color);
                self.add(name.as_str(), "Generic", model, mode, at)
            }
            PatchCommand::Move { fixture, at } => {
                let entry = self.entry(fixture)?;
                let rigged = compile(&entry.mode, at, entry.first, self.universes, self.lights)
                    .map_err(refusal)?;
                self.free_channels(fixture, at, entry.mode.channels.len())?;
                let patch = self.changed(fixture, |entry| {
                    entry.at = at;
                    entry.writers = rigged.writers;
                });
                let writers = PatchEffect::Writers(patch.writers.clone());
                Ok((Some(patch), vec![writers]))
            }
            PatchCommand::Unpatch { fixture } => {
                let entry = self.entry(fixture)?;
                let run = self.run(entry);
                let fixtures = self
                    .fixtures
                    .iter()
                    .filter(|e| e.number != fixture)
                    .cloned()
                    .collect();
                let gone = |member: &Member| match *member {
                    Member::Fixture { fixture: number } => number == fixture,
                    Member::Light { light } => entry.has(light),
                };
                let mut patch = self.with(fixtures);
                for group in &mut patch.groups {
                    group.members.retain(|member| !gone(member));
                }
                let mut effects = vec![PatchEffect::Writers(patch.writers.clone())];
                effects
                    .extend(run.map(|lights| PatchEffect::Send(EngineCommand::Darken { lights })));
                Ok((Some(patch), effects))
            }
            PatchCommand::Rename { fixture, name } => {
                self.entry(fixture)?;
                let patch = self.changed(fixture, |entry| entry.name = name.as_str().to_string());
                Ok((Some(patch), vec![]))
            }
            PatchCommand::Grab { target, value } => {
                let runs = self.runs(target)?;
                if runs.is_empty() {
                    return Err(Refusal::EmptyGroup); // a grab holds at least one light
                }
                self.supports(&runs, value)?;
                Ok((None, vec![PatchEffect::Grab { value, runs }]))
            }
            PatchCommand::Release { target, property } => {
                self.send(target, LightChange::Release { property })
            }
            PatchCommand::SetGroup { name, members } => {
                for member in members.as_slice() {
                    self.member(*member)?;
                }
                let group = Group {
                    name: name.as_str().to_string(),
                    members: members.as_slice().to_vec(),
                };
                let mut patch = self.clone();
                match patch.groups.iter_mut().find(|g| g.name == group.name) {
                    Some(made) => *made = group,
                    None => patch.groups.push(group),
                }
                Ok((Some(patch), vec![]))
            }
            PatchCommand::RemoveGroup { name } => {
                let mut patch = self.clone();
                patch.groups.retain(|g| g.name != name.as_str());
                Ok((Some(patch), vec![]))
            }
        }
    }

    /// This patch with `placed` added, each fixture at its own number and first light.
    /// All or none: the first that does not fit, by its number, and why.
    pub fn restore(&self, placed: Vec<Placed>) -> Result<Self, (u16, Unplaced)> {
        let mut fixtures = self.fixtures.clone();
        for p in placed {
            let number = p.fixture.get();
            let first = p
                .first
                .checked_sub(1)
                .and_then(|index| LightNumber::new(index, self.lights))
                .ok_or((number, Unplaced::Refused(Refusal::NoLightsLeft)))?;
            let rigged = compile(&p.mode, p.at, first, self.universes, self.lights)
                .map_err(|unfit| (number, Unplaced::Refused(refusal(unfit))))?;
            if let Some(fixture) = on(&fixtures, number, p.at, p.mode.channels.len()) {
                return Err((number, Unplaced::Refused(Refusal::Overlaps { fixture })));
            }
            let entry = Entry {
                number,
                name: p.name,
                maker: p.maker,
                model: p.model,
                mode: p.mode,
                text: None,
                at: p.at,
                first,
                lights: rigged.lights,
                writers: rigged.writers,
            };
            if fixtures
                .iter()
                .any(|e| e.number == number || e.shares_lights(&entry))
            {
                return Err((number, Unplaced::Taken));
            }
            let place = fixtures.partition_point(|e| e.number < number);
            fixtures.insert(place, Arc::new(entry));
        }
        Ok(self.with(fixtures))
    }

    /// The mode fixture `fixture` is patched in.
    pub fn mode(&self, fixture: u16) -> Option<&Mode> {
        self.entry(fixture).ok().map(|e| &e.mode)
    }

    /// Adds `mode` at `at` under the lowest free number, on the first free lights.
    fn add(
        &self,
        name: &str,
        maker: &str,
        model: &str,
        mode: Mode,
        at: ChannelNumbers,
    ) -> Result<(Option<Self>, Vec<PatchEffect>), Refusal> {
        let number = self.free_number().ok_or(Refusal::NoLightsLeft)?;
        let first = self.free_lights(lights_of(&mode))?;
        let rigged = compile(&mode, at, first, self.universes, self.lights).map_err(refusal)?;
        self.free_channels(number, at, mode.channels.len())?;
        let text = self
            .limit
            .and_then(|limit| (limit.encode)(maker, model, &mode));
        let entry = Entry {
            number,
            name: name.to_string(),
            maker: maker.to_string(),
            model: model.to_string(),
            mode,
            text,
            at,
            first,
            lights: rigged.lights,
            writers: rigged.writers,
        };
        let mut fixtures = self.fixtures.clone();
        let place = fixtures.partition_point(|e| e.number < number);
        fixtures.insert(place, Arc::new(entry));
        let patch = self.with(fixtures);
        let writers = PatchEffect::Writers(patch.writers.clone());
        Ok((
            Some(patch),
            vec![PatchEffect::Patched { fixture: number }, writers],
        ))
    }

    /// `change` for the lights `target` names: one run for a light or a fixture, the
    /// fewest runs for a group, lowest first. A group of more than one run is one effect.
    fn send(
        &self,
        target: Target,
        change: LightChange,
    ) -> Result<(Option<Self>, Vec<PatchEffect>), Refusal> {
        let runs = self.runs(target)?;
        let effects = match runs.as_slice() {
            [] => vec![],
            [run] => vec![PatchEffect::Send(change.on(*run))],
            [_, _, ..] => vec![PatchEffect::Runs { change, runs }],
        };
        Ok((None, effects))
    }

    /// `Unsupported` when a light of `runs` cannot show `value`.
    pub fn supports(&self, runs: &[LightRun], value: Value) -> Result<(), Refusal> {
        supports(&self.writers, runs, &value.light())
    }

    /// The lights `target` names: one run for a light or a fixture, the fewest runs for a
    /// group, lowest first. Refused when it names nothing patched.
    pub fn runs(&self, target: Target) -> Result<Vec<LightRun>, Refusal> {
        Ok(match target {
            Target::Light { light } => vec![self.member(Member::Light { light })?],
            Target::Fixture { fixture } => vec![self.member(Member::Fixture { fixture })?],
            Target::Group { group } => {
                let group = self
                    .groups
                    .iter()
                    .find(|g| g.name == group.as_str())
                    .ok_or(Refusal::NoSuchGroup)?;
                let runs = group
                    .members
                    .iter()
                    .map(|member| self.member(*member))
                    .collect::<Result<Vec<_>, _>>()?;
                self.joined(runs)
            }
        })
    }

    /// `runs` as the fewest runs that hold the same lights, lowest first.
    fn joined(&self, mut runs: Vec<LightRun>) -> Vec<LightRun> {
        runs.sort_unstable_by_key(|run| run.first().index());
        let mut joined: Vec<LightRun> = Vec::with_capacity(runs.len());
        for run in runs {
            let longer = joined.last().and_then(|last| {
                let end = last.first().index() + last.count();
                let to = run.first().index() + run.count();
                (run.first().index() <= end)
                    .then(|| {
                        LightRun::new(
                            last.first(),
                            end.max(to) - last.first().index(),
                            self.lights,
                        )
                    })
                    .flatten()
            });
            match (longer, joined.last_mut()) {
                (Some(longer), Some(last)) => *last = longer,
                (None, _) | (_, None) => joined.push(run),
            }
        }
        joined
    }

    /// Every patched fixture, by number, as the picture shows it.
    pub fn fixtures(&self) -> Vec<Patched<'_>> {
        self.fixtures
            .iter()
            .map(|e| Patched {
                fixture: e.number,
                name: &e.name,
                maker: &e.maker,
                model: &e.model,
                mode: &e.mode.name,
                universe: e.at.universe,
                address: e.at.address,
                channels: u16::try_from(e.mode.channels.len()).unwrap_or(u16::MAX),
                first: e.first.index().saturating_add(1),
                lights: e.lights,
                layout: e.mode.layout.map(|l| engine_core::Layout {
                    width: l.width,
                    height: l.height,
                }),
            })
            .collect()
    }

    /// The first fixture that would lose a channel with the outputs of `project`: its
    /// number, and why. `NotStreamed`: it runs over more than one universe and no output
    /// streams them all as one run. `NotSent`: an output streams one of its channels and
    /// never sends it. A fixture in one universe needs no output. The front keeps this
    /// `None` through every change of the patch and of the outputs.
    pub fn unstreamed(&self, project: &Project) -> Option<(u16, Refusal)> {
        self.fixtures.iter().find_map(|e| {
            let first = flat(e.at);
            let end = first + e.mode.channels.len();
            let last = unflat(end.checked_sub(1)?)?.universe;
            if last > e.at.universe && !project.streams(e.at.universe, last) {
                let first = e.at.universe;
                return Some((e.number, Refusal::NotStreamed { first, last }));
            }
            let unsent = project.sends().find_map(|(output, kind, run)| {
                let from = flat(ChannelNumbers {
                    universe: run.first(),
                    address: 1,
                });
                let to = from + usize::from(run.count()) * ADDRESSES;
                let whole = usize::from(kind.map_or(1, |kind| (self.whole.0)(kind).get()));
                let sent = from + (to - from) / whole * whole;
                // its channels from `sent` to `to` are streamed and never sent
                let lost = first.max(sent);
                (lost < end.min(to))
                    .then_some(lost)
                    .and_then(unflat)
                    .map(|at| Refusal::NotSent {
                        output,
                        universe: at.universe,
                        address: at.address,
                    })
            });
            unsent.map(|refusal| (e.number, refusal))
        })
    }

    /// Every group, in the order they were made, as the picture shows it.
    pub fn groups(&self) -> Vec<Grouped<'_>> {
        self.groups
            .iter()
            .map(|g| Grouped {
                name: &g.name,
                members: &g.members,
            })
            .collect()
    }

    /// The writers of every fixture, in the order of their numbers.
    pub fn writers(&self) -> &[Writer] {
        &self.writers
    }

    /// This patch with these fixtures: their writers made anew.
    fn with(&self, fixtures: Vec<Arc<Entry>>) -> Self {
        Self {
            universes: self.universes,
            lights: self.lights,
            writers: fixtures
                .iter()
                .flat_map(|e| e.writers.iter().copied())
                .collect(),
            fixtures,
            groups: self.groups.clone(),
            limit: self.limit,
            whole: self.whole,
        }
    }

    /// This patch with `change` made to fixture `number`.
    fn changed(&self, number: u16, change: impl FnOnce(&mut Entry)) -> Self {
        let mut fixtures = self.fixtures.clone();
        if let Some(entry) = fixtures.iter_mut().find(|e| e.number == number) {
            change(Arc::make_mut(entry));
        }
        self.with(fixtures)
    }

    fn entry(&self, number: u16) -> Result<&Entry, Refusal> {
        self.fixtures
            .iter()
            .find(|e| e.number == number)
            .map(AsRef::as_ref)
            .ok_or(Refusal::NotPatched)
    }

    /// The lights of `entry`.
    fn run(&self, entry: &Entry) -> Option<LightRun> {
        LightRun::new(entry.first, entry.lights, self.lights)
    }

    /// The lights of a patched light or fixture.
    fn member(&self, member: Member) -> Result<LightRun, Refusal> {
        match member {
            Member::Fixture { fixture } => {
                self.run(self.entry(fixture)?).ok_or(Refusal::NotPatched)
            }
            Member::Light { light } => self
                .fixtures
                .iter()
                .find(|e| e.has(light))
                .and_then(|_| LightNumber::new(light.checked_sub(1)?, self.lights))
                .and_then(|number| LightRun::new(number, 1, self.lights))
                .ok_or(Refusal::NoSuchLight),
        }
    }

    /// Refused when a fixture other than `number` has one of the `channels` channels from
    /// `at`, which must be checked.
    fn free_channels(
        &self,
        number: u16,
        at: ChannelNumbers,
        channels: usize,
    ) -> Result<(), Refusal> {
        on(&self.fixtures, number, at, channels)
            .map_or(Ok(()), |fixture| Err(Refusal::Overlaps { fixture }))
    }

    /// The lowest number no fixture has.
    fn free_number(&self) -> Option<u16> {
        let mut number = 1_u16;
        for entry in &self.fixtures {
            if entry.number != number {
                break;
            }
            number = number.checked_add(1)?;
        }
        Some(number)
    }

    /// The first of `needs` lights in a row that no fixture has, from the lowest.
    fn free_lights(&self, needs: usize) -> Result<LightNumber, Refusal> {
        let mut taken: Vec<(u16, u16)> = self
            .fixtures
            .iter()
            .map(|e| (e.first.index(), e.lights))
            .collect();
        taken.sort_unstable();
        let mut free = 0_usize;
        for (first, lights) in taken {
            if usize::from(first) >= free + needs {
                break;
            }
            free = free.max(usize::from(first) + usize::from(lights));
        }
        u16::try_from(free)
            .ok()
            .filter(|_| free + needs <= usize::from(self.lights.count()))
            .and_then(|free| LightNumber::new(free, self.lights))
            .ok_or(Refusal::NoLightsLeft)
    }
}

impl Entry {
    /// Whether `light` (counted from 1) is one of its lights.
    fn has(&self, light: u16) -> bool {
        light
            .checked_sub(1)
            .and_then(|index| index.checked_sub(self.first.index()))
            .is_some_and(|from| from < self.lights)
    }

    /// Whether one of its lights is one of `other`'s.
    fn shares_lights(&self, other: &Self) -> bool {
        let (from, to) = (self.first.index(), self.first.index() + self.lights);
        let (other_from, other_to) = (other.first.index(), other.first.index() + other.lights);
        from < other_to && other_from < to
    }
}

/// The lowest number of `fixtures` other than `number` that has one of the `channels`
/// channels from `at` (checked), counted over every universe in a row.
fn on(fixtures: &[Arc<Entry>], number: u16, at: ChannelNumbers, channels: usize) -> Option<u16> {
    let (from, to) = (flat(at), flat(at) + channels);
    fixtures
        .iter()
        .filter(|e| e.number != number)
        .find(|e| {
            let first = flat(e.at);
            first < to && from < first + e.mode.channels.len()
        })
        .map(|e| e.number)
}

/// Why a fixture does not fit, as the client is told.
fn refusal(unfit: Unfit) -> Refusal {
    match unfit {
        Unfit::Refused(refusal) => refusal,
        Unfit::PastTheEnd { channels } => Refusal::PastTheEnd {
            channels: u16::try_from(channels).unwrap_or(u16::MAX),
        },
        Unfit::TooManyLights { .. } => Refusal::NoLightsLeft,
    }
}
