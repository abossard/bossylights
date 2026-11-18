mod common;

use common::Receiver;
use engine::{
    ChannelNumbers, Color, Command, Engine, Levels, OutputKind, OutputNumber, QUEUE, Rate,
    Refusal, RunNumbers, Sender, Status, Universes,
};
use std::net::SocketAddrV4;
use std::time::Duration;

/// An engine with two universes and one DDP output that sends both to a receiver here.
fn start() -> (Engine, Sender, Receiver) {
    let (engine, mut sender) =
        Engine::start(Universes::new(2).unwrap()).expect("the tick thread starts");
    let receiver = Receiver::start();
    receiver.add(&mut sender, 2);
    (engine, sender, receiver)
}

fn channel(universe: u16, address: u16) -> ChannelNumbers {
    ChannelNumbers { universe, address }
}

fn set(universe: u16, address: u16, value: u8) -> Command<ChannelNumbers, RunNumbers> {
    Command::SetChannel {
        channel: channel(universe, address),
        value,
    }
}

fn output(number: u8) -> OutputNumber {
    OutputNumber::try_from(number).unwrap()
}

fn run(first: u16, count: u16) -> RunNumbers {
    RunNumbers { first, count }
}

#[test]
fn a_command_reaches_the_frame_and_the_state() {
    let release = |universe, address| Command::ReleaseChannel {
        channel: channel(universe, address),
    };
    let blackout = |on| Command::Blackout { on };
    // (commands, universe, address, held level, level in the frame)
    let cases = [
        (vec![set(1, 1, 255)], 1, 1, 255, 255),
        (vec![set(2, 512, 7)], 2, 512, 7, 7),
        (vec![set(1, 5, 9), release(1, 5)], 1, 5, 0, 0),
        (
            vec![set(1, 5, 9), set(2, 5, 9), Command::ReleaseAll],
            2,
            5,
            0,
            0,
        ),
        (vec![set(1, 5, 9), blackout(true)], 1, 5, 9, 0),
        (
            vec![set(1, 5, 9), blackout(true), blackout(false)],
            1,
            5,
            9,
            9,
        ),
    ];
    for (commands, universe, address, held, out) in cases {
        let (engine, mut sender, receiver) = start();
        for command in &commands {
            assert_eq!(sender.send(*command), Ok(()), "{command:?}");
        }
        receiver.two_more_frames();
        assert_eq!(receiver.level(universe, address), out, "{commands:?} out");
        let mut universes = [Levels::ZERO; 2];
        engine.read(&mut universes);
        let level = universes[usize::from(universe - 1)].0[usize::from(address - 1)];
        assert_eq!(level, held, "{commands:?} held");
        assert!(engine.send_to_frame_p95().is_some(), "the send was timed");
    }
}

#[test]
fn a_command_that_cannot_be_right_is_refused_and_changes_nothing() {
    let unreachable = OutputKind::Ddp {
        address: SocketAddrV4::new([127, 0, 0, 1].into(), 0),
        color: Color::Rgb,
    };
    let set_output = |kind, universes| Command::SetOutput {
        output: output(2),
        kind,
        universes,
        rate: None,
    };
    // (command, reason)
    let cases = [
        (set(3, 1, 9), Refusal::NoSuchUniverse { universes: 2 }),
        (set(0, 1, 9), Refusal::NoSuchUniverse { universes: 2 }),
        (set(1, 0, 9), Refusal::NoSuchAddress),
        (set(1, 513, 9), Refusal::NoSuchAddress),
        (
            Command::Tick {
                now: Duration::ZERO,
            },
            Refusal::NotACommand,
        ),
        (
            set_output(OutputKind::Mock, run(2, 2)),
            Refusal::NoSuchUniverse { universes: 2 },
        ),
        (
            set_output(OutputKind::Mock, run(0, 1)),
            Refusal::NoSuchUniverse { universes: 2 },
        ),
        (set_output(unreachable, run(1, 1)), Refusal::CannotOpen),
    ];
    let (engine, mut sender, receiver) = start();
    let before = engine.query();
    for (command, reason) in cases {
        assert_eq!(sender.send(command), Err(reason), "{command:?}");
    }
    receiver.two_more_frames();
    let mut universes = [Levels::ZERO; 2];
    let state = engine.read(&mut universes);
    assert_eq!(universes, [Levels::ZERO; 2]);
    assert!(!state.blackout);
    assert_eq!(state.outputs[1], None, "no second output");
    assert_eq!(state.outputs[0].map(|o| o.kind), before.outputs[0].map(|o| o.kind));
}

#[test]
fn outputs_are_added_changed_and_removed() {
    let mock = |rate| Command::SetOutput {
        output: output(3),
        kind: OutputKind::Mock,
        universes: run(2, 1),
        rate,
    };
    let remove = Command::RemoveOutput { output: output(3) };
    // (commands, the output's rate afterwards, or none)
    let cases = [
        (vec![mock(None)], Some(Rate::FortyFour)),
        (vec![mock(Some(Rate::Thirty))], Some(Rate::Thirty)),
        (vec![mock(None), mock(Some(Rate::Sixty))], Some(Rate::Sixty)),
        (vec![mock(None), remove], None),
        (vec![remove], None),
    ];
    for (commands, rate) in cases {
        let (engine, mut sender, receiver) = start();
        for command in &commands {
            assert_eq!(sender.send(*command), Ok(()), "{command:?}");
        }
        receiver.two_more_frames();
        receiver.two_more_frames(); // the mock, at 30 a second, had its first frame
        let state = engine.query();
        let third = state.outputs[2];
        assert_eq!(third.map(|o| o.rate), rate, "{commands:?}");
        if let Some(third) = third {
            assert_eq!(third.status, Status::Running, "{commands:?}");
            assert!(third.counters.accepted > 0, "{commands:?}");
            assert!(engine.lateness(output(3)).is_some(), "{commands:?}");
        }
        let ddp = state.outputs[0].expect("the DDP output stays");
        assert_eq!(ddp.status, Status::Running);
        assert_eq!(ddp.rate, Rate::Sixty, "the default for DDP");
        assert!(ddp.counters.accepted > 0);
        assert_eq!((ddp.counters.dropped, ddp.counters.failed), (0, 0));
    }
}

#[test]
fn a_full_queue_refuses_and_says_so() {
    let (_engine, mut sender, _receiver) = start();
    // A wake takes at most QUEUE commands; four queues' worth in a few microseconds
    // cannot all fit.
    let refused = (0..4 * QUEUE)
        .map(|_| sender.send(set(1, 1, 1)))
        .filter(|sent| *sent == Err(Refusal::QueueFull))
        .count();
    assert!(refused > 0, "nothing was refused");
}
