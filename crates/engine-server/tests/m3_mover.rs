//! M3 T4 end to end over the API: mover properties played as a saved step at address 1,
//! read on a fake DMX USB Pro. Elation EVC MH 12-CH aimed at pan 0x1234, tilt 0x5678 and
//! zoomed half: 12 34 56 78 at 1 to 4, 80 at 9. The same zoom on a Martin Rush MH6 Wash,
//! whose zoom runs big to small: 7f at 9. A Shehds LED Spot 100W 14 Channel red with
//! Static Gobo / Gobo 2: 7 at 6, 24 at 7, its rotating gobo wheel at its default at 8. A
//! gobo cuts to its next slot in the first frame handed over after its start, and black,
//! not on the Shehds wheel, is refused in a step and held directly.

mod common;

use common::{Heard, Socket, command, connect, open};
use engine::{Engine, Event, Outcome, Universes, Wakes};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task::block_in_place;

/// A folder of the three definitions: two from the fixture library's tests, the Martin
/// from the rig's.
/// Each call its own folder: the tests of this binary run side by side.
fn definitions() -> PathBuf {
    static MADE: AtomicUsize = AtomicUsize::new(0);
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let folder = std::env::temp_dir().join(format!(
        "bossylights-m3-mover-{}-{}",
        std::process::id(),
        MADE.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    for (from, file) in [
        ("../fixture-library/tests/definitions", "Elation-EVC-MH.qxf"),
        (
            "../fixture-library/tests/definitions",
            "Shehds-LED-Spot-100W.qxf",
        ),
        ("../rig/tests/definitions", "Martin-Rush-MH6-Wash.qxf"),
    ] {
        std::fs::copy(here.join(from).join(file), folder.join(file)).unwrap();
    }
    folder
}

/// An engine with the three definitions, universe 1 on the fake adapter heard into
/// `heard`, and `maker` `model` `mode` patched at address 1 as fixture 1. Its clock runs
/// until the test holds it.
async fn patched(heard: &Heard, stop: &Arc<AtomicBool>, fixture: [&str; 3]) -> (Socket, Wakes) {
    let opens = open(Some(Arc::clone(heard)), Arc::clone(stop));
    let (engine, sender, wakes) = Engine::start_stepped(Universes::new(1).unwrap(), opens).unwrap();
    let address = common::serve(engine, sender, Some(definitions())).await;
    let mut socket = connect(address).await;
    let dmx = json!({"SetOutput": {
        "output": 1, "kind": {"DmxUsbPro": {"adapter": 1}},
        "universes": {"first": 1, "count": 1}, "rate": null,
    }});
    assert_eq!(command(&mut socket, dmx).await, json!("Accepted"));
    let [maker, model, mode] = fixture;
    let patch = json!({"Patch": {
        "name": model, "maker": maker, "model": model, "mode": mode,
        "at": {"universe": 1, "address": 1},
    }});
    // the library is read on a thread of its own: patch once it is there
    let started = Instant::now();
    loop {
        let reply = command(&mut socket, patch.clone()).await;
        if reply != json!({"Refused": {"reason": "NoSuchFixture"}}) {
            assert_eq!(reply, json!({"Patched": {"fixture": 1}}), "{model}");
            return (socket, wakes);
        }
        assert!(started.elapsed() < Duration::from_secs(10), "no library");
        tokio::task::yield_now().await;
    }
}

/// A channel, counted from 1, and its byte.
type Bytes = (usize, u8);

/// Saves `values` on fixture 1 as look `look` and plays it. The play.
async fn play(socket: &mut Socket, look: u16, values: &Value) -> Value {
    let step = json!({"PutStep": {"look": look, "name": "mover", "step": {
        "target": {"Fixture": {"fixture": 1}}, "values": values,
    }}});
    assert_eq!(command(socket, step).await, json!("Accepted"), "{values}");
    let started = command(socket, json!({"StartLook": {"look": look}})).await;
    assert!(started["Started"]["play"].is_object(), "{started}");
    started["Started"]["play"].clone()
}

/// The frames handed over to the adapter (output 1) since the engine started, by when
/// each was due, how many of them were dropped, and what came of each.
#[derive(Default)]
struct Handed {
    dues: Vec<Duration>,
    dropped: usize,
    came: Vec<(Duration, Outcome)>,
}

impl Handed {
    /// Takes the tick's events, then waits until every frame handed over is heard or
    /// dropped.
    async fn drain(&mut self, wakes: &mut Wakes, heard: &Heard) {
        let started = Instant::now();
        loop {
            for event in wakes.events() {
                match event {
                    Event::Handed { output, due, .. } if output.slot() == 0 => self.dues.push(due),
                    Event::Came {
                        output,
                        outcome,
                        due,
                    } if output.slot() == 0 => {
                        self.dropped += usize::from(outcome == Outcome::Dropped);
                        self.came.push((due, outcome));
                    }
                    Event::Handed { .. } | Event::Came { .. } => {}
                }
            }
            if heard.lock().unwrap().len() + self.dropped >= self.dues.len() {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "frames handed over but not heard"
            );
            tokio::task::yield_now().await;
        }
    }
}

/// The tick held: the frames handed over and heard until then.
struct Held<'a> {
    handed: &'a mut Handed,
    /// How many frames were heard, and how many handed over, when the tick was held.
    heard: usize,
    dues: usize,
}

impl<'a> Held<'a> {
    /// `handed` has every frame handed over since the engine started.
    async fn new(handed: &'a mut Handed, wakes: &mut Wakes, heard: &Heard) -> Self {
        block_in_place(|| wakes.hold()).unwrap();
        handed.drain(wakes, heard).await;
        Self {
            heard: heard.lock().unwrap().len(),
            dues: handed.dues.len(),
            handed,
        }
    }

    /// Wakes the tick one frame at a time until the first frame due since it was held
    /// is taken by the adapter. That frame, as the adapter heard it.
    async fn first_due(&mut self, wakes: &mut Wakes, heard: &Heard) -> Vec<u8> {
        // what came of it is written in a later wake
        let first = loop {
            block_in_place(|| wakes.wake()).unwrap();
            self.handed.drain(wakes, heard).await;
            if let Some(&due) = self.handed.dues.get(self.dues)
                && let Some(&(_, outcome)) = self.handed.came.iter().find(|(came, _)| *came == due)
            {
                break outcome;
            }
        };
        assert_eq!(first, Outcome::Accepted, "the first frame due");
        heard.lock().unwrap()[self.heard].to_vec()
    }
}

/// Waits until the adapter's last frame shows `bytes` (channel from 1, byte). The index
/// of that frame.
async fn until_dmx(heard: &Heard, bytes: &[Bytes]) -> usize {
    let started = Instant::now();
    loop {
        {
            let frames = heard.lock().unwrap();
            if let Some(last) = frames.last()
                && bytes
                    .iter()
                    .all(|&(channel, byte)| last[channel - 1] == byte)
            {
                return frames.len() - 1;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "wanted {bytes:?}, last frame {:?}",
                frames.last().map(|f| &f[..14])
            );
        }
        tokio::task::yield_now().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mover_properties_reach_their_channels_at_address_1() {
    let aimed = json!({"position": {"pan": 0x1234, "tilt": 0x5678}, "zoom": 0x8000});
    let red = json!({"red": 65535, "green": 0, "blue": 0});
    // (maker, model, mode; the values; channels from 1 and their bytes)
    type Case<'a> = ([&'a str; 3], Value, &'a [Bytes]);
    let cases: [Case; 3] = [
        (
            ["Elation", "EVC MH", "12-CH"],
            aimed.clone(),
            &[(1, 0x12), (2, 0x34), (3, 0x56), (4, 0x78), (9, 0x80)],
        ),
        (
            ["Martin", "Rush MH6 Wash", "Standard"],
            json!({"zoom": 0x8000}),
            &[(9, 0x7f)],
        ),
        (
            ["Shehds", "LED Spot 100W", "14 Channel"],
            json!({"colour": red, "gobos": [3, null, null]}),
            &[(6, 7), (7, 24), (8, 0)],
        ),
    ];
    for (fixture, values, bytes) in cases {
        let heard: Heard = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (mut socket, _) = patched(&heard, &stop, fixture).await;
        play(&mut socket, 1, &values).await;
        until_dmx(&heard, bytes).await;
        stop.store(true, Ordering::Relaxed);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_gobo_cuts_to_its_slot_and_a_colour_not_on_the_wheel_is_refused() {
    let heard: Heard = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let shehds = ["Shehds", "LED Spot 100W", "14 Channel"];
    let (mut socket, mut wakes) = patched(&heard, &stop, shehds).await;

    let black = json!({"colour": {"red": 0, "green": 0, "blue": 0}});
    let step = json!({"PutStep": {"look": 9, "name": "black", "step": {
        "target": {"Fixture": {"fixture": 1}}, "values": black,
    }}});
    assert_eq!(
        command(&mut socket, step).await,
        json!({"Refused": {"reason": {"Unsupported": {"light": 1, "property": "Colour"}}}})
    );
    // the same black held directly: refused too
    let direct = json!({"SetColour": {
        "target": {"Fixture": {"fixture": 1}}, "red": 0, "green": 0, "blue": 0,
    }});
    assert_eq!(
        command(&mut socket, direct).await,
        json!({"Refused": {"reason": {"Unsupported": {"light": 1, "property": "Colour"}}}})
    );
    // a grab of red, then black as its update: refused too, and let go
    let fixture = json!({"Fixture": {"fixture": 1}});
    let red = json!({"Colour": {"red": 65535, "green": 0, "blue": 0}});
    let grabbed = command(
        &mut socket,
        json!({"SetProperty": {"target": fixture, "value": red}}),
    )
    .await;
    let grab = grabbed["Grabbed"]["grab"].clone();
    assert!(grab.is_object(), "{grabbed}");
    let update = json!({"UpdateGrab": {"grab": grab, "value": {"Colour": black["colour"]}}});
    assert_eq!(
        command(&mut socket, update).await,
        json!({"Refused": {"reason": {"Unsupported": {"light": 1, "property": "Colour"}}}})
    );
    let release = json!({"ReleaseProperty": {"target": fixture, "property": "Colour"}});
    assert_eq!(command(&mut socket, release).await, json!("Accepted"));
    let gobo = json!({"gobos": [null, 9, null]});
    let step = json!({"PutStep": {"look": 9, "name": "no slot 9", "step": {
        "target": {"Fixture": {"fixture": 1}}, "values": gobo,
    }}});
    assert_eq!(
        command(&mut socket, step).await,
        json!({"Refused": {"reason": {"Unsupported": {"light": 1, "property": {"Gobo": 2}}}}})
    );
    // no light has a fourth wheel: not a command
    let fourth = json!({"Gobo": {"wheel": 4, "slot": 1}});
    assert_eq!(
        command(
            &mut socket,
            json!({"SetProperty": {"target": fixture, "value": fourth}})
        )
        .await,
        json!({"Refused": {"reason": "NotACommand"}})
    );
    // slots count from 1: slot 0 is not a command on every wheel, one with no writer too
    let zero = |values: Value| {
        json!({"PutStep": {"look": 9, "name": "slot 0", "step": {
            "target": {"Fixture": {"fixture": 1}}, "values": values,
        }}})
    };
    let zeros = [
        zero(json!({"gobos": [0, null, null]})),
        zero(json!({"gobos": [null, null, 0]})),
        json!({"SetProperty": {"target": fixture, "value": {"Gobo": {"wheel": 1, "slot": 0}}}}),
        json!({"SetProperty": {"target": fixture, "value": {"Gobo": {"wheel": 3, "slot": 0}}}}),
    ];
    for zero in zeros {
        assert_eq!(
            command(&mut socket, zero.clone()).await,
            json!({"Refused": {"reason": "NotACommand"}}),
            "{zero}"
        );
    }

    // Gobo 2 (24), then Gobo 7 (74), the later start winning: the first frame due after
    // the second start is taken by the adapter and shows 74, and every frame heard between
    // shows one slot or the other, never a byte on the way
    play(&mut socket, 1, &json!({"gobos": [3, null, null]})).await;
    let from = until_dmx(&heard, &[(7, 24)]).await;
    let mut handed = Handed::default();
    let mut held = Held::new(&mut handed, &mut wakes, &heard).await;
    play(&mut socket, 2, &json!({"gobos": [8, null, null]})).await;
    assert_eq!(
        held.first_due(&mut wakes, &heard).await[6],
        74,
        "the first frame after the second start"
    );
    wakes.run();
    let to = until_dmx(&heard, &[(7, 74)]).await;
    let between: Vec<u8> = heard.lock().unwrap()[from..=to]
        .iter()
        .map(|frame| frame[6])
        .collect();
    assert!(
        between.iter().all(|&byte| byte == 24 || byte == 74),
        "channel 7 on the way: {between:?}"
    );
    stop.store(true, Ordering::Relaxed);
}

/// Each gobo wheel is its own claim (#179): on the Shehds, wheel 1 slot 3 (24 at 7) from
/// one look and wheel 2 slot 7 (64 at 8) from another, both Manual, show both in either
/// start order.
#[tokio::test(flavor = "multi_thread")]
async fn each_gobo_wheel_wins_on_its_own() {
    let heard: Heard = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let shehds = ["Shehds", "LED Spot 100W", "14 Channel"];
    let (mut socket, mut wakes) = patched(&heard, &stop, shehds).await;
    let wheel_1 = (json!({"gobos": [3, null, null]}), (7, 24));
    let wheel_2 = (json!({"gobos": [null, 7, null]}), (8, 64));
    let mut handed = Handed::default();
    for (first, second) in [(&wheel_1, &wheel_2), (&wheel_2, &wheel_1)] {
        play(&mut socket, 1, &first.0).await;
        until_dmx(&heard, &[first.1]).await;
        // the first frame due after the second start shows both
        let mut held = Held::new(&mut handed, &mut wakes, &heard).await;
        play(&mut socket, 2, &second.0).await;
        let frame = held.first_due(&mut wakes, &heard).await;
        assert_eq!(
            (frame[6], frame[7]),
            (24, 64),
            "{} after {}",
            second.0,
            first.0
        );
        wakes.run();
        assert_eq!(
            command(&mut socket, json!("ReleaseAll")).await,
            json!("Accepted")
        );
        until_dmx(&heard, &[(7, 0), (8, 0)]).await;
    }
    stop.store(true, Ordering::Relaxed);
}
