//! Channel model T4 (#250) over the v11 wire, read on a fake DMX USB Pro: who wins a
//! 16-bit dimmer as one word in both start orders, and a sequence item that leaves a
//! light out reveals the Autopilot under it.

mod common;

use common::{Heard, Socket, command, connect, open};
use engine::{Engine, Universes};
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Waits until the adapter's last frame has `wanted` at each 0-based slot.
async fn until_dmx(heard: &Heard, wanted: &[(usize, u8)]) {
    let started = Instant::now();
    loop {
        let seen: Option<Vec<(usize, u8)>> = heard
            .lock()
            .unwrap()
            .last()
            .map(|frame| wanted.iter().map(|&(at, _)| (at, frame[at])).collect());
        if seen.as_deref() == Some(wanted) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{seen:?}, wanted {wanted:?}"
        );
        tokio::task::yield_now().await;
    }
}

async fn accepted(socket: &mut Socket, sent: Value) {
    assert_eq!(
        command(socket, sent.clone()).await,
        json!("Accepted"),
        "{sent}"
    );
}

/// Starts look `look` at `rank`: its play.
async fn start(socket: &mut Socket, look: u16, rank: &str) -> Value {
    let started = command(socket, json!({"StartLook": {"look": look, "rank": rank}})).await;
    let play = started["Started"]["play"].clone();
    assert!(play.is_object(), "look {look} at {rank}: {started}");
    play
}

fn rules(brightness: &str) -> Value {
    json!({"SetMixRules": {"rules": {
        "brightness": brightness, "manual_end": "UntilRelease", "other_end": "Reveal",
    }}})
}

fn step(look: u16, fixture: u16, values: &Value) -> Value {
    json!({"PutStep": {"look": look, "name": format!("look {look}"), "step": {
        "target": {"Fixture": {"fixture": fixture}}, "values": values,
    }}})
}

/// An engine with a fake DMX USB Pro on universe 1 and the fixtures read from `folder`,
/// each `(maker, model, mode, address)` patched as fixtures 1, 2, ...
async fn desk(folder: &str, fixtures: &[(&str, &str, &str, u16)]) -> (Socket, Heard) {
    let heard: Heard = Arc::new(Mutex::new(Vec::new()));
    let opens = open(Some(Arc::clone(&heard)), Arc::new(AtomicBool::new(false)));
    let (engine, sender) = Engine::start_with(Universes::new(1).unwrap(), opens).unwrap();
    let definitions = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(folder);
    let address = common::serve(engine, sender, Some(definitions)).await;
    let mut socket = connect(address).await;
    accepted(
        &mut socket,
        json!({"SetOutput": {
            "output": 1, "kind": {"DmxUsbPro": {"adapter": 1}},
            "universes": {"first": 1, "count": 1}, "rate": null,
        }}),
    )
    .await;
    for (fixture, (maker, model, mode, address)) in (1u16..).zip(fixtures) {
        let patch = json!({"Patch": {
            "name": format!("{model} {fixture}"), "maker": maker, "model": model,
            "mode": mode, "at": {"universe": 1, "address": address},
        }});
        let started = Instant::now();
        loop {
            let reply = command(&mut socket, patch.clone()).await;
            if reply == json!({"Patched": {"fixture": fixture}}) {
                break;
            }
            assert_eq!(reply, json!({"Refused": {"reason": "NoSuchFixture"}}));
            assert!(started.elapsed() < Duration::from_secs(10), "no library");
            tokio::task::yield_now().await;
        }
    }
    (socket, heard)
}

