//! The project: what the engine was asked to be, owned by the engine front, off the tick
//! thread. Today its outputs; patch and groups come with their tickets.
//!
//! ```text
//! client Command --> Project::update --> new project + effect --> the front performs it:
//!                                                                 send an EngineCommand,
//!                                                                 or open a device, then Opened / OpenFailed
//! ```
//!
//! One output, as the project sees it:
//!
//! ```text
//! None --SetOutput--> opening --Opened--> Kind --SetOutput, same kind--> Kind (device kept)
//!                            --OpenFailed--> as before
//! any --RemoveOutput--> None        any --Attached--> Attached (a device brought by a test)
//! ```

use crate::{
    Command, EngineCommand, MAX_OUTPUTS, OutputKind, OutputNumber, PatchCommand, Rate, Refusal,
    Run, Universes,
};
use serde::Serialize;

/// What the front owns. Only [`Project::update`] changes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Project {
    /// The outputs, by number. `None` where none is set.
    #[cfg_attr(test, ts(type = "Array<ProjectOutput | null>"))]
    pub outputs: [Option<ProjectOutput>; MAX_OUTPUTS],
    /// The engine's universes: what a command is checked against.
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    universes: Universes,
    /// The run of each output whose device is being opened. They take no `SetOutput` or
    /// `RemoveOutput` until it is open or failed.
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    opening: [Option<Run>; MAX_OUTPUTS],
}

/// What an output sends to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    reason = "a client's command has no heap: `Patch` carries its four names in place. Moved once per request, off the tick thread"
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
    /// A device a test brought takes the place of `output`.
    Attached {
        output: OutputNumber,
        universes: Run,
    },
}

