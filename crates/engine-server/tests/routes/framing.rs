#[test]
fn one_v11_rgb_universe_hands_the_device_a_510_byte_box() {
    struct Measured {
        ddp: Box<dyn engine::Device>,
        length: Arc<AtomicUsize>,
    }

    impl engine::Device for Measured {
        fn take(&mut self, bytes: &[u8]) -> Outcome {
            self.length.store(bytes.len(), Ordering::Release);
            self.ddp.take(bytes)
        }
    }

    let (socket, kind) = ddp(Color::Rgb);
    let length = Arc::new(AtomicUsize::new(0));
    let (_engine, mut sender, wakes) = start(Box::new({
        let length = Arc::clone(&length);
        move |kind| {
            Ok(Box::new(Measured {
                ddp: outputs::open(kind)?,
                length: Arc::clone(&length),
            }))
        }
    }));
    sender
        .send(Command::SetOutput {
            output: output(1),
            kind,
            universes: RunNumbers { first: 1, count: 1 },
            rate: None,
        })
        .unwrap();
    hold(&mut sender, 509, 91);
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 510).2[509], 91);
    assert_eq!(length.load(Ordering::Acquire), 510);
}

#[test]
fn four_rgbw_rows_form_one_packet_without_padding() {
    let (_engine, mut sender, mut wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgbw);
    attach_ddp(&mut sender, kind, &ROWS);
    for (index, value) in [
        (0, 11),
        (512, 22),
        (1024, 33),
        (1536, 44),
        (1855, 55),
        (320, 99),
        (511, 99),
        (832, 99),
    ] {
        hold(&mut sender, index, value);
    }
    let at = wakes.wake().unwrap();
    let (_, _, payload) = packet(&socket, 0x41, 0x1B, 0, 1280);
    let mut expected = vec![0; 1280];
    for (index, value) in [(0, 11), (320, 22), (640, 33), (960, 44), (1279, 55)] {
        expected[index] = value;
    }
    assert_eq!(payload, expected);
    no_more_packets(&socket, &mut wakes, at);
}

#[test]
fn a_routed_rgb_frame_keeps_its_sequence_across_the_packet_boundary() {
    let (_engine, mut sender, mut wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgb);
    attach_ddp(
        &mut sender,
        kind,
        &[span(0, 512), span(512, 512), span(1024, 512)],
    );
    let mut expected = vec![0; 1536];
    for (index, value) in [(0, 11), (1439, 22), (1440, 33), (1535, 44)] {
        hold(&mut sender, index, value);
        expected[usize::from(index)] = value;
    }
    let at = wakes.wake().unwrap();
    let (source, sequence, head) = packet(&socket, 0x40, 0x0B, 0, 1440);
    let (last_source, last_sequence, tail) = packet(&socket, 0x41, 0x0B, 1440, 96);
    assert_eq!((last_source, last_sequence), (source, sequence));
    assert_eq!([head, tail].concat(), expected);
    no_more_packets(&socket, &mut wakes, at);
}

#[test]
fn a_pixel_split_across_spans_is_checked_as_one_unit() {
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let (socket, kind) = ddp(Color::Rgb);
    let (_unused, rgbw) = ddp(Color::Rgbw);
    assert!(Route::new(outputs::limits(rgbw), &[span(0, 319)]).is_err());
    assert_eq!(sender.project().output(output(1)), None);
    attach_ddp(&mut sender, kind, &[span(10, 2), span(100, 1)]);
    for (index, value) in [(10, 11), (11, 22), (100, 33)] {
        hold(&mut sender, index, value);
    }
    wakes.wake().unwrap();
    assert_eq!(packet(&socket, 0x41, 0x0B, 0, 3).2, [11, 22, 33]);
}

#[test]
fn a_route_that_reaches_past_this_engine_is_refused() {
    let (_engine, mut sender, _wakes) = start(Box::new(outputs::open));
    let (_socket, kind) = ddp(Color::Rgb);
    let route = Route::new(outputs::limits(kind), &[span(2047, 3)]).unwrap();
    let one = output(1);
    let before = sender.sent();
    let result = sender.attach_route(one, outputs::open(kind).unwrap(), route, Rate::Sixty);
    assert_eq!(result, Err(Refusal::NoSuchUniverse { universes: 4 }));
    assert_eq!(sender.project().output(one), None);
    assert_eq!(sender.sent(), before);
}

#[test]
fn the_second_universe_reaches_all_512_usb_slots() {
    let (_engine, mut sender, wakes) = start(Box::new(outputs::open));
    let mut end = attach_usb(&mut sender, 512);
    for (index, value) in [(0, 99), (511, 99), (512, 11), (1023, 55)] {
        hold(&mut sender, index, value);
    }
    wakes.wake().unwrap();
    let message = usb::read_message(&mut end);
    assert_eq!(&message[..5], [0x7E, 6, 1, 2, 0]);
    let mut expected = [0; 512];
    expected[0] = 11;
    expected[511] = 55;
    assert_eq!(&message[5..517], expected);
    assert_eq!(message[517], 0xE7);
}
