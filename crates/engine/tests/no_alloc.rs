//! Proves the tick thread allocates nothing per wake, also while it serves two outputs at
//! different rates, resolves 512 patched lights and a strip of 100 pixels over two
//! universes, takes commands, swaps in new writers, takes a group of 256 runs and
//! hands a reader a snapshot on every frame. And that it frees nothing: the old writers,
//! runs and a replaced device go to the free thread. A counting allocator sees every
//! allocation in this process, except those of the front while it sends: a light, patch or
//! attach command allocates there (the patch is a calculation on the heap, a device is
//! boxed), never on the tick thread. It counts the frees of the tick thread apart, by its
//! name. The outputs are attached and the lights patched before the window (the writers
//! compiled on the sending thread). In the window the main thread sends and reads
//! snapshots, which allocates nothing, and sleeps, so any allocation counted is the tick's
//! or the reader's. That each real device's `take` allocates nothing is proved in
//! `crates/outputs/tests/no_alloc.rs`.

#[path = "../../outputs/tests/counting/mod.rs"]
mod counting;
mod mock;

use engine::{
    Begun, ChannelNumbers, Color, Command, Engine, Fixture, Member, MemberList, Members, Rate,
    RunNumbers, Sender, Target, Text, Universes,
};
use engine_core::Pixels;

/// 512 lights: Generic RGB in its mode RGB, 170 to a universe.
const LIGHTS: u16 = 512;
const UNIVERSES: u16 = 4;

/// Sends `command` as the front, its allocations not counted. Whether it is taken.
fn send(sender: &mut Sender, command: Command, library: &[Fixture]) -> bool {
    let begun = counting::uncounted(|| sender.begin(command, library));
    matches!(begun, Ok(Begun::Queued | Begun::Patched { .. }))
}

fn text(text: &str) -> Text {
    Text::new(text).expect("a short text")
}

/// The test definition of a Generic RGB, read when the test is built.
const GENERIC_RGB: &[u8] =
    include_bytes!("../../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");

fn library() -> Vec<Fixture> {
    vec![fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB).expect("it reads")]
}

/// Light `n` of the 512 (from 0) at its place, 3 channels each: 170 to each of universes
/// 1 to 3, the last two at the end of universe 4, after the strip (no two fixtures share
/// a channel).
fn place(n: u16) -> ChannelNumbers {
    if n < 510 {
        ChannelNumbers {
            universe: 1 + n / 170,
            address: 1 + 3 * (n % 170),
        }
    } else {
        ChannelNumbers {
            universe: 4,
            address: 507 + 3 * (n - 510),
        }
    }
}

