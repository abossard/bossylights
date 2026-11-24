#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_device_reopens_the_old_run_after_a_refused_resize() {
    for ever_sent in [true, false] {
        check_recovery(ever_sent).await;
    }
}

/// Keeps reading the test's end of an adapter, as a plugged adapter takes every frame:
/// an end nobody reads fills up and the port is lost again. `stop` drops the end; a drop
/// (also a failed assert) tells the reader to stop and leaves it to end on its own.
struct Drain {
    stop: Arc<std::sync::atomic::AtomicBool>,
    reader: Option<thread::JoinHandle<()>>,
}

impl Drain {
    fn start(mut end: serialport::TTYPort) -> Self {
        use std::io::Read;

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let reader = thread::spawn(move || {
            let mut bytes = [0; 1024];
            // frames keep coming while the port runs, so each read returns soon
            while !stopped.load(Ordering::Acquire) && end.read(&mut bytes).is_ok_and(|n| n > 0) {}
        });
        Self {
            stop,
            reader: Some(reader),
        }
    }

    /// Stops reading and drops the end once the read under way returns.
    fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
    }
}

impl Drop for Drain {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

async fn health_event(
    watch: &mut common::Socket,
    wanted: &Value,
    events: &mut Vec<Value>,
) -> Value {
    use futures_util::StreamExt;

    tokio::time::timeout(usb::NEVER, async {
        loop {
            let message = watch.next().await.unwrap().unwrap();
            let reply: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            if let Some(rows) = reply["Health"]["events"].as_array() {
                events.extend(
                    rows.iter()
                        .filter(|e| e["output"] == 1)
                        .map(|e| e["event"].clone()),
                );
                if events.contains(wanted) {
                    return reply;
                }
            }
        }
    })
    .await
    .expect("health event for output 1")
}

async fn check_recovery(ever_sent: bool) {
    use std::sync::atomic::AtomicBool;

    struct Removable {
        ddp: Box<dyn engine::Device>,
        missing: Arc<AtomicBool>,
        accepts: bool,
    }
    impl engine::Device for Removable {
        fn take(&mut self, frame: &[u8]) -> Outcome {
            if self.missing.load(Ordering::Acquire) {
                Outcome::Failed
            } else if !self.accepts {
                Outcome::Dropped
            } else {
                self.ddp.take(frame)
            }
        }
    }

    let (socket, kind) = ddp(Color::Rgb);
    let missing = Arc::new(AtomicBool::new(false));
    let (entered, retrying) = mpsc::channel();
    let (release, available) = mpsc::channel();
    let mut attempt = 0;
    let (engine, sender, wakes) = start(Box::new(move |kind| {
        attempt += 1;
        match attempt {
            1 => Ok(Box::new(Removable {
                ddp: outputs::open(kind)?,
                missing: Arc::clone(&missing),
                accepts: ever_sent,
            })),
            2 | 3 => {
                // The resize and the first attempt to restore the old run both fail.
                missing.store(true, Ordering::Release);
                Err(std::io::ErrorKind::NotFound.into())
            }
            _ => {
                entered.send(()).unwrap();
                available.recv_timeout(usb::NEVER).unwrap();
                outputs::open(kind)
            }
        }
    }));
    wakes.run();
    let address = common::serve(engine, sender, None).await;
    let mut writer = common::connect(address).await;
    let mut other = common::connect(address).await;
    let mut watch = common::connect(address).await;
    let set = |count| {
        json!({"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": 1, "count": count}, "rate": "60"
        }})
    };
    assert_eq!(common::command(&mut writer, set(2)).await, "Accepted");
    common::send(&mut watch, &json!("Subscribe")).await;
    common::answer(&mut watch).await;
    let (counter, expected) = if ever_sent {
        ("accepted", "Running")
    } else {
        ("dropped", "NotFound")
    };
    let before = tokio::time::timeout(usb::NEVER, async {
        loop {
            common::send(&mut other, &json!("Query")).await;
            let picture = common::answer(&mut other).await;
            let old = &picture["Picture"]["state"]["outputs"][0];
            if old["counters"][counter].as_u64().is_some_and(|n| n >= 2) {
                break old.clone();
            }
        }
    })
    .await
    .expect("old device handed two frames");
    assert_eq!(before["status"], expected);

    // Hold the tick before it can return the old device, forcing a fresh open.
    wakes.hold().unwrap();
    assert_eq!(
        common::command(&mut writer, set(3)).await,
        json!({"Refused": {"reason": {"CannotOpen": {"why": {"Device": {"with": null}}}}}})
    );
    wakes.run();
    tokio::task::block_in_place(|| retrying.recv_timeout(usb::NEVER)).expect("retry the old run");
    let mut events = Vec::new();
    let lost = json!("OutputLost");
    let report = health_event(&mut watch, &lost, &mut events).await;
    assert!(
        report["Health"]["health"]["outputs"][0]["status"]["Reconnecting"]["failures"]
            .as_u64()
            .unwrap()
            >= 1
    );

    // This press reaches the front while the retry worker waits for the port.
    let press = json!({"SetChannel": {"channel": {"universe": 2, "address": 510}, "value": 73}});
    assert_eq!(common::command(&mut other, press).await, "Accepted");
    release.send(()).unwrap();
    tokio::task::block_in_place(|| {
        let started = Instant::now();
        loop {
            let (_, _, payload) = packet(&socket, 0x41, 0x0B, 0, 1023);
            if payload[1021] == 73 {
                break;
            }
            assert!(started.elapsed() < usb::NEVER, "old run never resumed"); // guard
        }
    });
    let recovered = json!({"Recovered": {"reason": "OutputAvailable"}});
    let report = health_event(&mut watch, &recovered, &mut events).await;
    assert_eq!(
        report["Health"]["health"]["outputs"][0]["status"],
        "Running"
    );
    assert_eq!(events.iter().filter(|event| **event == lost).count(), 1);
    common::send(&mut writer, &json!("Stop")).await;
}

