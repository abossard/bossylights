//! Raw holds and Blackout through the channel core (#247): the engine on its stepped
//! clock, and the bytes each frame hands the device, wake by wake. A hold sent before a
//! wake's cutoff is in that wake's frame; sent after it, in the next.
#![cfg(feature = "test-support")]

mod mock;

use engine::{
    Begun, ChannelNumbers, Command, Engine, Rate, RunNumbers, Sender, Text, Universes, Wakes,
};
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

/// A Generic RGB PAR: red, green and blue follow Master and Blackout (no dimmer
/// carries them).
const RGB: &[u8] =
    include_bytes!("../../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");

/// Patches a Generic RGB PAR at `at`, and lets a wake take the patch.
fn par(desk: &mut Desk, at: ChannelNumbers) {
    let library = [fixture_library::parse("Generic-Generic-RGB.qxf", RGB).unwrap()];
    let patch = Command::Patch {
        name: Text::new("PAR").unwrap(),
        maker: Text::new("Generic").unwrap(),
        model: Text::new("Generic RGB").unwrap(),
        mode: Text::new("RGB").unwrap(),
        at,
    };
    let begun = desk.sender.begin(patch, &library);
    assert!(
        matches!(begun, Ok(Begun::Patched { .. })),
        "patched at {at:?}"
    );
    desk.wake(&[], Sent::Before);
}

#[test]
fn a_raw_hold_and_blackout_are_in_the_frame_after_the_cutoff() {
    // the first channel is a PAR's red, the last a PAR's blue: both follow Blackout
    let hold = |channel, value| Command::SetChannel { channel, value };
    let release = |channel| Command::ReleaseChannel { channel };
    let blackout = |on| Command::Blackout { on };
    let master = |level| Command::SetMaster { level };
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
        // 17 x 128 / 255 = 8.5, 91 x 128 / 255 = 45.7
        ("master half", vec![master(128)], Before, (9, 46)),
        (
            "master full after the cutoff: not yet",
            vec![master(255)],
            After,
            (9, 46),
        ),
        ("master full in the next frame", vec![], Before, (17, 91)),
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
        par(&mut desk, FIRST);
        par(&mut desk, last_par());
        for (step, commands, sent, frame) in &steps {
            assert_eq!(desk.wake(commands, *sent), *frame, "{rate:?}: {step}");
        }
    }
}

/// A channel nobody patched follows nothing (#251, cx-3: unknown channels start
/// independent): Blackout leaves its raw hold.
#[test]
fn blackout_leaves_a_raw_hold_nobody_patched() {
    let mut desk = Desk::new(Rate::FortyFour);
    par(&mut desk, last_par());
    // (step, commands, the first channel (unpatched) and the last (a PAR's blue))
    let steps = [
        (
            "both held",
            vec![
                Command::SetChannel {
                    channel: FIRST,
                    value: 91,
                },
                Command::SetChannel {
                    channel: LAST,
                    value: 91,
                },
            ],
            (91, 91),
        ),
        ("blackout", vec![Command::Blackout { on: true }], (91, 0)),
        (
            "blackout off",
            vec![Command::Blackout { on: false }],
            (91, 91),
        ),
    ];
    for (step, commands, frame) in &steps {
        assert_eq!(desk.wake(commands, Sent::Before), *frame, "{step}");
    }
}

/// The PAR whose blue is the last channel.
const fn last_par() -> ChannelNumbers {
    ChannelNumbers {
        universe: 2,
        address: 510,
    }
}

/// A Vari-Lite VL2202 Spot: its strobe channel (8 in mode Standard 8bit) is Open at 0 to 2
/// and Closed at 3 to 5.
const VL2202: &[u8] = include_bytes!("../../rig/tests/definitions/Vari-Lite-VL2202-Spot.qxf");

#[test]
fn blackout_sends_each_channel_to_the_dark_its_fixture_file_gives() {
    // (step, commands, intensity at 1 and the shutter at 8 in that wake's frame).
    // Expected from the file: the shutter rests at its first Open byte, 0; its dark is the
    // first byte of its Closed range, 3 (docs/design/channel-model.md, Rules).
    let blackout = |on| vec![Command::Blackout { on }];
    let steps = [
        ("nothing plays: the shutter rests open", vec![], (0, 0)),
        ("blackout closes the shutter", blackout(true), (0, 3)),
        ("blackout off: open again", blackout(false), (0, 0)),
    ];
    let library = [fixture_library::parse("Vari-Lite-VL2202-Spot.qxf", VL2202).unwrap()];
    let text = |text| Text::new(text).unwrap();
    for rate in [Rate::FortyFour, Rate::Sixty] {
        let mut desk = Desk::new(rate);
        let patch = Command::Patch {
            name: text("Spot"),
            maker: text("Vari-Lite"),
            model: text("VL2202 Spot"),
            mode: text("Standard 8bit"),
            at: FIRST,
        };
        let begun = desk.sender.begin(patch, &library);
        assert!(
            matches!(begun, Ok(Begun::Patched { fixture: 1 })),
            "{rate:?}"
        );
        for (step, commands, (intensity, shutter)) in &steps {
            desk.wake(commands, Sent::Before);
            let frame = (desk.heard.level(1, 1), desk.heard.level(1, 8));
            assert_eq!(frame, (*intensity, *shutter), "{rate:?}: {step}");
        }
    }
}