fn main() {
    let universes = Universes::new(UNIVERSES).expect("4 universes");
    let (mut engine, mut sender) = Engine::start(universes).expect("the tick thread starts");
    let run = RunNumbers { first: 1, count: 1 };
    mock::attach(&mut sender, universes, 1, run, Rate::Sixty);
    // the second streams universes 3 and 4, which the strip below runs over
    let three_four = RunNumbers { first: 3, count: 2 };
    mock::attach(&mut sender, universes, 2, three_four, Rate::Thirty);
    let library = library();
    for n in 0..LIGHTS {
        let patch = Command::Patch {
            name: text("PAR"),
            maker: text("Generic"),
            model: text("Generic RGB"),
            mode: text("RGB"),
            at: place(n),
        };
        assert!(send(&mut sender, patch, &library), "light {n} is patched");
        let full = Command::SetIntensity {
            target: Target::Fixture { fixture: n + 1 },
            intensity: u16::MAX,
        };
        assert!(send(&mut sender, full, &library), "light {n} is lit");
        if n % 64 == 63 {
            // the queue takes a wake's worth: let the tick apply them
            let seen = engine.latest().state.frame;
            mock::wait_until("no frame went by", || engine.latest().state.frame > seen);
        }
    }
    // and a strip of 100 pixels that runs from universe 3 into 4, lit
    let strip = Command::PatchStrip {
        name: text("Strip"),
        pixels: Pixels::try_from(100).expect("100 pixels"),
        color: Color::Rgb,
        at: ChannelNumbers {
            universe: 3,
            address: 511,
        },
    };
    assert!(send(&mut sender, strip, &library), "the strip is patched");
    let full = Command::SetIntensity {
        target: Target::Fixture {
            fixture: LIGHTS + 1,
        },
        intensity: u16::MAX,
    };
    assert!(send(&mut sender, full, &library), "the strip is lit");
    // every second light, from light 2: 256 runs of one light, sent as one command
    let even: Vec<Member> = (2..=LIGHTS)
        .step_by(2)
        .map(|light| Member::Light { light })
        .collect();
    let group = Command::SetGroup {
        name: text("even"),
        members: MemberList::Fits(Members::new(&even).expect("256 members")),
    };
    assert!(send(&mut sender, group, &library), "the group is made");
    let green = Command::SetColour {
        target: Target::Group {
            group: text("even"),
        },
        red: 0,
        green: u16::MAX,
        blue: 0,
    };
    // Thread start-up may allocate: wait until both outputs were served.
    mock::wait_until("the outputs were never served", || {
        let outputs = engine.latest().state.outputs;
        outputs[..2]
            .iter()
            .all(|o| o.is_some_and(|o| o.counters.accepted > 0))
    });

    let (allocations, tick_frees) = (counting::allocations(), counting::tick_frees());
    let frame = engine.latest().state.frame;
    for value in 0..=255 {
        let channel = ChannelNumbers {
            universe: 1,
            address: 1 + u16::from(value),
        };
        let _ = sender.send(Command::SetChannel { channel, value });
        let _ = sender.send(Command::Blackout { on: value % 2 == 0 });
        let light = u16::from(value) * 2;
        let colour = Command::SetColour {
            target: Target::Light { light: light + 1 },
            red: u16::from(value) << 8,
            green: 0,
            blue: u16::MAX,
        };
        assert!(
            send(&mut sender, colour, &library),
            "a colour for light {light}"
        );
        if value % 16 == 8 {
            // the runs of the group: applied in one wake and sent back to be freed
            assert!(send(&mut sender, green, &library), "the group goes green");
        }
        if value % 16 == 0 {
            // new writers: the tick swaps them in and sends the old ones back
            let back = Command::Move {
                fixture: light + 1,
                at: place(light),
            };
            assert!(send(&mut sender, back, &library), "fixture {light} moves");
        }
        if value % 64 == 32 {
            // a new device for output 1: the tick swaps it in and sends the old one back
            counting::uncounted(|| mock::attach(&mut sender, universes, 1, run, Rate::Sixty));
        }
        // Pace the commands by the tick: a reader on every frame waits for the next picture.
        let seen = engine.latest().state.frame;
        mock::wait_until("no frame went by", || engine.latest().state.frame > seen);
    }
    let state = engine.latest().state;
    let lit = engine.latest().lights()[..usize::from(LIGHTS) + 100]
        .iter()
        .filter(|light| light.intensity == Some(u16::MAX))
        .count();
    let green_lights = engine.latest().lights()[..usize::from(LIGHTS)]
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|light| light.colour.is_some_and(|c| c.green == u16::MAX))
        .count();
    let ticks = state.frame - frame;
    let new_allocations = counting::allocations() - allocations;
    let new_tick_frees = counting::tick_frees() - tick_frees;
    drop(engine);

    assert!(ticks >= 5, "too few ticks to prove anything: {ticks}");
    assert!(ticks >= 256, "a frame went by for each of 256 commands: {ticks}");
    assert_eq!(
        lit,
        usize::from(LIGHTS) + 100,
        "every patched light is held lit"
    );
    assert_eq!(
        green_lights,
        even.len(),
        "every light of the group is green"
    );
    let accepted = state.outputs.map(|o| o.map(|o| o.counters.accepted));
    assert!(
        accepted[..2].iter().all(|n| n.is_some_and(|n| n > 5)),
        "both outputs were served: {accepted:?}"
    );
    assert_eq!(
        new_allocations, 0,
        "{ticks} ticks made {new_allocations} allocations"
    );
    assert_eq!(
        new_tick_frees, 0,
        "{ticks} ticks made {new_tick_frees} frees on the tick thread"
    );
}