#[test]
fn a_refused_restart_keeps_the_only_stream_for_a_patched_strip() {
    let (socket, kind) = ddp(Color::Rgb);
    let mut opened = false;
    let (engine, mut sender, wakes) = start(Box::new(move |kind| {
        if std::mem::replace(&mut opened, true) {
            return Err(std::io::ErrorKind::AddrNotAvailable.into());
        }
        outputs::open(kind)
    }));
    let one = output(1);
    let set = |count| Command::SetOutput {
        output: one,
        kind,
        universes: RunNumbers { first: 1, count },
        rate: None,
    };
    sender.send(set(2)).unwrap();
    sender
        .send(Command::PatchStrip {
            name: Text::new("Strip").unwrap(),
            pixels: Pixels::try_from(2).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 512,
            },
        })
        .unwrap();
    hold(&mut sender, 516, 91);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 1023).2[516], 91);
    let before = sender.project();
    assert_eq!(sender.patch().unstreamed(&before), None);

    engine.stall_reclamation(true);
    let Begun::Opening(restart) = sender.begin(set(3), &[]).unwrap() else {
        panic!("changing stream length must prepare a new lane");
    };
    wakes.wake().unwrap();
    let result = sender.finish(restart.open());
    engine.stall_reclamation(false);

    assert_eq!(
        result,
        Err(Unsent::Refused(Refusal::CannotOpen {
            why: engine::OpenFailure::Device { with: None }
        }))
    );
    let after = sender.project();
    assert_eq!(
        (after.output(one), sender.patch().unstreamed(&after)),
        (before.output(one), None),
        "a refused restart must leave the fixture's old stream in place"
    );
}

