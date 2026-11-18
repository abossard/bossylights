//! The patch on the real tick thread: the front compiles the writers and swaps them in,
//! the frame carries the lights. A light that stays patched keeps its held values through
//! any change of the patch.

mod mock;

use engine::{
    Begun, ChannelNumbers, Command, Engine, Fixture, Rate, Refusal, RunNumbers, Sender, Target,
    Text, Universes,
};
use mock::Heard;
use std::sync::Arc;

/// The test definition of a Generic RGB, read when the test is built.
const GENERIC_RGB: &[u8] =
    include_bytes!("../../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");

fn library() -> Vec<Fixture> {
    vec![fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB).expect("it reads")]
}

fn text(text: &str) -> Text {
    Text::new(text).unwrap()
}

fn at(universe: u16, address: u16) -> ChannelNumbers {
    ChannelNumbers { universe, address }
}

/// A Generic RGB in its mode RGB: red, green, blue, no dimmer.
fn par(name: &str, universe: u16, address: u16) -> Command {
    Command::Patch {
        name: text(name),
        maker: text("Generic"),
        model: text("Generic RGB"),
        mode: text("RGB"),
        at: at(universe, address),
    }
}

fn red(fixture: u16) -> [Command; 2] {
    let target = Target::Fixture { fixture };
    [
        Command::SetIntensity {
            target,
            intensity: u16::MAX,
        },
        Command::SetColour {
            target,
            red: u16::MAX,
            green: 0,
            blue: 0,
        },
    ]
}

/// An engine with two universes and a mock output that takes both.
fn start() -> (Engine, Sender, Arc<Heard>) {
    let (engine, mut sender) = Engine::start(Universes::new(2).unwrap()).unwrap();
    let run = RunNumbers { first: 1, count: 2 };
    let heard = mock::attach(&mut sender, engine.universes(), 1, run, Rate::Sixty);
    heard.two_more_frames();
    (engine, sender, heard)
}

/// Sends `command` with the library: the fixture number for a patch.
fn send(sender: &mut Sender, command: Command) -> Result<Option<u16>, Refusal> {
    match sender.begin(command, &library())? {
        Begun::Queued => Ok(None),
        Begun::Patched { fixture } => Ok(Some(fixture)),
        Begun::Opening(_) => panic!("{command:?} opens nothing"),
    }
}

fn bytes(heard: &Heard, universe: u16, first: u16) -> [u8; 3] {
    [0, 1, 2].map(|offset| heard.level(universe, first + offset))
}

#[test]
fn a_patched_fixture_shows_its_light_and_keeps_it_through_any_change_of_the_patch() {
    let unpatch = |fixture| Command::Unpatch { fixture };
    let moved = |fixture, universe, address| Command::Move {
        fixture,
        at: at(universe, address),
    };
    let rename = Command::Rename {
        fixture: 2,
        name: text("PAR 4, now left"),
    };
    // PAR 3 (1) at 1.37 and PAR 4 (2) at 1.50, PAR 4 red. Then a change, and where PAR 4
    // is: (change, PAR 4's universe and address, its bytes there, bytes left at 1.50)
    let cases = [
        (vec![], (1, 50), [255, 0, 0], [255, 0, 0]),
        (vec![moved(2, 2, 100)], (2, 100), [255, 0, 0], [0, 0, 0]),
        (vec![moved(1, 2, 1)], (1, 50), [255, 0, 0], [255, 0, 0]),
        (vec![unpatch(1)], (1, 50), [255, 0, 0], [255, 0, 0]),
        (
            vec![unpatch(1), par("PAR 5", 1, 1)],
            (1, 50),
            [255, 0, 0],
            [255, 0, 0],
        ),
        (vec![rename], (1, 50), [255, 0, 0], [255, 0, 0]),
        // unpatched: its writers and its held values go; patched again, it is dark
        (vec![unpatch(2)], (1, 50), [0, 0, 0], [0, 0, 0]),
        (
            vec![unpatch(2), par("PAR 4", 1, 50)],
            (1, 50),
            [0, 0, 0],
            [0, 0, 0],
        ),
    ];
    for (change, (universe, address), there, left) in cases {
        let (_engine, mut sender, heard) = start();
        assert_eq!(send(&mut sender, par("PAR 3", 1, 37)), Ok(Some(1)));
        assert_eq!(send(&mut sender, par("PAR 4", 1, 50)), Ok(Some(2)));
        for command in red(2) {
            assert_eq!(send(&mut sender, command), Ok(None));
        }
        for command in &change {
            assert!(send(&mut sender, *command).is_ok(), "{command:?}");
        }
        heard.two_more_frames();
        assert_eq!(bytes(&heard, universe, address), there, "{change:?}");
        assert_eq!(bytes(&heard, 1, 50), left, "{change:?} at 1.50");
        assert_eq!(bytes(&heard, 1, 37), [0, 0, 0], "{change:?}: PAR 3 is dark");
    }
}

#[test]
fn a_patch_the_front_refuses_changes_nothing() {
    let (_engine, mut sender, heard) = start();
    assert_eq!(send(&mut sender, par("PAR 3", 1, 37)), Ok(Some(1)));
    for command in red(1) {
        assert_eq!(send(&mut sender, command), Ok(None));
    }
    let before = sender.patch();
    // no library on `send`: a patch finds no fixture
    assert_eq!(sender.send(par("X", 1, 1)), Err(Refusal::NoSuchFixture));
    assert_eq!(
        send(&mut sender, par("X", 1, 511)),
        Err(Refusal::PastTheEnd { channels: 3 })
    );
    assert_eq!(
        send(&mut sender, Command::Unpatch { fixture: 9 }),
        Err(Refusal::NotPatched)
    );
    assert_eq!(sender.patch(), before);
    heard.two_more_frames();
    assert_eq!(bytes(&heard, 1, 37), [255, 0, 0]);
}
