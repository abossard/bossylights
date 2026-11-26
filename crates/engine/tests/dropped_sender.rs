//! A sender dropped while its commands are on their way: the tick still frees nothing
//! (`docs/design/theory.md`, idea 9; `docs/engine.md`, "The tick never frees a box").
//! Finding 1-01 of #349.
#![cfg(feature = "test-support")]

#[path = "../../outputs/tests/counting/mod.rs"]
mod counting;

use engine::{
    Begun, ChannelNumbers, Color, Command, Engine, Light, Rank, Rgb, Sender, Target, Text,
    Universes, Wakes, When,
};
use engine_core::{Pixels, Step};

/// A look of one step: the pixel at full, `red`.
fn step(look: u16, red: u16) -> Command {
    Command::PutStep {
        look,
        name: Text::new("one step").unwrap(),
        step: Step {
            target: Target::Fixture { fixture: 1 },
            values: Light {
                intensity: Some(u16::MAX),
                colour: Some(Rgb { red, ..Rgb::BLACK }),
                ..Light::DARK
            },
        }
        .into(),
    }
}

/// Starts `look` by hand: `ReleaseAll` ends it.
fn start(sender: &mut Sender, look: u16) {
    let command = Command::StartLook {
        look,
        rank: Rank::Manual,
        when: When::Now,
        usage: Default::default(),
    };
    assert!(matches!(
        sender.begin(command, &[]).unwrap(),
        Begun::Started { .. }
    ));
}

/// The pixel set red by hand: a grab, its box of words on the tick.
fn grab(sender: &mut Sender) {
    let command = Command::SetColour {
        target: Target::Light { light: 1 },
        red: u16::MAX,
        green: 0,
        blue: 0,
    };
    assert!(matches!(
        sender.begin(command, &[]).unwrap(),
        Begun::Grabbed { .. }
    ));
}

/// Two looks, the first playing.
fn play(sender: &mut Sender) {
    sender.send(step(1, u16::MAX)).unwrap();
    sender.send(step(2, u16::MAX)).unwrap();
    start(sender, 1);
}

/// An edit of the playing look (a new plan) and a second play.
fn edit_and_start(sender: &mut Sender) {
    sender.send(step(1, 0x8000)).unwrap();
    start(sender, 2);
}

fn release_all(sender: &mut Sender) {
    sender.send(Command::ReleaseAll).unwrap();
}

fn nothing(_: &mut Sender) {}

/// What a row sends.
type Sends = fn(&mut Sender);

/// A fresh engine with one pixel and `first` applied in one wake: the pixel lit. Its
/// wakes held.
fn lit(row: &str, first: Sends) -> (Engine, Sender, Wakes) {
    let (mut engine, mut sender, wakes) =
        Engine::start_stepped(Universes::ONE, Box::new(outputs::open)).unwrap();
    wakes.hold().unwrap();
    sender
        .send(Command::PatchStrip {
            name: Text::new("one pixel").unwrap(),
            pixels: Pixels::try_from(1).unwrap(),
            color: Color::Rgb,
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        })
        .unwrap();
    first(&mut sender);
    engine.latest();
    wakes.wake().unwrap();
    assert_eq!(engine.latest().sent(1, 1), 255, "{row}: the pixel was lit");
    (engine, sender, wakes)
}

#[test]
fn what_is_on_its_way_when_the_sender_drops_is_not_freed_on_the_tick() {
    // (row, applied in the first wake, on its way with ReleaseAll when the sender drops)
    let cases: [(&str, Sends, Sends); 2] = [
        ("a grab, then ReleaseAll", grab, nothing),
        (
            "a playing look, then its edit, a new play and ReleaseAll",
            play,
            edit_and_start,
        ),
    ];
    for (row, first, on_its_way) in cases {
        let (mut engine, mut sender, wakes) = lit(row, first);
        on_its_way(&mut sender);
        release_all(&mut sender);
        let applied = sender.sent();
        drop(sender);
        let before = counting::tick_frees();
        wakes.wake().unwrap();
        let picture = engine.latest();
        assert_eq!(
            picture.applied(),
            applied,
            "{row}: the tick applied ReleaseAll"
        );
        assert_eq!(picture.sent(1, 1), 0, "{row}: all was released");
        assert_eq!(
            counting::tick_frees() - before,
            0,
            "{row}: the tick must free no heap memory when it applies ReleaseAll"
        );
    }
}

/// The engine dropped before another wake: its tick runs one last wake, which applies what
/// was on its way. The boxes the sender left go only after that.
#[test]
fn the_engine_frees_a_dropped_senders_boxes_only_after_its_tick_ended() {
    // (row, on its way after a grab when the sender and then the engine drop)
    let cases: [(&str, Sends); 2] = [("nothing", nothing), ("ReleaseAll", release_all)];
    let [(_, ended), (_, released)] = cases.map(|(row, on_its_way)| {
        let (engine, mut sender, _wakes) = lit(row, grab);
        on_its_way(&mut sender);
        drop(sender);
        let before = counting::tick_frees();
        drop(engine);
        (row, counting::tick_frees() - before)
    });
    // `ended`: what the tick thread's own end frees, after its last wake
    assert_eq!(
        released, ended,
        "the tick's last wake must free nothing when it applies ReleaseAll"
    );
}
