mod mock;

use engine::{
    ChannelNumbers, Command, Engine, Levels, OutputKind, OutputNumber, QUEUE, Rate, Refusal,
    RunNumbers, Sender, Status, Universes,
};
use mock::Heard;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

/// An engine with two universes and one mock output that takes both, at 60 a second.
fn start() -> (Engine, Sender, Arc<Heard>) {
    let (engine, mut sender) =
        Engine::start(Universes::new(2).unwrap()).expect("the tick thread starts");
    let heard = mock::attach(&mut sender, engine.universes(), 1, run(1, 2), Rate::Sixty);
    heard.two_more_frames();
    (engine, sender, heard)
}

fn channel(universe: u16, address: u16) -> ChannelNumbers {
    ChannelNumbers { universe, address }
}

fn set(universe: u16, address: u16, value: u8) -> Command<ChannelNumbers, RunNumbers, OutputKind> {
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
        let (engine, mut sender, first) = start();
        for command in &commands {
            assert_eq!(sender.send(*command), Ok(()), "{command:?}");
        }
        first.two_more_frames();
        assert_eq!(first.level(universe, address), out, "{commands:?} out");
        let mut universes = [Levels::ZERO; 2];
        engine.read(&mut universes);
        let level = universes[usize::from(universe - 1)].0[usize::from(address - 1)];
        assert_eq!(level, held, "{commands:?} held");
        assert!(engine.send_to_frame_p95().is_some(), "the send was timed");
    }
}

#[test]
fn a_command_that_cannot_be_right_is_refused_and_changes_nothing() {
    // (command, reason). Commands with an output kind are checked in engine-core and
    // over the API: the engine names no kind.
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
    ];
    let (engine, mut sender, first) = start();
    let before = engine.query();
    for (command, reason) in cases {
        assert_eq!(sender.send(command), Err(reason), "{command:?}");
    }
    first.two_more_frames();
    let mut universes = [Levels::ZERO; 2];
    let state = engine.read(&mut universes);
    assert_eq!(universes, [Levels::ZERO; 2]);
    assert!(!state.blackout);
    assert_eq!(state.outputs[1], None, "no second output");
    assert_eq!(
        state.outputs[0].map(|o| (o.universes, o.rate)),
        before.outputs[0].map(|o| (o.universes, o.rate))
    );
}

#[test]
fn outputs_are_attached_changed_and_removed() {
    #[derive(Clone, Copy, Debug)]
    enum Step {
        Attach(Rate),
        Remove,
    }
    // (steps for output 3, its rate afterwards, or none)
    let cases = [
        (vec![Step::Attach(Rate::FortyFour)], Some(Rate::FortyFour)),
        (vec![Step::Attach(Rate::Thirty)], Some(Rate::Thirty)),
        (
            vec![Step::Attach(Rate::FortyFour), Step::Attach(Rate::Sixty)],
            Some(Rate::Sixty),
        ),
        (vec![Step::Attach(Rate::Thirty), Step::Remove], None),
        (vec![Step::Remove], None),
    ];
    for (steps, rate) in cases {
        let (engine, mut sender, first) = start();
        let mut heard = None;
        for step in &steps {
            match *step {
                Step::Attach(rate) => {
                    let universes = engine.universes();
                    heard = Some(mock::attach(&mut sender, universes, 3, run(2, 1), rate));
                }
                Step::Remove => {
                    let remove = Command::RemoveOutput { output: output(3) };
                    assert_eq!(sender.send(remove), Ok(()));
                }
            }
        }
        assert_eq!(sender.send(set(2, 1, 77)), Ok(()));
        first.two_more_frames();
        first.two_more_frames(); // a mock at 30 a second had a frame too
        let state = engine.query();
        let third = state.outputs[2];
        assert_eq!(third.map(|o| o.rate), rate, "{steps:?}");
        if let (Some(third), Some(heard)) = (third, &heard) {
            assert_eq!(third.status, Status::Running, "{steps:?}");
            assert!(third.counters.accepted > 0, "{steps:?}");
            assert!(heard.frames() > 0, "{steps:?}");
            assert_eq!(heard.level(1, 1), 77, "universe 2 of the frame");
            assert!(engine.lateness(output(3)).is_some(), "{steps:?}");
        }
        let one = state.outputs[0].expect("output 1 stays");
        assert_eq!((one.status, one.rate), (Status::Running, Rate::Sixty));
        assert!(one.counters.accepted > 0);
        assert_eq!((one.counters.dropped, one.counters.failed), (0, 0));
    }
}

#[test]
fn a_full_queue_refuses_and_says_so() {
    let (_engine, mut sender, _first) = start();
    // A wake takes at most QUEUE commands; four queues' worth in a few microseconds
    // cannot all fit.
    let refused = (0..4 * QUEUE)
        .map(|_| sender.send(set(1, 1, 1)))
        .filter(|sent| *sent == Err(Refusal::QueueFull))
        .count();
    assert!(refused > 0, "nothing was refused");
}

#[test]
fn after_the_last_output_is_removed_a_command_still_lands_and_the_engine_stops() {
    // Hang guards of seconds, not a rate. Three seconds of outputs used to leave the
    // engine asleep for longer than the guard.
    let guard = Duration::from_secs(2);
    // (outputs: number and rate)
    let cases: [&[(u8, Rate)]; 2] = [
        &[(1, Rate::Sixty)],
        &[(1, Rate::Sixty), (2, Rate::FortyFour)],
    ];
    for outputs in cases {
        let (engine, mut sender) = Engine::start(Universes::ONE).expect("the tick thread starts");
        for &(number, rate) in outputs {
            mock::attach(&mut sender, Universes::ONE, number, run(1, 1), rate);
        }
        thread::sleep(Duration::from_secs(3));
        for &(number, _) in outputs {
            let remove = Command::RemoveOutput {
                output: output(number),
            };
            assert_eq!(sender.send(remove), Ok(()), "{outputs:?}");
        }
        let wait_until = |what: &str, done: &dyn Fn(&Engine) -> bool| {
            let started = Instant::now();
            while !done(&engine) {
                assert!(started.elapsed() < guard, "{outputs:?}: {what}");
                thread::sleep(Duration::from_millis(1));
            }
        };
        let removed = |engine: &Engine| engine.query().outputs.iter().all(Option::is_none);
        wait_until("the outputs were never removed", &removed);
        assert_eq!(sender.send(set(1, 1, 42)), Ok(()), "{outputs:?}");
        let landed = |engine: &Engine| {
            let mut universes = [Levels::ZERO];
            engine.read(&mut universes);
            universes[0].0[0] == 42
        };
        wait_until("the command never landed", &landed);
        let (stopped, wait) = mpsc::channel();
        thread::spawn(move || {
            drop(engine);
            stopped.send(()).unwrap();
        });
        assert_eq!(
            wait.recv_timeout(guard),
            Ok(()),
            "{outputs:?}: the engine never stopped"
        );
    }
}
