#[cfg(feature = "test-support")]
use super::opening::waits;
use super::opening::{OpenAction, open_action};
use super::*;
#[cfg(feature = "test-support")]
use engine_core::{Percent, Period, Width};
use std::time::Instant;

/// docs/engine.md, "What the front reserves for a play", one row per pair.
#[test]
fn every_hold_and_event_pair_follows_the_front_table() {
    use Hold::{Kept, Playing};
    use HoldEvent::{KeepSent, KeptRead, Released, Start};
    for (hold, event, expected) in [
        (None, Start, Some(Playing)),
        (None, KeepSent, None),
        (None, KeptRead, None),
        (None, Released, None),
        (Some(Playing), Start, Some(Playing)),
        (Some(Playing), KeepSent, Some(Kept)),
        (Some(Playing), KeptRead, Some(Kept)),
        (Some(Playing), Released, None),
        (Some(Kept), Start, Some(Playing)),
        (Some(Kept), KeepSent, Some(Kept)),
        (Some(Kept), KeptRead, Some(Kept)),
        (Some(Kept), Released, None),
    ] {
        assert_eq!(next_hold(hold, event), expected, "{hold:?} + {event:?}");
    }
}

/// docs/engine.md, Released: new mix rules that no longer keep a stopped play free
/// its hold.
#[cfg(feature = "test-support")]
#[test]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test whose set-up fails fails"
)]
fn new_mix_rules_release_a_kept_hold_they_no_longer_keep() {
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(Universes::ONE, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    sender
        .send(Command::PatchStrip {
            name: Text::new("strip").unwrap(),
            pixels: engine_core::Pixels::try_from(1).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    sender
        .send(Command::PutStep {
            look: 1,
            name: Text::new("s").unwrap(),
            step: engine_core::Step {
                target: Target::Light { light: 1 },
                values: Light::DARK,
            }
            .into(),
        })
        .unwrap();
    let rules = |other_end| Command::SetMixRules {
        rules: MixRules {
            other_end,
            ..MixRules::default()
        },
    };
    sender.send(rules(OtherEnd::Stay)).unwrap();
    let start = Command::StartLook {
        look: 1,
        rank: Rank::Show,
        when: When::Now,
        usage: Default::default(),
    };
    let Ok(Begun::Started { play }) = sender.begin(start, &[]) else {
        panic!("look 1 starts");
    };
    sender.send(Command::StopLook { play }).unwrap();
    let hold = |sender: &Sender| {
        sender
            .plays
            .get(play.slot())
            .and_then(|live| live.as_ref().map(|live| live.hold))
    };
    assert_eq!(
        hold(&sender),
        Some(Hold::Kept),
        "Stay keeps the stopped play"
    );
    sender.send(rules(OtherEnd::Reveal)).unwrap();
    assert_eq!(hold(&sender), None, "Reveal releases it");
}

/// output-cutover.md, Lane recovery: a lost lane whose restart is refused after the
/// recovery worker ended is Stranded (Opening::open's request cannot be sent).
#[cfg(feature = "test-support")]
#[test]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test whose set-up fails fails"
)]
fn a_refused_restart_with_the_recovery_worker_gone_strands_the_lost_lane() {
    use crate::recovery::State as RecoveryState;
    struct Lost;
    impl Device for Lost {
        fn take(&mut self, _: &[u8]) -> Outcome {
            Outcome::Failed
        }
    }

    let (engine, mut sender, wakes) = Engine::start_stepped(
        Universes::new(2).unwrap(),
        Box::new(|_| Ok(Box::new(Lost) as Box<dyn Device>)),
    )
    .unwrap();
    wakes.hold().unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let set = |count| Command::SetOutput {
        output,
        kind: engine_core::TEST_STREAM_KIND,
        universes: RunNumbers { first: 1, count },
        rate: None,
    };
    sender.send(set(1)).unwrap();
    let lane = sender.lanes.get(output.slot()).cloned().flatten().unwrap();
    let started = Instant::now();
    while lane.recovery_state() != RecoveryState::Lost {
        assert!(started.elapsed() < Duration::from_secs(10), "never lost");
        wakes.wake().unwrap();
    }
    let Ok(Begun::Opening(opening)) = sender.begin(set(2), &[]) else {
        panic!("the larger route needs a new lane");
    };
    wakes.wake().unwrap();
    drop(engine); // the recovery worker ends with the engine
    let opened = opening.open();
    assert_eq!(lane.recovery_state(), RecoveryState::Stranded);
    drop(opened);
}

#[test]
fn opening_uses_the_reserved_device_or_waits_for_reclamation() {
    use DeviceSource::{Exclusive, Fresh, Reusable};
    use OpenAction::{Open, Reuse, Wait};
    let before = FREE_WITHIN - Duration::from_nanos(1);
    for (port, reclaiming, elapsed, expected) in [
        (Exclusive, true, before, Reuse),
        (Fresh, false, before, Open),
        (Fresh, true, before, Wait),
        (Fresh, true, FREE_WITHIN, Open),
        (Reusable(true), true, before, Reuse),
        (Reusable(true), true, FREE_WITHIN, Reuse),
        (Reusable(false), false, before, Wait),
        (Reusable(false), false, FREE_WITHIN, Open),
    ] {
        assert_eq!(open_action(port, reclaiming, elapsed), expected);
    }
}

/// Sends `command` until it is not refused for a full command queue: waits for the
/// tick to drain it. What came of it once the queue had room.
#[expect(clippy::unwrap_used, reason = "a test whose tick failed fails")]
fn once_drained(engine: &mut Engine, sender: &mut Sender, command: Command) -> Result<(), Unsent> {
    loop {
        let refused = sender.send(command);
        if refused != Err(Unsent::Refused(Refusal::QueueFull)) {
            return refused;
        }
        let started = Instant::now();
        while engine.fresh().unwrap().applied() < sender.sent() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the tick drains"
            );
        }
        if sender.commands.slots() == QUEUE {
            return sender.send(command); // drained: the retirement queue decides
        }
    }
}

/// The first nine channels of what an attached device was handed last.
struct Heard {
    frames: AtomicU64,
    levels: [std::sync::atomic::AtomicU8; 9],
}

struct Hears(Arc<Heard>);

impl Device for Hears {
    fn take(&mut self, frame: &[u8]) -> Outcome {
        for (cell, level) in self.0.levels.iter().zip(frame) {
            cell.store(*level, Ordering::Relaxed);
        }
        self.0.frames.fetch_add(1, Ordering::Release);
        Outcome::Accepted
    }
}

impl Heard {
    /// The channels of a frame made after every command sent so far was applied.
    #[expect(clippy::unwrap_used, reason = "a test whose tick failed fails")]
    fn after(&self, engine: &mut Engine, sender: &Sender) -> [u8; 9] {
        let started = Instant::now();
        while engine.fresh().unwrap().applied() < sender.sent() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the tick applies"
            );
        }
        let frames = self.frames.load(Ordering::Acquire);
        // the frame being made as it was applied may be handed next: two on
        while self.frames.load(Ordering::Acquire) < frames + 2 {
            assert!(started.elapsed() < Duration::from_secs(5), "frames go out");
            thread::sleep(Duration::from_millis(1)); // poll
        }
        self.levels
            .each_ref()
            .map(|level| level.load(Ordering::Relaxed))
    }
}

