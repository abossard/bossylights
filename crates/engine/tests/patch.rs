//! The patch on the real tick thread: the front compiles the writers and swaps them in,
//! the frame carries the lights. A light that stays patched keeps its held values through
//! any change of the patch.

mod mock;

use engine::{
    Begun, ChannelNumbers, Color, Command, Engine, Fixture, GrabId, Member, MemberList, Members,
    Property, QUEUE, Rate, Refusal, Rgb, RunNumbers, Sender, Target, Text, Universes, Value,
};
use engine_core::Pixels;
use mock::Heard;
use std::sync::Arc;

/// The test definition of a Generic RGB, read when the test is built.
const GENERIC_RGB: &[u8] =
    include_bytes!("../../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");

/// A Shehds LED Spot 100W: a colour wheel without black.
const SHEHDS: &[u8] =
    include_bytes!("../../fixture-library/tests/definitions/Shehds-LED-Spot-100W.qxf");

fn library() -> Vec<Fixture> {
    vec![
        fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB).expect("it reads"),
        fixture_library::parse("Shehds-LED-Spot-100W.qxf", SHEHDS).expect("it reads"),
    ]
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
        Begun::Queued | Begun::Started { .. } | Begun::Grabbed { .. } => Ok(None),
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
        send(&mut sender, par("X", 2, 511)),
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

#[test]
fn a_group_of_more_runs_than_a_wake_takes_lands_in_one_frame_all_or_none() {
    let universes = Universes::new(4).unwrap();
    let (_engine, mut sender) = Engine::start(universes).unwrap();
    let run = RunNumbers { first: 1, count: 4 };
    let heard = mock::attach(&mut sender, universes, 1, run, Rate::Sixty);
    // 600 pixels over universes 1 to 4, every second one in "odd": 300 runs of one light
    let strip = Command::PatchStrip {
        name: text("Strip"),
        pixels: Pixels::try_from(600).unwrap(),
        color: Color::Rgb,
        at: at(1, 1),
    };
    assert_eq!(send(&mut sender, strip), Ok(Some(1)));
    let full = Command::SetIntensity {
        target: Target::Fixture { fixture: 1 },
        intensity: u16::MAX,
    };
    assert_eq!(send(&mut sender, full), Ok(None));
    let odd: Vec<Member> = (1..=600)
        .step_by(2)
        .map(|light| Member::Light { light })
        .collect();
    assert!(odd.len() > QUEUE, "more runs than a wake takes commands");
    let group = Command::SetGroup {
        name: text("odd"),
        members: MemberList::Fits(Members::new(&odd).unwrap()),
    };
    assert_eq!(send(&mut sender, group), Ok(None));
    let target = Target::Group { group: text("odd") };
    let red = Command::SetColour {
        target,
        red: u16::MAX,
        green: 0,
        blue: 0,
    };
    let release = Command::ReleaseColour { target };
    heard.two_more_frames();
    let first = heard.frames() + 1;
    for _ in 0..20 {
        for (command, lit) in [(red, odd.len()), (release, 0)] {
            assert_eq!(send(&mut sender, command), Ok(None), "{command:?}");
            mock::wait_until("the group never showed", || {
                heard.lit_of(heard.frames()) == Some(lit)
            });
        }
    }
    let last = heard.frames();
    for n in first..=last {
        let lit = heard.lit_of(n).unwrap();
        assert!(
            lit == 0 || lit == odd.len(),
            "frame {n} shows {lit} of the group"
        );
    }
}

/// Sends `command` once the queue has room for it.
fn queued(sender: &mut Sender, command: Command) -> Result<Option<GrabId>, Refusal> {
    let started = std::time::Instant::now();
    loop {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "the queue drains"
        );
        match sender.begin(command, &library()) {
            Err(Refusal::QueueFull) => std::thread::sleep(std::time::Duration::from_millis(1)), // poll
            Ok(Begun::Grabbed { grab }) => return Ok(Some(grab)),
            Ok(_) => return Ok(None),
            Err(refused) => return Err(refused),
        }
    }
}

