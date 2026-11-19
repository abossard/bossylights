//! M3 T3 end to end over the API: who wins, read on a fake DMX USB Pro. Two Generic RGB
//! PARs at 1:1 and 1:4. Autopilot plays blue on both at full; Show green and Manual red
//! on PAR 1 only. PAR 1 shows the highest rank, PAR 2 stays blue. Each release reveals the
//! rank under it. Then each control on its own: explicit zero, blackout, `ReleaseChannels`,
//! `ReleaseAll`, the brightness setting, `Stay` and `Reveal`, both Manual end settings,
//! stopped holds after a settings edit, and a direct grab over a playing Manual look.

mod common;

use common::{Heard, Socket, answer, command, connect, open, send};
use engine::{Engine, Universes};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BLUE: [u8; 3] = [0, 0, 255];
const GREEN: [u8; 3] = [0, 255, 0];
const RED: [u8; 3] = [255, 0, 0];
const DARK: [u8; 3] = [0, 0, 0];

/// Waits until the adapter's last frame shows PAR 1 at `one` and PAR 2 at `two`.
async fn until_dmx(heard: &Heard, one: [u8; 3], two: [u8; 3]) {
    let started = Instant::now();
    loop {
        let dmx = heard
            .lock()
            .unwrap()
            .last()
            .map(|c| ([c[0], c[1], c[2]], [c[3], c[4], c[5]]));
        if dmx == Some((one, two)) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "1 to 6: {dmx:?}, wanted {one:?} {two:?}"
        );
        tokio::task::yield_now().await;
    }
}

/// Sends `command` until the library is read, then its answer.
async fn once_read(socket: &mut Socket, sent: &Value) -> Value {
    let started = Instant::now();
    loop {
        let reply = command(socket, sent.clone()).await;
        if reply != json!({"Refused": {"reason": "NoSuchFixture"}}) {
            return reply;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "no library");
        tokio::task::yield_now().await;
    }
}

/// Waits until the picture at `pointer` is `wanted`.
async fn until_picture(socket: &mut Socket, pointer: &str, wanted: &Value) {
    let started = Instant::now();
    loop {
        send(socket, &json!("Query")).await;
        let seen = answer(socket).await["Picture"].pointer(pointer).cloned();
        if seen.as_ref() == Some(wanted) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{pointer}: {seen:?}, wanted {wanted}"
        );
    }
}

/// Accepted, or the test stops.
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

fn rules(brightness: &str, manual_end: &str, other_end: &str) -> Value {
    json!({"SetMixRules": {"rules": {
        "brightness": brightness, "manual_end": manual_end, "other_end": other_end,
    }}})
}

fn par(fixture: u16) -> Value {
    json!({"Fixture": {"fixture": fixture}})
}

fn step(look: u16, target: &Value, values: &Value) -> Value {
    json!({"PutStep": {"look": look, "name": format!("look {look}"), "step": {
        "target": target, "values": values,
    }}})
}

fn rgb([red, green, blue]: [u8; 3]) -> Value {
    let wide = |byte: u8| u16::from(byte) * 257;
    json!({"red": wide(red), "green": wide(green), "blue": wide(blue)})
}

