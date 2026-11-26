#[test]
fn a_fresh_device_waits_for_the_last_old_send_before_accepting_a_reroute() {
    let (socket, kind) = ddp(Color::Rgb);
    let (entered, taking) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (fresh, reopened) = mpsc::channel();
    let (record, completed) = mpsc::sync_channel(2);
    let mut first = Some((entered, resume, record));
    let (mut engine, mut sender, mut wakes) = start(Box::new(move |kind| {
        let ddp = outputs::open(kind)?;
        if let Some((entered, resume, record)) = first.take() {
            Ok(Box::new(HoldFirstFrame {
                ddp: Box::new(RecordSend { ddp, record }),
                gate: Some((entered, resume)),
            }))
        } else {
            fresh.send(()).unwrap();
            Ok(ddp)
        }
    }));
    let set = |first| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: None,
    };
    for (index, marker) in [(0, 11), (512, 91)] {
        hold(&mut sender, index, marker);
    }
    sender.send(set(1)).unwrap();
    wakes.wake().unwrap();
    taking.recv_timeout(usb::NEVER).unwrap();
    wakes.events();

    // Keep the device on its way back while the blocking worker opens a fresh socket.
    engine.stall_reclamation(true);
    let Begun::Opening(restart) = sender.begin(set(2), &[]).unwrap() else {
        panic!("a shifted run needs a new lane");
    };
    hold(&mut sender, 0, 73); // the probe wake queues one old frame behind 11
    engine.latest();
    let due = wakes.wake().unwrap();
    assert_eq!(
        engine.latest().applied(),
        sender.sent(),
        "the probe wake applied 73"
    );
    let hands: Vec<_> = wakes
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Handed {
                output: number,
                due,
                ..
            } if number == output(1) => Some(due),
            Event::Handed { .. } | Event::Came { .. } | Event::Counted { .. } => None,
        })
        .collect();
    assert_eq!(hands, [due], "the probe wake handed one old frame");
    old_send_at_acceptance(
        &socket,
        release,
        completed,
        &[11, 73],
        || sender.finish(restart.open()).unwrap(),
        || reopened.recv_timeout(usb::NEVER).unwrap(),
    );
    engine.stall_reclamation(false);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 91);
}

/// Record the result after the real adapter has sent and before its worker returns.
struct RecordSend {
    ddp: Box<dyn engine::Device>,
    record: mpsc::SyncSender<(u8, Outcome)>,
}

impl engine::Device for RecordSend {
    fn take(&mut self, frame: &[u8]) -> Outcome {
        let result = self.ddp.take(frame);
        self.record.try_send((frame[0], result)).unwrap();
        result
    }
}

fn old_send_at_acceptance(
    socket: &UdpSocket,
    release: mpsc::Sender<()>,
    completed: mpsc::Receiver<(u8, Outcome)>,
    old_markers: &[u8],
    complete: impl FnOnce() + Send,
    ready: impl FnOnce(),
) {
    let (finished, accepted) = mpsc::channel();
    thread::scope(|threads| {
        threads.spawn(move || {
            complete();
            // Take only sends that have already completed when the writer returns.
            finished
                .send(completed.try_iter().collect::<Vec<_>>())
                .unwrap();
        });
        ready();
        let early = accepted.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        let sent = early.unwrap_or_else(|_| accepted.recv_timeout(usb::NEVER).unwrap());
        let expected: Vec<_> = old_markers
            .iter()
            .map(|&marker| (marker, Outcome::Accepted))
            .collect();
        assert_eq!(sent, expected, "completed adapter writes at acceptance");
    });
    for marker in old_markers {
        assert_eq!(packet(socket, 0x41, 0x0B, 0, 510).2[0], *marker);
    }
}

