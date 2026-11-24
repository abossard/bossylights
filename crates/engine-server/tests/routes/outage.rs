#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_reroute_keeps_the_old_run_sending_during_open() {
    let (socket, kind) = ddp(Color::Rgb);
    let (entered, reopening) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut opened = false;
    let (engine, sender, mut wakes) = Engine::start_stepped(
        Universes::new(2).unwrap(),
        Box::new(move |kind| {
            if !std::mem::replace(&mut opened, true) {
                return outputs::open(kind);
            }
            entered.send(()).unwrap();
            gate.recv_timeout(usb::NEVER).unwrap();
            Err(std::io::ErrorKind::NotFound.into())
        }),
    )
    .unwrap();
    engine.stall_reclamation(true);
    let address = common::serve(engine, sender, None).await;
    let mut writer = common::connect(address).await;
    let mut other = common::connect(address).await;
    wakes.hold().unwrap();
    let set = |first| {
        json!({"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": first, "count": 1}, "rate": "60"
        }})
    };
    let press =
        |value| json!({"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": value}});
    assert_eq!(common::command(&mut writer, set(1)).await, "Accepted");
    assert_eq!(common::command(&mut other, press(11)).await, "Accepted");
    let at = wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 11);
    wakes.events();

    common::send(&mut writer, &json!({"Send": {"command": set(2)}})).await;
    tokio::task::block_in_place(|| reopening.recv_timeout(usb::NEVER)).unwrap();
    assert_eq!(common::command(&mut other, press(91)).await, "Accepted");
    let period = Rate::Sixty.period();
    let at = wakes.wake_at(at + period).unwrap();
    let events = wakes.events();
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Handed { output: number, .. } if *number == output(1)
    )));
    let dropped = events.iter().any(|event| {
        matches!(
            event,
            Event::Came { output: number, outcome: Outcome::Dropped, .. } if *number == output(1)
        )
    });
    let while_pending = (!dropped).then(|| packet(&socket, 0x41, 0x0B, 0, 510).2[0]);

    release.send(()).unwrap();
    assert_eq!(
        common::answer(&mut writer).await,
        json!({"Refused": {"reason": {"CannotOpen": {"why": {"Device": {"with": null}}}}}})
    );
    wakes.wake_at(at + period).unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 91);
    wakes.run();
    common::send(&mut writer, &json!("Stop")).await;
    assert_eq!(
        while_pending,
        Some(91),
        "the due frame on the unchanged old run was dropped until the refusal: {events:?}"
    );
}

async fn output_picture(desk: &mut common::Socket, until: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(usb::NEVER, async {
        loop {
            common::send(desk, &json!("Query")).await;
            let reply = common::answer(desk).await;
            if until(&reply["Picture"]["state"]["outputs"][0]) {
                return reply;
            }
        }
    })
    .await
    .expect("an output picture matching the requested state")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepting_a_reroute_does_not_hide_the_adapters_existing_outage() {
    let kind = OutputKind::DmxUsbPro {
        adapter: Adapter::FIRST,
    };
    let (find, mut fake) = usb::Fake::new(true);
    let mut find = Some(find);
    let (engine, sender, wakes) = start(Box::new(move |_| {
        let pro = DmxUsbPro::open(find.take().ok_or(std::io::ErrorKind::AddrInUse)?)?;
        Ok(Box::new(pro))
    }));
    wakes.run();
    let address = common::serve(engine, sender, None).await;
    let mut writer = common::connect(address).await;
    let mut reader = common::connect(address).await;
    let set = |first| {
        json!({"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": first, "count": 1}, "rate": "44"
        }})
    };
    for (universe, value) in [(1, 11), (2, 91)] {
        let press = json!({"SetChannel": {
            "channel": {"universe": universe, "address": 1}, "value": value
        }});
        assert_eq!(common::command(&mut writer, press).await, "Accepted");
    }
    assert_eq!(common::command(&mut writer, set(1)).await, "Accepted");
    let mut end = tokio::task::block_in_place(|| fake.next_end());
    assert_eq!(
        tokio::task::block_in_place(|| usb::read_message(&mut end))[5],
        11
    );
    output_picture(&mut reader, |out| out["status"] == "Running").await;
    fake.plugged().store(false, Ordering::Release);
    drop(end);
    output_picture(&mut reader, |out| out["status"]["Reconnecting"].is_object()).await;

    assert_eq!(common::command(&mut writer, set(2)).await, "Accepted");
    let after = output_picture(&mut reader, |out| {
        out["universes"]["first"] == 2 && out["counters"]["failed"].as_u64().is_some_and(|n| n > 0)
    })
    .await;
    fake.plugged().store(true, Ordering::Release);
    let first = tokio::task::block_in_place(|| usb::read_message(&mut fake.next_end()));
    assert_eq!(first[5], 91);
    common::send(&mut writer, &json!("Stop")).await;

    let state = &after["Picture"]["state"]["outputs"][0]["status"];
    let health = &after["Picture"]["health"]["outputs"][0]["status"];
    assert!(
        state["Reconnecting"].is_object() && health["Reconnecting"].is_object(),
        "rerouting the same lost adapter reported state={state}, health={health}"
    );
}

