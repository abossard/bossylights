#[test]
fn a_press_already_due_on_the_old_run_reaches_the_wire_before_reroute_acceptance() {
    for publish in [false, true] {
        let (socket, kind) = ddp(Color::Rgb);
        let (entered, taking) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let mut gate = Some((entered, resume));
        let (_engine, mut sender, wakes) = start(Box::new(move |kind| {
            Ok(Box::new(HoldFirstFrame {
                ddp: outputs::open(kind)?,
                gate: gate.take(),
            }))
        }));
        let set = |first| Command::SetOutput {
            output: output(1),
            kind,
            universes: RunNumbers { first, count: 1 },
            rate: Some(Rate::Sixty),
        };
        hold(&mut sender, 0, 11);
        hold(&mut sender, 512, 91);
        sender.send(set(1)).unwrap();
        let first = wakes.wake().unwrap();
        taking.recv_timeout(usb::NEVER).unwrap();

        let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
            panic!("a shifted run needs a new lane");
        };
        wakes.wake_at(first).unwrap();
        let ready = opening.open();
        hold(&mut sender, 0, 73);
        // The only waiting frame is the press, due before publication. Keep the
        // previous datagram inside take until this cutover has reached the old lane.
        wakes.wake().unwrap();
        let handoff = publish.then(|| {
            let handoff = sender.publish_output(ready).unwrap().unwrap();
            wakes.wake().unwrap();
            handoff
        });
        release.send(()).unwrap();
        if let Some(handoff) = handoff {
            handoff.wait();
        }

        let (peer, _, in_flight) = packet(&socket, 0x41, 0x0B, 0, 510);
        let (next_peer, _, next) = packet(&socket, 0x41, 0x0B, 0, 510);
        assert_eq!(
            (in_flight[0], next[0], next_peer),
            (11, 73, peer),
            "publish={publish}: the single press frame due before the cutover"
        );
    }
}

#[test]
fn a_reroute_keeps_the_old_lanes_last_completed_failure() {
    struct FailsOnMarker(Box<dyn engine::Device>, mpsc::Sender<u8>);

    impl engine::Device for FailsOnMarker {
        fn take(&mut self, frame: &[u8]) -> Outcome {
            match frame[0] {
                marker @ (73 | 91) => {
                    self.1.send(marker).unwrap();
                    Outcome::Failed
                }
                _ => self.0.take(frame),
            }
        }
    }

    let (socket, kind) = ddp(Color::Rgb);
    let (failed, failures) = mpsc::channel();
    let (mut engine, mut sender, wakes) = start(Box::new(move |kind| {
        Ok(Box::new(FailsOnMarker(
            outputs::open(kind)?,
            failed.clone(),
        )))
    }));
    let set = |first| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    hold(&mut sender, 0, 11);
    hold(&mut sender, 512, 91);
    sender.send(set(1)).unwrap();
    let first = wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 11);

    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("a shifted run needs a new lane");
    };
    wakes.wake_at(first).unwrap();
    let ready = opening.open();
    let mut read = |at, first, failed| {
        let started = Instant::now();
        loop {
            engine.latest();
            wakes.wake_at(at).unwrap();
            let snapshot = engine.latest();
            if let Some(out) = snapshot.output(output(1))
                && out.universes.first() == first
                && out.counters.failed == failed
            {
                let mut health = HealthSample::new(Duration::ZERO, 0, Status::NotFound);
                assert!(snapshot.health_sample(output(1), &mut health));
                break (out.connection(), health.status);
            }
            assert!(
                started.elapsed() < usb::NEVER,
                "last state: {:?}",
                snapshot.output(output(1))
            ); // guard
        }
    };
    hold(&mut sender, 0, 73);
    let due = wakes.wake().unwrap();
    assert_eq!(failures.recv_timeout(usb::NEVER).unwrap(), 73);
    let reconnecting = Status::Reconnecting { failures: 1 };
    assert_eq!(read(due, 1, 1), (reconnecting, reconnecting));

    // The old send completes before finish returns, but there is no intervening
    // tick to collect its result. The first new-route send then fails as well.
    wakes.wake().unwrap();
    sender.finish(ready).unwrap();
    assert_eq!(failures.recv_timeout(usb::NEVER).unwrap(), 73);
    let due = wakes.wake().unwrap();
    assert_eq!(failures.recv_timeout(usb::NEVER).unwrap(), 91);
    let status = read(due, 2, 1);
    hold(&mut sender, 512, 21);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 21);
    let expected = Status::Reconnecting { failures: 3 };
    assert_eq!(status, (expected, expected));
}

#[test]
fn stopping_at_an_unfinished_cutoff_returns_the_writer_without_another_wake() {
    let (socket, kind) = ddp(Color::Rgb);
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let set = |first| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: Some(Rate::Sixty),
    };
    hold(&mut sender, 0, 11);
    sender.send(set(1)).unwrap();
    let at = wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[0], 11);
    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("rerouting needs another lane");
    };
    wakes.wake_at(at).unwrap();
    let opened = opening.open();
    wakes.cut().unwrap();
    let handoff = sender.publish_output(opened).unwrap().unwrap();
    let (finished, answer) = mpsc::channel();
    thread::scope(|scope| {
        scope.spawn(move || finished.send(handoff.wait()).unwrap());
        sender.stop_outputs();
        let result = answer.recv_timeout(usb::NEVER);
        wakes.finish().unwrap();
        assert_eq!(result, Ok(false), "T13 cancels the writer waiting at T5");
    });
    socket.set_nonblocking(true).unwrap();
    assert_eq!(
        socket.recv(&mut [0]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
