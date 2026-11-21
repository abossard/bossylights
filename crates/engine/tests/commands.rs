mod mock;
#[expect(
    clippy::disallowed_types,
    reason = "the fixture supplies an output kind"
)]
#[path = "../../outputs/tests/common/stream.rs"]
mod stream;

use engine::{
    Begun, ChannelNumbers, Color, Command, Engine, Levels, MAX_OUTPUTS, OutputNumber, QUEUE, Rate,
    Refusal, RunNumbers, Sender, Snapshot, Status, Universes,
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

fn set(universe: u16, address: u16, value: u8) -> Command {
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
        let (mut engine, mut sender, first) = start();
        for command in &commands {
            assert_eq!(sender.send(*command), Ok(()), "{command:?}");
        }
        first.took_everything_sent(&mut engine, &sender);
        assert_eq!(first.level(universe, address), out, "{commands:?} out");
        let picture = engine.fresh();
        let level = picture.universes()[usize::from(universe - 1)].0[usize::from(address - 1)];
        assert_eq!(level, held, "{commands:?} held");
        assert!(picture.send_to_frame_p95().is_some(), "the send was timed");
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
    ];
    let (mut engine, mut sender, _first) = start();
    let before = engine.fresh().state;
    for (command, reason) in cases {
        assert_eq!(sender.send(command), Err(reason), "{command:?}");
    }
    let picture = engine.fresh();
    let state = picture.state;
    assert_eq!(picture.universes(), [Levels::ZERO; 2]);
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
        let (mut engine, mut sender, _first) = start();
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
        if let (Some(_), Some(heard)) = (rate, &heard) {
            heard.took_everything_sent(&mut engine, &sender);
            // Running from its second frame taken; the first may carry the command.
            mock::wait_until("output 3 never ran", || {
                engine.latest().state.outputs[2].is_some_and(|o| o.status == Status::Running)
            });
        }
        let picture = engine.fresh();
        let state = picture.state;
        let third = state.outputs[2];
        assert_eq!(third.map(|o| o.rate), rate, "{steps:?}");
        if let (Some(third), Some(heard)) = (third, &heard) {
            assert_eq!(third.status, Status::Running, "{steps:?}");
            assert!(third.counters.accepted > 0, "{steps:?}");
            assert!(heard.frames() > 0, "{steps:?}");
            assert_eq!(heard.level(1, 1), 77, "universe 2 of the frame");
            assert!(picture.lateness(output(3)).is_some(), "{steps:?}");
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
        let (mut engine, mut sender) =
            Engine::start(Universes::ONE).expect("the tick thread starts");
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
        let mut wait_until = |what: &str, done: &dyn Fn(&Snapshot) -> bool| {
            let started = Instant::now();
            while !done(engine.latest()) {
                assert!(started.elapsed() < guard, "{outputs:?}: {what}"); // guard
                thread::sleep(Duration::from_millis(1)); // poll
            }
        };
        let removed = |picture: &Snapshot| picture.state.outputs.iter().all(Option::is_none);
        wait_until("the outputs were never removed", &removed);
        assert_eq!(sender.send(set(1, 1, 42)), Ok(()), "{outputs:?}");
        let landed = |picture: &Snapshot| picture.universes()[0].0[0] == 42;
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

/// A device whose drop waits until the test lets it go: a port that closes slowly.
#[expect(
    clippy::disallowed_types,
    reason = "only the test device waits to close"
)]
struct SlowToFree(mpsc::Receiver<()>);

impl engine::Device for SlowToFree {
    fn take(&mut self, _frame: &[u8]) -> engine::Outcome {
        engine::Outcome::Accepted
    }
}

impl Drop for SlowToFree {
    fn drop(&mut self) {
        let _ = self.0.recv_timeout(Duration::from_secs(10));
    }
}

