//! Proves the tick thread and each output's own thread allocate nothing per wake, also
//! while they serve two outputs at different rates, resolve 512 patched lights and a strip
//! of 100 pixels over two universes, take commands, swap in new writers, take a group of
//! 256 runs, start, restart, replan and release a play of a step over that group, grab,
//! update, end (kept or let go) and release a direct hold on one run and on two, clear the
//! kept ones by a change of the rules, mix it with the direct holds and hand a reader a
//! snapshot on every frame. A Wave runs over two strip pixels. Linked sequences run, replan and end inside the
//! counting window. The stepped clock takes full command bursts, replans 32 live roots
//! near the visit limit and ends all 32 while retirement is stalled. It also reads
//! output health through a device loss, return and replacement with all frees counted.
//! The tick frees nothing: the old writers, runs, plans and a replaced device with its thread go to
//! the free thread. A
//! counting allocator sees every allocation in this process, except those of the front
//! while it sends: a light, patch, look or attach command allocates there (the patch and a
//! plan are calculations on the heap, a device is boxed and gets a thread and frame boxes), never on
//! the tick thread. A new output's thread allocates as the system starts it, on itself,
//! before the attach returns: those count as the front's too. It counts the frees of the tick thread apart, by its
//! name. The outputs are attached and the lights patched before the window (the writers
//! compiled on the sending thread). In the window the main thread sends and reads
//! snapshots, which allocates nothing, and sleeps, so any allocation counted is the tick's
//! or the reader's. That each real device's `take` allocates nothing is proved in
//! `crates/outputs/tests/no_alloc.rs`.

#[path = "../../outputs/tests/counting/mod.rs"]
mod counting;
mod mock;

use engine::{
    Begun, Brightness, ChannelNumbers, Color, Command, Engine, Fixture, Light, ManualEnd, Member,
    MemberList, Members, MixRules, OtherEnd, PlayId, Property, Rank, Rate, Refusal, Rgb,
    RunNumbers, Sender, Target, Text, Universes, Value,
};
use engine_core::{Pixels, Step};

/// 512 lights: Generic RGB in its mode RGB, 170 to a universe.
const LIGHTS: u16 = 512;
const UNIVERSES: u16 = 4;

/// Sends `command` as the front, its allocations not counted. Whether it is taken.
fn send(sender: &mut Sender, command: Command, library: &[Fixture]) -> bool {
    let begun = counting::uncounted(|| sender.begin(command, library));
    matches!(
        begun,
        Ok(Begun::Queued | Begun::Patched { .. } | Begun::Grabbed { .. })
    )
}

/// Begins `command` as the front, its allocations not counted.
fn begin(sender: &mut Sender, command: Command, library: &[Fixture]) -> Result<Begun, Refusal> {
    counting::uncounted(|| sender.begin(command, library))
}

/// Look 1: the lights of the group "even" in `red` and full blue.
fn step(red: u16) -> Command {
    Command::PutStep {
        look: 1,
        name: text("even"),
        step: Step {
            target: Target::Group {
                group: text("even"),
            },
            values: Light {
                colour: Some(Rgb {
                    red,
                    green: 0,
                    blue: u16::MAX,
                }),
                ..Light::DARK
            },
        },
    }
}

fn text(text: &str) -> Text {
    Text::new(text).expect("a short text")
}

/// The test definition of a Generic RGB, read when the test is built.
const GENERIC_RGB: &[u8] =
    include_bytes!("../../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");

fn library() -> Vec<Fixture> {
    vec![fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB).expect("it reads")]
}

/// Light `n` of the 512 (from 0) at its place, 3 channels each: 170 to each of universes
/// 1 to 3, the last two at the end of universe 4, after the strip (no two fixtures share
/// a channel).
fn place(n: u16) -> ChannelNumbers {
    if n < 510 {
        ChannelNumbers {
            universe: 1 + n / 170,
            address: 1 + 3 * (n % 170),
        }
    } else {
        ChannelNumbers {
            universe: 4,
            address: 507 + 3 * (n - 510),
        }
    }
}