fn state_until(engine: &mut Engine, ready: impl Fn(Output) -> bool) -> Output {
    let started = Instant::now();
    loop {
        let state = engine
            .fresh()
            .unwrap()
            .output(OutputNumber::try_from(1).unwrap());
        if let Some(state) = state.filter(|state| ready(*state)) {
            return state;
        }
        assert!(
            started.elapsed() < usb::NEVER,
            "last output state: {state:?}"
        ); // guard
    }
}

#[test]
fn an_accepted_usb_reroute_stays_reconnecting_until_the_lost_port_returns() {
    let kind = OutputKind::DmxUsbPro {
        adapter: Adapter::FIRST,
    };
    let (find, mut adapter) = usb::Fake::new(true);
    let mut find = Some(find);
    let (mut engine, mut sender) = Engine::start_with(
        Universes::new(2).unwrap(),
        Box::new(move |_| {
            let find = find.take().ok_or(std::io::ErrorKind::AddrInUse)?;
            Ok(Box::new(DmxUsbPro::open(find)?))
        }),
    )
    .unwrap();
    let set = |first| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: None,
    };
    for (universe, value) in [(1, 11), (2, 91)] {
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
    let mut port = adapter.next_end();
    assert_eq!(usb::read_message(&mut port)[5], 11);
    state_until(&mut engine, Output::is_running);

    adapter.plugged().store(false, Ordering::Release);
    drop(port);
    state_until(&mut engine, |state| {
        matches!(state.connection(), Status::Reconnecting { .. })
    });
    sender.send(set(2)).unwrap();
    let after = state_until(&mut engine, |state| {
        state.universes.first() == 2 && state.counters.failed > 0
    });
    let mut health = HealthSample::new(Duration::ZERO, 0, Status::NotFound);
    assert!(
        engine
            .fresh()
            .unwrap()
            .health_sample(output(1), &mut health)
    );
    assert!(
        matches!(
            (after.connection(), health.status),
            (Status::Reconnecting { failures: earlier }, Status::Reconnecting { failures: later })
                if later >= earlier
        ),
        "state {:?}, subsequent health {:?}",
        after.connection(),
        health.status
    );

    adapter.plugged().store(true, Ordering::Release);
    let mut port = adapter.next_end();
    assert_eq!(usb::read_message(&mut port)[5], 91);
    state_until(&mut engine, Output::is_running);
    assert!(
        matches!(health.status, Status::Reconnecting { .. }),
        "the accepted run must show the existing outage, got {:?}",
        health.status
    );
}

fn first_rgb_slot(socket: &UdpSocket) -> u8 {
    let mut bytes = [0; 1536];
    assert_eq!(socket.recv(&mut bytes).unwrap(), 520);
    assert_eq!(&bytes[2..10], &[0x0B, 1, 0, 0, 0, 0, 1, 254]);
    bytes[10]
}