/// A gesture that lets go of what is held, sent with every play slot taken and no
/// return free.
#[derive(Clone, Copy, Debug)]
enum Gesture {
    ReleaseHold,
    StopLook,
    EndFlash,
    ReleaseProperty(Target),
    EndGrab,
    ReleaseAll,
    /// The grab ends and is kept, then rules that no longer keep it.
    Unkeep,
}

#[test]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    reason = "a test that cannot set itself up fails; one table"
)]
fn with_reclamation_stalled_every_release_needs_only_a_place_in_the_queue() {
    // a strip of three pixels: every play red on the middle one, a grab green on the
    // grabbed lights
    let one = Target::Light { light: 1 };
    let ends = Target::Group {
        group: Text::new("ends").unwrap(),
    };
    let (red, green, dark) = ([255, 0, 0], [0, 255, 0], [0, 0, 0]);
    let frame = |pixels: [[u8; 3]; 3]| -> [u8; 9] { pixels.concat().try_into().unwrap() };
    // (Manual end, grabbed, gesture, DMX after, plays after, lights the book holds a
    // colour on after)
    let cases: [(_, _, _, _, _, &[u16]); 10] = [
        (
            ManualEnd::UntilRelease,
            one,
            Gesture::ReleaseHold,
            frame([green, dark, dark]),
            0,
            &[1],
        ),
        (
            ManualEnd::UntilRelease,
            one,
            Gesture::StopLook,
            frame([green, red, dark]),
            PLAYS,
            &[1],
        ),
        (
            ManualEnd::UntilStop,
            one,
            Gesture::StopLook,
            frame([green, dark, dark]),
            0,
            &[1],
        ),
        (
            ManualEnd::UntilRelease,
            one,
            Gesture::EndFlash,
            frame([green, dark, dark]),
            0,
            &[1],
        ),
        (
            ManualEnd::UntilRelease,
            one,
            Gesture::ReleaseProperty(one),
            frame([dark, red, dark]),
            PLAYS,
            &[],
        ),
        (
            ManualEnd::UntilRelease,
            ends,
            Gesture::ReleaseProperty(ends),
            frame([dark, red, dark]),
            PLAYS,
            &[],
        ),
        (
            ManualEnd::UntilStop,
            one,
            Gesture::EndGrab,
            frame([dark, red, dark]),
            PLAYS,
            &[],
        ),
        (
            ManualEnd::UntilStop,
            ends,
            Gesture::EndGrab,
            frame([dark, red, dark]),
            PLAYS,
            &[],
        ),
        (
            ManualEnd::UntilRelease,
            ends,
            Gesture::ReleaseAll,
            frame([dark, dark, dark]),
            0,
            &[],
        ),
        (
            ManualEnd::UntilRelease,
            one,
            Gesture::Unkeep,
            frame([dark, red, dark]),
            PLAYS,
            &[],
        ),
    ];
    for (manual_end, grabbed, gesture, expected, plays_after, coloured) in cases {
        let case = format!("{manual_end:?} {grabbed:?} {gesture:?}");
        let (mut engine, mut sender) = Engine::start(Universes::new(1).unwrap()).unwrap();
        let heard = Arc::new(Heard {
            frames: AtomicU64::new(0),
            levels: Default::default(),
        });
        let run = RunNumbers { first: 1, count: 1 }
            .check(sender.universes)
            .unwrap();
        let output = OutputNumber::try_from(1).unwrap();
        sender
            .attach(
                output,
                Box::new(Hears(Arc::clone(&heard))),
                run,
                Rate::Sixty,
            )
            .unwrap();
        let strip = Command::PatchStrip {
            name: Text::new("strip").unwrap(),
            pixels: engine_core::Pixels::try_from(3).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        };
        assert!(sender.begin(strip, &[]).is_ok(), "{case}");
        let group = Command::SetGroup {
            name: Text::new("ends").unwrap(),
            members: MemberList::Fits(
                Members::new(&[Member::Light { light: 1 }, Member::Light { light: 3 }]).unwrap(),
            ),
        };
        sender.send(group).unwrap();
        let rules = MixRules {
            manual_end,
            ..MixRules::default()
        };
        sender.send(Command::SetMixRules { rules }).unwrap();
        let full = Command::SetIntensity {
            target: Target::Fixture { fixture: 1 },
            intensity: u16::MAX,
        };
        sender.send(full).unwrap();
        let step = engine_core::Step {
            target: Target::Light { light: 2 },
            values: Light {
                colour: Some(Rgb {
                    red: u16::MAX,
                    ..Rgb::BLACK
                }),
                ..Light::DARK
            },
        };
        let mut plays = Vec::new();
        for look in 1..=PLAYS as u16 {
            let name = Text::new("s").unwrap();
            sender
                .send(Command::PutStep {
                    look,
                    name,
                    step: step.into(),
                })
                .unwrap();
            let start = Command::StartLook {
                look,
                rank: Rank::Manual,
                when: When::Now,
                usage: Default::default(),
            };
            match sender.begin(start, &[]) {
                Ok(Begun::Started { play }) => plays.push(play),
                _ => panic!("look {look} plays"),
            }
        }
        let grab = Command::SetColour {
            target: grabbed,
            red: 0,
            green: u16::MAX,
            blue: 0,
        };
        let Ok(Begun::Grabbed { grab }) = sender.begin(grab, &[]) else {
            panic!("{case}: grabbed");
        };
        let held = if matches!(grabbed, Target::Group { .. }) {
            frame([green, red, green])
        } else {
            frame([green, red, dark])
        };
        assert_eq!(heard.after(&mut engine, &sender), held, "{case}: before");
        engine.shared.stalled.store(true, Ordering::Release);
        // restarts send old plans back that are never freed, until no more may go
        let restart = Command::StartLook {
            look: 1,
            rank: Rank::Manual,
            when: When::Now,
            usage: Default::default(),
        };
        while once_drained(&mut engine, &mut sender, restart).is_ok() {}
        assert_eq!(
            sender.send(restart),
            Err(Unsent::Refused(Refusal::QueueFull)),
            "{case}: the returns are full"
        );
        *plays.first_mut().unwrap() = PlayId::new(0, sender.generation).unwrap();
        let commands: Vec<Command> = match gesture {
            Gesture::ReleaseHold => plays
                .iter()
                .map(|&play| Command::ReleaseHold { play })
                .collect(),
            Gesture::StopLook => plays
                .iter()
                .map(|&play| Command::StopLook { play })
                .collect(),
            Gesture::EndFlash => plays
                .iter()
                .map(|&play| Command::EndFlash { play })
                .collect(),
            Gesture::ReleaseProperty(target) => vec![Command::ReleaseProperty {
                target,
                property: Property::Colour,
            }],
            Gesture::EndGrab => vec![Command::EndGrab { grab }],
            Gesture::ReleaseAll => vec![Command::ReleaseAll],
            Gesture::Unkeep => vec![
                Command::EndGrab { grab },
                Command::SetMixRules {
                    rules: MixRules {
                        manual_end: ManualEnd::UntilStop,
                        ..rules
                    },
                },
            ],
        };
        for command in commands {
            assert_eq!(
                once_drained(&mut engine, &mut sender, command),
                Ok(()),
                "{case}: {command:?}"
            );
        }
        assert_eq!(heard.after(&mut engine, &sender), expected, "{case}: after");
        let picture = engine.fresh().unwrap();
        let left = (0..PLAYS).filter(|&slot| picture.has_play(slot)).count();
        assert_eq!(left, plays_after, "{case}: plays");
        let held: Vec<u16> = sender
            .held_lights()
            .iter()
            .filter(|held| held.colour.is_some())
            .map(|held| held.light)
            .collect();
        assert_eq!(held, coloured, "{case}: the book");
        engine.shared.stalled.store(false, Ordering::Release);
    }
}

