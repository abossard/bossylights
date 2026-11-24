#[test]
fn a_returning_usb_port_sends_only_the_run_accepted_while_it_was_lost() {
    let kind = OutputKind::DmxUsbPro {
        adapter: Adapter::FIRST,
    };
    let (find, mut fake) = usb::Fake::new(true);
    let mut find = Some(find);
    let (monitor, opened) = mpsc::channel();
    let (_engine, mut sender, wakes) = start(Box::new(move |_| {
        let pro = DmxUsbPro::open(find.take().ok_or(std::io::ErrorKind::AddrInUse)?)?;
        monitor.send(pro.lost_with()).unwrap();
        Ok(Box::new(pro))
    }));
    let set = |first| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count: 1 },
        rate: None,
    };
    hold(&mut sender, 0, 11);
    hold(&mut sender, 512, 91);
    sender.send(set(1)).unwrap();
    let lost = opened.recv_timeout(usb::NEVER).unwrap();
    fake.plugged().store(false, Ordering::Release);
    drop(fake.next_end());
    // A failed write is normally replayed when the physical port returns.
    let started = Instant::now();
    while lost.get().is_none() {
        assert!(
            started.elapsed() < usb::NEVER,
            "the port never reported a failed write"
        ); // guard
        wakes.wake().unwrap();
    }
    let Begun::Opening(restart) = sender.begin(set(2), &[]).unwrap() else {
        panic!("a shifted route needs a new lane");
    };
    wakes.wake().unwrap();
    sender.finish(restart.open()).unwrap();

    fake.plugged().store(true, Ordering::Release);
    let mut end = fake.next_end();
    wakes.run();
    assert_eq!(usb::read_message(&mut end)[5], 91);
}

#[test]
fn route_edits_keep_every_copy_of_a_patched_channel_at_its_output_byte() {
    let before = [span(16, 4), span(8, 2), span(16, 2)];
    let split = [span(16, 1), span(17, 3), span(8, 2), span(16, 2)];
    let unpatched = [span(16, 4), span(24, 2), span(16, 2)];
    let dropped_copy = [span(16, 4), span(8, 2), span(24, 2)];
    let swapped = [span(18, 2), span(16, 2), span(8, 2), span(16, 2)];
    let displaced = Err(Refusal::StreamsFixture { fixture: 1 });
    for (after, expected) in [
        (split.as_slice(), Ok(())),
        (unpatched.as_slice(), Ok(())),
        (dropped_copy.as_slice(), displaced),
        (swapped.as_slice(), displaced),
    ] {
        let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
        let (socket, kind) = ddp(Color::Rgbw);
        attach_ddp(&mut sender, kind, &before);
        sender
            .send(Command::PatchStrip {
                name: Text::new("Pixel").unwrap(),
                pixels: Pixels::try_from(1).unwrap(),
                color: Color::Rgbw,
                at: ChannelNumbers {
                    universe: 1,
                    address: 17,
                },
            })
            .unwrap();
        hold(&mut sender, 16, 91);
        let route = Route::new(outputs::limits(kind), after).unwrap();
        let reply =
            sender.attach_route(output(1), outputs::open(kind).unwrap(), route, Rate::Sixty);
        assert_eq!(reply, expected, "{after:?}");
        wakes.wake().unwrap();
        assert_eq!(
            packet(&socket, 0x41, 0x1B, 0, 8).2,
            [91, 0, 0, 0, 0, 0, 91, 0]
        );
    }
}

#[test]
fn a_patch_during_an_open_must_fit_both_routes_at_the_same_output_bytes() {
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgbw);
    let set = |first, count| Command::SetOutput {
        output: output(1),
        kind,
        universes: RunNumbers { first, count },
        rate: None,
    };
    sender.send(set(1, 2)).unwrap();
    let Begun::Opening(opening) = sender.begin(set(2, 3), &[]).unwrap() else {
        panic!("a run of a new length must restart");
    };
    // Universe 1 would be dropped, universe 2 would move from byte 512 to 0,
    // and universe 3 is new to the output.
    for (universe, accepted) in [(1, false), (2, false), (3, true)] {
        let reply = sender.send(Command::PatchStrip {
            name: Text::new("Pixel").unwrap(),
            pixels: Pixels::try_from(1).unwrap(),
            color: Color::Rgbw,
            at: ChannelNumbers {
                universe,
                address: 1,
            },
        });
        assert_eq!(reply.is_ok(), accepted, "{universe}");
    }
    hold(&mut sender, 1024, 91);
    wakes.wake().unwrap();
    sender.finish(opening.open()).unwrap();
    socket.set_nonblocking(true).unwrap();
    while socket.recv(&mut [0; 2048]).is_ok() {}
    socket.set_nonblocking(false).unwrap();
    wakes.wake().unwrap();
    // After the shift, universe 3 begins at byte 512 of the stream.
    assert_eq!(packet(&socket, 0x40, 0x1B, 0, 1440).2[512], 91);
}

#[test]
fn the_first_ddp_row_and_usb_mirror_carry_the_same_channel_values() {
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgbw);
    let mut row = [0; 320];
    // Fill the row before attaching the outputs. Each batch fits in the command queue.
    for (index, value) in (0u16..).zip(&mut row) {
        *value = (index % 251 + 1) as u8;
        hold(&mut sender, index, *value);
        if usize::from(index) + 1 == engine::QUEUE {
            wakes.wake().unwrap();
        }
    }
    let mut end = attach_usb(&mut sender, 0);
    attach_ddp(&mut sender, kind, &ROWS);
    wakes.wake().unwrap();
    let (_, _, payload) = packet(&socket, 0x41, 0x1B, 0, 1280);
    let message = usb::read_message(&mut end);
    assert_eq!(message[5..325], row);
    assert_eq!(&payload[..320], &message[5..325]);
}