#[test]
fn a_resize_that_will_be_refused_keeps_sending_the_old_run_while_it_opens() {
    let (wire, kind) = stream::receiver(Color::Rgb);
    let (entered, opening) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut first = true;
    let (engine, mut sender, mut wakes) = Engine::start_stepped(
        Universes::new(2).unwrap(),
        Box::new(move |kind| {
            if std::mem::replace(&mut first, false) {
                return outputs::open(kind);
            }
            entered.send(()).unwrap();
            gate.recv_timeout(usb::NEVER).unwrap();
            Err(std::io::ErrorKind::NotFound.into())
        }),
    )
    .unwrap();
    let set = |count| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first: 1, count },
        rate: None,
    };
    wakes.hold().unwrap();
    sender.send(set(1)).unwrap();
    let initial = wakes.wake().unwrap();
    assert_eq!(first_rgb_slot(&wire), 0);

    engine.stall_reclamation(true);
    let Begun::Opening(restart) = sender.begin(set(2), &[]).unwrap() else {
        panic!("resizing an output needs a new lane");
    };
    wakes.wake_at(initial).unwrap();
    let worker = thread::spawn(|| restart.open());
    opening.recv_timeout(usb::NEVER).unwrap();

    wakes.events();
    sender
        .send(Command::SetChannel {
            channel: ChannelNumbers {
                universe: 1,
                address: 1,
            },
            value: 73,
        })
        .unwrap();
    let due = wakes.wake().unwrap();
    let started = Instant::now();
    let outcome = loop {
        if let Some(outcome) = wakes.events().into_iter().find_map(|event| match event {
            Event::Came {
                output: number,
                outcome,
                due: finished,
            } if number == output(1) && finished == due => Some(outcome),
            Event::Handed { .. } | Event::Came { .. } | Event::Counted { .. } => None,
        }) {
            break outcome;
        }
        assert!(started.elapsed() < usb::NEVER, "due frame never completed"); // guard
        wakes.wake_at(due).unwrap();
    };
    let marker = (outcome == Outcome::Accepted).then(|| first_rgb_slot(&wire));

    release.send(()).unwrap();
    assert_eq!(
        sender.finish(worker.join().unwrap()),
        Err(Unsent::Refused(Refusal::CannotOpen {
            why: engine::OpenFailure::Device { with: None }
        }))
    );
    engine.stall_reclamation(false);
    wakes.wake().unwrap();
    assert_eq!(first_rgb_slot(&wire), 73);
    assert_eq!((outcome, marker), (Outcome::Accepted, Some(73)));
}

