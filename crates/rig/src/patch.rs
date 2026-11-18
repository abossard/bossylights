//! The patch: which fixture of the library, in which mode, where, under what name, with
//! which lights. Owned by the engine front, changed only by [`Patch::update`]: a
//! calculation, project and command in, new project and effects out.
//!
//! | Command | Does | Refused when |
//! |---|---|---|
//! | patch | adds a fixture: the lowest free number, the first free lights from the lowest | no such fixture, no such mode, past the end of the universe, no lights left |
//! | move | new address, the same lights: their held values stay | not patched, past the end |
//! | unpatch | removes it, its lights' held values go | not patched |
//! | rename | | not patched |
//! | set intensity, set colour | sent for the lights of a fixture, or one light | not patched, no such light |
//!
//! Two fixtures on the same channels are allowed: each shows the other in its clashes.

use crate::{Unfit, compile, lights_of};
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use engine_core::{
    ChannelNumbers, EngineCommand, LightNumber, LightRun, Lights, PatchCommand, Patched, Refusal,
    Target, Universes, Writer,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    number: u16,
    name: String,
    maker: String,
    model: String,
    mode: Mode,
    at: ChannelNumbers,
    first: LightNumber,
    lights: u16,
    writers: Vec<Writer>,
    /// The numbers of the fixtures on any of its channels, lowest first.
    clashes: Vec<u16>,
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
}

impl Patch {
    /// An empty patch for an engine of `universes` and `lights`.
    pub const fn new(universes: Universes, lights: Lights) -> Self {
        Self {
            universes,
            lights,
            fixtures: Vec::new(),
            writers: Vec::new(),
        }
    }

    /// The only place the patch changes. `library` is where a patched fixture is looked
    /// up. The new patch is `None` when it stays as it is. On a refusal nothing changed.
    pub fn update(
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
                let number = self.free_number().ok_or(Refusal::NoLightsLeft)?;
                let first = self.free_lights(lights_of(mode))?;
                let rigged =
                    compile(mode, at, first, self.universes, self.lights).map_err(refusal)?;
                let entry = Entry {
                    number,
                    name: name.as_str().to_string(),
                    maker: fixture.maker.clone(),
                    model: fixture.model.clone(),
                    mode: mode.clone(),
                    at,
                    first,
                    lights: rigged.lights,
                    writers: rigged.writers,
                    clashes: vec![],
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
            PatchCommand::Move { fixture, at } => {
                let entry = self.entry(fixture)?;
                let rigged = compile(&entry.mode, at, entry.first, self.universes, self.lights)
                    .map_err(refusal)?;
                let patch = self.changed(fixture, |entry| {
                    entry.at = at;
                    entry.writers = rigged.writers;
                });
                let writers = PatchEffect::Writers(patch.writers.clone());
                Ok((Some(patch), vec![writers]))
            }
            PatchCommand::Unpatch { fixture } => {
                let run = self.run(self.entry(fixture)?);
                let fixtures = self
                    .fixtures
                    .iter()
                    .filter(|e| e.number != fixture)
                    .cloned()
                    .collect();
                let patch = self.with(fixtures);
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
            PatchCommand::SetIntensity { target, intensity } => {
                let lights = self.target(target)?;
                let send = EngineCommand::SetIntensity { lights, intensity };
                Ok((None, vec![PatchEffect::Send(send)]))
            }
            PatchCommand::SetColour {
                target,
                red,
                green,
                blue,
            } => {
                let lights = self.target(target)?;
                let send = EngineCommand::SetColour {
                    lights,
                    red,
                    green,
                    blue,
                };
                Ok((None, vec![PatchEffect::Send(send)]))
            }
        }
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
                clashes: &e.clashes,
            })
            .collect()
    }

    /// The writers of every fixture, in the order of their numbers.
    pub fn writers(&self) -> &[Writer] {
        &self.writers
    }

    /// This patch with these fixtures: their writers and clashes made anew.
    fn with(&self, mut fixtures: Vec<Arc<Entry>>) -> Self {
        let mut spans: Vec<(u16, u16, u16, u16)> = fixtures
            .iter()
            .map(|e| {
                let channels = u16::try_from(e.mode.channels.len()).unwrap_or(u16::MAX);
                let last = e.at.address.saturating_add(channels.saturating_sub(1));
                (e.at.universe, e.at.address, last, e.number)
            })
            .filter(|&(_, first, last, _)| last >= first)
            .collect();
        spans.sort_unstable();
        let mut pairs = vec![];
        for (i, &(universe, _, last, number)) in spans.iter().enumerate() {
            for &(other_universe, other_first, _, other) in spans.iter().skip(i + 1) {
                if other_universe != universe || other_first > last {
                    break;
                }
                pairs.push((number, other));
                pairs.push((other, number));
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        for entry in &mut fixtures {
            let from = pairs.partition_point(|&(of, _)| of < entry.number);
            let to = pairs.partition_point(|&(of, _)| of <= entry.number);
            let clashes: Vec<u16> = pairs
                .get(from..to)
                .unwrap_or_default()
                .iter()
                .map(|&(_, with)| with)
                .collect();
            if entry.clashes != clashes {
                Arc::make_mut(entry).clashes = clashes;
            }
        }
        Self {
            universes: self.universes,
            lights: self.lights,
            writers: fixtures
                .iter()
                .flat_map(|e| e.writers.iter().copied())
                .collect(),
            fixtures,
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

    /// The lights a light command names.
    fn target(&self, target: Target) -> Result<LightRun, Refusal> {
        match target {
            Target::Fixture { fixture } => {
                self.run(self.entry(fixture)?).ok_or(Refusal::NotPatched)
            }
            Target::Light { light } => {
                let index = light.checked_sub(1).ok_or(Refusal::NoSuchLight)?;
                self.fixtures
                    .iter()
                    .filter(|e| index >= e.first.index() && index - e.first.index() < e.lights)
                    .find_map(|_| LightNumber::new(index, self.lights))
                    .and_then(|number| LightRun::new(number, 1, self.lights))
                    .ok_or(Refusal::NoSuchLight)
            }
        }
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