#[tokio::test(flavor = "multi_thread")]
async fn rank_stop_and_release_decide_each_light_on_dmx() {
    let heard: Heard = Arc::new(Mutex::new(Vec::new()));
    let stop_adapter = Arc::new(AtomicBool::new(false));
    let opens = open(Some(Arc::clone(&heard)), Arc::clone(&stop_adapter));
    let (engine, sender) = Engine::start_with(Universes::new(1).unwrap(), opens).unwrap();
    let definitions = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixture-library/tests/definitions");
    let address = common::serve(engine, sender, Some(definitions)).await;
    let mut socket = connect(address).await;
    let s = &mut socket;
    accepted(
        s,
        json!({"SetOutput": {
            "output": 1, "kind": {"DmxUsbPro": {"adapter": 1}},
            "universes": {"first": 1, "count": 1}, "rate": null,
        }}),
    )
    .await;
    for (fixture, address) in [(1, 1), (2, 4)] {
        let patch = json!({"Patch": {
            "name": format!("PAR {fixture}"), "maker": "Generic", "model": "Generic RGB",
            "mode": "RGB", "at": {"universe": 1, "address": address},
        }});
        assert_eq!(
            once_read(s, &patch).await,
            json!({"Patched": {"fixture": fixture}})
        );
    }
    let both = json!({"Group": {"group": "both"}});
    accepted(
        s,
        json!({"SetGroup": {"name": "both", "members": [par(1), par(2)]}}),
    )
    .await;
    // 1 blue at full on both, 2 green, 3 red, 4 dark on PAR 1, 5 full, 6 dark on PAR 2
    let looks = [
        step(1, &both, &json!({"intensity": 65535, "colour": rgb(BLUE)})),
        step(2, &par(1), &json!({"colour": rgb(GREEN)})),
        step(3, &par(1), &json!({"colour": rgb(RED)})),
        step(4, &par(1), &json!({"intensity": 0})),
        step(5, &par(2), &json!({"intensity": 65535})),
        step(6, &par(2), &json!({"intensity": 0})),
    ];
    for look in looks {
        accepted(s, look).await;
    }

    // rank: blue Autopilot, green Show, red Manual, then green and blue on release
    let autopilot = start(s, 1, "Autopilot").await;
    until_dmx(&heard, BLUE, BLUE).await;
    let show = start(s, 2, "Show").await;
    until_dmx(&heard, GREEN, BLUE).await;
    let manual = start(s, 3, "Manual").await;
    until_dmx(&heard, RED, BLUE).await;
    until_picture(
        s,
        "/state/plays",
        &json!([
            {"play": autopilot, "look": 1, "rank": "Autopilot", "state": "Playing"},
            {"play": show, "look": 2, "rank": "Show", "state": "Playing"},
            {"play": manual, "look": 3, "rank": "Manual", "state": "Playing"},
        ]),
    )
    .await;
    accepted(s, json!({"ReleaseHold": {"play": manual}})).await;
    until_dmx(&heard, GREEN, BLUE).await;
    accepted(s, json!({"ReleaseHold": {"play": show}})).await;
    until_dmx(&heard, BLUE, BLUE).await;
    // started the other way round: the rank still decides, not the start
    let manual = start(s, 3, "Manual").await;
    let show = start(s, 2, "Show").await;
    until_dmx(&heard, RED, BLUE).await;
    // an old finger-up after a restart ends nothing
    let restarted = start(s, 3, "Manual").await;
    accepted(s, json!({"EndFlash": {"play": manual}})).await;
    accepted(s, json!({"EndFlash": {"play": show}})).await; // Show: not a flash
    until_dmx(&heard, RED, BLUE).await;
    accepted(s, json!({"EndFlash": {"play": restarted}})).await;
    until_dmx(&heard, GREEN, BLUE).await;
    accepted(s, json!({"ReleaseHold": {"play": show}})).await;
    until_dmx(&heard, BLUE, BLUE).await;

    // explicit zero: a Manual dark step over the Autopilot's full
    let zero = start(s, 4, "Manual").await;
    until_dmx(&heard, DARK, BLUE).await;
    accepted(s, json!({"ReleaseHold": {"play": zero}})).await;
    until_dmx(&heard, BLUE, BLUE).await;
    // blackout: every channel at zero, everything comes back
    accepted(s, json!({"Blackout": {"on": true}})).await;
    until_dmx(&heard, DARK, DARK).await;
    accepted(s, json!({"Blackout": {"on": false}})).await;
    until_dmx(&heard, BLUE, BLUE).await;
    // ReleaseChannels: the raw channels go, a Manual play stays
    let manual = start(s, 3, "Manual").await;
    let raw = |address: u16, value: u8| json!({"SetChannel": {"channel": {"universe": 1, "address": address}, "value": value}});
    accepted(s, raw(3, 9)).await;
    until_dmx(&heard, [255, 0, 9], BLUE).await;
    accepted(s, json!("ReleaseChannels")).await;
    until_dmx(&heard, RED, BLUE).await;
    // ReleaseAll: Manual plays, direct holds and raw channels go; Show and Autopilot stay
    let show = start(s, 2, "Show").await;
    accepted(
        s,
        json!({"SetColour": {"target": par(2), "red": 0, "green": 65535, "blue": 0}}),
    )
    .await;
    accepted(s, raw(6, 9)).await;
    until_dmx(&heard, RED, [0, 255, 9]).await;
    accepted(s, json!("ReleaseAll")).await;
    until_dmx(&heard, GREEN, BLUE).await;
    accepted(s, json!({"ReleaseHold": {"play": manual}})).await; // ended: a no-op
    accepted(s, json!({"ReleaseHold": {"play": show}})).await;
    until_dmx(&heard, BLUE, BLUE).await;

    // brightness: two Show values on PAR 2, the later dark, from latest to highest
    let full = start(s, 5, "Show").await;
    let dark = start(s, 6, "Show").await;
    until_dmx(&heard, BLUE, DARK).await;
    accepted(s, rules("Highest", "UntilRelease", "Reveal")).await;
    until_dmx(&heard, BLUE, BLUE).await;
    until_picture(s, "/state/rules/brightness", &json!("Highest")).await;
    accepted(s, rules("Latest", "UntilRelease", "Reveal")).await;
    until_dmx(&heard, BLUE, DARK).await;
    // Reveal: a stopped Show leaves nothing
    accepted(s, json!({"StopLook": {"play": dark}})).await;
    until_dmx(&heard, BLUE, BLUE).await;
    accepted(s, json!({"ReleaseHold": {"play": full}})).await;
    // Stay: a stopped Show stays, kept, until a settings edit to Reveal clears it
    accepted(s, rules("Latest", "UntilRelease", "Stay")).await;
    let show = start(s, 2, "Show").await;
    until_dmx(&heard, GREEN, BLUE).await;
    accepted(s, json!({"StopLook": {"play": show}})).await;
    until_picture(s, "/state/plays/1/state", &json!("Kept")).await;
    until_dmx(&heard, GREEN, BLUE).await;
    accepted(s, rules("Latest", "UntilRelease", "Reveal")).await;
    until_dmx(&heard, BLUE, BLUE).await;
    let alone = json!([{"play": autopilot, "look": 1, "rank": "Autopilot", "state": "Playing"}]);
    until_picture(s, "/state/plays", &alone).await;

    // Manual UntilRelease: a stopped Manual stays until released
    let manual = start(s, 3, "Manual").await;
    until_dmx(&heard, RED, BLUE).await;
    accepted(s, json!({"StopLook": {"play": manual}})).await;
    until_picture(s, "/state/plays/1/state", &json!("Kept")).await;
    until_dmx(&heard, RED, BLUE).await;
    accepted(s, json!({"ReleaseHold": {"play": manual}})).await;
    until_dmx(&heard, BLUE, BLUE).await;
    // a kept Manual, then a settings edit to UntilStop: it goes
    let manual = start(s, 3, "Manual").await;
    accepted(s, json!({"StopLook": {"play": manual}})).await;
    until_picture(s, "/state/plays/1/state", &json!("Kept")).await;
    accepted(s, rules("Latest", "UntilStop", "Reveal")).await;
    until_dmx(&heard, BLUE, BLUE).await;
    // Manual UntilStop: the stop ends it
    let manual = start(s, 3, "Manual").await;
    until_dmx(&heard, RED, BLUE).await;
    accepted(s, json!({"StopLook": {"play": manual}})).await;
    until_dmx(&heard, BLUE, BLUE).await;

    // a direct grab over a playing Manual look, released: the look shows again
    let manual = start(s, 3, "Manual").await;
    until_dmx(&heard, RED, BLUE).await;
    accepted(
        s,
        json!({"SetColour": {"target": par(1), "red": 65535, "green": 65535, "blue": 65535}}),
    )
    .await;
    until_dmx(&heard, [255, 255, 255], BLUE).await;
    accepted(s, json!({"ReleaseColour": {"target": par(1)}})).await;
    until_dmx(&heard, RED, BLUE).await;
    accepted(s, json!({"ReleaseHold": {"play": manual}})).await;
    until_dmx(&heard, BLUE, BLUE).await;

    send(s, &json!("Stop")).await;
    assert_eq!(answer(s).await, json!("Stopping"));
    stop_adapter.store(true, Ordering::Relaxed);
}
