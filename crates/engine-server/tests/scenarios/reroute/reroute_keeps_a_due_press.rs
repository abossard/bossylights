//! A pending reroute must preserve a due press on the old DDP run.

use crate::output_stream as stream;

use engine::{
    Begun, ChannelNumbers, Color, Command, Device, Engine, Event, Outcome, OutputNumber, Rate,
    RunNumbers, Universes,
};
use std::net::UdpSocket;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

const GUARD: Duration = Duration::from_secs(10);

struct HoldFirstWrite {
    ddp: Box<dyn Device>,
    gate: Option<(Sender<()>, Receiver<()>)>,
}

impl Device for HoldFirstWrite {
    fn take(&mut self, bytes: &[u8]) -> Outcome {
        if let Some((entered, release)) = self.gate.take() {
            entered.send(()).unwrap();
            let _ = release.recv_timeout(GUARD);
        }
        self.ddp.take(bytes)
    }
}

fn first_slot(socket: &UdpSocket) -> u8 {
    let mut packet = [0; 1536];
    assert_eq!(socket.recv(&mut packet).unwrap(), 520);
    packet[10]
}

#[test]
fn a_single_due_press_queued_before_reroute_reaches_the_old_run() {
    let (socket, kind) = stream::receiver(Color::Rgb);
    let output = OutputNumber::try_from(1).unwrap();
    let (entered, writing) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut gate = Some((entered, gate));
    let (_engine, mut sender, mut wakes) = Engine::start_stepped(
        Universes::new(2).unwrap(),
        Box::new(move |kind| {
            Ok(Box::new(HoldFirstWrite {
                ddp: outputs::open(kind)?,
                gate: gate.take(),
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
    let at = wakes.wake().unwrap();
    writing.recv_timeout(GUARD).unwrap();

    let Begun::Opening(opening) = sender.begin(set(2), &[]).unwrap() else {
        panic!("the new run needs a lane");
    };
    wakes.wake_at(at).unwrap();
    let opened = opening.open();
    sender.send(hold(1, 73)).unwrap();
    let press_due = wakes.wake().unwrap();
    assert!(wakes.events().iter().any(|event| matches!(
        event,
        Event::Handed { output: number, due, .. } if *number == output && *due == press_due
    )));

    // The old worker has one send in progress and exactly one due frame waiting.
    // Install the replacement before letting the in-progress send return.
    let handoff = sender.publish_output(opened).unwrap().unwrap();
    wakes.wake().unwrap();
    release.send(()).unwrap();
    handoff.wait();

    assert_eq!(first_slot(&socket), 11);
    assert_eq!(
        first_slot(&socket),
        73,
        "the only queued old frame carried the press"
    );
    assert_eq!(first_slot(&socket), 91);
}
