//! Remaining wire paths in docs/design/output-cutover.md. The preparing path (T1,
//! T2, T3, T4) and a publication during a wake (T5) use the gated opens in routes.rs.
//! The review_248 targets cover T8 to T11, including a return before the cutoff.
mod common;
#[path = "../../outputs/tests/common/stream.rs"]
mod stream;

use engine::{Color, Device, Engine, Outcome, OutputKind, Universes};
use serde_json::{Value, json};
use std::io;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

const GUARD: Duration = Duration::from_secs(10);

struct Drain {
    device: Box<dyn Device>,
    live: Arc<AtomicUsize>,
    gate: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
}

impl Drop for Drain {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::Release);
    }
}

impl Device for Drain {
    fn take(&mut self, bytes: &[u8]) -> Outcome {
        self.device.take(bytes)
    }

    fn finish_route(&mut self) {
        if let Some((entered, release)) = self.gate.take() {
            let _ = entered.send(());
            let _ = release.recv_timeout(GUARD);
        }
        self.device.finish_route();
    }
}

fn route(kind: OutputKind, first: u16, count: u16) -> Value {
    json!({"SetOutput": {
        "output": 1, "kind": kind, "universes": {"first": first, "count": count}, "rate": "60"
    }})
}

fn press(universe: u16, value: u8) -> Value {
    json!({"SetChannel": {"channel": {"universe": universe, "address": 1}, "value": value}})
}

fn read(wire: &UdpSocket, slots: u16) -> u8 {
    let mut packet = [0; 1500];
    assert_eq!(wire.recv(&mut packet).unwrap(), 10 + usize::from(slots));
    assert_eq!(packet[2..8], [0x0B, 1, 0, 0, 0, 0]);
    assert_eq!(packet[8..10], slots.to_be_bytes());
    packet[10]
}

fn empty(wire: &UdpSocket) {
    wire.set_nonblocking(true).unwrap();
    let result = wire.recv(&mut [0; 1500]);
    wire.set_nonblocking(false).unwrap();
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::WouldBlock);
}