#[test]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
fn a_play_started_as_the_generations_wrap_is_never_taken_for_a_cancelled_one() {
    let (mut engine, mut sender) = Engine::start(Universes::new(1).unwrap()).unwrap();
    let strip = Command::PatchStrip {
        name: Text::new("strip").unwrap(),
        pixels: engine_core::Pixels::try_from(1).unwrap(),
        color: Color::Rgb,
        at: ChannelNumbers {
            universe: 1,
            address: 1,
        },
    };
    assert!(sender.begin(strip, &[]).is_ok());
    let step = engine_core::Step {
        target: Target::Light { light: 1 },
        values: Light::DARK,
    };
    let name = Text::new("s").unwrap();
    sender
        .send(Command::PutStep {
            look: 1,
            name,
            step: step.into(),
        })
        .unwrap();
    // the last generation before the counter wraps
    sender.generation = u32::MAX;
    let start = Command::StartLook {
        look: 1,
        rank: Rank::Manual,
        when: When::Now,
        usage: Default::default(),
    };
    assert!(matches!(
        sender.begin(start, &[]),
        Ok(Begun::Started { .. })
    ));
    let started = Instant::now();
    while engine.fresh().unwrap().applied() < sender.sent() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the tick applies"
        );
    }
    assert_eq!(
        sender.send(Command::RemoveLook { look: 1 }),
        Err(Unsent::Refused(Refusal::LookInUse { look: 1 }))
    );
    assert!(engine.fresh().unwrap().has_play(0));
}

