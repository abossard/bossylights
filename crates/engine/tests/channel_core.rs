//! Raw holds and Blackout through the channel core (#247): the engine on its stepped
//! clock, and the bytes each frame hands the device, wake by wake. A hold sent before a
//! wake's cutoff is in that wake's frame; sent after it, in the next.
#![cfg(feature = "test-support")]

mod mock;

use engine::{ChannelNumbers, Command, Engine, Rate, RunNumbers, Sender, Universes, Wakes};
use mock::Heard;
use std::sync::Arc;

const FIRST: ChannelNumbers = ChannelNumbers {
    universe: 1,
    address: 1,
};
const LAST: ChannelNumbers = ChannelNumbers {
    universe: 2,
    address: 512,
};

/// When a command is sent: before the next wake's cutoff, or after it, while that wake
/// is held there.
#[derive(Clone, Copy, Debug)]
enum Sent {
    Before,
    After,
}

struct Desk {
    _engine: Engine,
    sender: Sender,
    wakes: Wakes,
    heard: Arc<Heard>,
}

impl Desk {
    /// Two universes, one device on both at `rate`, its first frame taken.
    fn new(rate: Rate) -> Self {
        let universes = Universes::new(2).unwrap();
        let (engine, mut sender, wakes) =
            Engine::start_stepped(universes, Box::new(outputs::open)).unwrap();
        wakes.hold().unwrap();
        wakes.wake().unwrap();
        let run = RunNumbers { first: 1, count: 2 };
        let heard = mock::attach(&mut sender, universes, 1, run, rate);
        let mut desk = Self {
            _engine: engine,
            sender,
            wakes,
            heard,
        };
        assert_eq!(desk.wake(&[], Sent::Before), (0, 0), "the first frame");
        desk
    }

    /// Sends `commands` before or after the next wake's cutoff, lets that wake through
    /// and returns the first and last channel of the frame it handed the device.
    fn wake(&mut self, commands: &[Command], sent: Sent) -> (u8, u8) {
        let before = self.heard.frames();
        match sent {
            Sent::Before => {
                self.send(commands);
                self.wakes.wake().unwrap();
            }
            Sent::After => {
                self.wakes.cut().unwrap();
                self.send(commands);
                self.wakes.finish().unwrap();
            }
        }
        mock::wait_until("the device took no frame", || self.heard.frames() > before);
        assert_eq!(self.heard.frames(), before + 1, "one frame per wake");
        (self.heard.level(1, 1), self.heard.level(2, 512))
    }

    fn send(&mut self, commands: &[Command]) {
        for command in commands {
            self.sender.send(*command).unwrap();
        }
    }
}

#[test]
fn a_raw_hold_and_blackout_are_in_the_frame_after_the_cutoff() {
    let hold = |channel, value| Command::SetChannel { channel, value };
    let release = |channel| Command::ReleaseChannel { channel };
    let blackout = |on| Command::Blackout { on };
    use Sent::{After, Before};
    // (step, commands, sent, first and last channel of that wake's frame)
    let steps = [
        (
            "91 before the cutoff",
            vec![hold(FIRST, 91)],
            Before,
            (91, 0),
        ),
        ("released", vec![release(FIRST)], Before, (0, 0)),
        (
            "91 after the cutoff: not yet",
            vec![hold(FIRST, 91)],
            After,
            (0, 0),
        ),
        ("the next frame carries it", vec![], Before, (91, 0)),
        ("the last channel", vec![hold(LAST, 91)], Before, (91, 91)),
        ("blackout", vec![blackout(true)], Before, (0, 0)),
        ("blackout stays", vec![], Before, (0, 0)),
        (
            "a hold under blackout",
            vec![hold(FIRST, 17)],
            Before,
            (0, 0),
        ),
        ("blackout off", vec![blackout(false)], Before, (17, 91)),
        (
            "blackout after the cutoff",
            vec![blackout(true)],
            After,
            (17, 91),
        ),
        ("dark in the next frame", vec![], Before, (0, 0)),
        (
            "off, every hold released",
            vec![blackout(false), Command::ReleaseChannels],
            Before,
            (0, 0),
        ),
    ];
    for rate in [Rate::FortyFour, Rate::Sixty] {
        let mut desk = Desk::new(rate);
        for (step, commands, sent, frame) in &steps {
            assert_eq!(desk.wake(commands, *sent), *frame, "{rate:?}: {step}");
        }
    }
}
