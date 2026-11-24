//! The state leaves the tick thread as one snapshot: a reader sends an empty box, the
//! tick thread fills it at the end of a wake and sends it back.

mod mock;

use engine::{ChannelNumbers, Command, Engine, OutputNumber, Rate, RunNumbers, Universes};

fn set(value: u8) -> Command {
    Command::SetChannel {
        channel: ChannelNumbers {
            universe: 1,
            address: 1,
        },
        value,
    }
}

#[test]
fn every_picture_is_one_wake_its_frame_outputs_and_levels_belong_together() {
    // (rate of the only output, universes)
    let cases = [(Rate::Sixty, 1), (Rate::FortyFour, 2), (Rate::Thirty, 1)];
    for (rate, count) in cases {
        let universes = Universes::new(count).unwrap();
        let (mut engine, mut sender) = Engine::start(universes).unwrap();
        let run = RunNumbers { first: 1, count };
        let heard = mock::attach(&mut sender, universes, 1, run, rate);
        heard.two_more_frames();
        let output = OutputNumber::try_from(1).unwrap();
        // One output: each wake hands it one frame, so frame less handed stays the same.
        let mut wakes_before = None;
        // (frame the picture's levels went out in, the level of channel 1 they hold)
        let mut pictures = vec![];
        for value in 1..=40 {
            assert_eq!(sender.send(set(value)), Ok(()));
            // Reads while the tick thread applies the commands, every picture checked.
            mock::wait_until("the command never reached a picture", || {
                let picture = engine.latest();
                if picture.output(output).is_none() {
                    return false;
                }
                let handed = picture.handed(output);
                let frame = picture.frame();
                let before = *wakes_before.get_or_insert(frame - handed);
                assert_eq!(frame - handed, before, "{rate:?}: frame {frame}");
                let held = picture.universes()[0].0[0];
                pictures.push((handed, held));
                held == value
            });
        }
        assert!(
            pictures.len() >= 40,
            "{rate:?}: {} pictures",
            pictures.len()
        );
        // The output's thread sends each frame a little after its wake.
        let last = pictures.iter().map(|(handed, _)| *handed).max().unwrap();
        mock::wait_until("the output never got its frames", || heard.frames() >= last);
        for (handed, held) in pictures {
            assert_eq!(
                heard.first_channel_of(handed),
                Some(held),
                "{rate:?}: a picture holds {held}, its output's frame {handed} carried another"
            );
        }
    }
}

#[test]
fn a_fresh_picture_carries_every_command_sent_before() {
    let blackout = |on| Command::Blackout { on };
    // (commands, held level of channel 1, blackout)
    let cases = [
        (vec![set(9)], 9, false),
        (vec![set(9), set(10)], 10, false),
        (vec![set(9), blackout(true)], 9, true),
        (vec![set(9), Command::ReleaseAll], 0, false),
    ];
    for (commands, held, on) in cases {
        let (mut engine, mut sender) = Engine::start(Universes::ONE).unwrap();
        for command in &commands {
            assert_eq!(sender.send(*command), Ok(()), "{commands:?}");
        }
        let picture = engine.fresh().unwrap();
        assert_eq!(picture.universes()[0].0[0], held, "{commands:?}");
        assert_eq!(picture.blackout(), on, "{commands:?}");
    }
}

#[test]
fn a_reader_that_stops_reading_holds_up_no_frame() {
    // (reads before it stops)
    for reads in [0, 1, 3] {
        let (mut engine, mut sender) = Engine::start(Universes::ONE).unwrap();
        for _ in 0..reads {
            engine.latest();
        }
        let run = RunNumbers { first: 1, count: 1 };
        let heard = mock::attach(&mut sender, Universes::ONE, 1, run, Rate::Sixty);
        for _ in 0..5 {
            heard.two_more_frames();
        }
    }
}