fn lights(lights: impl Iterator<Item = u16>) -> Command {
    let members: Vec<Member> = lights.map(|light| Member::Light { light }).collect();
    Command::SetGroup {
        name: text("g"),
        members: MemberList::Fits(Members::new(&members).unwrap()),
    }
}

fn grab_colour(group: &str) -> Command {
    Command::SetProperty {
        target: Target::Group { group: text(group) },
        value: Value::Colour(Rgb {
            red: u16::MAX,
            ..Rgb::BLACK
        }),
    }
}

/// How many grabs of `group` are admitted before the first refusal, of `most`.
fn grabs_until_refused(
    sender: &mut Sender,
    most: usize,
    mut group: impl FnMut(usize) -> Vec<Command>,
) -> (usize, Option<Refusal>) {
    for n in 0..most {
        for command in group(n) {
            if let Err(refused) = queued(sender, command) {
                return (n, Some(refused));
            }
        }
    }
    (most, None)
}

#[test]
fn the_live_grabs_stay_bounded_whatever_a_client_grabs() {
    // 1500 pixels over 9 universes, all but the end's odd lights grabbed again and again
    let odd = |from: u16, to: u16| (from..=to).step_by(2);
    let named = |name: &str, lights: Vec<u16>| {
        let members: Vec<Member> = lights
            .into_iter()
            .map(|light| Member::Light { light })
            .collect();
        Command::SetGroup {
            name: text(name),
            members: MemberList::Fits(Members::new(&members).unwrap()),
        }
    };
    let all = named("all", odd(1, 1023).collect());
    let low = named("low", odd(1, 511).collect());
    let high = named("high", odd(513, 1023).collect());
    // every light property: 4 and a gobo per wheel, 3
    let refused = Some(Refusal::GrabFull { most: 16_384 * 7 });
    // (what, groups made first, the grabs of each round, rounds, admitted, refused)
    type Round = Box<dyn FnMut(usize) -> Vec<Command>>;
    type Case = (
        &'static str,
        Vec<Command>,
        Round,
        usize,
        usize,
        Option<Refusal>,
    );
    let cases: [Case; 2] = [
        (
            // a grab emptied by two later ones, never by one: forgotten all the same
            "emptied by two",
            vec![all, low, high],
            Box::new(|_| vec![grab_colour("all"), grab_colour("low"), grab_colour("high")]),
            200,
            200,
            None,
        ),
        (
            // each a fresh group of 512 runs that keeps one light of its own: the 225th
            // would keep more runs than every light property held by a grab of its own
            "each keeps one light",
            vec![],
            Box::new(move |n| {
                let own = 1025 + 2 * u16::try_from(n).unwrap();
                vec![lights(odd(1, 1021).chain([own])), grab_colour("g")]
            }),
            230,
            224,
            refused,
        ),
    ];
    for (what, groups, round, rounds, admitted, refusal) in cases {
        let universes = Universes::new(9).unwrap();
        let (_engine, mut sender) = Engine::start(universes).unwrap();
        let run = RunNumbers { first: 1, count: 9 };
        let _heard = mock::attach(&mut sender, universes, 1, run, Rate::Sixty);
        let strip = Command::PatchStrip {
            name: text("Strip"),
            pixels: Pixels::try_from(1500).unwrap(),
            color: Color::Rgb,
            at: at(1, 1),
        };
        assert_eq!(send(&mut sender, strip), Ok(Some(1)), "{what}");
        for group in groups {
            assert_eq!(send(&mut sender, group), Ok(None), "{what}");
        }
        assert_eq!(
            grabs_until_refused(&mut sender, rounds, round),
            (admitted, refusal),
            "{what}"
        );
        // a release lets go, and the next grab is admitted
        let release = Command::ReleaseProperty {
            target: Target::Fixture { fixture: 1 },
            property: Property::Colour,
        };
        assert_eq!(queued(&mut sender, release), Ok(None), "{what}");
        assert!(
            matches!(queued(&mut sender, lights(odd(1, 3))), Ok(None)),
            "{what}"
        );
        let grab = queued(&mut sender, grab_colour("g"));
        assert!(matches!(grab, Ok(Some(_))), "{what}: {grab:?}");
    }
}