fn main() {
    #[cfg(feature = "test-support")]
    output_health();
    let universes = Universes::new(UNIVERSES).expect("4 universes");
    let (mut engine, mut sender) = Engine::start(universes).expect("the tick thread starts");
    let run = RunNumbers { first: 1, count: 1 };
    mock::attach(&mut sender, universes, 1, run, Rate::Sixty);
    // the second streams universes 3 and 4, which the strip below runs over
    let three_four = RunNumbers { first: 3, count: 2 };
    mock::attach(&mut sender, universes, 2, three_four, Rate::Thirty);
    let library = library();
    for n in 0..LIGHTS {
        let patch = Command::Patch {
            name: text("PAR"),
            maker: text("Generic"),
            model: text("Generic RGB"),
            mode: text("RGB"),
            at: place(n),
        };
        assert!(send(&mut sender, patch, &library), "light {n} is patched");
        let full = Command::SetIntensity {
            target: Target::Fixture { fixture: n + 1 },
            intensity: u16::MAX,
        };
        assert!(send(&mut sender, full, &library), "light {n} is lit");
        if n % 64 == 63 {
            // the queue takes a wake's worth: let the tick apply them
            let seen = engine.latest().state.frame;
            mock::wait_until("no frame went by", || engine.latest().state.frame > seen);
        }
    }
    // and a strip of 100 pixels that runs from universe 3 into 4, lit
    let strip = Command::PatchStrip {
        name: text("Strip"),
        pixels: Pixels::try_from(100).expect("100 pixels"),
        color: Color::Rgb,
        at: ChannelNumbers {
            universe: 3,
            address: 511,
        },
    };
    assert!(send(&mut sender, strip, &library), "the strip is patched");
    let full = Command::SetIntensity {
        target: Target::Fixture {
            fixture: LIGHTS + 1,
        },
        intensity: u16::MAX,
    };
    assert!(send(&mut sender, full, &library), "the strip is lit");
    // every second light, from light 2: 256 runs of one light, sent as one command
    let even: Vec<Member> = (2..=LIGHTS)
        .step_by(2)
        .map(|light| Member::Light { light })
        .collect();
    let group = Command::SetGroup {
        name: text("even"),
        members: MemberList::Fits(Members::new(&even).expect("256 members")),
    };
    assert!(send(&mut sender, group, &library), "the group is made");
    // two pixels of the strip apart: a grab of two runs
    let ends = Command::SetGroup {
        name: text("ends"),
        members: MemberList::Fits(
            Members::new(&[
                Member::Light { light: LIGHTS + 1 },
                Member::Light { light: LIGHTS + 3 },
            ])
            .expect("2 members"),
        ),
    };
    assert!(send(&mut sender, ends, &library), "the group is made");
    assert!(send(&mut sender, step(0), &library), "look 1 is saved");
    let green = Command::SetColour {
        target: Target::Group {
            group: text("even"),
        },
        red: 0,
        green: u16::MAX,
        blue: 0,
    };
    // Thread start-up may allocate: wait until both outputs were served.
    mock::wait_until("the outputs were never served", || {
        let outputs = engine.latest().state.outputs;
        outputs[..2]
            .iter()
            .all(|o| o.is_some_and(|o| o.counters.accepted > 0))
    });

    let (allocations, tick_frees) = (counting::allocations(), counting::tick_frees());
    let mut attaching = 0;
    let mut play: Option<PlayId> = None;
    let mut launched = false;
    let frame = engine.latest().state.frame;
    for value in 0..=255 {
        let channel = ChannelNumbers {
            universe: 1,
            address: 1 + u16::from(value),
        };
        let _ = sender.send(Command::SetChannel { channel, value });
        if value % 3 == 0 {
            let _ = sender.send(Command::ReleaseChannel { channel });
        }
        let _ = sender.send(Command::Blackout { on: value % 2 == 0 });
        let light = u16::from(value) * 2;
        let colour = Command::SetColour {
            target: Target::Light { light: light + 1 },
            red: u16::from(value) << 8,
            green: 0,
            blue: u16::MAX,
        };
        assert!(
            send(&mut sender, colour, &library),
            "a colour for light {light}"
        );
        if value % 16 == 8 {
            // the runs of the group, in place: applied in one wake
            assert!(send(&mut sender, green, &library), "the group goes green");
        }
        if value % 16 == 2 {
            // the tempo: a feed, taps, a release; an Autopilot play on the next beat or
            // bar, started afresh (the waiting one's plan goes back), stopped every other
            // time under Reveal: ended
            let bpm = engine::Bpm::new(30 + u16::from(value)).unwrap();
            assert!(
                send(&mut sender, Command::SetInternalTempo { bpm }, &library),
                "the feed"
            );
            assert!(send(&mut sender, Command::TapTempo, &library), "a tap");
            if value % 32 == 18 {
                assert!(
                    send(&mut sender, Command::ReleaseTempo, &library),
                    "released"
                );
            }
            let start = Command::StartLook {
                look: 1,
                rank: Rank::Autopilot,
                when: if value % 64 < 32 {
                    engine::When::Beat
                } else {
                    engine::When::Bar
                },
            };
            let Ok(Begun::Started { play: waits }) = begin(&mut sender, start, &library) else {
                panic!("look 1 waits at Autopilot");
            };
            if value % 32 == 2 {
                let stop = Command::StopLook { play: waits };
                assert!(
                    send(&mut sender, stop, &library),
                    "the Autopilot play stops"
                );
            }
        }
        if value % 16 == 4 {
            // a start, or a fresh start in the play's slot: the old plan goes back
            let start = begin(
                &mut sender,
                Command::StartLook {
                    look: 1,
                    rank: Rank::Manual,
                    when: engine::When::Now,
                },
                &library,
            );
            let Ok(Begun::Started { play: started }) = start else {
                panic!("look 1 starts");
            };
            play = Some(started);
        }
        if value % 16 == 6 {
            // the playing look saved anew: the tick swaps in its plan, the old one goes back
            let replan = step(u16::from(value) << 8);
            assert!(send(&mut sender, replan, &library), "look 1 is saved anew");
        }
        if value % 32 == 12
            && let Some(play) = play.take()
        {
            // released: its plan goes back
            let release = Command::ReleaseHold { play };
            assert!(send(&mut sender, release, &library), "the play is released");
        }
        if value % 32 == 28
            && let Some(play) = play.take()
        {
            // a flash let go: its plan goes back
            let end = Command::EndFlash { play };
            assert!(send(&mut sender, end, &library), "the flash ends");
        }
        if value % 16 == 10 {
            // the rules change: Stay keeps a stopped Show play, Reveal ends it, and a
            // change to Reveal ends the one kept: its plan goes back
            let stay = value % 32 == 10;
            let rules = MixRules {
                brightness: if stay {
                    Brightness::Highest
                } else {
                    Brightness::Latest
                },
                manual_end: ManualEnd::UntilRelease,
                other_end: if stay {
                    OtherEnd::Stay
                } else {
                    OtherEnd::Reveal
                },
            };
            assert!(
                send(&mut sender, Command::SetMixRules { rules }, &library),
                "the rules change"
            );
            let start = Command::StartLook {
                look: 1,
                rank: Rank::Show,
                when: engine::When::Now,
            };
            let Ok(Begun::Started { play: shown }) = begin(&mut sender, start, &library) else {
                panic!("look 1 starts at Show");
            };
            let stop = Command::StopLook { play: shown };
            assert!(send(&mut sender, stop, &library), "the Show play stops");
            if stay {
                // an Autopilot play waiting for the next bar, stopped under Stay:
                // cancelled, its plan goes back
                let start = Command::StartLook {
                    look: 1,
                    rank: Rank::Autopilot,
                    when: engine::When::Bar,
                };
                let Ok(Begun::Started { play: waits }) = begin(&mut sender, start, &library) else {
                    panic!("look 1 waits at Autopilot");
                };
                let stop = Command::StopLook { play: waits };
                assert!(send(&mut sender, stop, &library), "the waiting play stops");
            }
        }
        if value % 16 == 14 {
            // a grab on one run and on two: updated, then ended under the Manual end
            // rule, kept or let go; a kept one is cleared by the next change to UntilStop
            let manual_end = if value % 32 == 14 {
                ManualEnd::UntilRelease
            } else {
                ManualEnd::UntilStop
            };
            let rules = MixRules {
                brightness: Brightness::Latest,
                manual_end,
                other_end: OtherEnd::Reveal,
            };
            assert!(
                send(&mut sender, Command::SetMixRules { rules }, &library),
                "the rules change"
            );
            let strip = Target::Fixture {
                fixture: LIGHTS + 1,
            };
            let ends = Target::Group {
                group: text("ends"),
            };
            for target in [strip, ends] {
                let blue = Value::Colour(Rgb {
                    red: 0,
                    green: 0,
                    blue: u16::MAX,
                });
                let set = Command::SetProperty {
                    target,
                    value: blue,
                };
                let Ok(Begun::Grabbed { grab }) = begin(&mut sender, set, &library) else {
                    panic!("{target:?} is grabbed");
                };
                let red = Value::Colour(Rgb {
                    red: u16::from(value) << 8,
                    ..Rgb::BLACK
                });
                let update = Command::UpdateGrab { grab, value: red };
                assert!(send(&mut sender, update, &library), "the grab is updated");
                let end = Command::EndGrab { grab };
                assert!(send(&mut sender, end, &library), "the grab ends");
            }
            let release = Command::ReleaseProperty {
                target: ends,
                property: Property::Colour,
            };
            assert!(send(&mut sender, release, &library), "the colour is let go");
        }
        if value % 16 == 0 {
            // new writers: the tick swaps them in and sends the old ones back
            let back = Command::Move {
                fixture: light + 1,
                at: place(light),
            };
            assert!(send(&mut sender, back, &library), "fixture {light} moves");
        }
        if value % 64 == 32 {
            // a new device for output 1: the tick swaps it in and sends the old one back
            let before = counting::allocations();
            counting::uncounted(|| mock::attach(&mut sender, universes, 1, run, Rate::Sixty));
            attaching += counting::allocations() - before;
        }
        // Pace the commands by the tick: a reader on every frame waits for the next picture.
        let seen = engine.latest().state.frame;
        mock::wait_until("no frame went by", || engine.latest().state.frame > seen);
        launched |=
            engine.latest().state.plays.iter().any(|play| {
                play.rank == Rank::Autopilot && play.state == engine::PlayState::Playing
            });
    }
    let state = engine.latest().state;
    let lit = engine.latest().lights()[..usize::from(LIGHTS) + 100]
        .iter()
        .filter(|light| light.intensity == Some(u16::MAX))
        .count();
    let green_lights = engine.latest().lights()[..usize::from(LIGHTS)]
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|light| light.colour.is_some_and(|c| c.green == u16::MAX))
        .count();
    // Nested transition, a shared step replan, and a once-use frozen between wakes.
    // Start and finish inside the allocator window on the production tick.
    counting::uncounted(|| {
        use engine_core::{Repeat, Sequence, SequenceItem, Use};
        sender
            .send(Command::SetMixRules {
                rules: MixRules::default(),
            })
            .unwrap();
        for (look, child) in [(2, 1), (3, 2)] {
            let item = SequenceItem {
                look: child,
                width: 1,
                transition: 100,
                ..Default::default()
            };
            sender
                .send(Command::PutSequence {
                    look,
                    name: text("linked"),
                    sequence: Sequence::new(&[item, item]).unwrap(),
                })
                .unwrap();
        }
        sender
            .send(Command::StartUse {
                look: 3,
                rank: Rank::Manual,
                when: engine::When::Now,
                usage: Use {
                    repeat: Repeat::Once,
                    ..Default::default()
                },
            })
            .unwrap();
        sender.send(step(12345)).unwrap();
    });
    mock::wait_until("the linked sequence finished and kept its sample", || {
        engine
            .latest()
            .state
            .plays
            .iter()
            .any(|p| p.look == 3 && p.state == engine::PlayState::Kept)
    });
    // Sample a moving Wave on the two ends of the strip while counting.
    // Both swatches carry the same green, so every phase identifies this source.
    counting::uncounted(|| {
        use engine_core::{Palette, Pattern, Swatches, Use, WaveTarget};
        let swatches = Swatches::new(&[
            Rgb {
                red: u16::MAX,
                green: 4321,
                blue: 0,
            },
            Rgb {
                red: 0,
                green: 4321,
                blue: u16::MAX,
            },
        ])
        .unwrap();
        sender
            .send(Command::PutPattern {
                look: 4,
                name: text("wave"),
                pattern: Pattern::Wave {
                    target: WaveTarget::Group {
                        group: text("ends"),
                    },
                    period: 2,
                    palette: Palette::Inline(swatches),
                },
            })
            .unwrap();
        sender
            .send(Command::StartUse {
                look: 4,
                rank: Rank::Manual,
                when: engine::When::Now,
                usage: Use {
                    spread: 100,
                    ..Use::default()
                },
            })
            .unwrap();
    });
    mock::wait_until("both strip pixels receive Wave", || {
        let lights = engine.latest().lights();
        [usize::from(LIGHTS), usize::from(LIGHTS) + 2]
            .iter()
            .all(|&index| lights[index].colour.is_some_and(|rgb| rgb.green == 4321))
    });
    // a Manual play live as all is released
    let start = Command::StartLook {
        look: 1,
        rank: Rank::Manual,
        when: engine::When::Now,
    };
    assert!(
        matches!(
            begin(&mut sender, start, &library),
            Ok(Begun::Started { .. })
        ),
        "look 1 plays"
    );
    mock::wait_until("the Manual play never started", || {
        engine
            .latest()
            .state
            .plays
            .iter()
            .any(|play| play.rank == Rank::Manual)
    });
    // every Manual play, direct hold and raw channel let go: the plans go back
    assert!(
        send(&mut sender, Command::ReleaseAll, &library),
        "all is released"
    );
    let seen = engine.latest().state.frame;
    mock::wait_until("no frame went by", || {
        engine.latest().state.frame > seen + 1
    });
    assert!(
        engine
            .latest()
            .state
            .plays
            .iter()
            .all(|play| play.rank != Rank::Manual),
        "no Manual play is left"
    );
    let ticks = state.frame - frame;
    let new_allocations = counting::allocations() - allocations - attaching;
    let new_tick_frees = counting::tick_frees() - tick_frees;
    drop(engine);

    assert!(ticks >= 5, "too few ticks to prove anything: {ticks}");
    assert!(launched, "no play waiting for a beat launched");
    assert!(
        ticks >= 256,
        "a frame went by for each of 256 commands: {ticks}"
    );
    assert_eq!(
        lit,
        usize::from(LIGHTS) + 100,
        "every patched light is held lit"
    );
    assert_eq!(
        green_lights,
        even.len(),
        "every light of the group is green"
    );
    assert_eq!(
        state
            .plays
            .iter()
            .filter(|p| p.rank == Rank::Manual)
            .map(|p| p.play)
            .collect::<Vec<_>>(),
        play.into_iter().collect::<Vec<_>>(),
        "the last Manual play started and not let go still plays"
    );
    let accepted = state.outputs.map(|o| o.map(|o| o.counters.accepted));
    assert!(
        accepted[..2].iter().all(|n| n.is_some_and(|n| n > 5)),
        "both outputs were served: {accepted:?}"
    );
    assert_eq!(
        new_allocations, 0,
        "{ticks} ticks made {new_allocations} allocations"
    );
    assert_eq!(
        new_tick_frees, 0,
        "{ticks} ticks made {new_tick_frees} frees on the tick thread"
    );
    #[cfg(feature = "test-support")]
    linked_ends();
}