/// The Robe ColorSpot 575 AT in its 27-channel mode at 1:1 has its dimmer word at slots
/// 25 (coarse) and 26 (fine). Two Show looks differ only in the fine byte's carry:
/// 0x8100 and 0x80FF. Under Highest the greater word wins whichever started last, under
/// Latest the later start wins; either way both bytes come from one look.
#[tokio::test(flavor = "multi_thread")]
async fn a_sixteen_bit_dimmer_is_won_as_one_word_in_both_orders() {
    let (mut socket, heard) = desk(
        "../rig/tests/definitions",
        &[("Robe", "ColorSpot 575 AT", "1 - 27-Channel", 1)],
    )
    .await;
    let s = &mut socket;
    let (high, low) = (0x8100, 0x80FF);
    accepted(s, step(1, 1, &json!({"intensity": high}))).await;
    accepted(s, step(2, 1, &json!({"intensity": low}))).await;
    let word = |w: u16| [(25, w.to_be_bytes()[0]), (26, w.to_be_bytes()[1])];
    // (brightness rule, first started, then, the word that shows)
    let cases = [
        ("Highest", 1, 2, high),
        ("Highest", 2, 1, high),
        ("Latest", 1, 2, low),
        ("Latest", 2, 1, high),
    ];
    for (brightness, first, then, shows) in cases {
        accepted(s, rules(brightness)).await;
        let plays = [start(s, first, "Show").await, start(s, then, "Show").await];
        until_dmx(&heard, &word(shows)).await;
        for play in plays {
            accepted(s, json!({"ReleaseHold": {"play": play}})).await;
        }
        until_dmx(&heard, &word(0)).await;
    }
}

/// Two Generic RGB PARs at 1:1 and 1:4. The Autopilot holds PAR 1 at red 17. A Show
/// sequence plays red 91 on PAR 1, then a look on PAR 2 only: in that item PAR 1 is not
/// offered, so the Autopilot's 17 shows through, then 91 again on the next loop.
#[tokio::test(flavor = "multi_thread")]
async fn a_sequence_item_that_leaves_a_light_out_reveals_the_autopilot_under_it() {
    let par = ("Generic", "Generic RGB", "RGB");
    let (mut socket, heard) = desk(
        "../fixture-library/tests/definitions",
        &[(par.0, par.1, par.2, 1), (par.0, par.1, par.2, 4)],
    )
    .await;
    let s = &mut socket;
    let red = |byte: u16| json!({"intensity": 65535, "colour": {"red": byte * 257, "green": 0, "blue": 0}});
    accepted(s, step(1, 1, &red(17))).await;
    accepted(s, step(2, 1, &red(91))).await;
    accepted(s, step(3, 2, &red(50))).await;
    accepted(
        s,
        json!({"PutSequence": {"look": 4, "name": "gap", "sequence": {
            "items": [{"look": 2}, {"look": 3}]
        }}}),
    )
    .await;
    start(s, 1, "Autopilot").await;
    until_dmx(&heard, &[(0, 17), (3, 0)]).await;
    start(s, 4, "Show").await;
    until_dmx(&heard, &[(0, 91), (3, 0)]).await;
    until_dmx(&heard, &[(0, 17), (3, 50)]).await;
    until_dmx(&heard, &[(0, 91), (3, 0)]).await;
}

/// cx-4: a Generic RGB PAR has no dimmer, so its brightness lives in its colour bytes.
/// Over an Autopilot blue at full: a Show step of brightness alone offers nothing and the
/// blue stays; colour with brightness offers the colour scaled; a brightness grab alone
/// offers nothing. Each row ends released, back to blue. A raw 9 on PAR 2 marks a frame
/// that carries the row.
#[tokio::test(flavor = "multi_thread")]
async fn brightness_on_a_dimmerless_par_is_baked_into_its_colour() {
    let par = ("Generic", "Generic RGB", "RGB");
    let (mut socket, heard) = desk(
        "../fixture-library/tests/definitions",
        &[(par.0, par.1, par.2, 1), (par.0, par.1, par.2, 4)],
    )
    .await;
    let s = &mut socket;
    let colour = |[red, green, blue]: [u16; 3]| json!({"red": red, "green": green, "blue": blue});
    let blue = [(0, 0), (1, 0), (2, 255)];
    accepted(
        s,
        step(
            1,
            1,
            &json!({"intensity": 65535, "colour": colour([0, 0, 65535])}),
        ),
    )
    .await;
    accepted(s, step(2, 1, &json!({"intensity": 0}))).await;
    accepted(
        s,
        step(
            3,
            1,
            &json!({"intensity": 0x8080, "colour": colour([65535, 0, 0])}),
        ),
    )
    .await;
    start(s, 1, "Autopilot").await;
    until_dmx(&heard, &blue).await;
    // (row, look started at Show or a grab, what PAR 1 shows)
    type Shows = [(usize, u8); 3];
    let rows: [(&str, Value, Shows); 3] = [
        (
            "brightness alone",
            json!({"StartLook": {"look": 2, "rank": "Show"}}),
            blue,
        ),
        (
            "colour with brightness",
            json!({"StartLook": {"look": 3, "rank": "Show"}}),
            [(0, 128), (1, 0), (2, 0)],
        ),
        (
            "a brightness grab alone",
            json!({"SetIntensity": {"target": {"Fixture": {"fixture": 1}}, "intensity": 0}}),
            blue,
        ),
    ];
    for (row, sent, shows) in rows {
        let reply = command(s, sent).await;
        assert!(
            reply["Started"].is_object() || reply["Grabbed"].is_object(),
            "{row}: {reply}"
        );
        accepted(
            s,
            json!({"SetChannel": {"channel": {"universe": 1, "address": 6}, "value": 9}}),
        )
        .await;
        let mut marked = shows.to_vec();
        marked.push((5, 9));
        until_dmx(&heard, &marked).await;
        let play = &reply["Started"]["play"];
        let release = if play.is_null() {
            json!({"ReleaseIntensity": {"target": {"Fixture": {"fixture": 1}}}})
        } else {
            json!({"ReleaseHold": {"play": play}})
        };
        accepted(s, release).await;
        accepted(s, json!("ReleaseChannels")).await;
        until_dmx(&heard, &[blue[0], blue[1], blue[2], (5, 0)]).await;
    }
}

