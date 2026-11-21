//! No invalid patch: a fixture over more than one universe needs one output that streams
//! them all as one run. The front refuses what would break it, both ways: a patch or a
//! move, and a change of the outputs. On a refusal nothing changes.

use engine::Unsent;
use engine::{
    ChannelNumbers, Color, Command, Device, Engine, Outcome, OutputKind, OutputNumber, Rate,
    Refusal, RunNumbers, Sender, Universes,
};
use engine_core::Pixels;
use std::net::{SocketAddr, UdpSocket};

/// A device that takes every frame.
struct Taker;

impl Device for Taker {
    fn take(&mut self, _: &[u8]) -> Outcome {
        Outcome::Accepted
    }
}

/// What a step does.
#[derive(Clone, Copy, Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "a client's command has no heap: a step of a test table, moved a few times"
)]
enum Step {
    Send(Command),
    /// A device brought by the caller, as `output`, streaming `first` and on.
    Attach {
        output: u8,
        first: u16,
        count: u16,
    },
}

fn output(number: u8) -> OutputNumber {
    OutputNumber::try_from(number).unwrap()
}

#[test]
fn a_patch_and_the_outputs_never_leave_a_fixture_over_universes_no_output_streams() {
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    let SocketAddr::V4(address) = receiver.local_addr().unwrap() else {
        panic!("IPv4");
    };
    let ddp = |color| OutputKind::Ddp { address, color };
    let set = |number, kind, first, count| {
        Step::Send(Command::SetOutput {
            output: output(number),
            kind,
            universes: RunNumbers { first, count },
            rate: Some(Rate::Thirty),
        })
    };
    let remove = |number| {
        Step::Send(Command::RemoveOutput {
            output: output(number),
        })
    };
    // 150 RGB pixels from `address`: 450 channels
    let strip = |universe, address| {
        Step::Send(Command::PatchStrip {
            name: engine::Text::new("Strip").unwrap(),
            pixels: Pixels::try_from(150).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers { universe, address },
        })
    };
    let to = |universe, address| {
        Step::Send(Command::Move {
            fixture: 1,
            at: ChannelNumbers { universe, address },
        })
    };
    let attach = |output, first, count| Step::Attach {
        output,
        first,
        count,
    };
    let rgb = ddp(Color::Rgb);
    let rgbw = ddp(Color::Rgbw);
    // (steps before, step, refused or taken)
    let cases: [(Vec<Step>, Step, Result<(), Refusal>); 16] = [
        // a fixture in one universe needs no output
        (vec![], strip(1, 1), Ok(())),
        (
            vec![],
            strip(1, 400),
            Err(Refusal::NotStreamed { first: 1, last: 2 }),
        ),
        (
            vec![set(1, rgb, 1, 1)],
            strip(1, 400),
            Err(Refusal::NotStreamed { first: 1, last: 2 }),
        ),
        (vec![set(1, rgb, 1, 2)], strip(1, 400), Ok(())),
        (vec![attach(1, 1, 2)], strip(1, 400), Ok(())),
        // a move is checked as a patch
        (vec![set(1, rgb, 1, 2), strip(1, 400)], to(2, 1), Ok(())),
        (
            vec![set(1, rgb, 1, 2), strip(1, 400)],
            to(2, 400),
            Err(Refusal::NotStreamed { first: 2, last: 3 }),
        ),
        // the outputs, the other way
        (
            vec![set(1, rgb, 1, 2), strip(1, 400)],
            remove(1),
            Err(Refusal::StreamsFixture { fixture: 1 }),
        ),
        (
            vec![set(1, rgb, 1, 2), strip(1, 400)],
            set(1, rgb, 1, 1),
            Err(Refusal::StreamsFixture { fixture: 1 }),
        ),
        (
            vec![set(1, rgb, 1, 2), strip(1, 400)],
            set(1, rgb, 1, 3),
            Ok(()),
        ),
        // another kind: refused before its device is opened
        (
            vec![set(1, rgb, 1, 2), strip(1, 400)],
            set(1, rgbw, 2, 2),
            Err(Refusal::StreamsFixture { fixture: 1 }),
        ),
        (
            vec![set(1, rgb, 1, 2), strip(1, 400)],
            set(1, rgbw, 1, 2),
            Ok(()),
        ),
        (
            vec![set(1, rgb, 1, 2), strip(1, 400)],
            attach(1, 1, 1),
            Err(Refusal::StreamsFixture { fixture: 1 }),
        ),
        // another output that streams them lets this one go
        (
            vec![set(1, rgb, 1, 2), strip(1, 400), set(2, rgb, 1, 3)],
            remove(1),
            Ok(()),
        ),
        (
            vec![set(1, rgb, 1, 2), strip(1, 400), set(2, rgb, 2, 3)],
            remove(1),
            Err(Refusal::StreamsFixture { fixture: 1 }),
        ),
        (vec![set(1, rgb, 1, 2), strip(1, 1)], remove(1), Ok(())),
    ];
    for (before, step, expected) in cases {
        let (_engine, mut sender) = Engine::start(Universes::new(4).unwrap()).unwrap();
        for done in &before {
            assert_eq!(take(&mut sender, *done), Ok(()), "{done:?} before {step:?}");
        }
        let (project, patch) = (sender.project(), sender.patch());
        let taken = take(&mut sender, step);
        assert_eq!(
            taken,
            expected.map_err(Unsent::Refused),
            "{step:?} after {before:?}"
        );
        if taken.is_err() {
            assert_eq!(sender.project(), project, "{step:?}: the outputs stay");
            assert_eq!(sender.patch(), patch, "{step:?}: the patch stays");
        }
    }
}

fn take(sender: &mut Sender, step: Step) -> Result<(), Unsent> {
    match step {
        Step::Send(command) => sender.send(command),
        Step::Attach {
            output: number,
            first,
            count,
        } => {
            let run = RunNumbers { first, count }
                .check(Universes::new(4).unwrap())
                .unwrap();
            sender
                .attach(output(number), Box::new(Taker), run, Rate::Thirty)
                .map_err(Unsent::Refused)
        }
    }
}