#[cfg(feature = "test-support")]
#[test]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "an invalid fixture, missing play or missing frame fails this stepped test"
)]
fn linked_plans_at_the_fifo_cutoff() {
    use engine_core::{Repeat, Sequence, SequenceItem, Step, Use};

    let (mut engine, mut sender, wakes) =
        Engine::start_stepped(Universes::new(1).unwrap(), Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let name = Text::new("sequence").unwrap();
    sender
        .send(Command::PatchStrip {
            name,
            pixels: engine_core::Pixels::try_from(1).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    // A red marker: the strip has no dimmer, so a brightness alone offers nothing.
    let put = |look, red: Option<u8>| Command::PutStep {
        look,
        name,
        step: Step {
            target: Target::Light { light: 1 },
            values: Light {
                colour: red.map(|red| Rgb {
                    red: u16::from(red) << 8,
                    green: 0,
                    blue: 0,
                }),
                ..Light::DARK
            },
        }
        .into(),
    };
    let sequence = |look, children: &[u16]| Command::PutSequence {
        look,
        name,
        sequence: Sequence::new(
            &children
                .iter()
                .map(|&child| SequenceItem {
                    look: child,
                    width: Width::new(16).unwrap(),
                    ..Default::default()
                })
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    };
    let start = |look, rank, repeat| Command::StartLook {
        look,
        rank,
        when: When::Now,
        usage: Use {
            repeat,
            ..Default::default()
        },
    };
    sender
        .send(Command::SetInternalTempo {
            bpm: Bpm::new(60).unwrap(),
        })
        .unwrap();
    sender.send(put(1, Some(12))).unwrap();
    sender.send(put(2, None)).unwrap();
    sender.send(put(3, Some(65))).unwrap();
    for root in 10..10 + PLAYS as u16 {
        sender.send(sequence(root, &[1, 3])).unwrap();
    }
    let begun = |sender: &mut Sender, command| match sender.begin(command, &[]) {
        Ok(Begun::Started { play }) => play,
        _ => panic!("start refused"),
    };
    let time = wakes.wake().unwrap() + Duration::from_secs(1);
    wakes.set_time(time);
    let first = begun(&mut sender, start(10, Rank::Show, Repeat::Once));
    sender.send(put(1, Some(45))).unwrap();
    let mut ids = vec![first];
    for root in 11..10 + PLAYS as u16 {
        ids.push(begun(&mut sender, start(root, Rank::Manual, Repeat::Once)));
    }
    // Ask before each wake so latest() receives that exact frame.
    let picture = |engine: &mut Engine, at| {
        engine.latest();
        wakes.wake_at(at).unwrap();
        let snap = engine.latest();
        (snap.active_plays().collect::<Vec<_>>(), snap.sent(1, 1))
    };
    let (plays, intensity) = picture(&mut engine, time);
    assert_eq!(plays.len(), PLAYS);
    assert_eq!(intensity, 45);
    assert_eq!(plays.first().map(|play| play.play), Some(first));
    // All ends preceded the edit's timestamp, with no wake at the end.
    // The queued plans must be returned even for a removed or already-kept slot.
    let end = time + Duration::from_secs(3);
    wakes.set_time(end);
    sender.send(put(3, Some(78))).unwrap();
    let (plays, intensity) = picture(&mut engine, end);
    assert_eq!(intensity, 65);
    assert_eq!(plays.len(), PLAYS - 1);
    assert!(
        plays
            .iter()
            .all(|p| p.state == PlayState::Kept && ids.contains(&p.play))
    );
    sender.send(Command::RemoveLook { look: 10 }).unwrap();
    assert_eq!(
        sender.send(Command::RemoveLook { look: 11 }),
        Err(Unsent::Refused(Refusal::LookInUse { look: 11 }))
    );
    for play in ids {
        sender.send(Command::ReleaseHold { play }).unwrap();
    }
    sender.send(sequence(11, &[1, 2])).unwrap();
    let play = begun(&mut sender, start(11, Rank::Manual, Repeat::Loop));
    assert_eq!(picture(&mut engine, end).1, 45);
    let rest = end + Duration::from_millis(1500);
    wakes.set_time(rest);
    sender.send(Command::StopLook { play }).unwrap();
    // The stop was in the rest, although the next frame would be in the first item again.
    assert_eq!(
        picture(&mut engine, end + Duration::from_secs(2)),
        (vec![], 0)
    );
    sender.send(Command::RemoveLook { look: 11 }).unwrap();

    let from = end + Duration::from_secs(2);
    sender.send(sequence(11, &[1, 3, 2])).unwrap();
    let once = begun(&mut sender, start(11, Rank::Manual, Repeat::Once));
    assert_eq!(picture(&mut engine, from).1, 45);
    let shortened = from + Duration::from_millis(1500);
    assert_eq!(picture(&mut engine, shortened).1, 78);
    sender.send(sequence(11, &[1])).unwrap();
    let (plays, sample) = picture(&mut engine, shortened);
    assert_eq!(
        plays.first().map(|play| (play.play, play.state)),
        Some((once, PlayState::Kept))
    );
    assert_eq!(sample, 45);
    wakes.run();
}

#[cfg(feature = "test-support")]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_types,
    reason = "the test client reads real packets and fails on setup or payload errors"
)]
mod output_restart {
    use super::*;
    use engine_core::Pixels;
    mod stream {
        include!("../../../outputs/tests/common/stream.rs");
    }

    #[test]
    fn refused_restart_preserves_the_only_stream_for_a_patched_strip() {
        let (receiver, kind) = stream::receiver(Color::Rgb);
        let mut initial = true;
        let open = Box::new(move |kind| {
            if std::mem::take(&mut initial) {
                outputs::open(kind)
            } else {
                Err(io::Error::other("replacement device unavailable"))
            }
        });
        let universes = Universes::new(3).unwrap();
        let (mut engine, mut sender, wakes) = Engine::start_stepped(universes, open).unwrap();
        wakes.hold().unwrap();
        let output = OutputNumber::try_from(1).unwrap();
        let set = |count| Command::SetOutput {
            output,
            kind,
            universes: RunNumbers { first: 1, count },
            rate: Some(Rate::Sixty),
        };
        let at = ChannelNumbers {
            universe: 1,
            address: 512,
        };
        let hold = |value| Command::SetChannel { channel: at, value };
        sender.send(set(2)).unwrap();
        sender
            .send(Command::PatchStrip {
                name: Text::new("Strip").unwrap(),
                pixels: Pixels::try_from(2).unwrap(),
                color: Color::Rgb,
                at,
            })
            .unwrap();
        sender.send(hold(91)).unwrap();
        wakes.wake().unwrap();

        let mut packet = [0; 2048];
        assert_eq!(receiver.recv(&mut packet).unwrap(), 10 + 1023);
        assert_eq!(packet[10 + 511], 91);
        let before = sender.project().output(output);
        let old_run = RunNumbers { first: 1, count: 2 }.check(universes).unwrap();
        assert_eq!(sender.patch().unstreamed(&sender.project()), None);

        let Begun::Opening(restart) = sender.begin(set(3), &[]).unwrap() else {
            panic!("the longer run needs a restart");
        };
        // Hold the tick until the original device's hand-back times out. The fallback
        // opener then fails without ever obtaining the original, working device.
        assert_eq!(
            sender.finish(restart.open()),
            Err(Unsent::Refused(Refusal::CannotOpen {
                why: OpenFailure::Device { with: None }
            }))
        );
        sender.send(hold(73)).unwrap();
        engine.latest();
        wakes.wake().unwrap();
        let snapshot = engine.latest();
        assert_eq!(snapshot.applied(), sender.sent());
        assert_eq!(
            (
                sender.project().output(output),
                sender.patch().unstreamed(&sender.project()),
                snapshot.output(output).map(|out| out.universes),
            ),
            (before, None, Some(old_run)),
            "the refused edit must leave the fixture on its previous output"
        );
        assert_eq!(receiver.recv(&mut packet).unwrap(), 10 + 1023);
        assert_eq!(packet[10 + 511], 73);
    }

    #[test]
    fn a_route_edit_cannot_drop_or_move_a_patched_byte() {
        let mut actual = Vec::new();
        // (fixture universe, run length, packet length, offset before the move)
        for (fixture_universe, count, len, offset) in [(1, 1, 510, 0), (2, 2, 1023, 512)] {
            let (receiver, kind) = stream::receiver(Color::Rgb);
            let universes = Universes::new(3).unwrap();
            let (_engine, mut sender, wakes) =
                Engine::start_stepped(universes, Box::new(outputs::open)).unwrap();
            wakes.hold().unwrap();
            let output = OutputNumber::try_from(1).unwrap();
            let set = |first| Command::SetOutput {
                output,
                kind,
                universes: RunNumbers { first, count },
                rate: Some(Rate::Sixty),
            };
            let at = ChannelNumbers {
                universe: fixture_universe,
                address: 1,
            };
            sender.send(set(1)).unwrap();
            sender
                .send(Command::PatchStrip {
                    name: Text::new("Strip").unwrap(),
                    pixels: Pixels::try_from(1).unwrap(),
                    color: Color::Rgb,
                    at,
                })
                .unwrap();
            sender
                .send(Command::SetChannel {
                    channel: at,
                    value: 91,
                })
                .unwrap();
            wakes.wake().unwrap();
            let mut packet = [0; 2048];
            assert_eq!(receiver.recv(&mut packet).unwrap(), 10 + len);
            assert_eq!(packet[10 + offset], 91);

            let refusal = sender.send(set(2)).is_err();
            wakes.wake().unwrap();
            assert_eq!(receiver.recv(&mut packet).unwrap(), 10 + len);
            let in_packet = packet[10..10 + len].iter().position(|byte| *byte == 91);
            actual.push((refusal, in_packet));
        }
        assert_eq!(actual, [(true, Some(0)), (true, Some(512))]);
    }
}

#[cfg(feature = "test-support")]
#[test]
#[expect(clippy::unwrap_used, clippy::panic, reason = "test setup must succeed")]
fn a_restart_with_a_full_fifo_commits_only_a_working_replacement() {
    struct Healthy;
    impl Device for Healthy {
        fn take(&mut self, _: &[u8]) -> Outcome {
            Outcome::Accepted
        }
    }

    // (the fresh open succeeds, finish answers, accepted universe count)
    let cases = [
        (true, Ok(()), 3),
        (
            false,
            Err(Unsent::Refused(Refusal::CannotOpen {
                why: OpenFailure::Device { with: None },
            })),
            2,
        ),
    ];
    for (fresh, answer, count) in cases {
        let universes = Universes::new(3).unwrap();
        let opens = Arc::new(AtomicU32::new(0));
        let open: Open = Box::new({
            let opens = Arc::clone(&opens);
            move |_| {
                // the second open is the fresh one of the restart
                if opens.fetch_add(1, Ordering::Relaxed) == 1 && !fresh {
                    return Err(io::Error::other("no device"));
                }
                Ok(Box::new(Healthy))
            }
        });
        let (mut engine, mut sender, wakes) = Engine::start_stepped(universes, open).unwrap();
        wakes.hold().unwrap();
        let output = OutputNumber::try_from(1).unwrap();
        let set = |count| Command::SetOutput {
            output,
            kind: engine_core::TEST_STREAM_KIND,
            universes: RunNumbers { first: 1, count },
            rate: None,
        };
        sender.send(set(2)).unwrap();
        // This fixture uses the only output spanning both universes.
        sender
            .send(Command::PatchStrip {
                name: Text::new("Strip").unwrap(),
                pixels: engine_core::Pixels::try_from(2).unwrap(),
                color: Color::Rgb,
                at: ChannelNumbers {
                    universe: 1,
                    address: 512,
                },
            })
            .unwrap();
        wakes.wake().unwrap();

        // Keep the tick paused until the hand-back has timed out. Other clients
        // may use every unreserved FIFO place while the worker is waiting.
        let Ok(Begun::Opening(restart)) = sender.begin(set(3), &[]) else {
            panic!("changing the route length must retire the current lane");
        };
        while sender.send(Command::SetMaster { level: 255 }).is_ok() {}
        assert_eq!(sender.finish(restart.open()), answer, "fresh {fresh}");
        let run = RunNumbers { first: 1, count }.check(universes).unwrap();
        let expected = Some(ProjectOutput::Kind {
            kind: engine_core::TEST_STREAM_KIND,
            universes: run,
        });
        let before_cutover = RunNumbers { first: 1, count: 2 }.check(universes).unwrap();
        assert_eq!(
            sender.project().output(output),
            Some(ProjectOutput::Kind {
                kind: engine_core::TEST_STREAM_KIND,
                universes: before_cutover,
            }),
            "the old run is visible before T9"
        );
        engine.latest();
        wakes.wake().unwrap();
        assert_eq!(sender.project().output(output), expected, "fresh {fresh}");
        let snapshot = engine.latest();
        assert_eq!(snapshot.applied(), sender.sent());
        let state = snapshot.output(output);
        assert_eq!(state.map(|o| o.universes), Some(run));
        assert_eq!(
            sender.devices,
            u64::from(fresh),
            "only a successful resize retires the old lane"
        );
        assert_eq!(sender.patch().unstreamed(&sender.project()), None);
        let started = Instant::now();
        while sender.devices != sender.shared.devices_freed.load(Ordering::Acquire) {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the old lane was never freed, fresh {fresh}"
            );
            sender.free.unpark();
            thread::sleep(Duration::from_millis(1)); // poll
        }
        assert_eq!(opens.load(Ordering::Relaxed), 2);
        let started = Instant::now();
        loop {
            wakes.wake().unwrap();
            if engine
                .latest()
                .output(output)
                .is_some_and(Output::is_running)
            {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the output never ran, fresh {fresh}"
            );
        }
    }
}

/// A device that takes its time to close: until the test lets it.
#[cfg(feature = "test-support")]
struct Closing {
    closing: Arc<AtomicBool>,
    closes: Arc<AtomicBool>,
}

#[cfg(feature = "test-support")]
impl Device for Closing {
    fn take(&mut self, _frame: &[u8]) -> Outcome {
        Outcome::Accepted
    }
}

#[cfg(feature = "test-support")]
impl Drop for Closing {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::Release);
        let started = Instant::now();
        while !self.closes.load(Ordering::Acquire) && started.elapsed() < Duration::from_secs(10) {
            thread::sleep(Duration::from_millis(1)); // poll
        }
    }
}