#[test]
fn an_explicit_reroute_waits_for_the_last_send_in_the_old_lane() {
    let (socket, kind) = ddp(Color::Rgb);
    let (entered, taking) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (record, completed) = mpsc::sync_channel(1);
    let mut gate = Some((entered, resume));
    let (_engine, mut sender, wakes) = start(Box::new(move |kind| {
        Ok(Box::new(HoldFirstFrame {
            ddp: Box::new(RecordSend {
                ddp: outputs::open(kind)?,
                record: record.clone(),
            }),
            gate: gate.take(),
        }))
    }));
    for (channel, level) in [(0, 11), (15, 91)] {
        hold(&mut sender, channel, level);
    }
    sender
        .send(Command::SetOutput {
            output: output(1),
            kind,
            universes: RunNumbers { first: 1, count: 1 },
            rate: Some(Rate::Sixty),
        })
        .unwrap();
    wakes.wake().unwrap();
    taking.recv_timeout(usb::NEVER).unwrap();

    // Moving to channel 15 changes the route within the same displayed universe.
    let (started, attaching) = mpsc::channel();
    old_send_at_acceptance(
        &socket,
        release,
        completed,
        &[11],
        || {
            started.send(()).unwrap();
            attach_ddp(&mut sender, kind, &[span(15, 3)]);
        },
        || attaching.recv_timeout(usb::NEVER).unwrap(),
    );
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 3).2[0], 91);
}

#[test]
fn an_explicit_reroute_refused_by_a_full_queue_keeps_sending_the_old_route() {
    let (socket, kind) = ddp(Color::Rgb);
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    attach_ddp(&mut sender, kind, &[span(15, 3)]);
    hold(&mut sender, 15, 11);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 3).2[0], 11);

    while sender.send(Command::SetMaster { level: 255 }).is_ok() {}
    let new_route = Route::new(outputs::limits(kind), &[span(123, 3)]).unwrap();
    let result = sender.attach_route(
        output(1),
        outputs::open(kind).unwrap(),
        new_route,
        Rate::Sixty,
    );
    assert_eq!(result, Err(Refusal::QueueFull));
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 3).2[0], 11);
}

#[test]
fn resizing_and_shifting_a_run_restart_its_lane_on_the_same_device() {
    let (socket, kind) = ddp(Color::Rgb);
    let opens = Arc::new(AtomicUsize::new(0));
    let (_engine, mut sender, wakes) = start(Box::new({
        let opens = Arc::clone(&opens);
        move |kind| {
            opens.fetch_add(1, Ordering::Relaxed);
            outputs::open(kind)
        }
    }));
    let one = output(1);
    let set = |first, count| Command::SetOutput {
        output: one,
        kind,
        universes: RunNumbers { first, count },
        rate: None,
    };
    sender.send(set(1, 1)).unwrap();
    hold(&mut sender, 600, 91);
    wakes.wake().unwrap();
    let (peer, _, payload) = packet(&socket, 0x41, 0x0B, 0, 510);
    assert_eq!(payload, vec![0; 510]);

    let Begun::Opening(opening) = sender.begin(set(1, 2), &[]).unwrap() else {
        panic!("a changed length needs a new lane");
    };
    assert_eq!(
        sender.send(set(2, 2)),
        Err(Unsent::Refused(Refusal::Opening))
    );
    assert_eq!(
        sender.send(Command::RemoveOutput { output: one }),
        Err(Unsent::Refused(Refusal::Opening))
    );
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2, vec![0; 510]);
    sender.finish(opening.open()).unwrap();
    // Reading the first frame after each edit rules out a stale handover from the old lane.
    for (first, marker) in [(1, 600), (2, 88)] {
        if first == 2 {
            let Begun::Opening(opening) = sender.begin(set(first, 2), &[]).unwrap() else {
                panic!("a shifted run needs a new lane");
            };
            wakes.wake().unwrap();
            assert_eq!(packet(&socket, 0x41, 0x0B, 0, 1023).2[600], 91);
            sender.finish(opening.open()).unwrap();
        }
        wakes.wake().unwrap();
        let (source, _, payload) = packet(&socket, 0x41, 0x0B, 0, 1023);
        let mut expected = vec![0; 1023];
        expected[marker] = 91;
        assert_eq!(payload, expected);
        assert_eq!(source, peer);
    }
    assert_eq!(opens.load(Ordering::Relaxed), 1);
}

