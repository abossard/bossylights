//! The project: what the engine was asked to be, owned by the engine front, off the tick
//! thread. Today its outputs; patch and groups come with their tickets.
//!
//! ```text
//! client Command --> Project::update --> new project + effect --> the front performs it:
//!                                                                 send an EngineCommand,
//!                                                                 or open a device, then Opened / OpenFailed
//! ```
//!
//! One output, as the project sees it, is one machine (`Setup`): every state and event
//! in the table "An output in the project" in `docs/engine.md`, walked by a test.

use crate::{
    BlackoutAsk, Command, EngineCommand, LookCommand, MAX_OUTPUTS, MemberList, Members, OutputKind,
    OutputNumber, PatchCommand, Property, Rate, Refusal, Rgb, Run, Universes, Value,
};
use serde::Serialize;

/// What the front owns. Only [`Project::update`] changes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Project {
    // made from `slots` by `with` alone
    /// The outputs, by number. `None` where none is set.
    #[cfg_attr(feature = "ts", ts(type = "Array<ProjectOutput | null>"))]
    outputs: [Option<ProjectOutput>; MAX_OUTPUTS],
    /// The engine's universes: what a command is checked against.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    universes: Universes,
    /// Each output's state, and what its last switch replaced.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    slots: [Slot; MAX_OUTPUTS],
}

/// One output: its state and `before`, the settled state its last switch replaced, the
/// only one [`ProjectEvent::Restore`] goes back to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Slot {
    setup: Setup,
    before: Option<Settled>,
}

/// Where one output is: settled, or switching to a device being opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Setup {
    Settled(Settled),
    /// The wire still shows `was`; `to` is what `Opened` makes it.
    Switching {
        was: Settled,
        to: Opening,
    },
}

impl Default for Setup {
    fn default() -> Self {
        Self::Settled(Settled::Empty)
    }
}

impl Setup {
    const fn settled(self) -> Settled {
        match self {
            Self::Settled(settled) | Self::Switching { was: settled, .. } => settled,
        }
    }
}

/// An output with no device change under way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Settled {
    #[default]
    Empty,
    /// A device the engine opened for this kind.
    Kind { kind: OutputKind, run: Run },
    /// A device a test brought, sending the whole run.
    Attached { run: Run },
    /// A device a test brought with its own route: its bounds are display data, never a
    /// stream a patch check can count on (a route can have holes).
    Routed { bounds: Run },
}

impl Settled {
    const fn wire(self) -> Option<ProjectOutput> {
        match self {
            Self::Empty => None,
            Self::Kind { kind, run } => Some(ProjectOutput::Kind {
                kind,
                universes: run,
            }),
            Self::Attached { run } | Self::Routed { bounds: run } => {
                Some(ProjectOutput::Attached { universes: run })
            }
        }
    }

    /// What it sends that a patch check can count on: its kind, if known, and its run.
    const fn stream(self) -> Option<(Option<OutputKind>, Run)> {
        match self {
            Self::Kind { kind, run } => Some((Some(kind), run)),
            Self::Attached { run } => Some((None, run)),
            Self::Empty | Self::Routed { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opening {
    Open(OutputKind, Run),
    Restart(OutputKind, Run),
}

impl Opening {
    fn next(self) -> (OutputKind, Run) {
        match self {
            Self::Open(kind, run) | Self::Restart(kind, run) => (kind, run),
        }
    }
}

/// What an output sends to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum ProjectOutput {
    /// A device the engine opened for this kind: where it sends is in the kind.
    Kind { kind: OutputKind, universes: Run },
    /// A device a test brought itself. Its kind is not known.
    Attached { universes: Run },
}

/// What happens to the project: a client's command, or what came of an effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a client's command has no heap: `Patch` carries its four names in place, `SetGroup` its members. Moved once per request, off the tick thread"
)]
pub enum ProjectEvent {
    Client(Command),
    /// The device of an `Open` effect is open and on its way to the tick thread.
    Opened {
        output: OutputNumber,
        kind: OutputKind,
    },
    /// The device of an `Open` effect could not be opened or sent.
    OpenFailed {
        output: OutputNumber,
    },
    /// A device with its own route. Only its bounds are visible on the wire.
    Routed {
        output: OutputNumber,
        bounds: Run,
    },
    /// A device a test brought takes the place of `output`.
    Attached {
        output: OutputNumber,
        universes: Run,
    },
    /// Back to the settled state the output's last switch replaced, for a view of the
    /// project before that switch is visible. Refused (`Stale`) when there is none.
    Restore {
        output: OutputNumber,
    },
}

/// What the front must do after an update, as data. `rate: None` is the kind's default,
/// which only `outputs` knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a client's command has no heap: `Patch` carries its four names in place, `SetGroup` its members. Moved once per request, off the tick thread"
)]
pub enum ProjectEffect {
    /// Send this to the tick thread.
    Send(EngineCommand),
    /// The front counts who keeps Blackout on and tells the tick (`BlackoutHold`).
    Blackout(BlackoutAsk),
    /// The patch checks this and says what follows (`crates/rig`).
    Patch(PatchCommand),
    /// The saved looks and the plays check this and say what follows (`crates/looks`).
    Look(LookCommand),
    /// The output keeps its lane and run. Its rate may change.
    Keep {
        output: OutputNumber,
        kind: OutputKind,
        universes: Run,
        rate: Option<Rate>,
    },
    /// Take the device back, start its new lane, then report `Opened` or `OpenFailed`.
    Restart {
        output: OutputNumber,
        kind: OutputKind,
        universes: Run,
        rate: Option<Rate>,
    },
    /// Open a device of `kind`, off the tick thread and off the API, then attach it and
    /// tell the project `Opened`, or `OpenFailed`.
    Open {
        output: OutputNumber,
        kind: OutputKind,
        universes: Run,
        rate: Option<Rate>,
    },
}