#[derive(Clone, Copy)]
enum DuringDrain {
    EditAgain,
    Stop,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_next_open_waits_for_the_previous_cutover_to_be_visible() {
    walk(DuringDrain::EditAgain).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_discards_the_held_frame_and_answers_the_waiting_writer() {
    walk(DuringDrain::Stop).await;
}

/// T6/T7. More due frames than the lane has boxes arrive during the drain. The newest
/// must reach the wire on T8 with the clock still held. T13 discards them all.
async fn walk(action: DuringDrain) {
    let (wire, kind) = stream::receiver(Color::Rgb);
    let (third_wire, third_kind) = stream::receiver(Color::Rgb);
    let (opening, third_opened) = mpsc::channel();
    let (entered, draining) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut gate = Some((entered, wait));
    let live = Arc::new(AtomicUsize::new(0));
    let live_on_open = Arc::clone(&live);
    let (engine, sender, wakes) = Engine::start_stepped(
        Universes::new(3).unwrap(),
        Box::new(move |requested| {
            if requested == third_kind {
                let _ = opening.send(());
            }
            let device = outputs::open(requested)?;
            live_on_open.fetch_add(1, Ordering::Release);
            Ok(Box::new(Drain {
                device,
                live: Arc::clone(&live_on_open),
                gate: gate.take(),
            }))
        }),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(engine_server::serve(listener, engine, sender, None, None));
    let mut writer = common::connect(address).await;
    let mut second = common::connect(address).await;
    let mut performer = common::connect(address).await;
    wakes.hold().unwrap();
    for (universe, level) in [(1, 11), (2, 61), (3, 93)] {
        assert_eq!(
            common::command(&mut performer, press(universe, level)).await,
            "Accepted"
        );
    }
    assert_eq!(
        common::command(&mut writer, route(kind, 1, 1)).await,
        "Accepted"
    );
    let at = wakes.wake().unwrap();
    assert_eq!(read(&wire, 510), 11);
    common::send(
        &mut writer,
        &json!({"Send": {"command": route(kind, 2, 2)}}),
    )
    .await;
    tokio::time::timeout(GUARD, async {
        while draining.try_recv().is_err() {
            wakes.wake_at(at).unwrap();
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("old device enters finish_route");

    for level in 62..=67 {
        assert_eq!(
            common::command(&mut performer, press(2, level)).await,
            "Accepted"
        );
        wakes.wake().unwrap();
    }
    empty(&wire);

    match action {
        DuringDrain::EditAgain => {
            // The pixel crosses a universe boundary that only the pending
            // output streams. It cannot appear in a picture of the old run.
            let pixel = json!({"PatchStrip": {
                "name": "Edge", "pixels": 1, "color": "Rgb", "at": {"universe": 2, "address": 512}
            }});
            assert_eq!(
                common::command(&mut performer, pixel).await,
                json!({"Refused": {"reason": {"NotStreamed": {"first": 2, "last": 3}}}})
            );
            // T12: begin before T8, then keep the clock held across Accepted.
            common::send(
                &mut second,
                &json!({"Send": {"command": route(third_kind, 3, 1)}}),
            )
            .await;
            let premature =
                tokio::task::block_in_place(|| third_opened.recv_timeout(Duration::from_secs(1)));
            assert_eq!(premature, Err(mpsc::RecvTimeoutError::Timeout));
            drop(release);
            assert_eq!(common::answer(&mut writer).await, "Accepted");
            assert_eq!(
                read(&wire, 1023),
                67,
                "the newest held frame goes without a wake"
            );
            empty(&wire);
            let premature =
                tokio::task::block_in_place(|| third_opened.recv_timeout(Duration::from_secs(1)));
            assert_eq!(
                premature,
                Err(mpsc::RecvTimeoutError::Timeout),
                "a second open preceded T9"
            );
            // T9 makes the second route visible. Only now may the third open.
            wakes.wake().unwrap();
            assert_eq!(read(&wire, 1023), 67);
            assert_eq!(common::answer(&mut second).await, "Accepted");
            third_opened.recv_timeout(GUARD).unwrap();
            wakes.wake().unwrap();
            assert_eq!(read(&third_wire, 510), 93);
            common::send(&mut performer, &json!("Stop")).await;
            assert_eq!(common::answer(&mut performer).await, "Stopping");
        }
        DuringDrain::Stop => {
            // T13: the new worker has a queued frame. It cannot send on release.
            common::send(&mut second, &json!("Stop")).await;
            assert_eq!(common::answer(&mut second).await, "Stopping");
            let answer =
                tokio::time::timeout(Duration::from_secs(2), common::answer(&mut writer)).await;
            drop(release); // let cleanup run even when the writer failed to answer
            assert_eq!(
                answer.expect("stop answers while the adapter is still draining"),
                "Stopping"
            );
        }
    }
    drop(
        tokio::time::timeout(GUARD, server)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    );
    empty(&wire);
    empty(&third_wire);
    tokio::time::timeout(GUARD, async {
        while live.load(Ordering::Acquire) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("every opened adapter returned and was freed");
}

/// A failed open (T4) registers recovery on the old lane. Its retry can finish
/// after T8: the reused lane then needs its own recovery and sends on its new run.
#[test]
fn an_old_retry_in_flight_at_cutover_recovers_the_reused_adapter() {
    use engine::{Begun, ChannelNumbers, Command, Rate, Refusal, RunNumbers};

    struct LostAfterFirst(Box<dyn Device>, bool);
    impl Device for LostAfterFirst {
        fn take(&mut self, bytes: &[u8]) -> Outcome {
            if std::mem::replace(&mut self.1, true) {
                Outcome::Failed
            } else {
                self.0.take(bytes)
            }
        }
    }

    let (wire, kind) = stream::receiver(Color::Rgb);
    let (retry, attempts) = mpsc::channel();
    let (release, wait) = mpsc::channel::<()>();
    let mut attempt = 0;
    let (engine, mut sender, wakes) = Engine::start_stepped(
        Universes::new(2).unwrap(),
        Box::new(move |kind| {
            attempt += 1;
            match attempt {
                1 => Ok(Box::new(LostAfterFirst(outputs::open(kind)?, false))),
                2 => Err(io::ErrorKind::NotFound.into()),
                _ => {
                    retry.send(attempt).unwrap();
                    if attempt == 3 {
                        let _ = wait.recv_timeout(GUARD);
                    }
                    outputs::open(kind)
                }
            }
        }),
    )
    .unwrap();
    wakes.hold().unwrap();
    let set = |first| Command::SetOutput {
        output: 1.try_into().unwrap(),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    for (universe, value) in [(1, 11), (2, 93)] {
        sender
            .send(Command::SetChannel {
                channel: ChannelNumbers {
                    universe,
                    address: 1,
                },
                value,
            })
            .unwrap();
    }
    sender.send(set(1)).unwrap();
    wakes.wake().unwrap();
    assert_eq!(read(&wire, 510), 11);
    // Force a failed fresh open, then a failed old send and a blocked retry.
    engine.stall_reclamation(true);
    assert_eq!(sender.send(set(2)), Err(Refusal::CannotOpen));
    let at = wakes.wake().unwrap();
    assert_eq!(attempts.recv_timeout(GUARD).unwrap(), 3);
    engine.stall_reclamation(false);

    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("the shifted run needs a replacement lane");
    };
    wakes.wake_at(at).unwrap();
    sender.finish(opening.open()).unwrap();
    drop(release);
    wakes.wake().unwrap();
    assert_eq!(
        attempts.recv_timeout(GUARD).unwrap(),
        4,
        "the new lane inherits the failed adapter and its recovery"
    );
    // The fourth open signalled before installation, so step a wake until the
    // real socket receives a frame. A stale delivery to the old run would be 11.
    wire.set_nonblocking(true).unwrap();
    let started = std::time::Instant::now();
    let mut packet = [0; 1500];
    loop {
        wakes.wake().unwrap();
        match wire.recv(&mut packet) {
            Ok(n) => {
                assert_eq!((n, packet[10]), (520, 93));
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("DDP read: {e}"),
        }
        assert!(
            started.elapsed() < GUARD,
            "the reused adapter did not return"
        ); // guard
    }
}

#[test]
fn a_removal_after_acceptance_cannot_bring_back_the_accepted_lane() {
    use engine::{Begun, ChannelNumbers, Command, Event, Refusal, RunNumbers};
    let (wire, kind) = stream::receiver(Color::Rgb);
    let (mut engine, mut sender, mut wakes) =
        Engine::start_stepped(Universes::new(2).unwrap(), Box::new(outputs::open)).unwrap();
    let output = 1.try_into().unwrap();
    let set = |first| Command::SetOutput {
        output,
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: None,
    };
    let remove = Command::RemoveOutput { output };
    wakes.hold().unwrap();
    for (universe, value) in [(1, 11), (2, 93)] {
        sender
            .send(Command::SetChannel {
                channel: ChannelNumbers {
                    universe,
                    address: 1,
                },
                value,
            })
            .unwrap();
    }
    sender.send(set(1)).unwrap();
    let at = wakes.wake().unwrap();
    assert_eq!(read(&wire, 510), 11);
    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("a shifted run needs a new lane");
    };
    wakes.wake_at(at).unwrap();
    sender.finish(opening.open()).unwrap(); // T8 before T9 with the clock held.

    // A caller sharing a front can retry Opening after T9. A removal which
    // already got Ok must also be reflected by the very next snapshot.
    let removed = sender.send(remove);
    engine.latest();
    wakes.wake().unwrap();
    if removed == Err(Refusal::Opening) {
        assert_eq!(
            engine.latest().state.outputs[0].unwrap().universes.first(),
            2
        );
        assert_eq!(read(&wire, 510), 93);
        sender.send(remove).unwrap();
        engine.latest();
        wakes.wake().unwrap();
    } else {
        assert_eq!(removed, Ok(()));
    }
    assert_eq!(engine.latest().state.outputs[0], None);
    wakes.events();
    wakes.wake().unwrap();
    assert!(!wakes.events().iter().any(|event| matches!(event,
        Event::Handed { output: number, .. } if *number == output
    )));
    empty(&wire);
}

/// T1 depends on replacing the lane even when both adapters use identical spans.
#[test]
fn a_fresh_adapter_on_the_same_route_waits_for_the_old_frames_and_drain() {
    use engine::{ChannelNumbers, Command, Rate, Route, Span};

    #[derive(Debug, PartialEq)]
    enum Stage {
        Draining,
        Accepted,
    }

    struct Old {
        device: Box<dyn Device>,
        write: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
        drain: Option<(mpsc::Sender<Stage>, mpsc::Receiver<()>)>,
    }

    impl Device for Old {
        fn take(&mut self, bytes: &[u8]) -> Outcome {
            if let Some((entered, release)) = self.write.take() {
                entered.send(()).unwrap();
                release.recv_timeout(GUARD).unwrap();
            }
            self.device.take(bytes)
        }

        fn finish_route(&mut self) {
            if let Some((stage, release)) = self.drain.take() {
                let _ = stage.send(Stage::Draining);
                let _ = release.recv_timeout(GUARD);
            }
            self.device.finish_route();
        }
    }

    let (old_wire, old_kind) = stream::receiver(Color::Rgb);
    let (new_wire, new_kind) = stream::receiver(Color::Rgb);
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(Universes::ONE, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let (entered, writing) = mpsc::channel();
    let (send_old, release_write) = mpsc::channel();
    let (stage, observed) = mpsc::channel();
    let (drained, release_drain) = mpsc::channel();
    let output = 1.try_into().unwrap();
    let route = || Route::new(outputs::Limits::RAW, &[Span { start: 0, len: 3 }]).unwrap();
    let set = |value| Command::SetChannel {
        channel: ChannelNumbers {
            universe: 1,
            address: 1,
        },
        value,
    };
    sender.send(set(11)).unwrap();
    sender
        .attach_route(
            output,
            Box::new(Old {
                device: outputs::open(old_kind).unwrap(),
                write: Some((entered, release_write)),
                drain: Some((stage.clone(), release_drain)),
            }),
            route(),
            Rate::Sixty,
        )
        .unwrap();
    let at = wakes.wake().unwrap();
    writing.recv_timeout(GUARD).unwrap();
    sender.send(set(73)).unwrap();
    wakes.wake_at(at + Rate::Sixty.period()).unwrap();
    // The clock stays held through T8. The old adapter reports when both writes
    // are complete and keeps its drain open until the test has read the wire.
    std::thread::scope(|threads| {
        let sender = &mut sender;
        threads.spawn(move || {
            sender
                .attach_route(
                    output,
                    outputs::open(new_kind).unwrap(),
                    route(),
                    Rate::Sixty,
                )
                .unwrap();
            stage.send(Stage::Accepted).unwrap();
        });
        send_old.send(()).unwrap();
        let first = observed.recv_timeout(GUARD).unwrap();
        if first == Stage::Draining {
            assert_eq!([read(&old_wire, 3), read(&old_wire, 3)], [11, 73]);
            empty(&new_wire);
        }
        drop(drained);
        assert_eq!(
            first,
            Stage::Draining,
            "writer returned before the old adapter drained"
        );
        assert_eq!(observed.recv_timeout(GUARD).unwrap(), Stage::Accepted);
    });
    sender.send(set(91)).unwrap();
    wakes.wake().unwrap();
    assert_eq!(read(&new_wire, 3), 91);
    empty(&old_wire);
}

/// T7 to T9. The held lane counts how late each frame it held was handed: one a newer
/// frame replaced until the tick replaced it, the newest until T8 sent it. The new run
/// shows those counts from T9 on.
#[test]
fn the_new_run_shows_how_late_its_held_frames_were() {
    use engine::{Begun, Command, Event, OutputNumber, Rate, RunNumbers, Status};
    use engine_core::health::HealthSample;

    let (wire, kind) = stream::receiver(Color::Rgb);
    let (mut engine, mut sender, mut wakes) =
        Engine::start_stepped(Universes::new(2).unwrap(), Box::new(outputs::open)).unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let set = |first| Command::SetOutput {
        output,
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    wakes.hold().unwrap();
    sender.send(set(1)).unwrap();
    let at = wakes.wake().unwrap();
    read(&wire, 510);
    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("the shifted run needs a new lane");
    };
    wakes.wake_at(at).unwrap();
    let handoff = sender.publish_output(opening.open()).unwrap().unwrap();
    wakes.events();
    // T6 and T7: the new lane holds a frame, then one 100 ms past the first's due.
    let first = wakes.wake().unwrap();
    let replaced = wakes.wake_at(first + Duration::from_millis(100)).unwrap();
    let held: Vec<_> = wakes
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Handed {
                output: number,
                due,
                ..
            } if number == output => Some(due),
            Event::Handed { .. } | Event::Came { .. } | Event::Counted { .. } => None,
        })
        .collect();
    let [older, newest] = held[..] else {
        panic!("one held frame per wake: {held:?}");
    };
    // T8 sends the newest held frame 300 ms after the wake that held it.
    let sent = replaced + Duration::from_millis(300);
    wakes.set_time(sent);
    assert!(handoff.wait());
    read(&wire, 510);

    let mut sample = HealthSample::new(Duration::ZERO, 0, Status::NotFound);
    engine.latest();
    wakes.wake().unwrap(); // T9
    assert!(engine.latest().health_sample(output, &mut sample));
    assert_eq!(
        (sample.lateness.iter().sum::<u64>(), sample.lateness_worst),
        (1, replaced - older),
        "the replaced frame, late until the wake that replaced it"
    );
    let started = std::time::Instant::now();
    // A later wake collects T8's receipt.
    while sample.lateness_worst != sent - newest {
        let late = started.elapsed() > GUARD; // guard
        assert!(!late, "the newest held frame, late until T8 sent it");
        engine.latest();
        wakes.wake().unwrap();
        assert!(engine.latest().health_sample(output, &mut sample));
    }
}

/// T9 counts the presses of the switching output's old frames on the old run, before
/// the new run shows. Every other output's frames are counted after the wake's
/// hand-overs, so output 2's count comes first although output 1 is served first.
#[test]
fn t9_counts_its_own_outputs_old_frames_before_any_other() {
    use engine::{Begun, Command, Event, OutputNumber, Rate, RunNumbers};

    let wires = [stream::receiver(Color::Rgb), stream::receiver(Color::Rgb)];
    let (_engine, mut sender, mut wakes) =
        Engine::start_stepped(Universes::new(3).unwrap(), Box::new(outputs::open)).unwrap();
    let set = |number: u8, first| Command::SetOutput {
        output: OutputNumber::try_from(number).unwrap(),
        kind: wires[usize::from(number) - 1].1,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    let both = || {
        for (wire, _) in &wires {
            read(wire, 510);
        }
    };
    wakes.hold().unwrap();
    sender.send(set(1, 1)).unwrap();
    sender.send(set(2, 2)).unwrap();
    let at = wakes.wake().unwrap();
    both();
    let Begun::Opening(opening) = sender.begin(set(2, 3), &[]).unwrap() else {
        panic!("the shifted run needs a new lane");
    };
    wakes.wake_at(at).unwrap();
    let opened = opening.open();
    // Both old lanes send an accepted frame that only T9's wake collects.
    wakes.wake().unwrap();
    both();
    assert!(sender.publish_output(opened).unwrap().unwrap().wait());
    wakes.events();
    wakes.wake().unwrap(); // T9
    let counted: Vec<_> = wakes
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Counted { output } => Some(output),
            Event::Handed { .. } | Event::Came { .. } => None,
        })
        .collect();
    let number = |n: u8| OutputNumber::try_from(n).unwrap();
    assert_eq!(counted, [number(2), number(1)]);
}

/// T12. An open begun after T8 waits off the tick until T9 shows the previous switch,
/// then starts from it (T1). With no T9 within its bound it is refused and opens nothing.
#[test]
fn an_open_begun_before_t9_waits_for_it_or_is_refused() {
    use engine::{Begun, Command, OutputNumber, Rate, Refusal, RunNumbers};

    // (T9 comes while the open waits, devices opened for the third route, the answer)
    for (shown, opened, answer) in [(true, 1, Ok(())), (false, 0, Err(Refusal::CannotOpen))] {
        let (wire, kind) = stream::receiver(Color::Rgb);
        let (_third_wire, third_kind) = stream::receiver(Color::Rgb);
        let opens = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&opens);
        let (_engine, mut sender, wakes) = Engine::start_stepped(
            Universes::new(2).unwrap(),
            Box::new(move |requested| {
                if requested == third_kind {
                    counted.fetch_add(1, Ordering::Release);
                }
                outputs::open(requested)
            }),
        )
        .unwrap();
        let output = OutputNumber::try_from(1).unwrap();
        let set = |kind, first| Command::SetOutput {
            output,
            kind,
            universes: RunNumbers { first, count: 1 },
            rate: Some(Rate::Sixty),
        };
        wakes.hold().unwrap();
        sender.send(set(kind, 1)).unwrap();
        let at = wakes.wake().unwrap();
        read(&wire, 510);
        let Begun::Opening(second) = sender.begin(set(kind, 2), &[]).unwrap() else {
            panic!("the shifted run needs a new lane");
        };
        wakes.wake_at(at).unwrap();
        sender.finish(second.open()).unwrap(); // T8 with the clock held
        let Begun::Opening(third) = sender.begin(set(third_kind, 1), &[]).unwrap() else {
            panic!("another kind needs a new lane");
        };
        let (answer_to, open) = mpsc::channel();
        std::thread::spawn(move || answer_to.send(third.open()));
        if shown {
            wakes.wake().unwrap(); // T9
        }
        let third = open
            .recv_timeout(GUARD)
            .expect("the open ends within its bound");
        assert!(sender.cutover_pending(output), "shown {shown}");
        assert_eq!(opens.load(Ordering::Acquire), opened, "shown {shown}");
        assert_eq!(sender.finish(third), answer, "shown {shown}");
    }
}

/// A patch while a switch to an explicit route awaits T9 must fit the old runs too. An
/// explicit route's bounds never guarantee a stream, so neither does the run it replaces;
/// output 2's pending run does not stream until its own T9.
#[test]
fn a_pending_explicit_route_streams_no_patched_fixture_until_t9() {
    use engine::{
        Begun, ChannelNumbers, Command, OutputNumber, Rate, Refusal, Route, RunNumbers, Span, Text,
    };
    use engine_core::Pixels;

    let (_wire, kind) = stream::receiver(Color::Rgb);
    let (_second_wire, second_kind) = stream::receiver(Color::Rgb);
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(Universes::new(3).unwrap(), Box::new(outputs::open)).unwrap();
    let set = |number: u8, kind, first| Command::SetOutput {
        output: OutputNumber::try_from(number).unwrap(),
        kind,
        universes: RunNumbers { first, count: 2 },
        rate: Some(Rate::Sixty),
    };
    let edge = || Command::PatchStrip {
        name: Text::new("Edge").unwrap(),
        pixels: Pixels::try_from(1).unwrap(),
        color: Color::Rgb,
        at: ChannelNumbers {
            universe: 1,
            address: 511,
        },
    };
    wakes.hold().unwrap();
    sender.send(set(1, kind, 1)).unwrap();
    sender.send(set(2, second_kind, 2)).unwrap();
    let at = wakes.wake().unwrap();
    // Output 2 moves onto universes 1 and 2; output 1 onto an explicit route.
    let Begun::Opening(opening) = sender.begin(set(2, second_kind, 1), &[]).unwrap() else {
        panic!("the shifted run needs a new lane");
    };
    wakes.wake_at(at).unwrap();
    sender.finish(opening.open()).unwrap();
    let holes = [Span { start: 0, len: 3 }, Span { start: 512, len: 3 }];
    sender
        .attach_route(
            OutputNumber::try_from(1).unwrap(),
            outputs::open(kind).unwrap(),
            Route::new(outputs::Limits::RAW, &holes).unwrap(),
            Rate::Sixty,
        )
        .unwrap();
    assert_eq!(
        sender.send(edge()),
        Err(Refusal::NotStreamed { first: 1, last: 2 })
    );
    wakes.wake().unwrap(); // T9 of both
    assert_eq!(sender.send(edge()), Ok(()));
}

/// F1 of #257. The old run's counters end at the last snapshot before T9: the frame
/// and press T9's wake collects are counted on the old run and leave with it. No reader
/// sees them, and the new run starts without them.
#[test]
fn the_old_runs_counters_end_at_the_last_snapshot_before_t9() {
    use engine::{Begun, ChannelNumbers, Command, Event, OutputNumber, Rate, RunNumbers, Status};
    use engine_core::health::HealthSample;

    let (wire, kind) = stream::receiver(Color::Rgb);
    let (mut engine, mut sender, mut wakes) =
        Engine::start_stepped(Universes::new(2).unwrap(), Box::new(outputs::open)).unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let set = |first| Command::SetOutput {
        output,
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    wakes.hold().unwrap();
    sender.send(set(1)).unwrap();
    let at = wakes.wake().unwrap();
    read(&wire, 510);
    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("the shifted run needs a new lane");
    };
    wakes.wake_at(at).unwrap();
    let opened = opening.open();
    let press = Command::SetChannel {
        channel: ChannelNumbers {
            universe: 1,
            address: 1,
        },
        value: 77,
    };
    sender.send(press).unwrap();
    engine.latest(); // asks for the next wake's snapshot
    // The old lane sends the press; only T9's wake collects it.
    wakes.wake().unwrap();
    assert_eq!(read(&wire, 510), 77);
    let mut before = HealthSample::new(Duration::ZERO, 0, Status::NotFound);
    assert!(engine.latest().health_sample(output, &mut before));
    assert!(sender.publish_output(opened).unwrap().unwrap().wait());
    wakes.events();
    wakes.wake().unwrap(); // T9
    assert!(
        wakes
            .events()
            .iter()
            .any(|event| matches!(event, Event::Counted { output: o } if *o == output)),
        "T9 counts the old frame"
    );
    let mut after = HealthSample::new(Duration::ZERO, 0, Status::NotFound);
    assert!(engine.latest().health_sample(output, &mut after));
    let presses = |sample: &HealthSample| sample.press.iter().sum::<u64>();
    assert_eq!(
        (presses(&before), after.generation, presses(&after)),
        (0, before.generation + 1, 0),
        "the press collected at T9 left with the old run"
    );
}