#[test]
fn a_prepared_reroute_keeps_the_old_run_sending_until_finish() {
    let (socket, kind) = ddp(Color::Rgb);
    let (_engine, mut sender, mut wakes) = start(Box::new(outputs::open));
    let one = output(1);
    let set = |first| Command::SetOutput {
        output: one,
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    hold(&mut sender, 0, 11);
    hold(&mut sender, 512, 91);
    sender.send(set(1)).unwrap();
    let initial = wakes.wake().unwrap();
    let (peer, _, frame) = packet(&socket, 0x41, 0x0B, 0, 510);
    assert_eq!(frame[0], 11);

    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("shifting the run needs a new lane");
    };
    wakes.wake_at(initial).unwrap();
    let ready = opening.open();
    hold(&mut sender, 0, 73);
    let due = wakes.wake().unwrap();
    assert!(!wakes.events().iter().any(|event| matches!(event,
        Event::Came { output, outcome: Outcome::Dropped, due: when } if *output == one && *when == due
    )), "the old run dropped the press before finish");
    let (old, _, frame) = packet(&socket, 0x41, 0x0B, 0, 510);
    assert_eq!((old, frame[0]), (peer, 73));

    sender.finish(ready).unwrap();
    wakes.wake().unwrap();
    let (new, _, frame) = packet(&socket, 0x41, 0x0B, 0, 510);
    assert_eq!((new, frame[0]), (peer, 91));
}

#[test]
fn a_reroute_after_cutoff_keeps_the_due_press_on_the_old_run() {
    let (socket, kind) = ddp(Color::Rgb);
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let set = |first| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    hold(&mut sender, 0, 11);
    hold(&mut sender, 512, 91);
    sender.send(set(1)).unwrap();
    let initial = wakes.wake().unwrap();
    let (peer, _, frame) = packet(&socket, 0x41, 0x0B, 0, 510);
    assert_eq!(frame[0], 11);

    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("shifting the run needs a new lane");
    };
    wakes.wake_at(initial).unwrap();
    let ready = opening.open();
    hold(&mut sender, 0, 73);
    wakes.cut().unwrap(); // This due wake has taken the press into its FIFO prefix.

    thread::scope(|threads| {
        let (done, finished) = mpsc::channel();
        let receipt = sender.publish_output(ready).unwrap().unwrap();
        threads.spawn(move || {
            receipt.wait();
            done.send(()).unwrap();
        });
        // Allow an early acknowledgement to finish taking the port while the tick
        // is held. A correct handoff may still be waiting for the last old write.
        let early = finished.recv_timeout(Duration::from_secs(1));
        wakes.finish().unwrap();
        wakes.wake().unwrap();
        if early.is_err() {
            finished.recv_timeout(usb::NEVER).unwrap();
        }
    });
    for marker in [73, 91] {
        let (from, _, frame) = packet(&socket, 0x41, 0x0B, 0, 510);
        assert_eq!((from, frame[0]), (peer, marker));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_clients_press_reaches_the_next_due_frame_while_an_open_waits() {
    press_during_setup(Pause::Open).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_clients_press_reaches_the_next_due_frame_while_a_lane_starts() {
    press_during_setup(Pause::LaneStart).await;
}

#[test]
fn finishing_a_prepared_reroute_needs_no_second_blocking_worker() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (socket, kind) = ddp(Color::Rgb);
            let (engine, mut sender, wakes) =
                Engine::start_stepped(Universes::new(2).unwrap(), Box::new(outputs::open)).unwrap();
            sender
                .send(Command::SetOutput {
                    output: output(1),
                    kind,
                    universes: RunNumbers { first: 1, count: 1 },
                    rate: Some(Rate::Sixty),
                })
                .unwrap();
            hold(&mut sender, 512, 91);
            let (entered, starting) = mpsc::channel();
            let (release_start, wait_start) = mpsc::channel();
            sender.pause_lane_start(output(1), entered, wait_start);
            let mut desk = common::connect(common::serve(engine, sender, None).await).await;
            wakes.hold().unwrap();
            let press =
                json!({"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": 11}});
            assert_eq!(common::command(&mut desk, press).await, "Accepted");
            let last = wakes.wake().unwrap();
            // Read through the final old packet, including any from server startup.
            while packet(&socket, 0x41, 0x0B, 0, 510).2[0] != 11 {}
            common::send(
                &mut desk,
                &json!({"Send": {"command": {"SetOutput": {
                    "output": 1, "kind": kind, "universes": {"first": 2, "count": 1}, "rate": "60"
                }}}}),
            )
            .await;
            tokio::time::timeout(usb::NEVER, async {
                while starting.try_recv().is_err() {
                    wakes.wake_at(last).unwrap();
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("replacement reached Lane::start");

            // The queued job occupies the only worker as soon as the opener returns.
            // Dropping its gate also releases it if an assertion unwinds.
            let (release_pool, wait_pool) = mpsc::channel::<()>();
            let queued = tokio::task::spawn_blocking(move || {
                let _ = wait_pool.recv();
            });
            drop(release_start);
            assert_eq!(common::answer(&mut desk).await, "Accepted");
            wakes.wake().unwrap();
            assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 91);
            drop(release_pool);
            queued.await.unwrap();
            wakes.run();
            common::send(&mut desk, &json!("Stop")).await;
        });
}

