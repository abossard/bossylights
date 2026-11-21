use engine::{
    ChannelNumbers, Command, Engine, OutputNumber, Rate, RunNumbers, Universes, Unsent,
};
use outputs::{Device, Outcome};
use std::sync::mpsc;
use std::time::Duration;

struct Output;

impl Device for Output {
    fn take(&mut self, _: &[u8]) -> Outcome {
        Outcome::Accepted
    }
}

struct HeldOutput {
    heard: mpsc::Sender<u8>,
    release_first: Option<mpsc::Receiver<()>>,
}

impl Device for HeldOutput {
    fn take(&mut self, bytes: &[u8]) -> Outcome {
        self.heard.send(bytes[0]).unwrap();
        if let Some(release_first) = self.release_first.take() {
            release_first.recv().unwrap();
        }
        Outcome::Accepted
    }
}

#[test]
fn stopping_cancels_a_queued_frame_even_with_a_remove_at_the_next_cutoff() {
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(Universes::ONE, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();

    let (send_frame, frames) = mpsc::channel();
    let (release_first, released) = mpsc::channel();
    let output = OutputNumber::try_from(1).unwrap();
    let run = RunNumbers { first: 1, count: 1 }
        .check(Universes::ONE)
        .unwrap();
    let rate = Rate::Sixty;
    sender
        .attach(
            output,
            Box::new(HeldOutput {
                heard: send_frame,
                release_first: Some(released),
            }),
            run,
            rate,
        )
        .unwrap();

    let first = wakes.wake().unwrap();
    assert_eq!(frames.recv_timeout(Duration::from_secs(2)), Ok(0));
    sender
        .send(Command::SetChannel {
            channel: ChannelNumbers {
                universe: 1,
                address: 1,
            },
            value: 91,
        })
        .unwrap();
    wakes.wake_at(first + rate.period()).unwrap();

    sender.send(Command::RemoveOutput { output }).unwrap();
    sender.stop_outputs();
    release_first.send(()).unwrap();

    // No further wake: the output is still installed on the tick. T13 must
    // prevent a new write after the already-started one is released.
    let next = frames.recv_timeout(Duration::from_secs(2));
    assert!(next.is_err(), "frame after stop: {next:?}");
}

/// output-cutover T13 and "any other pair": in Stopped, `begin` starts no switch. The
/// edit is refused `Opening` and the project stays.
#[test]
fn a_stopped_engine_begins_no_switch() {
    use engine::{Color, OutputKind, Refusal};
    use std::net::{Ipv4Addr, SocketAddrV4};

    let universes = Universes::new(2).unwrap();
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(universes, Box::new(|_| Ok(Box::new(Output)))).unwrap();
    wakes.hold().unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let set = |count| Command::SetOutput {
        output,
        kind: OutputKind::Ddp {
            address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
            color: Color::Rgbw,
        },
        universes: RunNumbers { first: 1, count },
        rate: None,
    };
    sender.send(set(1)).unwrap();
    wakes.wake().unwrap();
    let prior = sender.project().output(output);
    sender.stop_outputs();
    let begun = sender.begin(set(2), &[]);
    assert_eq!(
        (begun.err(), sender.project().output(output)),
        (Some(Refusal::Opening), prior)
    );
}

#[test]
fn finishing_an_open_after_stop_does_not_accept_or_queue_the_route() {
    use engine::{Begun, Color, OutputKind};
    use std::net::{Ipv4Addr, SocketAddrV4};

    let universes = Universes::new(2).unwrap();
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(universes, Box::new(|_| Ok(Box::new(Output)))).unwrap();
    wakes.hold().unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let set = |count| Command::SetOutput {
        output,
        kind: OutputKind::Ddp {
            address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
            color: Color::Rgbw,
        },
        universes: RunNumbers { first: 1, count },
        rate: None,
    };

    sender.send(set(1)).unwrap();
    wakes.wake().unwrap();
    let prior = sender.project().output(output);
    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("the larger route needs a new lane");
    };
    // The nonblocking opener waits for its reuse probe to be reclaimed.
    wakes.wake().unwrap();
    let opened = opening.open();
    sender.stop_outputs();

    let sent = sender.sent();
    let result = sender.finish(opened);
    // T13: publish in Stopped is refused, the project goes back, the writer hears Stopping
    assert_eq!(
        (result, sender.sent(), sender.project().output(output)),
        (Err(Unsent::Stopping), sent, prior)
    );
}