#[test]
fn a_device_the_full_queue_refuses_is_freed_without_holding_up_the_sender() {
    // (attempt) until one attach finds the queue full: a wake may empty it in between.
    let started = Instant::now();
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the queue was never full"
        );
        let (_engine, mut sender) = Engine::start(Universes::ONE).unwrap();
        let run = run(1, 1).check(Universes::ONE).unwrap();
        let (let_go, hold) = mpsc::channel();
        let device = Box::new(SlowToFree(hold));
        while sender.send(set(1, 1, 1)) != Err(Refusal::QueueFull) {}
        let (done, answered) = mpsc::channel();
        let attaching = thread::spawn(move || {
            let _ = done.send(sender.attach(output(1), device, run, Rate::Sixty));
        });
        let answer = answered.recv_timeout(Duration::from_secs(2));
        drop(let_go);
        attaching.join().unwrap();
        match answer {
            Ok(Err(Refusal::QueueFull)) => return,
            Ok(_) => {} // a wake emptied the queue first: try again
            Err(_) => panic!("the sender waited for the refused device to be freed"),
        }
    }
}

/// Keep the removed device on its way back until the caller drops the handle.
#[expect(clippy::disallowed_types, reason = "the test holds the device's close")]
fn remove_slow_device(sender: &mut Sender) -> mpsc::Sender<()> {
    let (let_go, hold) = mpsc::channel();
    let first = output(1);
    sender
        .attach(
            first,
            Box::new(SlowToFree(hold)),
            run(1, 1).check(Universes::ONE).unwrap(),
            Rate::Sixty,
        )
        .unwrap();
    sender
        .send(Command::RemoveOutput { output: first })
        .unwrap();
    let_go
}

#[test]
fn removing_an_output_needs_a_free_return_only_if_it_has_a_device() {
    let universes = Universes::new(2).unwrap();
    let (mut engine, mut sender) = Engine::start(universes).unwrap();
    let (_receiver, kind) = stream::receiver(Color::Rgb);
    let set = |count| Command::SetOutput {
        output: output(3),
        kind,
        universes: run(1, count),
        rate: None,
    };
    sender.send(set(1)).unwrap();
    let closing = remove_slow_device(&mut sender);
    mock::attach(&mut sender, universes, 2, run(1, 1), Rate::Sixty);

    // One device is closing. Leave just two more returns for a restart to
    // hand back its old lane and then install its replacement.
    let patch = sender.patch();
    for n in 0..QUEUE + MAX_OUTPUTS - 3 {
        if n % (QUEUE / 2) == 0 {
            engine.fresh();
        }
        sender.load((*patch).clone()).unwrap();
    }
    engine.fresh();
    let Begun::Opening(restart) = sender.begin(set(2), &[]).unwrap() else {
        panic!("the longer route needs to reopen");
    };
    assert_eq!(sender.load((*patch).clone()), Err(Refusal::QueueFull));
    for (number, expected) in [(1, Ok(())), (2, Err(Refusal::QueueFull))] {
        let sent = sender.sent();
        let project = sender.project();
        let remove = Command::RemoveOutput {
            output: output(number),
        };
        assert_eq!(sender.send(remove), expected, "output {number}");
        assert_eq!(sender.sent(), sent + u64::from(expected.is_ok()));
        assert_eq!(sender.project(), project);
        assert_eq!(engine.fresh().applied(), sender.sent());
    }
    drop(closing);
    sender.finish(restart.open()).unwrap();
}

#[test]
fn a_reopen_gives_a_closing_port_its_full_second_before_trying_again() {
    let (attempted, attempts) = mpsc::channel();
    let open = Box::new(move |_| {
        attempted.send(Instant::now()).unwrap();
        Err(std::io::Error::other("the old port is still closing"))
    });
    let (_engine, mut sender) = Engine::start_with(Universes::ONE, open).unwrap();
    let (_receiver, kind) = stream::receiver(Color::Rgb);
    let _closing = remove_slow_device(&mut sender);
    let reopen = Command::SetOutput {
        output: output(1),
        kind,
        universes: run(1, 1),
        rate: None,
    };
    let before = Instant::now();
    assert_eq!(sender.send(reopen), Err(Refusal::CannotOpen));
    let attempted = attempts.try_recv().unwrap();
    assert!(attempted.duration_since(before) >= Duration::from_secs(1));
}