impl Project {
    /// An empty project for an engine of `universes`.
    pub const fn new(universes: Universes) -> Self {
        Self {
            outputs: [None; MAX_OUTPUTS],
            universes,
            slots: [Slot {
                setup: Setup::Settled(Settled::Empty),
                before: None,
            }; MAX_OUTPUTS],
        }
    }

    /// The only place the project changes: event in, new project and effect out, or why
    /// not. On a refusal nothing changed.
    pub fn update(self, event: ProjectEvent) -> Result<(Self, Option<ProjectEffect>), Refusal> {
        let send = |command| Ok((self, Some(ProjectEffect::Send(command))));
        let patch = |command| Ok((self, Some(ProjectEffect::Patch(command))));
        let look_effect = |command| Ok((self, Some(ProjectEffect::Look(command))));
        let universes = self.universes;
        match event {
            ProjectEvent::Client(command) => match command {
                Command::SetChannel { channel, value } => send(EngineCommand::SetChannel {
                    channel: channel.check(universes)?,
                    value,
                }),
                Command::ReleaseChannel { channel } => send(EngineCommand::ReleaseChannel {
                    channel: channel.check(universes)?,
                }),
                Command::ReleaseChannels => send(EngineCommand::ReleaseChannels),
                Command::Blackout { on } => Ok((
                    self,
                    Some(ProjectEffect::Blackout(BlackoutAsk::Hold { on })),
                )),
                Command::SetBlackoutLatch { on } => Ok((
                    self,
                    Some(ProjectEffect::Blackout(BlackoutAsk::Latch { on })),
                )),
                Command::SetMaster { level } => send(EngineCommand::Master { level }),
                Command::SetOutput {
                    output,
                    kind,
                    universes: run,
                    rate,
                } => {
                    let universes = run.check(universes)?;
                    self.idle(output)?;
                    let was = self.settled(output);
                    let (to, effect) = match was {
                        Settled::Kind { kind: same, run } if same == kind => {
                            if run == universes {
                                return Ok((
                                    self,
                                    Some(ProjectEffect::Keep {
                                        output,
                                        kind,
                                        universes,
                                        rate,
                                    }),
                                ));
                            }
                            (
                                Opening::Restart(kind, universes),
                                ProjectEffect::Restart {
                                    output,
                                    kind,
                                    universes,
                                    rate,
                                },
                            )
                        }
                        Settled::Empty
                        | Settled::Kind { .. }
                        | Settled::Attached { .. }
                        | Settled::Routed { .. } => (
                            Opening::Open(kind, universes),
                            ProjectEffect::Open {
                                output,
                                kind,
                                universes,
                                rate,
                            },
                        ),
                    };
                    let project = self.with(output, |_| Slot {
                        setup: Setup::Switching { was, to },
                        before: Some(was),
                    });
                    Ok((project, Some(effect)))
                }
                Command::Patch {
                    name,
                    maker,
                    model,
                    mode,
                    at,
                } => patch(PatchCommand::Patch {
                    name,
                    maker,
                    model,
                    mode,
                    at,
                }),
                Command::PatchStrip {
                    name,
                    pixels,
                    color,
                    at,
                } => patch(PatchCommand::PatchStrip {
                    name,
                    pixels,
                    color,
                    at,
                }),
                Command::Move { fixture, at } => patch(PatchCommand::Move { fixture, at }),
                Command::Unpatch { fixture } => patch(PatchCommand::Unpatch { fixture }),
                Command::Rename { fixture, name } => patch(PatchCommand::Rename { fixture, name }),
                Command::SetIntensity { target, intensity } => patch(PatchCommand::Grab {
                    target,
                    value: Value::Intensity(intensity),
                }),
                Command::SetColour {
                    target,
                    red,
                    green,
                    blue,
                } => patch(PatchCommand::Grab {
                    target,
                    value: Value::Colour(Rgb { red, green, blue }),
                }),
                Command::SetProperty { target, value } => {
                    patch(PatchCommand::Grab { target, value })
                }
                Command::ReleaseIntensity { target } => patch(PatchCommand::Release {
                    target,
                    property: Property::Intensity,
                }),
                Command::ReleaseColour { target } => patch(PatchCommand::Release {
                    target,
                    property: Property::Colour,
                }),
                Command::ReleaseProperty { target, property } => {
                    patch(PatchCommand::Release { target, property })
                }
                Command::UpdateGrab { grab, value } => {
                    look_effect(LookCommand::UpdateGrab { grab, value })
                }
                Command::EndGrab { grab } => look_effect(LookCommand::EndGrab { grab }),
                Command::SetGroup { name, members } => match members {
                    MemberList::Fits(members) => patch(PatchCommand::SetGroup { name, members }),
                    MemberList::TooMany => Err(Refusal::TooManyMembers {
                        most: u16::try_from(Members::MOST).unwrap_or(u16::MAX),
                    }),
                },
                Command::RemoveGroup { name } => patch(PatchCommand::RemoveGroup { name }),
                Command::PutStep { look, name, step } => {
                    look_effect(LookCommand::Put(crate::SavedLook {
                        look,
                        name,
                        definition: crate::Definition::Step(step),
                    }))
                }
                Command::PutSequence {
                    look,
                    name,
                    sequence,
                } => look_effect(LookCommand::Put(crate::SavedLook {
                    look,
                    name,
                    definition: crate::Definition::Sequence(sequence),
                })),
                Command::PutPattern {
                    look,
                    name,
                    pattern,
                } => look_effect(LookCommand::Put(crate::SavedLook {
                    look,
                    name,
                    definition: crate::Definition::Pattern(pattern),
                })),
                Command::RemoveLook { look } => Ok((
                    self,
                    Some(ProjectEffect::Look(LookCommand::RemoveLook { look })),
                )),
                Command::StartLook {
                    look,
                    rank,
                    when,
                    usage,
                } => look_effect(LookCommand::StartLook {
                    look,
                    rank,
                    when,
                    usage,
                }),
                Command::SetInternalTempo { bpm } => send(EngineCommand::Feed { bpm }),
                Command::TapTempo => send(EngineCommand::Tap),
                Command::ReleaseTempo => send(EngineCommand::ReleaseTempo),
                Command::StopLook { play } => look_effect(LookCommand::StopLook { play }),
                Command::EndFlash { play } => look_effect(LookCommand::EndFlash { play }),
                Command::ReleaseHold { play } => look_effect(LookCommand::ReleaseHold { play }),
                Command::ReleaseAll => look_effect(LookCommand::ReleaseAll),
                Command::SetMixRules { rules } => look_effect(LookCommand::SetMixRules { rules }),
                Command::RemoveOutput { output } => {
                    self.idle(output)?;
                    let project = self.with(output, |_| Slot::default());
                    Ok((
                        project,
                        Some(ProjectEffect::Send(EngineCommand::RemoveOutput { output })),
                    ))
                }
            },
            ProjectEvent::Opened { output, kind } => {
                let project = self.with(output, |slot| match slot.setup {
                    Setup::Switching { to, .. } => Slot {
                        setup: Setup::Settled(Settled::Kind {
                            kind,
                            run: to.next().1,
                        }),
                        ..slot
                    },
                    Setup::Settled(_) => slot,
                });
                Ok((project, None))
            }
            ProjectEvent::OpenFailed { output } => {
                let project = self.with(output, |slot| match slot.setup {
                    Setup::Switching { was, .. } => Slot {
                        setup: Setup::Settled(was),
                        before: None,
                    },
                    Setup::Settled(_) => slot,
                });
                Ok((project, None))
            }
            ProjectEvent::Attached {
                output,
                universes: run,
            } => {
                self.idle(output)?;
                Ok((self.attach(output, Settled::Attached { run }), None))
            }
            ProjectEvent::Routed { output, bounds } => {
                self.idle(output)?;
                Ok((self.attach(output, Settled::Routed { bounds }), None))
            }
            ProjectEvent::Restore { output } => {
                let before = self.slot(output).and_then(|slot| slot.before);
                let before = before.ok_or(Refusal::Stale)?;
                let project = self.with(output, |_| Slot {
                    setup: Setup::Settled(before),
                    before: None,
                });
                Ok((project, None))
            }
        }
    }