#[test]
fn a_stop_after_the_switch_is_queued_still_answers_stopping() {
    use engine::{Begun, Color, OutputKind};
    use std::net::{Ipv4Addr, SocketAddrV4};

    let universes = Universes::new(2).unwrap();
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(universes, Box::new(|_| Ok(Box::new(Output)))).unwrap();
    wakes.hold().unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let set = |count| Command::SetOutput {
        output,
        kind: OutputKind::Ddp {
            address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
            color: Color::Rgbw,
        },
        universes: RunNumbers { first: 1, count },
        rate: None,
    };
    sender.send(set(1)).unwrap();
    wakes.wake().unwrap();
    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("the larger route needs a new lane");
    };
    wakes.wake().unwrap();
    let opened = opening.open();
    // The race: the life was Running at the check, the stop lands before T8.
    let handoff = sender
        .publish_output(opened)
        .unwrap()
        .expect("an old lane to hand over");
    sender.stop_outputs();
    assert!(!handoff.wait(), "T13: the waiting writer gets Stopping");
}

#[test]
fn abandoning_an_opening_does_not_hold_the_only_opener_forever() {
    use engine::{Begun, Color, OutputKind};
    use std::net::{Ipv4Addr, SocketAddrV4};

    let (_engine, mut sender, wakes) =
        Engine::start_stepped(Universes::ONE, Box::new(|_| Ok(Box::new(Output)))).unwrap();
    wakes.hold().unwrap();
    let set = |number| Command::SetOutput {
        output: OutputNumber::try_from(number).unwrap(),
        kind: OutputKind::Ddp {
            address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
            color: Color::Rgbw,
        },
        universes: RunNumbers { first: 1, count: 1 },
        rate: None,
    };

    let Begun::Opening(cancelled) = sender.begin(set(1), &[]).unwrap() else {
        panic!("the new output needs an opener");
    };
    drop(cancelled);
    assert_eq!(sender.send(set(2)), Ok(()));
}

/// output-cutover T4, "opening abandoned": an opened lane let go before `finish` gives
/// the opener back, the old lane serves on, the project is as before.
#[test]
fn abandoning_an_opened_restart_puts_the_output_back() {
    use engine::{Begun, Color, OutputKind};
    use std::net::{Ipv4Addr, SocketAddrV4};

    let universes = Universes::new(2).unwrap();
    let (_engine, mut sender, wakes) =
        Engine::start_stepped(universes, Box::new(|_| Ok(Box::new(Output)))).unwrap();
    wakes.hold().unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let set = |count| Command::SetOutput {
        output,
        kind: OutputKind::Ddp {
            address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
            color: Color::Rgbw,
        },
        universes: RunNumbers { first: 1, count },
        rate: None,
    };
    sender.send(set(1)).unwrap();
    wakes.wake().unwrap();
    let prior = sender.project().output(output);
    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("the larger route needs a new lane");
    };
    wakes.wake().unwrap();
    let opened = opening.open();
    assert!(sender.cutover_pending(output), "preparing");
    drop(opened);

    // The next edit takes the abandoned one back first.
    assert_eq!(sender.send(Command::ReleaseAll), Ok(()));
    assert_eq!(
        (sender.cutover_pending(output), sender.project().output(output)),
        (false, prior)
    );
    // And the opener is there again: the same restart goes through.
    let Begun::Opening(again) = sender.begin(set(2), &[]).unwrap() else {
        panic!("the larger route needs a new lane");
    };
    wakes.wake().unwrap();
    assert_eq!(sender.finish(again.open()), Ok(()));
}

/// output-cutover T13: a stop between `begin` and `open` opens no device; the writer
/// hears `Stopping`, nothing is queued, the project stays.
#[test]
fn a_stop_before_open_opens_no_device() {
    use engine::{Begun, Color, OutputKind};
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let opens = Arc::new(AtomicUsize::new(0));
    let universes = Universes::new(2).unwrap();
    let (_engine, mut sender, wakes) = Engine::start_stepped(
        universes,
        Box::new({
            let opens = Arc::clone(&opens);
            move |_| {
                opens.fetch_add(1, Ordering::AcqRel);
                Ok(Box::new(Output))
            }
        }),
    )
    .unwrap();
    wakes.hold().unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    // Another address: the restart opens a fresh device instead of reusing the socket.
    let set = |port| Command::SetOutput {
        output,
        kind: OutputKind::Ddp {
            address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, port),
            color: Color::Rgbw,
        },
        universes: RunNumbers { first: 1, count: 1 },
        rate: None,
    };
    sender.send(set(4048)).unwrap();
    wakes.wake().unwrap();
    let prior = sender.project().output(output);
    let Begun::Opening(opening) = sender.begin(set(4049), &[]).unwrap() else {
        panic!("another address needs a new lane");
    };
    wakes.wake().unwrap();
    let before = opens.load(Ordering::Acquire);
    sender.stop_outputs();
    let opened = opening.open();
    let sent = sender.sent();
    let result = sender.finish(opened);
    assert_eq!(
        (
            opens.load(Ordering::Acquire) - before,
            result,
            sender.sent(),
            sender.project().output(output)
        ),
        (0, Err(Unsent::Stopping), sent, prior)
    );
}