/// What the front must do after an update, as data. `rate: None` is the kind's default,
/// which only `outputs` knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a client's command has no heap: `Patch` carries its four names in place. Moved once per request, off the tick thread"
)]
pub enum ProjectEffect {
    /// Send this to the tick thread.
    Send(EngineCommand),
    /// The patch checks this and says what follows (`crates/rig`).
    Patch(PatchCommand),
    /// The output keeps its device: only its run and rate change.
    Keep {
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
            opening: [None; MAX_OUTPUTS],
        }
    }

    /// The only place the project changes: event in, new project and effect out, or why
    /// not. On a refusal nothing changed.
    pub fn update(self, event: ProjectEvent) -> Result<(Self, Option<ProjectEffect>), Refusal> {
        let send = |command| Ok((self, Some(ProjectEffect::Send(command))));
        let patch = |command| Ok((self, Some(ProjectEffect::Patch(command))));
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
                Command::ReleaseAll => send(EngineCommand::ReleaseAll),
                Command::Blackout { on } => send(EngineCommand::Blackout { on }),
                Command::SetOutput {
                    output,
                    kind,
                    universes: run,
                    rate,
                } => {
                    let universes = run.check(universes)?;
                    self.idle(output)?;
                    if matches!(self.output(output), Some(ProjectOutput::Kind { kind: was, .. }) if was == kind)
                    {
                        let keep = ProjectEffect::Keep {
                            output,
                            kind,
                            universes,
                            rate,
                        };
                        let project = self.with(output, |slot, _| {
                            *slot = Some(ProjectOutput::Kind { kind, universes });
                        });
                        return Ok((project, Some(keep)));
                    }
                    let open = ProjectEffect::Open {
                        output,
                        kind,
                        universes,
                        rate,
                    };
                    let project = self.with(output, |_, opening| *opening = Some(universes));
                    Ok((project, Some(open)))
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
                Command::SetIntensity { target, intensity } => {
                    patch(PatchCommand::SetIntensity { target, intensity })
                }
                Command::SetColour {
                    target,
                    red,
                    green,
                    blue,
                } => patch(PatchCommand::SetColour {
                    target,
                    red,
                    green,
                    blue,
                }),
                Command::ReleaseIntensity { target } => {
                    patch(PatchCommand::ReleaseIntensity { target })
                }
                Command::ReleaseColour { target } => patch(PatchCommand::ReleaseColour { target }),
                Command::RemoveOutput { output } => {
                    self.idle(output)?;
                    let project = self.with(output, |slot, _| *slot = None);
                    Ok((
                        project,
                        Some(ProjectEffect::Send(EngineCommand::RemoveOutput { output })),
                    ))
                }
            },
            ProjectEvent::Opened { output, kind } => {
                let project = self.with(output, |slot, opening| {
                    if let Some(universes) = opening.take() {
                        *slot = Some(ProjectOutput::Kind { kind, universes });
                    }
                });
                Ok((project, None))
            }
            ProjectEvent::OpenFailed { output } => {
                Ok((self.with(output, |_, opening| *opening = None), None))
            }
            ProjectEvent::Attached { output, universes } => {
                self.idle(output)?;
                Ok((
                    self.with(output, |slot, _| {
                        *slot = Some(ProjectOutput::Attached { universes });
                    }),
                    None,
                ))
            }
        }
    }

    /// What `output` sends to, if it is set.
    pub fn output(&self, output: OutputNumber) -> Option<ProjectOutput> {
        self.outputs.get(output.slot()).copied().flatten()
    }

    /// Whether one output streams every universe from `first` to `last` as one run. An
    /// output being opened counts only when both its run and the one it opens with do:
    /// either may be its run once the device is open or failed.
    pub fn streams(&self, first: u16, last: u16) -> bool {
        self.outputs
            .iter()
            .zip(&self.opening)
            .any(|(output, opening)| {
                let run = match output {
                    Some(
                        ProjectOutput::Kind { universes, .. }
                        | ProjectOutput::Attached { universes },
                    ) => *universes,
                    None => return false,
                };
                run.covers(first, last) && opening.is_none_or(|next| next.covers(first, last))
            })
    }

    /// `Opening` while the device of `output` is being opened.
    fn idle(&self, output: OutputNumber) -> Result<(), Refusal> {
        match self.opening.get(output.slot()) {
            Some(Some(_)) => Err(Refusal::Opening),
            Some(None) | None => Ok(()),
        }
    }

    /// This project with `change` made to the slot of `output`.
    fn with(
        mut self,
        output: OutputNumber,
        change: impl FnOnce(&mut Option<ProjectOutput>, &mut Option<Run>),
    ) -> Self {
        let slot = output.slot();
        if let (Some(kind), Some(opening)) =
            (self.outputs.get_mut(slot), self.opening.get_mut(slot))
        {
            change(kind, opening);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Channel, ChannelNumbers, RunNumbers, Target, kinds};

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
        let kind = kinds::EXAMPLE;
        let other = kinds::OTHER;
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
        let cases: [(Vec<ProjectEvent>, ProjectEvent, Done, Option<ProjectOutput>); 19] = [
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
                Ok(Some(ProjectEffect::Patch(PatchCommand::SetIntensity {
                    target,
                    intensity: 9,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::ReleaseIntensity { target }),
                Ok(Some(ProjectEffect::Patch(PatchCommand::ReleaseIntensity {
                    target,
                }))),
                None,
            ),
            (
                vec![],
                Client(Command::ReleaseColour { target }),
                Ok(Some(ProjectEffect::Patch(PatchCommand::ReleaseColour {
                    target,
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
                Client(Command::ReleaseAll),
                Ok(Some(ProjectEffect::Send(EngineCommand::ReleaseAll))),
                None,
            ),
            (
                vec![],
                Client(Command::Blackout { on: true }),
                Ok(Some(ProjectEffect::Send(EngineCommand::Blackout {
                    on: true,
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
    fn an_output_streams_universes_when_its_run_has_them_all() {
        use ProjectEvent::{Attached, Client, OpenFailed, Opened};
        let four = Universes::new(4).unwrap();
        let (kind, other) = (kinds::EXAMPLE, kinds::OTHER);
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
            // the same kind keeps its device: the new run counts at once
            (
                vec![set(1, kind, 1, 1), opened(1, kind), set(1, kind, 1, 2)],
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
                vec![
                    Attached {
                        output: output(1),
                        universes: run(1, 4),
                    },
                ],
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