fn first_channel(socket: &UdpSocket) -> u8 {
    let mut packet = [0; 1500];
    assert_eq!(socket.recv(&mut packet).unwrap(), 520);
    assert_eq!(packet[8..10], 510_u16.to_be_bytes());
    packet[10]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_press_reaches_the_accepted_output_while_its_replacement_lane_starts() {
    let (socket, kind) = stream::receiver(Color::Rgb);
    let (engine, mut sender, mut wakes) =
        Engine::start_stepped(Universes::new(2).unwrap(), Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    for (universe, value) in [(1, 11), (2, 22)] {
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
    sender
        .send(Command::SetOutput {
            output,
            kind,
            universes: RunNumbers { first: 1, count: 1 },
            rate: Some(Rate::Sixty),
        })
        .unwrap();
    wakes.wake().unwrap();
    assert_eq!(first_channel(&socket), 11);

    let (entered, starting) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    sender.pause_lane_start(output, entered, gate);
    wakes.run();
    let address = common::serve(engine, sender, None).await;
    let mut writer = common::connect(address).await;
    let mut performer = common::connect(address).await;
    wakes.hold().unwrap();
    let start = wakes.wake().unwrap();
    common::send(
        &mut writer,
        &json!({"Send": {"command": {"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": 2, "count": 1}, "rate": "60"
        }}}}),
    )
    .await;

    // Apply the pending route work without advancing the output's due time.
    tokio::time::timeout(Duration::from_secs(10), async {
        while starting.try_recv().is_err() {
            wakes.wake_at(start).unwrap();
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the replacement reaches Lane::start");
    let _ = wakes.events();
    socket.set_nonblocking(true).unwrap();
    while socket.recv(&mut [0; 1500]).is_ok() {}
    socket.set_nonblocking(false).unwrap();

    let press = json!({"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": 91}});
    assert_eq!(common::command(&mut performer, press).await, "Accepted");
    let due = wakes.wake_at(start + Rate::Sixty.period()).unwrap();
    let events = wakes.events();
    let dropped = events.iter().any(|event| {
        matches!(event, Event::Came { output: number, outcome: Outcome::Dropped, due: time }
            if *number == output && *time == due)
    });
    let displayed = (!dropped).then(|| first_channel(&socket));

    release.send(()).unwrap();
    assert_eq!(common::answer(&mut writer).await, "Accepted");
    wakes.wake_at(due + Rate::Sixty.period()).unwrap();
    assert_eq!(first_channel(&socket), 22);
    wakes.run();
    common::send(&mut performer, &json!("Stop")).await;

    assert_eq!(
        displayed,
        Some(91),
        "the still-accepted route's first due frame after the press: {events:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_adapter_reports_one_loss_and_one_return_across_a_reroute() {
    let (find, mut adapter) = usb::Fake::new(true);
    let mut find = Some(find);
    let (engine, sender, wakes) = start(Box::new(move |_| {
        let find = find.take().ok_or(std::io::ErrorKind::AddrInUse)?;
        Ok(Box::new(DmxUsbPro::open(find)?))
    }));
    // The exclusive port must move even when the free thread cannot reclaim lanes.
    engine.stall_reclamation(true);
    wakes.run();
    let address = common::serve(engine, sender, None).await;
    let mut desk = common::connect(address).await;
    let mut listener = common::connect(address).await;
    let kind = OutputKind::DmxUsbPro {
        adapter: Adapter::FIRST,
    };
    let route = |first| {
        json!({"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": first, "count": 1}, "rate": null
        }})
    };
    let press = json!({"SetChannel": {"channel": {"universe": 2, "address": 1}, "value": 91}});
    assert_eq!(common::command(&mut desk, press).await, "Accepted");
    assert_eq!(common::command(&mut desk, route(1)).await, "Accepted");
    let end = Drain::start(tokio::task::block_in_place(|| adapter.next_end()));
    output_picture(&mut desk, |out| out["status"] == "Running").await;
    common::send(&mut listener, &json!("Subscribe")).await;
    common::answer(&mut listener).await;

    let mut events = Vec::new();
    let lost = json!("OutputLost");
    let returned = json!({"Recovered": {"reason": "OutputAvailable"}});
    adapter.plugged().store(false, Ordering::Release);
    tokio::task::block_in_place(|| end.stop());
    health_event(&mut listener, &lost, &mut events).await;
    let before = output_picture(&mut desk, |out| {
        out["status"]["Reconnecting"]["failures"]
            .as_u64()
            .is_some_and(|n| n >= 2)
    })
    .await;
    let previous_failures =
        before["Picture"]["state"]["outputs"][0]["status"]["Reconnecting"]["failures"]
            .as_u64()
            .unwrap();
    assert_eq!(common::command(&mut desk, route(2)).await, "Accepted");
    let after = output_picture(&mut desk, |out| {
        out["universes"]["first"] == 2 && out["counters"]["failed"].as_u64().is_some_and(|n| n > 0)
    })
    .await;
    let output = &after["Picture"]["state"]["outputs"][0];
    assert!(
        output["status"]["Reconnecting"]["failures"]
            .as_u64()
            .unwrap()
            >= previous_failures + output["counters"]["failed"].as_u64().unwrap()
    );
    adapter.plugged().store(true, Ordering::Release);
    let mut end = tokio::task::block_in_place(|| adapter.next_end());
    assert_eq!(
        tokio::task::block_in_place(|| usb::read_message(&mut end))[5],
        91
    );
    let end = Drain::start(end);
    let report = health_event(&mut listener, &returned, &mut events).await;
    assert_eq!(
        report["Health"]["health"]["outputs"][0]["status"],
        "Running"
    );
    events.retain(|event| *event == lost || *event == returned);
    assert_eq!(events, [lost, returned]);
    tokio::task::block_in_place(|| end.stop());
    common::send(&mut desk, &json!("Stop")).await;
}
