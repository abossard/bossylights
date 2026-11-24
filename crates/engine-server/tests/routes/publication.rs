#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_v11_picture_has_one_run_for_an_output_during_a_paused_restart() {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        unreachable!("bound to IPv4 loopback");
    };
    let kind = OutputKind::Ddp {
        address,
        color: Color::Rgb,
    };
    let (entered, reopening) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut first_open = true;
    let open = Box::new(move |kind| {
        if first_open {
            first_open = false;
        } else {
            entered.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(20)).unwrap();
        }
        outputs::open(kind)
    });
    let (engine, sender, wakes) = Engine::start_stepped(Universes::new(2).unwrap(), open).unwrap();
    engine.stall_reclamation(true);
    let address = common::serve(engine, sender, None).await;
    let (mut writer, mut reader) = tokio::time::timeout(Duration::from_secs(10), async {
        (
            common::connect(address).await,
            common::connect(address).await,
        )
    })
    .await
    .expect("the server starts and accepts both connections");
    wakes.hold().unwrap();
    let set = |count| {
        json!({"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": 1, "count": count}, "rate": null
        }})
    };
    assert_eq!(common::command(&mut writer, set(1)).await, "Accepted");
    let before = picture_after(&mut reader, &wakes).await;
    common::send(&mut writer, &json!({"Send": {"command": set(2)}})).await;
    // Reclamation is paused, so the restart falls back to an open, gated by this test.
    tokio::task::block_in_place(|| reopening.recv_timeout(Duration::from_secs(10))).unwrap();
    let pending = picture_after(&mut reader, &wakes).await;
    release.send(()).unwrap();
    assert_eq!(common::answer(&mut writer).await, "Accepted");
    let after = picture_after(&mut reader, &wakes).await;
    wakes.run();
    common::send(&mut writer, &json!("Stop")).await;

    for (picture, count) in [(before, 1), (pending, 1), (after, 2)] {
        let picture = &picture["Picture"];
        let project = &picture["project"]["outputs"][0]["Kind"]["universes"];
        let state = &picture["state"]["outputs"][0]["universes"];
        assert_eq!(project, &json!({"first": 1, "count": count}));
        assert_eq!(state, project, "a picture must agree on the output run");
    }
}

struct HoldFirstFrame {
    ddp: Box<dyn engine::Device>,
    gate: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
}

impl engine::Device for HoldFirstFrame {
    fn take(&mut self, frame: &[u8]) -> Outcome {
        if let Some((entered, release)) = self.gate.take() {
            entered.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        self.ddp.take(frame)
    }
}

#[test]
fn an_accepted_reroute_sends_only_the_newest_queued_old_frame_first() {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let timeout = Duration::from_secs(10);
    socket.set_read_timeout(Some(timeout)).unwrap();
    let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        unreachable!("bound to IPv4 loopback");
    };
    let kind = OutputKind::Ddp {
        address,
        color: Color::Rgb,
    };
    let (entered, taking) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut gate = Some((entered, wait));
    let (record, completed) = mpsc::sync_channel(3);
    let open = Box::new(move |kind| {
        Ok(Box::new(HoldFirstFrame {
            ddp: Box::new(RecordSend {
                ddp: outputs::open(kind)?,
                record: record.clone(),
            }),
            gate: gate.take(),
        }) as Box<dyn engine::Device>)
    });
    let (_engine, mut sender, mut wakes) =
        Engine::start_stepped(Universes::new(2).unwrap(), open).unwrap();
    wakes.hold().unwrap();
    let set = |first| Command::SetOutput {
        output: OutputNumber::try_from(1).unwrap(),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    let hold = |universe, value| Command::SetChannel {
        channel: ChannelNumbers {
            universe,
            address: 1,
        },
        value,
    };
    let read = || {
        let mut packet = [0; 1500];
        assert_eq!(socket.recv(&mut packet).unwrap(), 520);
        packet[10]
    };
    sender.send(hold(1, 11)).unwrap();
    sender.send(hold(2, 91)).unwrap();
    sender.send(set(1)).unwrap();
    let mut at = wakes.wake().unwrap();
    let first = at;
    taking.recv_timeout(timeout).unwrap();
    // Occupy both remaining lane boxes while the first send is in progress.
    let period = Rate::Sixty.period();
    for marker in [22, 33] {
        sender.send(hold(1, marker)).unwrap();
        at = wakes.wake_at(at + period).unwrap();
    }

    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("the shifted run needs a lane");
    };
    at = wakes.wake_at(at + period).unwrap(); // all three old boxes are occupied
    let handoff = sender.publish_output(opening.open()).unwrap().unwrap();
    wakes.wake_at(at).unwrap();
    release.send(()).unwrap();
    assert!(handoff.wait());
    assert_eq!(
        completed.try_iter().collect::<Vec<_>>(),
        [(11, Outcome::Accepted), (33, Outcome::Accepted)],
        "only the in-flight and newest old writes completed at acceptance"
    );
    assert_eq!(read(), 11, "the frame already inside the device finishes");
    assert_eq!(
        read(),
        33,
        "the newest queued old frame finishes before acceptance"
    );

    wakes.wake_at(at + period).unwrap();
    assert_eq!(read(), 91, "the first new send after accepting the edit");

    let events = wakes.events();
    for (due, outcome) in [
        (first, Outcome::Accepted),
        (first + period, Outcome::Dropped),
        (first + 2 * period, Outcome::Accepted),
        (at, Outcome::Dropped),
    ] {
        let finished: Vec<_> = events
            .iter()
            .filter_map(|event| match *event {
                Event::Came {
                    due: when, outcome, ..
                } if when == due => Some(outcome),
                Event::Handed { .. } | Event::Came { .. } | Event::Counted { .. } => None,
            })
            .collect();
        assert_eq!(
            finished,
            [outcome],
            "one outcome for old frame due at {due:?}"
        );
    }
}

#[test]
fn a_319_byte_route_cannot_be_attached_to_rgbw() {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let kind = OutputKind::Ddp {
        address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, socket.local_addr().unwrap().port()),
        color: Color::Rgbw,
    };
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(Universes::ONE, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let route = Route::new(outputs::Limits::RAW, &[Span { start: 0, len: 319 }]).unwrap();
    let before = sender.sent();
    let result = sender.attach_route(output, outputs::open(kind).unwrap(), route, Rate::Sixty);

    let packet_length = if result.is_ok() {
        wakes.wake().unwrap();
        Some(socket.recv(&mut [0; 2048]).unwrap())
    } else {
        None
    };
    assert!(
        result.is_err(),
        "accepted an invalid RGBW route and emitted a datagram of {packet_length:?} bytes including its 10-byte header"
    );
    assert_eq!(sender.project().output(output), None);
    assert_eq!(sender.sent(), before);
}
