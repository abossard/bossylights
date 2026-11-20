//! Linked sequences through /api, sampled on the stepped clock and read on a DMX USB Pro.

mod common;

use common::{Heard, Socket, answer, command, connect, open, send, serve};
use engine::{Engine, Universes, Wakes};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::task::block_in_place;

/// Only one test holds the fake port at a time.
static PORT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const GREEN: [u8; 3] = [0, 255, 0];
const YELLOW: [u8; 3] = [255, 255, 0];

fn step(look: u16, colour: [u8; 3]) -> Value {
    let [red, green, blue] = colour.map(|byte| u16::from(byte) * 257);
    json!({"PutStep": {"look": look, "name": format!("step {look}"), "step": {
        "target": {"Fixture": {"fixture": 1}},
        "values": {"intensity": 65535, "colour": {"red": red, "green": green, "blue": blue}}
    }}})
}

fn sequence(look: u16, items: Value) -> Value {
    json!({"PutSequence": {"look": look, "name": format!("sequence {look}"), "sequence": {
        "items": items
    }}})
}

async fn accepted(socket: &mut Socket, sent: Value) {
    assert_eq!(
        command(socket, sent.clone()).await,
        json!("Accepted"),
        "{sent}"
    );
}

async fn start(socket: &mut Socket, look: u16, rank: &str) -> Value {
    let started = command(socket, json!({"StartLook": {"look": look, "rank": rank}})).await;
    let play = &started["Started"]["play"];
    assert!(play.is_object(), "look {look}: {started}");
    play.clone()
}

/// Each sample is at least one output period after the last. Hold the clock before
/// attaching the adapter and drain its complete frame before advancing time again.
async fn shows<const N: usize>(wakes: &Wakes, heard: &Heard, at: Duration, expected: [u8; N]) {
    let next = heard.lock().unwrap().len();
    assert_eq!(block_in_place(|| wakes.wake_at(at)).unwrap(), at);
    let waited = Instant::now();
    loop {
        if let Some(frame) = heard.lock().unwrap().get(next) {
            assert_eq!(frame[..N], expected, "at {at:?}");
            return;
        }
        assert!(
            waited.elapsed() < Duration::from_secs(10),
            "no DMX at {at:?}"
        );
        tokio::time::sleep(Duration::from_millis(1)).await; // poll
    }
}