#[test]
fn a_light_let_go_does_not_veto_an_update_of_the_lights_still_held() {
    let (_engine, mut sender, heard) = start();
    let shehds = Command::Patch {
        name: text("Spot"),
        maker: text("Shehds"),
        model: text("LED Spot 100W"),
        mode: text("14 Channel"),
        at: at(1, 10),
    };
    assert_eq!(send(&mut sender, par("PAR", 1, 1)), Ok(Some(1)));
    assert_eq!(send(&mut sender, shehds), Ok(Some(2)));
    let both = Command::SetGroup {
        name: text("both"),
        members: MemberList::Fits(
            Members::new(&[
                Member::Fixture { fixture: 1 },
                Member::Fixture { fixture: 2 },
            ])
            .unwrap(),
        ),
    };
    assert_eq!(send(&mut sender, both), Ok(None));
    let [full, _] = red(1);
    assert_eq!(send(&mut sender, full), Ok(None));
    let Ok(Some(grab)) = queued(&mut sender, grab_colour("both")) else {
        panic!("red is grabbed");
    };
    heard.two_more_frames();
    assert_eq!(bytes(&heard, 1, 1), [255, 0, 0]);
    let release = Command::ReleaseProperty {
        target: Target::Fixture { fixture: 2 },
        property: Property::Colour,
    };
    assert_eq!(send(&mut sender, release), Ok(None));
    // black, which the spot's wheel cannot show: the PAR alone still holds the grab
    let black = Command::UpdateGrab {
        grab,
        value: Value::Colour(Rgb::BLACK),
    };
    assert_eq!(send(&mut sender, black), Ok(None));
    heard.two_more_frames();
    assert_eq!(bytes(&heard, 1, 1), [0, 0, 0]);
}

#[test]
fn a_kept_plays_frozen_lights_cannot_be_unpatched_under_it() {
    let red_on = |fixture| Command::PutStep {
        look: 1,
        name: text("red"),
        step: engine_core::Step {
            target: Target::Fixture { fixture },
            values: engine::Light {
                intensity: Some(u16::MAX),
                colour: Some(Rgb {
                    red: u16::MAX,
                    ..Rgb::BLACK
                }),
                ..engine::Light::DARK
            },
        },
    };
    // (stopped first, unpatching A, then on A's channels)
    let cases = [
        (true, Err(Refusal::LookInUse { look: 1 }), [255, 0, 0]),
        (false, Ok(None), [0, 0, 0]),
    ];
    for (stopped, unpatched, on_a) in cases {
        let (_engine, mut sender, heard) = start();
        assert_eq!(send(&mut sender, par("A", 1, 1)), Ok(Some(1)));
        assert_eq!(send(&mut sender, par("B", 1, 4)), Ok(Some(2)));
        assert_eq!(send(&mut sender, red_on(1)), Ok(None));
        let start = Command::StartLook {
            look: 1,
            rank: engine::Rank::Manual,
            when: engine::When::Now,
        };
        let Ok(Begun::Started { play }) = sender.begin(start, &library()) else {
            panic!("look 1 plays");
        };
        if stopped {
            // kept under UntilRelease: its plan stays red on A
            assert_eq!(send(&mut sender, Command::StopLook { play }), Ok(None));
        }
        assert_eq!(send(&mut sender, red_on(2)), Ok(None), "stopped {stopped}");
        assert_eq!(
            send(&mut sender, Command::Unpatch { fixture: 1 }),
            unpatched,
            "stopped {stopped}"
        );
        if unpatched.is_ok() {
            assert_eq!(send(&mut sender, par("C", 1, 1)), Ok(Some(1)));
        }
        heard.two_more_frames();
        assert_eq!(bytes(&heard, 1, 1), on_a, "stopped {stopped}");
    }
}