/// Stay and Reveal per channel. The Autopilot holds PAR 1 at red 17 and PAR 2 at red 33.
/// A Show sequence plays red 91 on PAR 1 for a beat, then red 50 on PAR 2 only for ten.
/// Stopped in its second item: under Stay each channel keeps its last offer, so PAR 2
/// stays 50 and PAR 1, in a gap, keeps none and shows 17; releasing the kept play reveals
/// 33. Under Reveal the stop gives both back to the Autopilot at once.
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_show_stays_or_reveals_per_channel_and_a_kept_play_is_released() {
    let par = ("Generic", "Generic RGB", "RGB");
    let (mut socket, heard) = desk(
        "../fixture-library/tests/definitions",
        &[(par.0, par.1, par.2, 1), (par.0, par.1, par.2, 4)],
    )
    .await;
    let s = &mut socket;
    let red = |byte: u16| json!({"intensity": 65535, "colour": {"red": byte * 257, "green": 0, "blue": 0}});
    accepted(s, step(1, 1, &red(17))).await;
    accepted(s, step(2, 2, &red(33))).await;
    accepted(s, step(3, 1, &red(91))).await;
    accepted(s, step(4, 2, &red(50))).await;
    accepted(
        s,
        json!({"PutSequence": {"look": 5, "name": "gap", "sequence": {
            "items": [{"look": 3}, {"look": 4, "width": 160}]
        }}}),
    )
    .await;
    start(s, 1, "Autopilot").await;
    start(s, 2, "Autopilot").await;
    until_dmx(&heard, &[(0, 17), (3, 33)]).await;
    // (other end rule, PAR 1 and PAR 2 after the stop)
    for (other_end, stopped) in [("Stay", [(0, 17), (3, 50)]), ("Reveal", [(0, 17), (3, 33)])] {
        accepted(
            s,
            json!({"SetMixRules": {"rules": {
                "brightness": "Latest", "manual_end": "UntilRelease", "other_end": other_end,
            }}}),
        )
        .await;
        let play = start(s, 5, "Show").await;
        until_dmx(&heard, &[(0, 91), (3, 33)]).await;
        until_dmx(&heard, &[(0, 17), (3, 50)]).await; // the second item, ten beats long
        accepted(s, json!({"StopLook": {"play": play}})).await;
        accepted(
            s,
            json!({"SetChannel": {"channel": {"universe": 1, "address": 9}, "value": 7}}),
        )
        .await;
        let mut marked = stopped.to_vec();
        marked.push((8, 7));
        until_dmx(&heard, &marked).await;
        accepted(s, json!({"ReleaseHold": {"play": play}})).await;
        accepted(s, json!("ReleaseChannels")).await;
        until_dmx(&heard, &[(0, 17), (3, 33), (8, 0)]).await;
    }
}
