//! A same-adapter reroute preserves failures already reported by the old lane.

#[path = "../../outputs/tests/common/stream.rs"]
mod stream;

use engine::{
    Begun, ChannelNumbers, Color, Command, Device, Engine, Event, Outcome, OutputNumber, Rate,
    RunNumbers, Status, Universes,
};
use std::net::UdpSocket;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

const GUARD: Duration = Duration::from_secs(10);

struct LosingAdapter {
    ddp: Box<dyn Device>,
    attempted: Sender<u8>,
    fail_first_write: Receiver<()>,
    restored: Receiver<()>,
}

impl Device for LosingAdapter {
    fn take(&mut self, bytes: &[u8]) -> Outcome {
        match bytes[0] {
            31 => {
                self.attempted.send(31).unwrap();
                let _ = self.fail_first_write.recv_timeout(GUARD);
                Outcome::Failed
            }
            32 => {
                self.attempted.send(32).unwrap();
                Outcome::Failed
            }
            91 => {
                let _ = self.restored.recv_timeout(GUARD);
                self.ddp.take(bytes)
            }
            _ => self.ddp.take(bytes),
        }
    }
}

fn first_slot(socket: &UdpSocket) -> u8 {
    let mut packet = [0; 1536];
    assert_eq!(socket.recv(&mut packet).unwrap(), 520);
    packet[10]
}

#[test]
fn a_failure_returned_before_reroute_is_kept_on_the_same_adapter() {
    let (socket, kind) = stream::receiver(Color::Rgb);
    let output = OutputNumber::try_from(1).unwrap();
    let (attempted, attempts) = mpsc::channel();
    let (release, fail_first_write) = mpsc::channel();
    let (restore, restored) = mpsc::channel();
    let mut first = Some((attempted, fail_first_write, restored));
    let (mut engine, mut sender, mut wakes) = Engine::start_stepped(
        Universes::new(2).unwrap(),
        Box::new(move |kind| {
            let (attempted, fail_first_write, restored) =
                first.take().expect("the same adapter must be kept");
            Ok(Box::new(LosingAdapter {
                ddp: outputs::open(kind)?,
                attempted,
                fail_first_write,
                restored,
            }))
        }),
    )
    .unwrap();
    wakes.hold().unwrap();
    let set = |first| Command::SetOutput {
        output,
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

    sender.send(hold(1, 11)).unwrap();
    sender.send(hold(2, 91)).unwrap();
    sender.send(set(1)).unwrap();
    wakes.wake().unwrap();
    assert_eq!(first_slot(&socket), 11);
    let at = wakes.wake().unwrap();
    assert_eq!(first_slot(&socket), 11);
    let started = Instant::now();
    loop {
        if engine.latest().state.outputs[0].is_some_and(|out| out.status == Status::Running) {
            break;
        }
        assert!(started.elapsed() < GUARD, "adapter never reached Running"); // guard
        wakes.wake_at(at).unwrap();
    }

    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("the new run needs a lane");
    };
    wakes.wake_at(at).unwrap(); // Let the reuse probe reach the reclaimer.
    let opened = opening.open();

    sender.send(hold(1, 31)).unwrap();
    let lost_at = wakes.wake().unwrap();
    assert_eq!(attempts.recv_timeout(GUARD).unwrap(), 31);
    sender.send(hold(1, 32)).unwrap();
    wakes.wake().unwrap();
    release.send(()).unwrap();
    assert_eq!(attempts.recv_timeout(GUARD).unwrap(), 32);
    // The adapter can only start 32 after it has returned 31's failure to the tick.
    // The tick is held and so has yet to consume that result.
    sender.finish(opened).unwrap();
    engine.latest();
    wakes.wake().unwrap();
    let rerouted = engine.latest().state.outputs[0].unwrap();
    assert_eq!(rerouted.universes.first(), 2);
    restore.send(()).unwrap();
    drop(restore);
    assert_eq!(first_slot(&socket), 91);
    let counted = wakes.events().iter().any(|event| {
        matches!(
            event,
            Event::Came { output: number, outcome: Outcome::Failed, due }
                if *number == output && *due == lost_at
        )
    });
    assert_eq!(
        (rerouted.status, counted),
        (Status::Reconnecting { failures: 2 }, true),
        "the completed failure for the last accepted route vanished at the switch"
    );
}