#[cfg(feature = "test-support")]
#[test]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test that cannot set itself up fails"
)]
fn an_opening_waits_for_the_device_sent_back_not_for_a_cancelled_plan() {
    let (closing, closes) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let open: Open = Box::new(|_kind| Err(io::Error::other("the test attaches its device")));
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(Universes::new(1).unwrap(), open).unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let run = RunNumbers { first: 1, count: 1 }
        .check(sender.universes)
        .unwrap();
    sender
        .attach(
            output,
            Box::new(Closing {
                closing: Arc::clone(&closing),
                closes: Arc::clone(&closes),
            }),
            run,
            Rate::Sixty,
        )
        .unwrap();
    let strip = Command::PatchStrip {
        name: Text::new("strip").unwrap(),
        pixels: engine_core::Pixels::try_from(1).unwrap(),
        color: Color::Rgb,
        at: ChannelNumbers {
            universe: 1,
            address: 1,
        },
    };
    assert!(sender.begin(strip, &[]).is_ok());
    let step = engine_core::Step {
        target: Target::Light { light: 1 },
        values: Light::DARK,
    };
    let name = Text::new("s").unwrap();
    sender
        .send(Command::PutStep {
            look: 1,
            name,
            step: step.into(),
        })
        .unwrap();
    let rules = MixRules {
        other_end: OtherEnd::Stay,
        ..MixRules::default()
    };
    sender.send(Command::SetMixRules { rules }).unwrap();
    // off the grid: the bar is ahead
    wakes.hold().unwrap();
    wakes.wake_at(Duration::from_millis(100)).unwrap();
    let start = Command::StartLook {
        look: 1,
        rank: Rank::Show,
        when: When::Bar,
        usage: Default::default(),
    };
    let Ok(Begun::Started { play }) = sender.begin(start, &[]) else {
        panic!("look 1 waits");
    };
    wakes.wake().unwrap();
    // stopped and removed before the wake: an opening now has not reaped the cancel
    sender.send(Command::StopLook { play }).unwrap();
    sender.send(Command::RemoveOutput { output }).unwrap();
    // the plan goes back, then the device; the plan is freed, the device closes
    wakes.wake().unwrap();
    let started = Instant::now();
    while !closing.load(Ordering::Acquire) {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the device closes"
        );
        thread::sleep(Duration::from_millis(1)); // poll
    }
    assert!(
        waits(&sender.shared, sender.devices),
        "the port is still held"
    );
    closes.store(true, Ordering::Release);
    while waits(&sender.shared, sender.devices) {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the device is freed"
        );
        thread::sleep(Duration::from_millis(1)); // poll
    }
    wakes.run();
}

#[cfg(feature = "test-support")]
#[test]
fn linked_roots_book_aggregate_work_until_the_fifo_drains() {
    for repeat in [Repeat::Loop, Repeat::Once] {
        check_linked_roots(repeat);
    }
}