/// With reclamation held, a free anywhere (including on either output thread) fails.
#[cfg(feature = "test-support")]
fn output_health() {
    use engine::{Device, Outcome, OutputNumber, Route, Span, Status};
    use engine_core::health::HealthSample;
    use std::time::Duration;

    struct UnplugOnOne;
    impl Device for UnplugOnOne {
        fn take(&mut self, frame: &[u8]) -> Outcome {
            if frame[0] == 1 {
                Outcome::Failed
            } else {
                Outcome::Accepted
            }
        }
    }

    let universes = Universes::new(2).unwrap();
    let (mut engine, mut sender, wakes) = Engine::start_stepped(
        universes,
        Box::new(|_| Ok(Box::new(UnplugOnOne) as Box<dyn Device>)),
    )
    .unwrap();
    wakes.hold().unwrap();
    engine.stall_reclamation(true);
    let one = OutputNumber::try_from(1).unwrap();
    let kind = engine_core::TEST_STREAM_KIND;
    let spans = [Span { start: 0, len: 2 }, Span { start: 512, len: 1 }];
    let channel = ChannelNumbers {
        universe: 1,
        address: 1,
    };
    let counts = || (counting::allocations(), counting::frees());
    let mut sample = HealthSample::new(Duration::ZERO, 0, Status::NotFound);
    for generation in 1..=2 {
        // The front makes the lane and its boxes. Count from before the tick applies it.
        sender
            .attach_route(
                one,
                Box::new(UnplugOnOne),
                Route::new(outputs::limits(kind), &spans).unwrap(),
                Rate::Sixty,
            )
            .unwrap();
        let before = counts();
        let mut initial_presses = 0;
        for (value, outages) in [(0, 0), (1, 1), (0, 1)] {
            sender.send(Command::SetChannel { channel, value }).unwrap();
            mock::wait_until("the output did not take the new level", || {
                engine.latest();
                wakes.wake().unwrap();
                let snapshot = engine.latest();
                snapshot.applied() == sender.sent()
                    && snapshot.state.outputs[one.slot()].is_some_and(|o| {
                        if value == 1 {
                            o.counters.failed >= 2
                        } else {
                            o.status == Status::Running
                        }
                    })
            });
            assert!(engine.latest().health_sample(one, &mut sample));
            assert_eq!((sample.generation, sample.outages), (generation, outages));
            let presses = sample.press.iter().sum::<u64>();
            match (value, outages) {
                (0, 0) => initial_presses = presses,
                (1, _) => assert_eq!(presses, initial_presses),
                _ => assert_eq!(presses, initial_presses + 2),
            }
        }
        assert_eq!(
            counts(),
            before,
            "allocation or free during output health service"
        );
    }

    // A reconfigure that keeps the lane, with a frame of the old generation in flight.
    let two = OutputNumber::try_from(2).unwrap();
    let set_two = |first, count| Command::SetOutput {
        output: two,
        kind,
        universes: RunNumbers { first, count },
        rate: Some(Rate::Sixty),
    };
    let Begun::Opening(opening) = begin(&mut sender, set_two(1, 1), &[]).unwrap() else {
        panic!("the new device needs an opening");
    };
    let opened = opening.open();
    let before = counts();
    sender.finish(opened).unwrap();
    assert_eq!(
        counts(),
        before,
        "publishing a ready lane allocated or freed"
    );
    let runs = |engine: &mut Engine, sender: &Sender, generation, presses| {
        let mut sample = HealthSample::new(Duration::ZERO, 0, Status::NotFound);
        mock::wait_until("output 2 did not run", || {
            wakes.wake().unwrap();
            let snapshot = engine.latest();
            snapshot.applied() == sender.sent()
                && snapshot.health_sample(two, &mut sample)
                && (sample.generation, sample.status) == (generation, Status::Running)
                && sample.press.iter().sum::<u64>() == presses
        });
    };
    runs(&mut engine, &sender, 1, 0);
    let before = counts();
    wakes.wake().unwrap(); // hands a frame of generation 1
    assert!(
        send(&mut sender, set_two(2, 1), &[]),
        "output 2 moves, same device"
    );
    sender
        .send(Command::SetChannel { channel, value: 0 })
        .unwrap();
    runs(&mut engine, &sender, 2, 1);
    assert_eq!(
        counts(),
        before,
        "allocation or free during a reconfigure that keeps the lane"
    );
    // Retire the lane with the free thread stalled. The only work in this
    // window is the tick handing the old lane back and serving the routed mirror.
    let restart = begin(&mut sender, set_two(1, 2), &[]).unwrap();
    let Begun::Opening(restart) = restart else {
        panic!("a longer route needs new boxes");
    };
    let before = counts();
    wakes.wake().unwrap();
    assert_eq!(counts(), before, "retiring the lane allocated or freed");

    engine.stall_reclamation(false);
    let opened = restart.open(); // the worker joins one thread and starts another
    let before = counts();
    sender.finish(opened).unwrap();
    runs(&mut engine, &sender, 4, 0);
    assert_eq!(
        counts(),
        before,
        "installing or serving the restarted lane allocated or freed"
    );
}