    /// What `output` sends to, if it is set.
    pub fn output(&self, output: OutputNumber) -> Option<ProjectOutput> {
        self.outputs.get(output.slot()).copied().flatten()
    }

    /// The outputs, by number, as the wire shows them.
    pub const fn outputs(&self) -> &[Option<ProjectOutput>; MAX_OUTPUTS] {
        &self.outputs
    }

    /// Whether one output streams every universe from `first` to `last` as one run. An
    /// output changing kind must cover it before and after opening. A restart retains the old
    /// run on failure, so a patch must fit either outcome. A routed attachment never counts.
    pub fn streams(&self, first: u16, last: u16) -> bool {
        self.slots.iter().any(|slot| {
            slot.setup
                .settled()
                .stream()
                .is_some_and(|(_, run)| run.covers(first, last))
                && match slot.setup {
                    Setup::Settled(_) => true,
                    Setup::Switching { to, .. } => to.next().1.covers(first, last),
                }
        })
    }

    /// What each output sends: its number as a client counts it, its kind and its run. The
    /// kind is `None` for a device a test brought, which sends every channel. An output
    /// changing kind appears with its current and requested runs. The same holds for a
    /// restart until the new lane is ready.
    pub fn sends(&self) -> impl Iterator<Item = (u8, Option<OutputKind>, Run)> + '_ {
        (1_u8..).zip(&self.slots).flat_map(|(number, slot)| {
            let next = match slot.setup {
                Setup::Switching { to, .. } => {
                    let (kind, run) = to.next();
                    Some((Some(kind), run))
                }
                Setup::Settled(_) => None,
            };
            slot.setup
                .settled()
                .stream()
                .into_iter()
                .chain(next)
                .map(move |(kind, run)| (number, kind, run))
        })
    }

    /// The lane of this output is returning its device for a restart.
    pub fn restarting(&self, output: OutputNumber) -> bool {
        self.slot(output).is_some_and(|slot| {
            matches!(
                slot.setup,
                Setup::Switching {
                    to: Opening::Restart(_, _),
                    ..
                }
            )
        })
    }

    fn slot(&self, output: OutputNumber) -> Option<Slot> {
        self.slots.get(output.slot()).copied()
    }

    /// What `output` has settled on: while it switches, what it switches from.
    fn settled(&self, output: OutputNumber) -> Settled {
        self.slot(output)
            .map_or(Settled::Empty, |slot| slot.setup.settled())
    }

    /// `Opening` while the device of `output` is being opened.
    fn idle(&self, output: OutputNumber) -> Result<(), Refusal> {
        match self.slot(output).map(|slot| slot.setup) {
            Some(Setup::Switching { .. }) => Err(Refusal::Opening),
            Some(Setup::Settled(_)) | None => Ok(()),
        }
    }

    /// A device a test brought: it replaces what was settled there.
    fn attach(self, output: OutputNumber, to: Settled) -> Self {
        self.with(output, |slot| Slot {
            setup: Setup::Settled(to),
            before: Some(slot.setup.settled()),
        })
    }

    /// This project with `change` made to the slot of `output`: the one writer of an
    /// output, so its wire value always follows its state.
    fn with(mut self, output: OutputNumber, change: impl FnOnce(Slot) -> Slot) -> Self {
        let index = output.slot();
        if let (Some(wire), Some(slot)) = (self.outputs.get_mut(index), self.slots.get_mut(index)) {
            *slot = change(*slot);
            *wire = slot.setup.settled().wire();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Channel, ChannelNumbers, Member, MemberList, Members, RunNumbers, Target, Text};

    fn output(number: u8) -> OutputNumber {
        OutputNumber::try_from(number).unwrap()
    }

    fn checked(universe: u16, address: u16) -> Channel {
        ChannelNumbers { universe, address }
            .check(Universes::ONE)
            .unwrap()
    }

    #[test]
    fn update_checks_a_command_against_the_project_and_says_what_to_do() {
        use ProjectEvent::{Attached, Client, OpenFailed, Opened};
        let empty = Project::new(Universes::ONE);
        let kind = crate::EXAMPLE;
        let other = crate::OTHER;
        let one = RunNumbers { first: 1, count: 1 };
        let run = one.check(Universes::ONE).unwrap();
        let numbers = |universe, address| ChannelNumbers { universe, address };
        let set_output = |kind| {
            Client(Command::SetOutput {
                output: output(1),
                kind,
                universes: one,
                rate: None,
            })
        };
        let open = |kind| ProjectEffect::Open {
            output: output(1),
            kind,
            universes: run,
            rate: None,
        };
        let opened = |kind| Opened {
            output: output(1),
            kind,
        };
        let remove = Client(Command::RemoveOutput { output: output(1) });
        // (events before, event, its effect or why not, output 1 after)
        type Done = Result<Option<ProjectEffect>, Refusal>;
        let target = Target::Fixture { fixture: 3 };
        let name = Text::new("front").unwrap();
        let members =
            Members::new(&[Member::Fixture { fixture: 3 }, Member::Light { light: 1 }]).unwrap();
        let cases: [(Vec<ProjectEvent>, ProjectEvent, Done, Option<ProjectOutput>); 27] = [
            // the patch checks what names a fixture or a light
            (
                vec![],
                Client(Command::Unpatch { fixture: 3 }),
                Ok(Some(ProjectEffect::Patch(PatchCommand::Unpatch {
                    fixture: 3,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::SetIntensity {
                    target,
                    intensity: 9,
                }),
                Ok(Some(ProjectEffect::Patch(PatchCommand::Grab {
                    target,
                    value: Value::Intensity(9),
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::ReleaseIntensity { target }),
                Ok(Some(ProjectEffect::Patch(PatchCommand::Release {
                    target,
                    property: Property::Intensity,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::ReleaseColour { target }),
                Ok(Some(ProjectEffect::Patch(PatchCommand::Release {
                    target,
                    property: Property::Colour,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::SetChannel {
                    channel: numbers(1, 512),
                    value: 7,
                }),
                Ok(Some(ProjectEffect::Send(EngineCommand::SetChannel {
                    channel: checked(1, 512),
                    value: 7,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::SetGroup {
                    name,
                    members: MemberList::Fits(members),
                }),
                Ok(Some(ProjectEffect::Patch(PatchCommand::SetGroup {
                    name,
                    members,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::SetGroup {
                    name,
                    members: MemberList::TooMany,
                }),
                Err(Refusal::TooManyMembers { most: 512 }),
                None,
            ),
            (
                vec![],
                Client(Command::RemoveGroup { name }),
                Ok(Some(ProjectEffect::Patch(PatchCommand::RemoveGroup {
                    name,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::SetChannel {
                    channel: numbers(2, 1),
                    value: 7,
                }),
                Err(Refusal::NoSuchUniverse { universes: 1 }),
                None,
            ),
            (
                vec![],
                Client(Command::ReleaseChannel {
                    channel: numbers(1, 0),
                }),
                Err(Refusal::NoSuchAddress),
                None,
            ),
            (
                vec![],
                Client(Command::ReleaseChannels),
                Ok(Some(ProjectEffect::Send(EngineCommand::ReleaseChannels))),
                None,
            ),
            (
                vec![],
                Client(Command::ReleaseAll),
                Ok(Some(ProjectEffect::Look(LookCommand::ReleaseAll))),
                None,
            ),
            (
                vec![],
                Client(Command::Blackout { on: true }),
                Ok(Some(ProjectEffect::Blackout(BlackoutAsk::Hold {
                    on: true,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::Blackout { on: false }),
                Ok(Some(ProjectEffect::Blackout(BlackoutAsk::Hold {
                    on: false,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::SetBlackoutLatch { on: true }),
                Ok(Some(ProjectEffect::Blackout(BlackoutAsk::Latch {
                    on: true,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::SetBlackoutLatch { on: false }),
                Ok(Some(ProjectEffect::Blackout(BlackoutAsk::Latch {
                    on: false,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::SetMaster { level: 128 }),
                Ok(Some(ProjectEffect::Send(EngineCommand::Master {
                    level: 128,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::SetOutput {
                    output: output(1),
                    kind,
                    universes: RunNumbers { first: 1, count: 2 },
                    rate: None,
                }),
                Err(Refusal::NoSuchUniverse { universes: 1 }),
                None,
            ),
            // a new output: open its device; the project changes once it is open
            (vec![], set_output(kind), Ok(Some(open(kind))), None),
            (
                vec![set_output(kind)],
                opened(kind),
                Ok(None),
                Some(ProjectOutput::Kind {
                    kind,
                    universes: run,
                }),
            ),
            (
                vec![set_output(kind)],
                OpenFailed { output: output(1) },
                Ok(None),
                None,
            ),
            // while it opens, the output takes no other device
            (
                vec![set_output(kind)],
                set_output(other),
                Err(Refusal::Opening),
                None,
            ),
            (vec![set_output(kind)], remove, Err(Refusal::Opening), None),
            // the same kind keeps its device, another kind opens one
            (
                vec![set_output(kind), opened(kind)],
                Client(Command::SetOutput {
                    output: output(1),
                    kind,
                    universes: one,
                    rate: Some(Rate::Thirty),
                }),
                Ok(Some(ProjectEffect::Keep {
                    output: output(1),
                    kind,
                    universes: run,
                    rate: Some(Rate::Thirty),
                })),
                Some(ProjectOutput::Kind {
                    kind,
                    universes: run,
                }),
            ),
            (
                vec![set_output(kind), opened(kind)],
                set_output(other),
                Ok(Some(open(other))),
                Some(ProjectOutput::Kind {
                    kind,
                    universes: run,
                }),
            ),
            (
                vec![set_output(kind), opened(kind)],
                remove,
                Ok(Some(ProjectEffect::Send(EngineCommand::RemoveOutput {
                    output: output(1),
                }))),
                None,
            ),
            (
                vec![set_output(kind), opened(kind)],
                Attached {
                    output: output(1),
                    universes: run,
                },
                Ok(None),
                Some(ProjectOutput::Attached { universes: run }),
            ),
        ];
        for (before, event, expected, after) in cases {
            let project = before
                .iter()
                .fold(empty, |project, event| project.update(*event).unwrap().0);
            let updated = project.update(event);
            assert_eq!(
                updated.map(|(_, effect)| effect),
                expected,
                "{event:?} after {before:?}"
            );
            let project = updated.map_or(project, |(project, _)| project);
            assert_eq!(
                project.output(output(1)),
                after,
                "{event:?} after {before:?}"
            );
        }
    }

    #[test]
    fn changing_the_run_restarts_with_the_same_device() {
        let output = output(4);
        let kind = crate::EXAMPLE;
        let four = Universes::new(4).unwrap();
        let before = RunNumbers { first: 1, count: 2 };
        let set = |universes| {
            ProjectEvent::Client(Command::SetOutput {
                output,
                kind,
                universes,
                rate: None,
            })
        };
        let opened = ProjectEvent::Opened { output, kind };
        let ready = Project::new(four)
            .update(set(before))
            .unwrap()
            .0
            .update(opened)
            .unwrap()
            .0;
        let (kept, effect) = ready.update(set(before)).unwrap();
        assert_eq!(
            effect,
            Some(ProjectEffect::Keep {
                output,
                kind,
                universes: before.check(four).unwrap(),
                rate: None
            })
        );
        assert_eq!(kept, ready);

        let shifted = RunNumbers { first: 2, count: 2 };
        assert_eq!(
            ready.update(set(shifted)).unwrap().1,
            Some(ProjectEffect::Restart {
                output,
                kind,
                universes: shifted.check(four).unwrap(),
                rate: None
            })
        );

        let longer = RunNumbers { first: 1, count: 3 };
        let next = longer.check(four).unwrap();
        let (waiting, effect) = ready.update(set(longer)).unwrap();
        assert_eq!(
            effect,
            Some(ProjectEffect::Restart {
                output,
                kind,
                universes: next,
                rate: None
            })
        );
        assert!(waiting.restarting(output));
        assert!(!waiting.streams(1, 3));
        assert_eq!(
            waiting.sends().collect::<Vec<_>>(),
            vec![
                (4, Some(kind), before.check(four).unwrap()),
                (4, Some(kind), next)
            ]
        );
        let remove = ProjectEvent::Client(Command::RemoveOutput { output });
        for event in [set(before), remove] {
            assert_eq!(waiting.update(event), Err(Refusal::Opening));
        }
        let cases = [
            (
                opened,
                Some(ProjectOutput::Kind {
                    kind,
                    universes: next,
                }),
                None,
            ),
            (
                ProjectEvent::OpenFailed { output },
                ready.output(output),
                None,
            ),
        ];
        for (event, slot, expected) in cases {
            let (done, effect) = waiting.update(event).unwrap();
            assert_eq!(done.output(output), slot);
            assert_eq!(done.streams(1, 3), event == opened);
            assert_eq!(effect, expected);
            assert!(!done.restarting(output));
            assert!(done.update(set(before)).is_ok());
        }
    }

    /// Every cell of "An output in the project" (`docs/engine.md`), for output 1.
    #[test]
    fn an_output_in_the_project_follows_its_table() {
        use ProjectEvent::{Attached, Client, OpenFailed, Opened, Restore, Routed};
        let two = Universes::new(2).unwrap();
        let numbers = |count| RunNumbers { first: 1, count };
        let (one_run, two_run) = (
            numbers(1).check(two).unwrap(),
            numbers(2).check(two).unwrap(),
        );
        let (kind, other) = (crate::EXAMPLE, crate::OTHER);
        let set_output = |kind, count| {
            Client(Command::SetOutput {
                output: output(1),
                kind,
                universes: numbers(count),
                rate: None,
            })
        };
        let opened = Opened {
            output: output(1),
            kind,
        };
        let events = [
            ("SetOutput, same kind and run", set_output(kind, 1)),
            ("SetOutput, same kind, new run", set_output(kind, 2)),
            ("SetOutput, other kind", set_output(other, 1)),
            (
                "RemoveOutput",
                Client(Command::RemoveOutput { output: output(1) }),
            ),
            ("Opened", opened),
            ("OpenFailed", OpenFailed { output: output(1) }),
            (
                "Attached",
                Attached {
                    output: output(1),
                    universes: two_run,
                },
            ),
            (
                "Routed",
                Routed {
                    output: output(1),
                    bounds: two_run,
                },
            ),
            ("Restore", Restore { output: output(1) }),
        ];
        // what an event did: its effect by name, or why it was refused
        let did = |done: &Result<(Project, Option<ProjectEffect>), Refusal>| match done {
            Ok((_, None)) => "-",
            Ok((_, Some(ProjectEffect::Keep { .. }))) => "Keep",
            Ok((_, Some(ProjectEffect::Restart { .. }))) => "Restart",
            Ok((_, Some(ProjectEffect::Open { .. }))) => "Open",
            Ok((_, Some(ProjectEffect::Send(EngineCommand::RemoveOutput { .. })))) => "Remove",
            Ok((_, Some(_))) => "other effect",
            Err(Refusal::Opening) => "refused Opening",
            Err(Refusal::Stale) => "refused Stale",
            Err(_) => "refused",
        };
        let k = |run| {
            Some(ProjectOutput::Kind {
                kind,
                universes: run,
            })
        };
        let a = |run| Some(ProjectOutput::Attached { universes: run });
        let (k1, k2, a1, a2) = (k(one_run), k(two_run), a(one_run), a(two_run));
        let refused = "refused Opening";
        // (state, events that make it, then per event: what it did, output 1 on the wire)
        type Cell = (&'static str, Option<ProjectOutput>);
        let rows: [(&str, Vec<ProjectEvent>, [Cell; 9]); 6] = [
            (
                "Empty",
                vec![],
                [
                    ("Open", None),
                    ("Open", None),
                    ("Open", None),
                    ("Remove", None),
                    ("-", None),
                    ("-", None),
                    ("-", a2),
                    ("-", a2),
                    ("refused Stale", None),
                ],
            ),
            (
                "Kind",
                vec![set_output(kind, 1), opened],
                [
                    ("Keep", k1),
                    ("Restart", k1),
                    ("Open", k1),
                    ("Remove", None),
                    ("-", k1),
                    ("-", k1),
                    ("-", a2),
                    ("-", a2),
                    ("-", None),
                ],
            ),
            (
                "Attached",
                vec![Attached {
                    output: output(1),
                    universes: one_run,
                }],
                [
                    ("Open", a1),
                    ("Open", a1),
                    ("Open", a1),
                    ("Remove", None),
                    ("-", a1),
                    ("-", a1),
                    ("-", a2),
                    ("-", a2),
                    ("-", None),
                ],
            ),
            (
                "Routed",
                vec![Routed {
                    output: output(1),
                    bounds: one_run,
                }],
                [
                    ("Open", a1),
                    ("Open", a1),
                    ("Open", a1),
                    ("Remove", None),
                    ("-", a1),
                    ("-", a1),
                    ("-", a2),
                    ("-", a2),
                    ("-", None),
                ],
            ),
            (
                "Opening",
                vec![set_output(kind, 1)],
                [
                    (refused, None),
                    (refused, None),
                    (refused, None),
                    (refused, None),
                    ("-", k1),
                    ("-", None),
                    (refused, None),
                    (refused, None),
                    ("-", None),
                ],
            ),
            (
                "Restarting",
                vec![set_output(kind, 1), opened, set_output(kind, 2)],
                [
                    (refused, k1),
                    (refused, k1),
                    (refused, k1),
                    (refused, k1),
                    ("-", k2),
                    ("-", k1),
                    (refused, k1),
                    (refused, k1),
                    ("-", k1),
                ],
            ),
        ];
        for (state, making, cells) in rows {
            let project = making
                .into_iter()
                .fold(Project::new(two), |p, e| p.update(e).unwrap().0);
            for ((event, happens), (expected, wire)) in events.iter().zip(cells) {
                let done = project.update(*happens);
                let after = done.map_or(project, |(after, _)| after);
                assert_eq!(
                    (did(&done), after.output(output(1))),
                    (expected, wire),
                    "{state} + {event}"
                );
            }
        }
        // a restore goes back once: what the switch replaced is used up
        let restored = [set_output(kind, 1), opened, Restore { output: output(1) }]
            .into_iter()
            .fold(Project::new(two), |p, e| p.update(e).unwrap().0);
        assert_eq!(
            restored.update(Restore { output: output(1) }),
            Err(Refusal::Stale)
        );
    }

    #[test]
    fn routed_bounds_show_on_the_wire_but_only_runs_count_for_patch_checks() {
        let four = Universes::new(4).unwrap();
        let bounds = RunNumbers { first: 1, count: 4 }.check(four).unwrap();
        let attached = |n| ProjectEvent::Attached {
            output: output(n),
            universes: bounds,
        };
        let routed = |n| ProjectEvent::Routed {
            output: output(n),
            bounds,
        };
        let removed = |n| ProjectEvent::Client(Command::RemoveOutput { output: output(n) });
        let slot = Some(ProjectOutput::Attached { universes: bounds });
        let cases = [
            (vec![routed(1)], slot, vec![]),
            (vec![attached(1), routed(1)], slot, vec![]),
            (vec![routed(1), attached(2)], slot, vec![(2, None, bounds)]),
            (vec![routed(1), attached(1)], slot, vec![(1, None, bounds)]),
            (vec![routed(1), removed(1)], None, vec![]),
        ];
        for (events, shown, sends) in cases {
            let project = events
                .into_iter()
                .fold(Project::new(four), |p, e| p.update(e).unwrap().0);
            assert_eq!(project.output(output(1)), shown);
            assert_eq!(project.streams(1, 4), !sends.is_empty());
            assert_eq!(project.sends().collect::<Vec<_>>(), sends);
        }
    }

    #[test]
    fn each_output_sends_its_kind_and_run_and_while_it_opens_the_next_too() {
        use ProjectEvent::{Attached, Client, OpenFailed, Opened};
        let four = Universes::new(4).unwrap();
        let (kind, other) = (crate::EXAMPLE, crate::OTHER);
        let run = |first, count| RunNumbers { first, count }.check(four).unwrap();
        let set = |number, kind, first, count| {
            Client(Command::SetOutput {
                output: output(number),
                kind,
                universes: RunNumbers { first, count },
                rate: None,
            })
        };
        let opened = |number, kind| Opened {
            output: output(number),
            kind,
        };
        // (events, sends: output, kind, run)
        let cases = [
            (vec![], vec![]),
            (
                vec![set(1, kind, 1, 2), opened(1, kind)],
                vec![(1, Some(kind), run(1, 2))],
            ),
            (vec![set(1, kind, 1, 2)], vec![(1, Some(kind), run(1, 2))]),
            (
                vec![set(1, kind, 1, 2), OpenFailed { output: output(1) }],
                vec![],
            ),
            (
                vec![set(1, kind, 1, 1), opened(1, kind), set(1, other, 2, 1)],
                vec![(1, Some(kind), run(1, 1)), (1, Some(other), run(2, 1))],
            ),
            (
                vec![
                    set(2, other, 3, 1),
                    opened(2, other),
                    Attached {
                        output: output(1),
                        universes: run(1, 4),
                    },
                ],
                vec![(1, None, run(1, 4)), (2, Some(other), run(3, 1))],
            ),
        ];
        for (events, expected) in cases {
            let project = events.iter().fold(Project::new(four), |project, event| {
                project.update(*event).unwrap().0
            });
            let sends: Vec<_> = project.sends().collect();
            assert_eq!(sends, expected, "after {events:?}");
        }
    }

    #[test]
    fn an_output_streams_universes_when_its_run_has_them_all() {
        use ProjectEvent::{Attached, Client, OpenFailed, Opened};
        let four = Universes::new(4).unwrap();
        let (kind, other) = (crate::EXAMPLE, crate::OTHER);
        let run = |first, count| RunNumbers { first, count }.check(four).unwrap();
        let set = |number, kind, first, count| {
            Client(Command::SetOutput {
                output: output(number),
                kind,
                universes: RunNumbers { first, count },
                rate: None,
            })
        };
        let opened = |number, kind| Opened {
            output: output(number),
            kind,
        };
        // (events, first, last, streamed)
        let cases = [
            (vec![], 1, 2, false),
            (vec![set(1, kind, 1, 3), opened(1, kind)], 1, 2, true),
            (vec![set(1, kind, 1, 3), opened(1, kind)], 2, 3, true),
            (vec![set(1, kind, 1, 3), opened(1, kind)], 3, 4, false),
            (vec![set(1, kind, 2, 2), opened(1, kind)], 1, 2, false),
            // two outputs do not make one run
            (
                vec![
                    set(1, kind, 1, 1),
                    opened(1, kind),
                    set(2, kind, 2, 1),
                    opened(2, kind),
                ],
                1,
                2,
                false,
            ),
            // A shifted run counts only after its replacement lane is ready.
            (
                vec![set(1, kind, 2, 2), opened(1, kind), set(1, kind, 1, 2)],
                1,
                2,
                false,
            ),
            (
                vec![
                    set(1, kind, 2, 2),
                    opened(1, kind),
                    set(1, kind, 1, 2),
                    opened(1, kind),
                ],
                1,
                2,
                true,
            ),
            // a new output counts once open, not while it opens or when it failed
            (vec![set(1, kind, 1, 2)], 1, 2, false),
            (
                vec![set(1, kind, 1, 2), OpenFailed { output: output(1) }],
                1,
                2,
                false,
            ),
            // while another kind opens, the old run or the new one may stay: both must
            (
                vec![set(1, kind, 1, 2), opened(1, kind), set(1, other, 1, 1)],
                1,
                2,
                false,
            ),
            (
                vec![set(1, kind, 1, 3), opened(1, kind), set(1, other, 1, 2)],
                1,
                2,
                true,
            ),
            (
                vec![
                    set(1, kind, 1, 1),
                    opened(1, kind),
                    set(1, other, 1, 2),
                    opened(1, other),
                ],
                1,
                2,
                true,
            ),
            (
                vec![Attached {
                    output: output(1),
                    universes: run(1, 4),
                }],
                3,
                4,
                true,
            ),
            (
                vec![
                    set(1, kind, 1, 2),
                    opened(1, kind),
                    Client(Command::RemoveOutput { output: output(1) }),
                ],
                1,
                2,
                false,
            ),
        ];
        for (events, first, last, expected) in cases {
            let project = events.iter().fold(Project::new(four), |project, event| {
                project.update(*event).unwrap().0
            });
            assert_eq!(
                project.streams(first, last),
                expected,
                "{first} to {last} after {events:?}"
            );
        }
    }
}