#[test]
fn replacing_a_route_cannot_drop_a_patched_pixel() {
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgbw);
    let one = output(1);
    sender
        .send(Command::SetOutput {
            output: one,
            kind,
            universes: RunNumbers { first: 1, count: 1 },
            rate: None,
        })
        .unwrap();
    sender
        .send(Command::PatchStrip {
            name: Text::new("Pixel").unwrap(),
            pixels: Pixels::try_from(1).unwrap(),
            color: Color::Rgbw,
            at: ChannelNumbers {
                universe: 1,
                address: 317,
            },
        })
        .unwrap();
    hold(&mut sender, 319, 91);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x1B, 0, 512).2[319], 91);

    let route = Route::new(outputs::limits(kind), &[span(0, 316), span(512, 4)]).unwrap();
    let result = sender.attach_route(one, outputs::open(kind).unwrap(), route, Rate::Sixty);
    if result.is_ok() {
        wakes.wake().unwrap();
        let (_, _, payload) = packet(&socket, 0x41, 0x1B, 0, 320);
        assert_eq!(
            payload[319], 91,
            "the accepted reroute dropped the patched pixel"
        );
    }
    assert_eq!(result, Err(Refusal::StreamsFixture { fixture: 1 }));
}

#[test]
fn the_engine_refuses_a_319_byte_route_for_an_rgbw_device() {
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgbw);
    let spans = [span(0, 319)];
    let one = output(1);
    let route = Route::new(outputs::Limits::RAW, &spans).unwrap();
    let result = sender.attach_route(one, outputs::open(kind).unwrap(), route, Rate::Sixty);
    if result.is_ok() {
        wakes.wake().unwrap();
        let sent = packet(&socket, 0x41, 0x1B, 0, 316).2.len();
        panic!("accepted a 319-byte RGBW route and sent only {sent} bytes");
    }
    assert_eq!(sender.project().output(one), None);
}

struct ObserveLane {
    ddp: Box<dyn engine::Device>,
    sent: mpsc::Sender<ThreadId>,
}

impl engine::Device for ObserveLane {
    fn take(&mut self, frame: &[u8]) -> Outcome {
        let outcome = self.ddp.take(frame);
        let _ = self.sent.send(thread::current().id());
        outcome
    }
}

#[test]
fn shifting_one_v11_universe_restarts_the_lane_and_keeps_the_device() {
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
    let (sent, lane) = mpsc::channel();
    let open = Box::new(move |kind| {
        Ok(Box::new(ObserveLane {
            ddp: outputs::open(kind)?,
            sent: sent.clone(),
        }) as Box<dyn engine::Device>)
    });
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(Universes::new(2).unwrap(), open).unwrap();
    wakes.hold().unwrap();
    let set = |first| Command::SetOutput {
        output: OutputNumber::try_from(1).unwrap(),
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
    let read = |marker| {
        let mut packet = [0; 1500];
        let (size, peer) = socket.recv_from(&mut packet).unwrap();
        assert_eq!(size, 520, "510 RGB bytes and the 10-byte DDP header");
        assert_eq!(packet[10], marker);
        (peer, lane.recv_timeout(timeout).unwrap())
    };

    sender.send(set(1)).unwrap();
    wakes.wake().unwrap();
    let (first_peer, first_lane) = read(11);
    if let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() {
        wakes.wake().unwrap();
        assert_eq!(read(11), (first_peer, first_lane));
        sender.finish(opening.open()).unwrap();
    }
    wakes.wake().unwrap();
    let (second_peer, second_lane) = read(91);
    assert_eq!(first_peer, second_peer, "keep the DDP socket");
    assert_ne!(
        first_lane, second_lane,
        "restart the lane after moving the run"
    );
}

async fn picture_after(desk: &mut common::Socket, wakes: &Wakes) -> Value {
    let previous = wakes.woke();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            wakes.wake().unwrap();
            common::send(desk, &json!("Query")).await;
            let reply = common::answer(desk).await;
            if reply["Picture"]["state"]["frame"].as_u64().unwrap() > previous {
                return reply;
            }
        }
    })
    .await
    .expect("the reader publishes a new frame")
}