/// Queue two replacement revisions while every once-use crosses its end between wakes.
#[cfg(feature = "test-support")]
fn linked_ends() {
    use engine_core::{PLAYS, PlayState, Repeat, Sequence, SequenceItem, Use, When};
    use std::sync::Arc;
    use std::time::Duration;

    let (mut engine, mut sender, wakes) =
        Engine::start_stepped(Universes::new(1).unwrap(), Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let name = text("once");
    let strip = Target::Fixture { fixture: 1 };
    let leaf = |intensity, target| Command::PutStep {
        look: 0,
        name,
        step: Step {
            target,
            values: Light {
                intensity: Some(intensity),
                ..Light::DARK
            },
        },
    };
    let start = |sender: &mut Sender, slot: usize, rank| match begin(
        sender,
        Command::StartUse {
            look: slot as u16 + 1,
            rank,
            when: When::Now,
            usage: Use {
                repeat: Repeat::Once,
                ..Default::default()
            },
        },
        &[],
    ) {
        Ok(Begun::Started { play }) => play,
        _ => panic!("once-use must start"),
    };
    sender
        .send(Command::PatchStrip {
            name,
            pixels: Pixels::try_from(93).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    sender.send(leaf(0, strip)).unwrap();
    let sequence = Sequence::new(&[SequenceItem::default()]).unwrap();
    for look in 1..=PLAYS as u16 {
        sender
            .send(Command::PutSequence {
                look,
                name,
                sequence,
            })
            .unwrap();
    }
    // Warm the worker threads and wait for a complete return trip before counting.
    let old = Arc::downgrade(&sender.looks().plan(1).unwrap());
    let prime = start(&mut sender, 0, Rank::Manual);
    sender.send(Command::ReleaseHold { play: prime }).unwrap();
    sender.send(leaf(1, strip)).unwrap();
    let mut at = wakes.wake().unwrap();
    mock::wait_until("the priming plan was never reclaimed", || {
        old.strong_count() == 0
    });

    // Each one-item root reserves two samples of eleven visits per pixel.
    // This is the largest common strip all 32 of these sequences can play.
    let root_work = looks::play_work(sender.looks().plan(1).unwrap().work());
    assert!(root_work * PLAYS <= looks::WORK_LIMIT);
    assert!(root_work * PLAYS + (root_work / 93) * PLAYS > looks::WORK_LIMIT);
    let counters = || (counting::allocations(), counting::tick_frees());
    let before = counters();
    let mut previous = [prime; PLAYS];
    let mut value = 1;
    for (rank, release_first) in [
        (Rank::Manual, false),
        (Rank::Show, true),
        (Rank::Show, false),
    ] {
        let ids: [PlayId; PLAYS] = std::array::from_fn(|slot| start(&mut sender, slot, rank));
        engine.latest();
        wakes.wake_at(at).unwrap();
        let playing = &engine.latest().state.plays;
        assert_eq!(playing.iter().count(), PLAYS);
        assert!(
            playing
                .iter()
                .all(|p| p.state == PlayState::Playing && ids.contains(&p.play))
        );
        if rank == Rank::Show {
            for (fresh, old) in ids.iter().zip(previous) {
                assert_eq!(fresh.slot, old.slot);
                assert_ne!(fresh.generation, old.generation);
            }
        }
        let sent = sender.sent();
        let extra_root = Command::StartLook {
            look: 0,
            rank,
            when: When::Now,
        };
        assert!(matches!(
            begin(&mut sender, extra_root, &[]),
            Err(Refusal::PlayFull)
        ));
        let too_large = Command::SetIntensity {
            target: strip,
            intensity: u16::MAX,
        };
        assert!(matches!(
            begin(&mut sender, too_large, &[]),
            Err(Refusal::WorkLimit)
        ));
        assert_eq!(sender.sent(), sent);
        // Stall retirement on the last pass, before its swaps and all 32 ends.
        let stall = rank == Rank::Show && !release_first;
        if stall {
            engine.stall_reclamation(true);
            assert!(send(&mut sender, leaf(value + 3, strip), &[]));
            engine.latest();
            wakes.wake_at(at).unwrap();
            let live = engine.latest();
            assert_eq!(live.state.plays.iter().count(), PLAYS);
            assert!(
                live.lights()[..93]
                    .iter()
                    .all(|light| light.intensity == Some(value + 3)),
                "the replacement lights every pixel while the 32 plays are still running"
            );
        }
        let sent = sender.sent();
        at += Duration::from_secs(2);
        wakes.set_time(at);
        for next in [value + 1, value + 2] {
            assert!(send(&mut sender, leaf(next, strip), &[]));
        }
        // Let the front forget one Show generation before the queued swaps arrive.
        // Only the tick now owns its superseded middle plan.
        if release_first {
            assert!(send(
                &mut sender,
                Command::ReleaseHold { play: ids[0] },
                &[]
            ));
        }
        // The two revisions queue one swap per root. Fill the remaining FIFO
        // exactly; a refused blackout must not replace the last accepted one.
        for _ in (sender.sent() - sent) as usize..engine::QUEUE {
            assert!(send(&mut sender, Command::Blackout { on: false }, &[]));
        }
        let full = sender.sent();
        assert!(matches!(
            begin(&mut sender, Command::Blackout { on: true }, &[]),
            Err(Refusal::QueueFull)
        ));
        assert_eq!(sender.sent(), full);
        engine.latest();
        wakes.wake_at(at).unwrap();
        let snapshot = engine.latest();
        assert_eq!(snapshot.applied(), full);
        if rank == Rank::Manual {
            assert_eq!(snapshot.state.plays.iter().count(), PLAYS);
            assert!(
                snapshot
                    .state
                    .plays
                    .iter()
                    .all(|p| p.state == PlayState::Kept)
            );
            assert!(
                snapshot.lights()[..93]
                    .iter()
                    .all(|l| l.intensity == Some(value))
            );
            for play in ids {
                assert!(send(&mut sender, Command::ReleaseHold { play }, &[]));
            }
        } else {
            assert_eq!(snapshot.state.plays.iter().count(), 0);
            assert!(
                snapshot.lights()[..93]
                    .iter()
                    .all(|l| l.intensity.is_none())
            );
        }
        if stall {
            engine.stall_reclamation(false);
        }
        previous = ids;
        value += 2;
    }
    // Shrink the plans to one pixel before testing the independent retirement
    // slot limit. All generations that ended must be reusable.
    assert!(send(
        &mut sender,
        leaf(value, Target::Light { light: 1 }),
        &[]
    ));
    for (slot, old) in previous.iter().enumerate() {
        let fresh = start(&mut sender, slot, Rank::Show);
        assert_eq!(fresh.slot, old.slot);
        assert_ne!(fresh.generation, old.generation);
    }
    wakes.wake_at(at).unwrap();
    assert_eq!(
        counters(),
        before,
        "linked ends allocated or freed on the tick"
    );
    // Each live play holds one return place. With the tick paused, every restart
    // reserves another until these commands fill all the remaining places.
    let restart = Command::StartLook {
        look: 1,
        rank: Rank::Show,
        when: When::Now,
    };
    for _ in PLAYS..engine::QUEUE + engine::MAX_OUTPUTS {
        mock::wait_until(
            "a once-end plan swap never returned its place",
            || match sender.send(restart) {
                Ok(()) => true,
                Err(Refusal::QueueFull) => false,
                Err(reason) => panic!("restart refused: {reason:?}"),
            },
        );
    }
    assert_eq!(sender.send(restart), Err(Refusal::QueueFull));
    wakes.run();
}
