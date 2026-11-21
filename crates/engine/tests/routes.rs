use engine::{
    Begun, ChannelNumbers, Color, Command, Engine, OutputNumber, ProjectOutput, QUEUE, Rate,
    Refusal, Route, RunNumbers, Span, Target, Text, Universes, Value,
};
use std::io;

mod mock;

#[expect(clippy::disallowed_types, reason = "the test listens to a real output")]
#[path = "../../outputs/tests/common/stream.rs"]
mod stream;

#[test]
fn a_failed_first_open_leaves_the_output_free_for_a_retry() {
    let (_receiver, kind) = stream::receiver(Color::Rgbw);
    let mut first = true;
    let open = Box::new(move |kind| {
        if std::mem::take(&mut first) {
            Err(io::Error::other("device unavailable"))
        } else {
            outputs::open(kind)
        }
    });
    let (_engine, mut sender) = Engine::start_with(Universes::ONE, open).unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let run = RunNumbers { first: 1, count: 1 };
    let set = Command::SetOutput {
        output,
        kind,
        universes: run,
        rate: Some(Rate::Sixty),
    };
    let opened = ProjectOutput::Kind {
        kind,
        universes: run.check(Universes::ONE).unwrap(),
    };
    // A failed first open has no previous lane to resume. The retry sends one attach.
    for (answer, project, sent) in [
        (Err(Refusal::CannotOpen), None, 0),
        (Ok(()), Some(opened), 1),
    ] {
        assert_eq!(sender.send(set), answer);
        assert_eq!(sender.project().output(output), project);
        assert_eq!(sender.sent(), sent);
    }
}

#[test]
fn an_attached_route_must_fit_whole_rgbw_pixels() {
    let output = OutputNumber::try_from(1).unwrap();
    for (len, answer) in [(319, Err(Refusal::CannotOpen)), (320, Ok(()))] {
        let (receiver, kind) = stream::receiver(Color::Rgbw);
        let (_engine, mut sender) = Engine::start(Universes::ONE).unwrap();
        let route = Route::new(outputs::Limits::RAW, &[Span { start: 0, len }]).unwrap();
        let device = outputs::open(kind).unwrap();
        assert_eq!(
            sender.attach_route(output, device, route, Rate::Sixty),
            answer,
            "{len} bytes"
        );
        assert_eq!(sender.sent(), u64::from(answer.is_ok()));
        assert_eq!(sender.project().output(output).is_some(), answer.is_ok());
        if answer.is_ok() {
            let mut packet = [0; 1024];
            assert_eq!(receiver.recv(&mut packet).unwrap(), 10 + len as usize);
        }
    }
}

#[test]
fn a_grab_update_can_fill_the_last_place_left_by_an_output_resize() {
    let (_receiver, kind) = stream::receiver(Color::Rgb);
    let output = OutputNumber::try_from(1).unwrap();
    let set_output = |count| Command::SetOutput {
        output,
        kind,
        universes: RunNumbers { first: 1, count },
        rate: None,
    };
    let (mut engine, mut sender) = Engine::start(Universes::new(2).unwrap()).unwrap();
    sender.send(set_output(1)).unwrap();
    sender
        .send(Command::PatchStrip {
            name: Text::new("pixel").unwrap(),
            pixels: engine_core::Pixels::try_from(1).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    let press = Command::SetIntensity {
        target: Target::Light { light: 1 },
        intensity: 0,
    };
    let Begun::Grabbed { grab } = sender.begin(press, &[]).unwrap() else {
        panic!("a press starts a grab");
    };
    let Begun::Opening(resize) = sender.begin(set_output(2), &[]).unwrap() else {
        panic!("a longer route needs a new lane");
    };
    let opened = resize.open();
    let filler = Command::Blackout { on: false };
    let update = |level| Command::UpdateGrab {
        grab,
        value: Value::Intensity(level),
    };

    // One place belongs to finish. From an empty FIFO the other QUEUE - 1
    // places must fit QUEUE - 2 single commands and the grab update's chunk.
    mock::wait_until("a wake drained every burst before the queue filled", || {
        engine.fresh();
        for _ in 2..QUEUE {
            sender.send(filler).unwrap();
        }
        assert_eq!(sender.send(update(1234)), Ok(()));
        // Neither another chunk nor a single command may take finish's place.
        // If a wake made room in between, repeat from an empty FIFO.
        [update(4321), filler]
            .into_iter()
            .all(|command| match sender.send(command) {
                Err(Refusal::QueueFull) => true,
                Ok(()) => false,
                Err(other) => panic!("unexpected refusal: {other:?}"),
            })
    });
    sender.finish(opened).unwrap();
    let snapshot = engine.fresh();
    assert_eq!(snapshot.applied(), sender.sent());
    assert_eq!(snapshot.lights()[0].intensity, Some(1234));
    assert_eq!(
        snapshot.state.outputs[output.slot()]
            .unwrap()
            .universes
            .count(),
        2
    );
}