async fn pixel_desk(pixels: u16) -> (Socket, Wakes, Heard, Arc<AtomicBool>, Duration) {
    let heard = Heard::default();
    let stop = Arc::new(AtomicBool::new(false));
    let opens = open(Some(Arc::clone(&heard)), Arc::clone(&stop));
    let (engine, sender, wakes) = Engine::start_stepped(Universes::new(1).unwrap(), opens).unwrap();
    let mut socket = connect(serve(engine, sender, None).await).await;
    block_in_place(|| wakes.hold()).unwrap();
    let zero = block_in_place(|| wakes.wake()).unwrap() + Duration::from_secs(1);

    accepted(
        &mut socket,
        json!({"SetOutput": {
            "output": 1, "kind": {"DmxUsbPro": {"adapter": 1}},
            "universes": {"first": 1, "count": 1}, "rate": "60"
        }}),
    )
    .await;
    let patch = json!({"PatchStrip": {"name": "Pixel", "pixels": pixels, "color": "Rgb",
        "at": {"universe": 1, "address": 1}}});
    assert_eq!(
        command(&mut socket, patch).await,
        json!({"Patched": {"fixture": 1}})
    );
    accepted(&mut socket, json!({"SetInternalTempo": {"bpm": 120}})).await;
    (socket, wakes, heard, stop, zero)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_linked_transition_manual_release_and_a_shared_edit_reach_the_wire() {
    let _port = PORT.lock().await;
    let (mut socket, wakes, heard, stop, zero) = pixel_desk(1).await;
    let at = |ms| zero + Duration::from_millis(ms);
    for (id, colour) in [(1, RED), (2, BLUE), (3, GREEN)] {
        accepted(&mut socket, step(id, colour)).await;
    }
    // Child 4: one beat red, then two blue with a one-beat transition.
    accepted(
        &mut socket,
        sequence(
            4,
            json!([{"look": 1}, {"look": 2, "width": 32, "transition": 50}]),
        ),
    )
    .await;
    accepted(
        &mut socket,
        sequence(
            5,
            json!([{"look": 4, "width": 48, "use": {"speed": "Double"}}]),
        ),
    )
    .await;
    let parent = start(&mut socket, 5, "Show").await;
    for (ms, rgb) in [(0, RED), (375, [128, 0, 128]), (500, BLUE)] {
        shows(&wakes, &heard, at(ms), rgb).await;
    }

    let manual = start(&mut socket, 3, "Manual").await;
    for ms in [550, 800] {
        shows(&wakes, &heard, at(ms), GREEN).await;
    }
    accepted(&mut socket, json!({"ReleaseHold": {"play": manual}})).await;
    shows(&wakes, &heard, at(825), RED).await; // child looped under Manual
    accepted(&mut socket, json!({"ReleaseHold": {"play": parent}})).await;

    // Parent 6: one green beat, three shared red. Parent 7 starts a beat later and
    // covers it with one blue beat, one shared red, two blue. Their starts differ.
    accepted(
        &mut socket,
        sequence(6, json!([{"look": 3}, {"look": 1, "width": 48}])),
    )
    .await;
    accepted(
        &mut socket,
        sequence(
            7,
            json!([{"look": 2}, {"look": 1}, {"look": 2, "width": 32}]),
        ),
    )
    .await;
    let lower = start(&mut socket, 6, "Autopilot").await;
    shows(&wakes, &heard, at(2000), GREEN).await;
    let upper = start(&mut socket, 7, "Show").await;
    shows(&wakes, &heard, at(2500), BLUE).await;
    shows(&wakes, &heard, at(3100), RED).await;
    accepted(&mut socket, step(1, YELLOW)).await;
    shows(&wakes, &heard, at(3150), YELLOW).await; // parent 7, still in item two
    shows(&wakes, &heard, at(3520), BLUE).await; // parent 7 kept its original start
    accepted(&mut socket, json!({"ReleaseHold": {"play": upper}})).await;
    shows(&wakes, &heard, at(3570), YELLOW).await; // parent 6 read the edit too
    shows(&wakes, &heard, at(4020), GREEN).await; // original four-beat boundary

    accepted(&mut socket, json!({"ReleaseHold": {"play": lower}})).await;
    let double_once = json!({"StartLook": {"look": 6, "rank": "Show",
        "use": {"speed": "Double", "repeat": "Once"}}});
    assert!(command(&mut socket, double_once).await["Started"]["play"].is_object());
    // Four beats at double speed finish one second after the applying frame.
    for (ms, rgb) in [(4100, GREEN), (4400, YELLOW), (5200, [0, 0, 0])] {
        shows(&wakes, &heard, at(ms), rgb).await;
    }

    send(&mut socket, &json!("Stop")).await;
    wakes.run();
    stop.store(true, Ordering::Relaxed);
}

/// Wait until the reader has published the last stepped frame, including any wake
/// needed to hand back an outstanding snapshot. The clock then stays on that frame.
async fn picture_at(socket: &mut Socket, wakes: &Wakes, at: Duration) -> Value {
    let waited = Instant::now();
    loop {
        send(socket, &json!("Query")).await;
        let reply = answer(socket).await;
        let picture = &reply["Picture"];
        let frame = wakes.woke();
        if picture["state"]["frame"].as_u64() == Some(frame) {
            return picture.clone();
        }
        assert!(
            waited.elapsed() < Duration::from_secs(10),
            "no picture of frame {frame}"
        );
        block_in_place(|| wakes.wake_at(at)).unwrap();
        tokio::time::sleep(engine::BEAT).await; // let the snapshot reader poll
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn refused_graph_edits_keep_the_authored_looks_the_play_and_its_dmx() {
    let _port = PORT.lock().await;
    let (mut socket, wakes, heard, stop, zero) = pixel_desk(1).await;
    let repeated = |look, count| Value::Array(vec![json!({"look": look}); count]);
    accepted(&mut socket, step(1, RED)).await;
    for (id, child, count) in [(2, 1, 1), (3, 2, 64), (4, 3, 64)] {
        accepted(&mut socket, sequence(id, repeated(child, count))).await;
    }
    start(&mut socket, 2, "Show").await;
    shows(&wakes, &heard, zero, RED).await;
    let before = picture_at(&mut socket, &wakes, zero).await;
    assert_eq!(before["looks"].as_array().unwrap().len(), 4);
    assert_eq!(before["state"]["plays"].as_array().unwrap().len(), 1);

    // 64 references fit in look 2 alone. Look 4 would expand them 64 × 64 times.
    let cases = [
        ("LookCycle", sequence(2, repeated(4, 1))),
        ("WrongLookKind", sequence(1, repeated(2, 1))),
        ("WrongLookKind", step(2, BLUE)),
        ("WorkLimit", sequence(2, repeated(1, 64))),
        ("WorkLimit", sequence(5, repeated(4, 64))),
        (
            "InvalidTiming",
            json!({"StartLook":{"look":1,"use":{"repeat":"Once"}}}),
        ),
        (
            "InvalidTiming",
            json!({"StartLook":{"look":2,"use":{"spread":101}}}),
        ),
    ];
    for (i, (reason, edit)) in cases.into_iter().enumerate() {
        let reply = command(&mut socket, edit).await;
        assert_eq!(reply, json!({"Refused": {"reason": reason}}), "case {i}");
        let at = zero + Duration::from_secs(i as u64 + 1);
        shows(&wakes, &heard, at, RED).await;
        let after = picture_at(&mut socket, &wakes, at).await;
        assert_eq!(after["looks"], before["looks"], "case {i}");
        assert_eq!(
            after["state"]["plays"], before["state"]["plays"],
            "case {i}"
        );
    }

    send(&mut socket, &json!("Stop")).await;
    wakes.run();
    stop.store(true, Ordering::Relaxed);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rule_change_reclaims_a_once_use_kept_earlier_in_the_same_wake() {
    let _port = PORT.lock().await;
    let (mut socket, wakes, heard, stop, zero) = pixel_desk(1).await;
    accepted(&mut socket, step(1, RED)).await;
    accepted(&mut socket, step(3, GREEN)).await;
    let rules = |manual, other| {
        json!({"SetMixRules": {"rules":{
            "brightness":"Latest", "manual_end":manual, "other_end":other
        }}})
    };
    for (index, rank, manual, other) in [
        (0, "Manual", "UntilStop", "Stay"),
        (1, "Show", "UntilRelease", "Reveal"),
    ] {
        let from = zero + Duration::from_secs(2 * index);
        accepted(&mut socket, rules("UntilRelease", "Stay")).await;
        accepted(&mut socket, sequence(2, json!([{"look":1}]))).await;
        let start_once = json!({"StartLook": {"look":2, "rank":rank, "use":{"repeat":"Once"}}});
        let old = command(&mut socket, start_once).await["Started"]["play"].clone();
        assert!(old.is_object());
        shows(&wakes, &heard, from, RED).await;

        // The last wake saw Playing. At the next event the sequence has finished,
        // and the old rules keep it. The edit in this wake releases that sample.
        let end = from + Duration::from_secs(1);
        wakes.set_time(end);
        accepted(&mut socket, rules(manual, other)).await;
        shows(&wakes, &heard, end, [0, 0, 0]).await;
        accepted(&mut socket, json!({"RemoveLook": {"look":2}})).await;

        let next = start(&mut socket, 3, "Show").await;
        assert_eq!(next["slot"], old["slot"], "{rank}");
        assert_ne!(next["generation"], old["generation"], "{rank}");
        shows(&wakes, &heard, end + Duration::from_millis(100), GREEN).await;
        accepted(&mut socket, json!({"ReleaseHold":{"play":next}})).await;
    }
    send(&mut socket, &json!("Stop")).await;
    wakes.run();
    stop.store(true, Ordering::Relaxed);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_running_sequence_follows_the_reordered_group_during_spread() {
    let _port = PORT.lock().await;
    let (mut socket, wakes, heard, stop, zero) = pixel_desk(2).await;
    let group = |first, second| {
        json!({"SetGroup":{"name":"row","members":[
            {"Light":{"light":first}}, {"Light":{"light":second}}
        ]}})
    };
    accepted(&mut socket, group(2, 1)).await;
    for (id, rgb) in [(1, RED), (2, BLUE)] {
        let mut colour = step(id, rgb);
        colour["PutStep"]["step"]["target"] = json!({"Group":{"group":"row"}});
        accepted(&mut socket, colour).await;
    }
    accepted(
        &mut socket,
        sequence(3, json!([{"look":1},{"look":2,"use":{"spread":100}}])),
    )
    .await;
    start(&mut socket, 3, "Show").await;
    // One beat is 500 ms. The second light in saved order enters blue at 750 ms.
    shows(&wakes, &heard, zero, [255, 0, 0, 255, 0, 0]).await;
    shows(
        &wakes,
        &heard,
        zero + Duration::from_millis(600),
        [255, 0, 0, 0, 0, 255],
    )
    .await;
    accepted(&mut socket, group(1, 2)).await;
    shows(
        &wakes,
        &heard,
        zero + Duration::from_millis(650),
        [0, 0, 255, 255, 0, 0],
    )
    .await;
    shows(
        &wakes,
        &heard,
        zero + Duration::from_millis(800),
        [0, 0, 255, 0, 0, 255],
    )
    .await;
    send(&mut socket, &json!("Stop")).await;
    wakes.run();
    stop.store(true, Ordering::Relaxed);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_start_edit_and_restart_at_one_stamp_reach_one_frame() {
    let _port = PORT.lock().await;
    let (mut socket, wakes, heard, stop, zero) = pixel_desk(1).await;
    accepted(&mut socket, step(1, RED)).await;
    accepted(&mut socket, sequence(2, json!([{"look":1}]))).await;

    wakes.set_time(zero);
    let old = start(&mut socket, 2, "Show").await;
    accepted(&mut socket, step(1, BLUE)).await;
    let next = start(&mut socket, 2, "Show").await;
    assert_eq!(old["slot"], next["slot"]);
    assert_ne!(old["generation"], next["generation"]);
    accepted(&mut socket, json!({"ReleaseHold": {"play":old}})).await;

    shows(&wakes, &heard, zero, BLUE).await;
    let picture = picture_at(&mut socket, &wakes, zero).await;
    let plays = picture["state"]["plays"].as_array().unwrap();
    assert_eq!(plays.len(), 1);
    assert_eq!(plays[0]["play"], next);
    accepted(&mut socket, json!({"ReleaseHold": {"play":next}})).await;
    shows(&wakes, &heard, zero + Duration::from_secs(1), [0, 0, 0]).await;

    send(&mut socket, &json!("Stop")).await;
    wakes.run();
    stop.store(true, Ordering::Relaxed);
}

#[tokio::test(flavor = "multi_thread")]
async fn combined_spread_wraps_at_an_exact_child_edge_on_dmx() {
    let _port = PORT.lock().await;
    let (mut socket, wakes, heard, stop, zero) = pixel_desk(3).await;
    for (id, rgb) in [(1, RED), (2, BLUE)] {
        accepted(&mut socket, step(id, rgb)).await;
    }
    accepted(
        &mut socket,
        sequence(3, json!([{"look":1,"width":8},{"look":2,"width":8}])),
    )
    .await;
    accepted(
        &mut socket,
        sequence(4, json!([{"look":3,"width":32,"use":{"spread":100}}])),
    )
    .await;
    let command_to_start = json!({"StartLook":{"look":4,"use":{"spread":100}}});
    assert!(command(&mut socket, command_to_start).await["Started"]["play"].is_object());

    // On the root's first frame the child offsets are 0, 2/3 + 1/3, and 4/3 + 2/3 beats.
    // The child is one beat long, so every light starts on its first item.
    shows(&wakes, &heard, zero, [255, 0, 0, 255, 0, 0, 255, 0, 0]).await;
    send(&mut socket, &json!("Stop")).await;
    wakes.run();
    stop.store(true, Ordering::Relaxed);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sixteenth_beat_cuts_at_the_exact_edge_across_a_tempo_change() {
    let _port = PORT.lock().await;
    let (mut socket, wakes, heard, stop, zero) = pixel_desk(1).await;
    for (id, rgb) in [(1, RED), (2, BLUE)] {
        accepted(&mut socket, step(id, rgb)).await;
    }
    accepted(
        &mut socket,
        sequence(3, json!([{"look":1,"width":1},{"look":2}])),
    )
    .await;
    start(&mut socket, 3, "Show").await;
    shows(&wakes, &heard, zero, RED).await;

    wakes.set_time(zero + Duration::from_millis(10));
    accepted(&mut socket, json!({"SetInternalTempo":{"bpm":60}})).await;
    // 10 ms at 120 BPM and 42.5 ms at 60 BPM add up to exactly 1/16 beat.
    shows(&wakes, &heard, zero + Duration::from_micros(52_500), BLUE).await;
    send(&mut socket, &json!("Stop")).await;
    wakes.run();
    stop.store(true, Ordering::Relaxed);
}

#[tokio::test(flavor = "multi_thread")]
async fn idle_sequence_edits_are_bounded_while_the_reader_has_one_frame() {
    let _port = PORT.lock().await;
    const PIXELS: u16 = 170;
    const PARENTS: usize = 48;
    const BINDINGS_PER_LIGHT: usize = 2; // the parent and its one linked step
    const PLAN_LIMIT: usize = 64 * 1024 * 1024;
    let bytes_per_edit = PARENTS
        * usize::from(PIXELS)
        * BINDINGS_PER_LIGHT
        * std::mem::size_of::<engine_core::Binding>();
    let (mut socket, wakes, heard, stop, zero) = pixel_desk(PIXELS).await;
    accepted(&mut socket, step(1, RED)).await;
    for look in 2..=u16::try_from(PARENTS + 1).unwrap() {
        accepted(&mut socket, sequence(look, json!([{"look":1}]))).await;
    }
    shows(&wakes, &heard, zero, [0, 0, 0]).await;
    let before = picture_at(&mut socket, &wakes, zero).await;
    let mut edits = 0;
    let mut refused = false;
    for index in 0..=PLAN_LIMIT / bytes_per_edit {
        let rgb = if index % 2 == 0 { BLUE } else { RED };
        let reply = command(&mut socket, step(1, rgb)).await;
        if reply == json!({"Refused":{"reason":"WorkLimit"}})
            || reply == json!({"Refused":{"reason":"QueueFull"}})
        {
            refused = true;
            break;
        }
        assert_eq!(reply, json!("Accepted"));
        edits += 1;
    }
    send(&mut socket, &json!("Query")).await;
    let after = answer(&mut socket).await;
    assert_eq!(after["Picture"]["state"]["frame"], before["state"]["frame"]);
    if refused {
        // The edit backlog cannot spend a performance command's FIFO room.
        accepted(&mut socket, json!("ReleaseAll")).await;
        let now = zero + Duration::from_secs(1);
        shows(&wakes, &heard, now, [0, 0, 0]).await;
        let published = picture_at(&mut socket, &wakes, now).await;
        let last = if edits % 2 == 1 { BLUE } else { RED };
        assert_eq!(
            published["looks"][0]["step"]["values"]["colour"],
            step(1, last)["PutStep"]["step"]["values"]["colour"]
        );
        accepted(&mut socket, step(1, GREEN)).await;
        start(&mut socket, 2, "Manual").await;
        shows(&wakes, &heard, now + Duration::from_secs(1), GREEN).await;
    }
    send(&mut socket, &json!("Stop")).await;
    wakes.run();
    stop.store(true, Ordering::Relaxed);

    // Every accepted revision changes every parent plan. The reader cannot reclaim any
    // of these revisions until it receives a newer frame, and these bytes count only bindings.
    let retained = edits * bytes_per_edit;
    assert!(
        retained <= PLAN_LIMIT,
        "{edits} edits retained at least {retained} plan bytes while the frame was held"
    );
    // Every per-light binding slice also owns a pointer and a length.
    let slice_bytes = PARENTS * usize::from(PIXELS) * size_of::<Box<[engine_core::Binding]>>();
    assert!(retained + edits * slice_bytes <= PLAN_LIMIT);
}

#[tokio::test(flavor = "multi_thread")]
async fn tiny_edits_leave_room_to_release_at_the_history_limit() {
    let _port = PORT.lock().await;
    let (mut socket, wakes, heard, stop, zero) = pixel_desk(1).await;
    accepted(&mut socket, step(1, RED)).await;
    let playing = start(&mut socket, 1, "Manual").await;
    shows(&wakes, &heard, zero, RED).await;
    picture_at(&mut socket, &wakes, zero).await;

    // Renaming shares the compiled plan and queues no work on the tick.
    let mut refused = false;
    for n in 0..=engine::QUEUE {
        let mut rename = step(1, RED);
        rename["PutStep"]["name"] = json!(n.to_string());
        let reply = command(&mut socket, rename).await;
        if reply == json!({"Refused":{"reason":"WorkLimit"}})
            || reply == json!({"Refused":{"reason":"QueueFull"}})
        {
            refused = true;
            break;
        }
        assert_eq!(reply, json!("Accepted"));
    }
    accepted(&mut socket, json!({"ReleaseHold":{"play":playing}})).await;
    let now = zero + Duration::from_secs(1);
    shows(&wakes, &heard, now, [0, 0, 0]).await;
    picture_at(&mut socket, &wakes, now).await;
    accepted(&mut socket, step(1, BLUE)).await;
    send(&mut socket, &json!("Stop")).await;
    wakes.run();
    stop.store(true, Ordering::Relaxed);
    assert!(refused, "tiny edits also need a finite revision count");
}