#[cfg(feature = "test-support")]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "an invalid fixture, missing play or missing frame fails this stepped test"
)]
fn check_linked_roots(repeat: Repeat) {
    use engine_core::{Sequence, SequenceItem, Step};

    let (mut engine, mut sender, wakes) =
        Engine::start_stepped(Universes::new(1).unwrap(), Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let name = Text::new("linked fades").unwrap();
    sender
        .send(Command::PatchStrip {
            name,
            pixels: engine_core::Pixels::try_from(100).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    // A red marker: the strip has no dimmer, so a brightness alone offers nothing.
    let leaf = |red: u8| Command::PutStep {
        look: 1,
        name,
        step: Step {
            target: Target::Fixture { fixture: 1 },
            values: Light {
                colour: Some(Rgb {
                    red: u16::from(red) << 8,
                    green: 0,
                    blue: 0,
                }),
                ..Light::DARK
            },
        }
        .into(),
    };
    sender.send(leaf(11)).unwrap();
    sender
        .send(Command::SetMixRules {
            rules: MixRules {
                other_end: OtherEnd::Stay,
                ..Default::default()
            },
        })
        .unwrap();
    // Two roots share three nested transitions over a hundred lights.
    // Each fits alone. Three plays fit with their terminal samples reserved.
    for (look, child) in [(2, 1), (3, 2), (4, 3), (5, 3), (6, 4)] {
        sender
            .send(Command::PutSequence {
                look,
                name,
                sequence: Sequence::new(&[SequenceItem {
                    look: child,
                    transition: Percent::MOST,
                    ..Default::default()
                }])
                .unwrap(),
            })
            .unwrap();
    }
    let start = |look, rank| Command::StartLook {
        look,
        rank,
        when: When::Now,
        usage: Use {
            repeat,
            ..Default::default()
        },
    };
    let started = |s: &mut Sender, cmd| match s.begin(cmd, &[]) {
        Ok(Begun::Started { play }) => play,
        _ => panic!("expected an admitted play"),
    };
    let clock = wakes.wake().unwrap() + Duration::from_secs(1);
    wakes.set_time(clock);
    let picture = |engine: &mut Engine| {
        engine.latest();
        wakes.wake_at(clock).unwrap();
        let snapshot = engine.latest();
        (
            snapshot.active_plays().collect::<Vec<_>>(),
            (0..100)
                .map(|light| snapshot.sent(1, 3 * light + 1))
                .collect::<Vec<_>>(),
        )
    };
    let ids = [Rank::Manual, Rank::Show, Rank::Autopilot]
        .map(|rank| started(&mut sender, start(4, rank)));
    let before = picture(&mut engine);
    assert_eq!(before.0.iter().map(|p| p.play).collect::<Vec<_>>(), ids);
    assert_eq!(before.1, vec![11; 100]);
    let saved = sender.looks().all();
    let tail = sender.sent();
    for (case, command) in [
        ("another linked root", start(5, Rank::Manual)),
        ("restart a reserved root", start(4, Rank::Manual)),
        (
            "expand every parent's shared sequence",
            Command::PutSequence {
                look: 2,
                name,
                sequence: Sequence::new(
                    &[SequenceItem {
                        look: 1,
                        transition: Percent::MOST,
                        ..Default::default()
                    }; 4],
                )
                .unwrap(),
            },
        ),
    ] {
        assert_eq!(
            sender.send(command),
            Err(Unsent::Refused(Refusal::WorkLimit)),
            "{case}"
        );
        assert_eq!(sender.sent(), tail, "{case}");
        assert_eq!(sender.looks().all(), saved, "{case}");
        assert_eq!(picture(&mut engine), before, "{case}");
    }
    // Expanded groups share the FIFO prefix with these roots. One hundred
    // lights fit once, but a queueful would exhaust the remaining visits.
    let mut refusal = None;
    for _ in 0..QUEUE - PLAYS {
        let tail = sender.sent();
        if let Err(reason) = sender.send(Command::SetColour {
            target: Target::Fixture { fixture: 1 },
            red: 0,
            green: 0,
            blue: 0,
        }) {
            assert_eq!(sender.sent(), tail);
            refusal = Some(reason);
            break;
        }
    }
    assert_eq!(refusal, Some(Unsent::Refused(Refusal::WorkLimit)));
    for play in ids {
        if repeat == Repeat::Loop {
            sender.send(Command::StopLook { play }).unwrap();
        }
        sender.send(Command::ReleaseHold { play }).unwrap();
    }
    // A loop stopped under Stay and a once-use can sample before release.
    // Together with the queued group writes, a larger root still cannot fit.
    assert_eq!(
        sender.send(start(6, Rank::Manual)),
        Err(Unsent::Refused(Refusal::WorkLimit))
    );
    // The queued colour grabs hold black.
    assert_eq!(picture(&mut engine), (vec![], vec![0; 100]));
    let retry = started(&mut sender, start(6, Rank::Manual));
    sender.send(leaf(22)).unwrap();
    let (plays, reds) = picture(&mut engine);
    assert_eq!(plays.iter().map(|p| p.play).collect::<Vec<_>>(), [retry]);
    // The later start wins over the older colour grabs.
    assert_eq!(reds, vec![22; 100]);
    wakes.run();
}

#[cfg(feature = "test-support")]
#[test]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a missing fixture, stream or play fails this stepped admission test"
)]
fn a_saved_wave_can_start_alone_at_the_work_limit() {
    let universes = Universes::new(25).unwrap();
    let (mut engine, mut sender, wakes) =
        Engine::start_stepped(universes, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    sender
        .attach(
            OutputNumber::try_from(1).unwrap(),
            Box::new(Hears(Arc::new(Heard {
                frames: AtomicU64::new(0),
                levels: Default::default(),
            }))),
            RunNumbers {
                first: 1,
                count: 25,
            }
            .check(universes)
            .unwrap(),
            Rate::Sixty,
        )
        .unwrap();
    let name = Text::new("wall").unwrap();
    // The panel fills exactly 24 universes. The extra pixel begins the next one.
    for (pixels, universe) in [(4096, 1), (1, 25)] {
        sender
            .send(Command::PatchStrip {
                name,
                pixels: engine_core::Pixels::try_from(pixels).unwrap(),
                color: Color::Rgb,
                at: ChannelNumbers {
                    universe,
                    address: 1,
                },
            })
            .unwrap();
    }
    sender
        .send(Command::SetGroup {
            name,
            members: MemberList::Fits(
                Members::new(&[
                    Member::Fixture { fixture: 1 },
                    Member::Fixture { fixture: 2 },
                ])
                .unwrap(),
            ),
        })
        .unwrap();
    const RED: Rgb = Rgb {
        red: u16::MAX,
        green: 0,
        blue: 0,
    };
    let wave = |look, target| Command::PutPattern {
        look,
        name,
        pattern: Pattern::Wave {
            target,
            period: Period::new(1).unwrap(),
            palette: Palette::Inline(Swatches::new(&[RED]).unwrap()),
        },
    };
    let wall = wave(1, WaveTarget::Matrix { fixture: 1 });
    assert!(
        edits(&wall),
        "Wave compilation belongs in off-lock edit preparation"
    );
    sender.send(wall).unwrap();
    sender
        .send(wave(2, WaveTarget::Matrix { fixture: 2 }))
        .unwrap();
    let start = |look| Command::StartLook {
        look,
        rank: Rank::Show,
        when: When::Now,
        usage: Default::default(),
    };
    let clock = wakes.wake().unwrap();
    let Ok(Begun::Started { play }) = sender.begin(start(1), &[]) else {
        panic!("a saved 4096-light Wave must be playable on its own");
    };
    let applying_frame = |engine: &mut Engine| {
        engine.latest();
        wakes.wake_at(clock).unwrap();
        let snapshot = engine.latest();
        (
            snapshot
                .active_plays()
                .map(|row| row.play)
                .collect::<Vec<_>>(),
            // Light 4096's red: universe 24, address 3 * 4095 - 23 * 512 + 1.
            snapshot.sent(24, 510),
        )
    };
    let before = applying_frame(&mut engine);
    assert_eq!(before, (vec![play], 255));
    let saved = sender.looks().all();
    let tail = sender.sent();
    for command in [start(2), wave(3, WaveTarget::Group { group: name })] {
        assert_eq!(
            sender.send(command),
            Err(Unsent::Refused(Refusal::WorkLimit))
        );
        assert_eq!(sender.sent(), tail);
        assert_eq!(sender.looks().all(), saved);
        assert_eq!(applying_frame(&mut engine), before);
    }
    sender.send(Command::ReleaseHold { play }).unwrap();
    let Ok(Begun::Started { play: next }) = sender.begin(start(2), &[]) else {
        panic!("release frees the loop's frame reservation");
    };
    assert_eq!(applying_frame(&mut engine), (vec![next], 0));

    // A stop that keeps the Manual Wave must still sample it at the stop.
    // Releasing it in that prefix only frees the following frame sample.
    sender.send(Command::ReleaseHold { play: next }).unwrap();
    let manual = Command::StartLook {
        look: 1,
        rank: Rank::Manual,
        when: When::Now,
        usage: Default::default(),
    };
    let Ok(Begun::Started { play }) = sender.begin(manual, &[]) else {
        panic!("the Manual Wave starts");
    };
    let intensity = Command::SetIntensity {
        target: Target::Light { light: 1 },
        intensity: u16::MAX,
    };
    sender.send(intensity).unwrap();
    assert_eq!(applying_frame(&mut engine).0, [play]);
    sender.send(Command::StopLook { play }).unwrap();
    // Moving a sample from the live Wave into the stop event preserves
    // the headroom to write the same light property again.
    assert_eq!(sender.send(intensity), Ok(()));
    sender.send(Command::ReleaseHold { play }).unwrap();
    assert_eq!(
        sender.send(start(1)),
        Err(Unsent::Refused(Refusal::WorkLimit))
    );
    assert_eq!(applying_frame(&mut engine), (vec![], 0));
    sender.send(start(1)).unwrap();
    assert_eq!(applying_frame(&mut engine).0.len(), 1);
    wakes.run();
}

#[cfg(feature = "test-support")]
#[test]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "an invalid fixture, missing play or missing frame fails this stepped test"
)]
fn a_kept_once_parent_retains_its_nested_look_ids_until_release() {
    use engine_core::{Repeat, Sequence, SequenceItem, Step, Use};

    let (mut engine, mut sender, wakes) =
        Engine::start_stepped(Universes::new(1).unwrap(), Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let name = Text::new("held links").unwrap();
    sender
        .send(Command::PatchStrip {
            name,
            pixels: engine_core::Pixels::try_from(1).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    // Colours: the strip has no dimmer, so a brightness alone offers nothing to keep.
    for (look, red) in [(1, 10 << 8), (2, 20 << 8)] {
        sender
            .send(Command::PutStep {
                look,
                name,
                step: Step {
                    target: Target::Light { light: 1 },
                    values: Light {
                        colour: Some(Rgb {
                            red,
                            green: 0,
                            blue: 0,
                        }),
                        ..Light::DARK
                    },
                }
                .into(),
            })
            .unwrap();
    }
    let link = |look, child| Command::PutSequence {
        look,
        name,
        sequence: Sequence::new(&[SequenceItem {
            look: child,
            ..Default::default()
        }])
        .unwrap(),
    };
    sender.send(link(3, 1)).unwrap();
    sender.send(link(4, 3)).unwrap();
    let start = wakes.wake().unwrap();
    let Ok(Begun::Started { play }) = sender.begin(
        Command::StartLook {
            look: 4,
            rank: Rank::Manual,
            when: When::Now,
            usage: Use {
                repeat: Repeat::Once,
                ..Default::default()
            },
        },
        &[],
    ) else {
        panic!("the parent starts");
    };
    wakes.wake_at(start).unwrap();
    engine.latest();
    wakes.wake_at(start + Duration::from_secs(2)).unwrap();
    assert_eq!(
        engine.latest().active_plays().next().unwrap().state,
        PlayState::Kept
    );
    // Both saved sequences now reference step 2. The frozen generation reached
    // sequence 3 and step 1 through the old definitions.
    sender.send(link(4, 2)).unwrap();
    sender.send(link(3, 2)).unwrap();
    for look in [1, 3] {
        assert_eq!(
            sender.send(Command::RemoveLook { look }),
            Err(Unsent::Refused(Refusal::LookInUse { look: 4 }))
        );
    }
    sender.send(Command::ReleaseHold { play }).unwrap();
    for look in [1, 3] {
        sender.send(Command::RemoveLook { look }).unwrap();
    }
    wakes.run();
}

/// The tick never frees a grab's box: the book keeps each box a command lets go, and the
/// command's own change list, until the tick applied that command. A front that dropped
/// them at once would leave the tick the last owner.
#[cfg(feature = "test-support")]
#[test]
#[expect(
    clippy::unwrap_used,
    reason = "a stepped test that cannot set itself up fails"
)]
fn a_grab_box_outlives_the_command_that_lets_it_go_until_the_tick_applied_it() {
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(Universes::new(1).unwrap(), Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    sender
        .send(Command::PatchStrip {
            name: Text::new("strip").unwrap(),
            pixels: engine_core::Pixels::try_from(1).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    let light = Target::Light { light: 1 };
    let grab = Command::SetColour {
        target: light,
        red: u16::MAX,
        green: 0,
        blue: 0,
    };
    let release = Command::ReleaseColour { target: light };
    let next = Command::SetMaster { level: 255 };
    // (step, a wake before the command, boxes waiting for the tick after it, the grabs'
    // revision: one more for the grab and for the release, none for Master)
    let steps = [
        ("the grab sent: its change list", grab, false, 1, 1),
        (
            "the grab applied, the release sent: the box and its change list",
            release,
            true,
            2,
            2,
        ),
        ("the release not applied yet", next, false, 2, 2),
        ("applied", next, true, 0, 2),
    ];
    for (step, command, wake, waiting, revision) in steps {
        if wake {
            wakes.wake().unwrap();
        }
        sender.send(command).unwrap();
        assert_eq!(sender.grabs.retiring(), waiting, "{step}");
        assert_eq!(sender.grabs_revision(), revision, "{step}: revision");
    }
    wakes.run();
}

/// A kept play holds the place in its plan where it stopped: when the tempo doubles after
/// the stop, its bytes stay, while a playing Wave moves with time.
#[cfg(feature = "test-support")]
#[test]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a missing fixture, play or frame fails this stepped test"
)]
fn a_kept_wave_keeps_its_bytes_when_the_tempo_doubles() {
    let (mut engine, mut sender, wakes) =
        Engine::start_stepped(Universes::new(1).unwrap(), Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let name = Text::new("kept wave").unwrap();
    sender
        .send(Command::PatchStrip {
            name,
            pixels: engine_core::Pixels::try_from(8).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    let tempo = |bpm| Command::SetInternalTempo {
        bpm: Bpm::new(bpm).unwrap(),
    };
    sender.send(tempo(60)).unwrap();
    sender
        .send(Command::SetMixRules {
            rules: MixRules {
                other_end: OtherEnd::Stay,
                ..Default::default()
            },
        })
        .unwrap();
    let red = Rgb {
        red: u16::MAX,
        green: 0,
        blue: 0,
    };
    sender
        .send(Command::PutPattern {
            look: 1,
            name,
            pattern: Pattern::Wave {
                target: WaveTarget::Matrix { fixture: 1 },
                period: Period::new(4).unwrap(),
                palette: Palette::Inline(Swatches::new(&[red, Rgb::BLACK]).unwrap()),
            },
        })
        .unwrap();
    let start = wakes.wake().unwrap();
    wakes.set_time(start);
    let Ok(Begun::Started { play }) = sender.begin(
        Command::StartLook {
            look: 1,
            rank: Rank::Show,
            when: When::Now,
            usage: Default::default(),
        },
        &[],
    ) else {
        panic!("the Wave starts");
    };
    // The red bytes of the 8 pixels in the frame at `at`.
    let mut reds = |at| {
        engine.latest();
        wakes.wake_at(at).unwrap();
        let snapshot = engine.latest();
        (1..=8)
            .map(|pixel| snapshot.sent(1, 3 * pixel - 2))
            .collect::<Vec<_>>()
    };
    let second = |tenths: u64| start + Duration::from_millis(100 * tenths);
    let playing = reds(second(10));
    let stopped = reds(second(15));
    assert_ne!(playing, stopped, "a playing Wave moves");
    wakes.set_time(second(15));
    sender.send(Command::StopLook { play }).unwrap();
    assert_eq!(reds(second(15)), stopped, "kept where it stopped");
    sender.send(tempo(120)).unwrap();
    assert_eq!(
        reds(second(25)),
        stopped,
        "the tempo doubled, the kept bytes stay"
    );
    wakes.run();
}

/// A strip of three pixels at 1, each channel held at 200: all nine follow Blackout.
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
fn lit_strip() -> (Engine, Sender, Arc<Heard>) {
    let (engine, mut sender) = Engine::start(Universes::ONE).unwrap();
    let heard = Arc::new(Heard {
        frames: AtomicU64::new(0),
        levels: Default::default(),
    });
    let run = RunNumbers { first: 1, count: 1 }
        .check(sender.universes)
        .unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let hears = Box::new(Hears(Arc::clone(&heard)));
    sender.attach(output, hears, run, Rate::Sixty).unwrap();
    let strip = Command::PatchStrip {
        name: Text::new("strip").unwrap(),
        pixels: engine_core::Pixels::try_from(3).unwrap(),
        color: Color::Rgb,
        at: ChannelNumbers {
            universe: 1,
            address: 1,
        },
    };
    sender.send(strip).unwrap();
    for address in 1..=9 {
        let channel = ChannelNumbers {
            universe: 1,
            address,
        };
        sender
            .send(Command::SetChannel {
                channel,
                value: 200,
            })
            .unwrap();
    }
    (engine, sender, heard)
}

/// `crew/plans/251.md`, Blackout: the rule's cases on the bytes of the next frame, and
/// what the picture reads back. Each hand that holds keeps one place in the queue.
#[test]
#[expect(clippy::unwrap_used, reason = "a test whose tick failed fails")]
fn two_hands_and_the_latch_keep_blackout_on_the_bytes() {
    let (mut engine, mut sender, heard) = lit_strip();
    let (a, b) = (sender.hand(), sender.hand());
    assert_ne!(a, b, "each connection its own hand");
    assert!(![a, b].contains(&Hand::ENGINE), "never the engine's own");
    let (lit, dark) = ([200; 9], [0; 9]);
    let blackout = |on| Command::Blackout { on };
    let latch = |on| Command::SetBlackoutLatch { on };
    // (step, hand, command or its drop, bytes after, latched, holders)
    let steps = [
        ("A on", a, Some(blackout(true)), dark, false, 1),
        ("B on", b, Some(blackout(true)), dark, false, 2),
        (
            "A off: B still holds",
            a,
            Some(blackout(false)),
            dark,
            false,
            1,
        ),
        ("B off", b, Some(blackout(false)), lit, false, 0),
        ("A on twice", a, Some(blackout(true)), dark, false, 1),
        ("", a, Some(blackout(true)), dark, false, 1),
        ("A drops: lit", a, None, lit, false, 0),
        ("latch on", b, Some(latch(true)), dark, true, 0),
        ("B on", b, Some(blackout(true)), dark, true, 1),
        ("B drops: the latch stays", b, None, dark, true, 0),
        (
            "the engine's hand lets the latch go",
            Hand::ENGINE,
            Some(latch(false)),
            lit,
            false,
            0,
        ),
        ("A on again", a, Some(blackout(true)), dark, false, 1),
        ("latch on again", b, Some(latch(true)), dark, true, 1),
        (
            "ReleaseAll keeps the latch and the hold",
            Hand::ENGINE,
            Some(Command::ReleaseAll),
            dark,
            true,
            1,
        ),
    ];
    assert_eq!(heard.after(&mut engine, &sender), lit, "held at 200");
    for (step, hand, command, bytes, latched, holders) in steps {
        match command {
            Some(command) => drop(sender.begin_as(hand, command, &[]).unwrap()),
            None => sender.drop_hand(hand).unwrap(),
        }
        assert_eq!(heard.after(&mut engine, &sender), bytes, "{step}");
        let hold = engine.fresh().unwrap().performance().blackout_hold;
        let want = engine_core::BlackoutHold { latched, holders };
        assert_eq!(hold, want, "{step}: read back");
        assert_eq!(
            sender.commands.held, holders as usize,
            "{step}: let-go places"
        );
    }
}

/// A hand's let-go keeps its place in the queue from its first hold: with the tick
/// gone, the queue fills, and the let-go still goes in.
#[test]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
fn a_hands_let_go_keeps_its_place_in_the_queue() {
    let (engine, mut sender) = Engine::start(Universes::ONE).unwrap();
    drop(engine); // nothing drains the queue now
    let (a, b) = (sender.hand(), sender.hand());
    let raw = Command::SetChannel {
        channel: ChannelNumbers {
            universe: 1,
            address: 1,
        },
        value: 1,
    };
    let fill = |sender: &mut Sender, free: usize| {
        while sender.commands.slots() > free {
            sender.send(raw).unwrap();
        }
    };
    let on = Command::Blackout { on: true };
    let full = Err(Refusal::QueueFull);
    fill(&mut sender, 1);
    let refused = sender.begin_as(a, on, &[]).map(drop);
    assert_eq!(refused, full, "one place: none for the let-go");
    assert_eq!(
        (sender.blackout.hold().holders, sender.commands.held),
        (0, 0),
        "refused: nothing changes"
    );
    assert_eq!(sender.commands.slots(), 1, "the place is still free");
    // the hold and its let-go: two places
    let (engine, mut sender) = Engine::start(Universes::ONE).unwrap();
    drop(engine);
    let a = sender.hand();
    fill(&mut sender, 2);
    sender.begin_as(a, on, &[]).unwrap();
    assert_eq!((sender.commands.slots(), sender.commands.held), (0, 1));
    assert_eq!(sender.send(raw), Err(Unsent::Refused(Refusal::QueueFull)));
    let latch = Command::SetBlackoutLatch { on: true };
    assert_eq!(sender.begin_as(b, latch, &[]).map(drop), full);
    assert_eq!(
        sender.blackout.hold(),
        engine_core::BlackoutHold {
            latched: false,
            holders: 1
        },
        "a refused latch changes nothing"
    );
    assert_eq!(
        sender.begin_as(a, on, &[]).map(drop),
        Ok(()),
        "A holds already: nothing to queue"
    );
    assert_eq!(
        sender
            .begin_as(b, Command::Blackout { on: false }, &[])
            .map(drop),
        Ok(()),
        "B holds nothing: nothing to queue"
    );
    assert_eq!(sender.begin_as(b, on, &[]).map(drop), full);
    sender.drop_hand(a).unwrap();
    assert_eq!(sender.blackout.hold().holders, 0, "A let go");
    assert_eq!((sender.commands.slots(), sender.commands.held), (0, 0));
    // the let-go by a press, with the queue full
    let (engine, mut sender) = Engine::start(Universes::ONE).unwrap();
    drop(engine);
    let a = sender.hand();
    fill(&mut sender, 2);
    sender.begin_as(a, on, &[]).unwrap();
    assert_eq!(sender.commands.slots(), 0);
    sender
        .begin_as(a, Command::Blackout { on: false }, &[])
        .unwrap();
    assert_eq!(sender.blackout.hold().holders, 0, "A let go by a press");
}

/// The 17th hand to hold finds the book's first room full: it holds like the first.
#[test]
#[expect(clippy::unwrap_used, reason = "a test whose tick failed fails")]
fn seventeen_hands_hold_blackout() {
    let (mut engine, mut sender, heard) = lit_strip();
    let hands: Vec<Hand> = (0..17).map(|_| sender.hand()).collect();
    for hand in &hands {
        sender
            .begin_as(*hand, Command::Blackout { on: true }, &[])
            .unwrap();
    }
    assert_eq!(heard.after(&mut engine, &sender), [0; 9]);
    let hold = engine.fresh().unwrap().performance().blackout_hold;
    assert_eq!(hold.holders, 17);
    for hand in hands {
        sender.drop_hand(hand).unwrap();
    }
    assert_eq!(heard.after(&mut engine, &sender), [200; 9]);
}

/// A let-go the queue refuses changes nothing: the hand still holds and keeps its place.
/// Its kept place makes this unreachable; the test fills the queue past it.
#[test]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
fn a_refused_let_go_changes_nothing() {
    let (engine, mut sender) = Engine::start(Universes::ONE).unwrap();
    drop(engine); // nothing drains the queue now
    let a = sender.hand();
    sender
        .begin_as(a, Command::Blackout { on: true }, &[])
        .unwrap();
    let filler = || Sent {
        command: EngineCommand::ReleaseChannels,
        at: Duration::ZERO,
        brings: Brings::Nothing,
    };
    while sender.commands.fifo.push(filler()).is_ok() {}
    assert_eq!(sender.drop_hand(a), Err(Refusal::QueueFull));
    assert_eq!(
        (sender.blackout.hold().holders, sender.commands.held),
        (1, 1)
    );
}
