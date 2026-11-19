//! Proves the tick thread allocates nothing per wake, also while it serves two outputs at
//! different rates, resolves 512 patched lights, takes commands, swaps in new writers and
//! hands a reader a snapshot on every frame. A counting allocator sees every allocation in
//! this process, except those of the front while it sends: a light or patch command
//! allocates there (the patch is a calculation on the heap), never on the tick thread. The
//! outputs are attached and the lights patched before the window (a device is boxed, the
//! writers compiled on the sending thread). In the window the main thread sends and reads
//! snapshots, which allocates nothing, and sleeps, so any allocation counted is the tick's
//! or the reader's. That each real device's `take` allocates nothing is proved in
//! `crates/outputs/tests/no_alloc.rs`.

#[path = "../../outputs/tests/counting/mod.rs"]
mod counting;
mod mock;

use engine::{
    Begun, ChannelNumbers, Command, Engine, Fixture, Rate, RunNumbers, Sender, Target, Text,
    Universes,
};

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

/// Light `n` of the 512 (from 0) at its place: 170 to a universe, 3 channels each.
fn place(n: u16) -> ChannelNumbers {
    ChannelNumbers {
        universe: 1 + n / 170,
        address: 1 + 3 * (n % 170),
    }
}

fn main() {
    let universes = Universes::new(UNIVERSES).expect("4 universes");
    let (mut engine, mut sender) = Engine::start(universes).expect("the tick thread starts");
    let run = RunNumbers { first: 1, count: 1 };
    mock::attach(&mut sender, universes, 1, run, Rate::Sixty);
    mock::attach(&mut sender, universes, 2, run, Rate::Thirty);
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
    // Thread start-up may allocate: wait until both outputs were served.
    mock::wait_until("the outputs were never served", || {
        let outputs = engine.latest().state.outputs;
        outputs[..2]
            .iter()
            .all(|o| o.is_some_and(|o| o.counters.accepted > 0))
    });

    let allocations = counting::allocations();
    let frame = engine.latest().state.frame;
    let mut pictures = 0;
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
        if value % 16 == 0 {
            // new writers: the tick swaps them in and sends the old ones back
            let back = Command::Move {
                fixture: light + 1,
                at: place(light),
            };
            assert!(send(&mut sender, back, &library), "fixture {light} moves");
        }
        // Pace the commands by the tick: a reader on every frame waits for the next picture.
        let seen = engine.latest().state.frame;
        mock::wait_until("no frame went by", || engine.latest().state.frame > seen);
        pictures += 1;
    }
    let state = engine.latest().state;
    let lit = engine.latest().lights()[..usize::from(LIGHTS)]
        .iter()
        .filter(|light| light.intensity == u16::MAX)
        .count();
    let ticks = state.frame - frame;
    let new_allocations = counting::allocations() - allocations;
    drop(engine);

    assert!(ticks >= 5, "too few ticks to prove anything: {ticks}");
    assert_eq!(pictures, 256, "the reader saw every frame it waited for");
    assert_eq!(lit, usize::from(LIGHTS), "every patched light is held lit");
    let accepted = state.outputs.map(|o| o.map(|o| o.counters.accepted));
    assert!(
        accepted[..2].iter().all(|n| n.is_some_and(|n| n > 5)),
        "both outputs were served: {accepted:?}"
    );
    assert_eq!(
        new_allocations, 0,
        "{ticks} ticks made {new_allocations} allocations"
    );
}