enum Pause {
    Open,
    LaneStart,
}

async fn press_during_setup(at: Pause) {
    let (running, running_kind) = ddp(Color::Rgb);
    let (_paused, paused_kind) = ddp(Color::Rgb);
    let (entered, opening) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (engine, sender, wakes) = match at {
        Pause::Open => Engine::start_stepped(
            Universes::ONE,
            Box::new(move |kind| {
                if kind == paused_kind {
                    entered.send(()).unwrap();
                    gate.recv_timeout(usb::NEVER).unwrap();
                }
                outputs::open(kind)
            }),
        )
        .unwrap(),
        Pause::LaneStart => {
            let (engine, mut sender, wakes) =
                Engine::start_stepped(Universes::ONE, Box::new(outputs::open)).unwrap();
            sender.pause_lane_start(output(1), entered, gate);
            (engine, sender, wakes)
        }
    };
    let address = common::serve(engine, sender, None).await;
    let mut first = common::connect(address).await;
    let mut second = common::connect(address).await;
    wakes.hold().unwrap();
    let set = |number, kind| {
        json!({"SetOutput": {
            "output": number, "kind": kind, "universes": {"first": 1, "count": 1}, "rate": "60"
        }})
    };
    assert_eq!(
        common::command(&mut second, set(2, running_kind)).await,
        "Accepted"
    );
    wakes.wake().unwrap();
    assert_eq!(packet(&running, 0x41, 0x0B, 0, 510).2[0], 0);

    common::send(
        &mut first,
        &json!({"Send": {"command": set(1, paused_kind)}}),
    )
    .await;
    tokio::task::block_in_place(|| opening.recv_timeout(usb::NEVER)).unwrap();
    let press = json!({"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": 91}});
    assert_eq!(common::command(&mut second, press).await, "Accepted");
    wakes.wake().unwrap();
    assert_eq!(packet(&running, 0x41, 0x0B, 0, 510).2[0], 91);
    release.send(()).unwrap();
    assert_eq!(common::answer(&mut first).await, "Accepted");
    wakes.run();
    common::send(&mut second, &json!("Stop")).await;
}
