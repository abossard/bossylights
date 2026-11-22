//! Wave commands through /api, with a stepped beat clock and complete DMX USB Pro frames.

mod common;

use common::{Heard, Socket, command, connect, open, send, serve};
use engine::{Engine, Universes, Wakes};
use serde_json::{Value, json};
use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::task::block_in_place;

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const GREEN: [u8; 3] = [0, 255, 0];
const YELLOW: [u8; 3] = [255, 255, 0];

fn swatches(colours: &[[u8; 3]]) -> Vec<Value> {
    colours
        .iter()
        .map(|rgb| {
            let [red, green, blue] = rgb.map(|byte| u16::from(byte) * 257);
            json!({"red":red,"green":green,"blue":blue})
        })
        .collect()
}

fn wave(look: u16, target: Value, palette: Value) -> Value {
    json!({"PutPattern":{"look":look,"name":"wave","pattern":{"Wave":{
       "target":target,"period":2,"palette":palette
    }}}})
}

fn palette(colours: &[[u8; 3]]) -> Value {
    json!({"PutStep":{"look":1,"name":"palette","step":{
        "target":{"Fixture":{"fixture":1}},"values":{},"swatches":swatches(colours)
    }}})
}

async fn accepted(socket: &mut Socket, request: Value) {
    assert_eq!(
        command(socket, request.clone()).await,
        json!("Accepted"),
        "{request}"
    );
}

async fn start(socket: &mut Socket, look: u16, spread: u8, speed: &str) -> Value {
    let reply = command(
        socket,
        json!({"StartLook":{
            "look":look,"use":{"spread":spread,"speed":speed}
        }}),
    )
    .await;
    let play = &reply["Started"]["play"];
    assert!(play.is_object(), "{reply}");
    play.clone()
}

struct Desk {
    socket: Socket,
    wakes: Wakes,
    heard: Heard,
    stop: Arc<AtomicBool>,
    zero: Duration,
}

impl Desk {
    async fn new(fixtures: Option<PathBuf>) -> Self {
        let heard = Heard::default();
        let stop = Arc::new(AtomicBool::new(false));
        let opens = open(Some(heard.clone()), stop.clone());
        let (engine, sender, wakes) = Engine::start_stepped(Universes::ONE, opens).unwrap();
        let mut socket = connect(serve(engine, sender, fixtures).await).await;
        block_in_place(|| wakes.hold()).unwrap();
        let zero = block_in_place(|| wakes.wake()).unwrap() + Duration::from_secs(1);
        accepted(
            &mut socket,
            json!({"SetOutput":{"output":1,"kind":{"DmxUsbPro":{"adapter":1}},
            "universes":{"first":1,"count":1},"rate":"60"}}),
        )
        .await;
        accepted(&mut socket, json!({"SetInternalTempo":{"bpm":120}})).await;
        Self {
            socket,
            wakes,
            heard,
            stop,
            zero,
        }
    }

    // Each sample is due. Read the very next complete message after the stepped wake.
    async fn frame(&self, ms: u64) -> [u8; 512] {
        let at = self.zero + Duration::from_millis(ms);
        let next = self.heard.lock().unwrap().len();
        assert_eq!(block_in_place(|| self.wakes.wake_at(at)).unwrap(), at);
        let started = Instant::now();
        loop {
            if let Some(frame) = self.heard.lock().unwrap().get(next) {
                return *frame;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "no DMX at {ms} ms"
            );
            tokio::time::sleep(Duration::from_millis(1)).await; // poll
        }
    }

    async fn intensity(&mut self, target: Value, byte: u8) {
        let reply = command(
            &mut self.socket,
            json!({"SetIntensity":{
                "target":target,"intensity":u16::from(byte)*257
            }}),
        )
        .await;
        assert!(common::taken(&reply), "{reply}");
    }

    async fn finish(mut self) {
        send(&mut self.socket, &json!("Stop")).await;
        self.wakes.run();
        self.stop.store(true, Ordering::Relaxed);
    }
}

// The triples in DMX address order belong to cells 2, 1, 3, 0. Each head
// also names channel zero, the one master owned by the fixture.
fn matrix_definition() -> PathBuf {
    let mut xml = String::from(
        r#"<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
        <Manufacturer>Test</Manufacturer><Model>Panel</Model><Type>LED Bar (Pixels)</Type>
        <Physical><Layout Width="2" Height="2"/></Physical>
        <Channel Name="Master" Preset="IntensityDimmer"/>"#,
    );
    let mut mode = String::from(r#"<Mode Name="Grid"><Channel Number="0">Master</Channel>"#);
    for triple in 0..4 {
        for (component, name) in ["Red", "Green", "Blue"].iter().enumerate() {
            let channel = 1 + triple * 3 + component;
            write!(
                xml,
                r#"<Channel Name="{name} {triple}" Preset="Intensity{name}"/>"#
            )
            .unwrap();
            write!(
                mode,
                r#"<Channel Number="{channel}">{name} {triple}</Channel>"#
            )
            .unwrap();
        }
    }
    xml.push_str(&mode);
    for first in [10, 4, 1, 7] {
        write!(xml,"<Head><Channel>0</Channel><Channel>{first}</Channel><Channel>{}</Channel><Channel>{}</Channel></Head>", first+1, first+2).unwrap();
    }
    xml.push_str("</Mode></FixtureDefinition>");
    let folder = std::env::temp_dir().join(format!("bossylights-m3-wave-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Panel.qxf"), xml).unwrap();
    folder
}

#[tokio::test(flavor = "multi_thread")]
async fn matrix_columns_follow_head_order_at_beat_tempo_and_wrap_boundaries() {
    let folder = matrix_definition();
    let mut desk = Desk::new(Some(folder.clone())).await;
    let patch = json!({"Patch":{"name":"panel","maker":"Test","model":"Panel","mode":"Grid",
        "at":{"universe":1,"address":1}}});
    let waited = Instant::now();
    loop {
        let reply = command(&mut desk.socket, patch.clone()).await;
        if reply != json!({"Refused":{"reason":"NoSuchFixture"}}) {
            assert_eq!(reply, json!({"Patched":{"fixture":1}}));
            break;
        }
        assert!(
            waited.elapsed() < Duration::from_secs(10),
            "fixture not loaded"
        );
        tokio::task::yield_now().await;
    }
    std::fs::remove_file(folder.join("Panel.qxf")).unwrap();
    std::fs::remove_dir(folder).unwrap();

    desk.intensity(json!({"Fixture":{"fixture":1}}), 255).await;
    desk.intensity(json!({"Light":{"light":1}}), 173).await;
    accepted(
        &mut desk.socket,
        wave(
            1,
            json!({"Matrix":{"fixture":1}}),
            json!({"Inline":swatches(&[RED,BLUE])}),
        ),
    )
    .await;
    desk.wakes.set_time(desk.zero);
    start(&mut desk.socket, 1, 100, "Normal").await;
    for (ms, left, right) in [(0, RED, BLUE), (500, BLUE, RED), (1500, RED, BLUE)] {
        if ms == 1500 {
            accepted(&mut desk.socket, json!({"SetInternalTempo":{"bpm":60}})).await;
        }
        let frame = desk.frame(ms).await;
        assert_eq!(frame[0], 173, "master at {ms} ms");
        assert_eq!(
            frame[1..13],
            [left, right, right, left].concat(),
            "cells 2, 1, 3, 0 at {ms} ms"
        );
    }
    // On a once pass the right column reaches the end half a pass earlier
    // and holds the final colour. The Manual play keeps both cells at its end.
    desk.wakes.set_time(desk.zero + Duration::from_millis(2000));
    let once =
        json!({"StartLook":{"look":1,"use":{"spread":100,"speed":"Double","repeat":"Once"}}});
    assert!(command(&mut desk.socket, once).await["Started"]["play"].is_object());
    for (ms, left, right) in [
        (2000, RED, BLUE),
        (2750, [128, 0, 128], RED),
        (3000, RED, RED),
    ] {
        let frame = desk.frame(ms).await;
        assert_eq!(frame[0], 173, "master at {ms} ms");
        assert_eq!(
            frame[1..13],
            [left, right, right, left].concat(),
            "once at {ms} ms"
        );
    }

    // A dimmer among RGB heads invalidates the entire group. Test both the
    // initial Wave definition and a later group edit that would break it.
    accepted(&mut desk.socket, group("mixed", 2, 1)).await;
    let group_wave = wave(
        2,
        json!({"Group":{"group":"mixed"}}),
        json!({"Inline":swatches(&[RED])}),
    );
    assert_eq!(
        command(&mut desk.socket, group_wave.clone()).await,
        json!({"Refused":{"reason":{"Unsupported":{"light":1,"property":"Colour"}}}})
    );
    accepted(&mut desk.socket, group("mixed", 2, 3)).await;
    accepted(&mut desk.socket, group_wave).await;
    assert_eq!(
        command(&mut desk.socket, group("mixed", 2, 1)).await,
        json!({"Refused":{"reason":{"LookInUse":{"look":2}}}})
    );
    desk.finish().await;
}

fn group(name: &str, first: u16, second: u16) -> Value {
    json!({"SetGroup":{"name":name,"members":[{"Light":{"light":first}},{"Light":{"light":second}}]}})
}

#[tokio::test(flavor = "multi_thread")]
async fn one_beat_wave_rounds_exact_swatch_midpoints_on_dmx() {
    let mut desk = Desk::new(None).await;
    let patch = json!({"PatchStrip":{"name":"pixel","pixels":1,"color":"Rgb","at":{"universe":1,"address":1}}});
    assert_eq!(
        command(&mut desk.socket, patch).await,
        json!({"Patched":{"fixture":1}})
    );
    desk.intensity(json!({"Fixture":{"fixture":1}}), 255).await;
    let mut pattern = wave(
        1,
        json!({"Matrix":{"fixture":1}}),
        json!({"Inline":swatches(&[RED, BLUE, GREEN])}),
    );
    pattern["PutPattern"]["pattern"]["Wave"]["period"] = json!(1);
    accepted(&mut desk.socket, pattern).await;
    desk.wakes.set_time(desk.zero);
    accepted(&mut desk.socket, json!({"SetInternalTempo":{"bpm":125}})).await;
    start(&mut desk.socket, 1, 0, "Normal").await;

    let mut actual = Vec::new();
    let mut expected = Vec::new();
    for (ms, rgb) in [
        (0, RED),
        (80, [128, 0, 128]),
        (160, BLUE),
        (240, [0, 128, 128]),
        (320, GREEN),
        (400, [128, 128, 0]),
        (480, RED),
        (520, [128, 0, 128]),
    ] {
        if ms == 480 {
            desk.wakes.set_time(desk.zero + Duration::from_millis(ms));
            accepted(&mut desk.socket, json!({"SetInternalTempo":{"bpm":250}})).await;
        }
        let frame = desk.frame(ms).await;
        actual.push((ms, [frame[0], frame[1], frame[2]]));
        expected.push((ms, rgb));
    }
    desk.finish().await;
    assert_eq!(actual, expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_once_sequence_keeps_the_left_limit_of_its_fade_into_a_wave() {
    let mut desk = Desk::new(None).await;
    let patch = json!({"PatchStrip":{"name":"pixel","pixels":1,"color":"Rgb","at":{"universe":1,"address":1}}});
    assert_eq!(
        command(&mut desk.socket, patch).await,
        json!({"Patched":{"fixture":1}})
    );
    desk.intensity(json!({"Fixture":{"fixture":1}}), 255).await;
    for edit in [
        json!({"PutStep":{"look":1,"name":"blue","step":{
            "target":{"Fixture":{"fixture":1}},
            "values":{"colour":{"red":0,"green":0,"blue":65535}}
        }}}),
        wave(
            2,
            json!({"Matrix":{"fixture":1}}),
            json!({"Inline":swatches(&[RED, YELLOW])}),
        ),
        json!({"PutSequence":{"look":3,"name":"fade into Wave","sequence":{"items":[
            {"look":1,"width":8},{"look":2,"width":16,"transition":100}
        ]}}}),
        json!({"PutSequence":{"look":4,"name":"one beat","sequence":{"items":[{"look":3}]}}}),
    ] {
        accepted(&mut desk.socket, edit).await;
    }
    desk.wakes.set_time(desk.zero);
    let once = json!({"StartLook":{"look":4,"use":{"repeat":"Once"}}});
    assert!(command(&mut desk.socket, once).await["Started"]["play"].is_object());

    // At beat one, the 50% fade into the Wave reaches a rising half tie on red.
    // The sample immediately before that beat must round down and remain held.
    let mut actual = Vec::new();
    for ms in [0, 375, 500, 750] {
        let frame = desk.frame(ms).await;
        actual.push((ms, [frame[0], frame[1], frame[2]]));
    }
    desk.finish().await;
    assert_eq!(
        actual,
        [
            (0, BLUE),
            (375, [64, 16, 191]),
            (500, [127, 63, 128]),
            (750, [127, 63, 128]),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn group_reorder_and_palette_edits_reach_a_wave_and_a_playing_sequence() {
    let mut desk = Desk::new(None).await;
    let patch = json!({"PatchStrip":{"name":"pixels","pixels":4,"color":"Rgb","at":{"universe":1,"address":1}}});
    assert_eq!(
        command(&mut desk.socket, patch).await,
        json!({"Patched":{"fixture":1}})
    );
    desk.intensity(json!({"Fixture":{"fixture":1}}), 255).await;
    for edit in [
        group("left", 1, 2),
        group("right", 3, 4),
        palette(&[RED, BLUE]),
    ] {
        accepted(&mut desk.socket, edit).await;
    }
    for (look, name) in [(2, "left"), (3, "right")] {
        accepted(
            &mut desk.socket,
            wave(
                look,
                json!({"Group":{"group":name}}),
                json!({"Step":{"look":1}}),
            ),
        )
        .await;
    }
    let sequence = |speed| {
        json!({"PutSequence":{"look":4,"name":"row","sequence":{
        "items":[{"look":3,"width":128,"use":{"spread":100,"speed":speed}}]}}})
    };
    accepted(&mut desk.socket, sequence("Normal")).await;
    desk.wakes.set_time(desk.zero);
    let direct = start(&mut desk.socket, 2, 100, "Normal").await;
    start(&mut desk.socket, 4, 0, "Normal").await;
    let first = desk.frame(0).await;
    assert_eq!(first[..12], [RED, BLUE, RED, BLUE].concat());

    let warm = [191, 0, 64];
    let cool = [64, 0, 191];
    assert_eq!(
        desk.frame(125).await[..12],
        [warm, cool, warm, cool].concat()
    );

    // The reorder is read one period later at the same nonzero phase.
    accepted(&mut desk.socket, group("left", 2, 1)).await;
    let reordered = desk.frame(1125).await;
    assert_eq!(reordered[..12], [cool, warm, warm, cool].concat());

    accepted(&mut desk.socket, palette(&[GREEN, YELLOW])).await;
    let edited = desk.frame(1500).await;
    assert_eq!(edited[..12], [GREEN, YELLOW, YELLOW, GREEN].concat());

    // Changing only the nested speed keeps its parent's elapsed time. At 1.75
    // periods the double-speed child has reached exactly its second swatch.
    desk.wakes.set_time(desk.zero + Duration::from_millis(1750));
    accepted(&mut desk.socket, sequence("Double")).await;
    let sped = desk.frame(1750).await;
    let lime = [128, 255, 0];
    assert_eq!(sped[..12], [lime, lime, YELLOW, GREEN].concat());

    accepted(&mut desk.socket, palette(&[YELLOW])).await;
    assert_eq!(desk.frame(2000).await[..12], [YELLOW; 4].concat());
    let target = json!({"Group":{"group":"left"}});
    let five = [RED, BLUE, GREEN, RED, BLUE];
    let refusals = [
        (palette(&[]), "EmptyPalette"),
        (palette(&five), "NotACommand"),
        (
            wave(2, target.clone(), json!({"Inline":swatches(&five)})),
            "NotACommand",
        ),
        (
            wave(2, target.clone(), json!({"Step":{"look":"one"}})),
            "NotACommand",
        ),
        (
            wave(2, target, json!({"Inline":[{"red":-1,"green":2,"blue":3}]})),
            "NotACommand",
        ),
    ];
    for (index, (edit, reason)) in refusals.into_iter().enumerate() {
        assert_eq!(
            command(&mut desk.socket, edit).await,
            json!({"Refused":{"reason":reason}})
        );
        let ms = 2020 + 20 * index as u64;
        assert_eq!(
            desk.frame(ms).await[..12],
            [YELLOW; 4].concat(),
            "refusal {index}"
        );
    }
    // The stopped Wave keeps its original lights and palette generation, even
    // after both authored Waves move to inline colours on the other group.
    accepted(&mut desk.socket, json!({"StopLook":{"play":direct}})).await;
    for (look, colour) in [(2, RED), (3, BLUE)] {
        accepted(
            &mut desk.socket,
            wave(
                look,
                json!({"Group":{"group":"right"}}),
                json!({"Inline":swatches(&[colour])}),
            ),
        )
        .await;
    }
    assert_eq!(
        command(&mut desk.socket, palette(&[])).await,
        json!({"Refused":{"reason":"EmptyPalette"}}),
        "the kept Wave still references its original palette"
    );
    assert_eq!(
        desk.frame(2150).await[..12],
        [YELLOW, YELLOW, BLUE, BLUE].concat(),
        "the first frame after the refused edit"
    );
    let remove = json!({"RemoveLook":{"look":1}});
    for edit in [remove.clone(), group("left", 2, 3)] {
        assert_eq!(
            command(&mut desk.socket, edit).await,
            json!({"Refused":{"reason":{"LookInUse":{"look":2}}}})
        );
    }
    assert_eq!(
        desk.frame(2200).await[..12],
        [YELLOW, YELLOW, BLUE, BLUE].concat()
    );
    accepted(&mut desk.socket, json!({"ReleaseHold":{"play":direct}})).await;
    accepted(&mut desk.socket, palette(&[])).await;
    accepted(&mut desk.socket, remove).await;
    accepted(&mut desk.socket, group("left", 2, 3)).await;
    assert_eq!(
        desk.frame(2220).await[..12],
        [[0; 3], [0; 3], BLUE, BLUE].concat()
    );
    desk.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn one_beat_wave_rounds_both_channels_up_at_an_exact_midpoint() {
    let mut desk = Desk::new(None).await;
    let patch = json!({"PatchStrip":{"name":"pixel","pixels":1,"color":"Rgb",
        "at":{"universe":1,"address":1}}});
    assert_eq!(
        command(&mut desk.socket, patch).await,
        json!({"Patched":{"fixture":1}})
    );
    desk.intensity(json!({"Fixture":{"fixture":1}}), 255).await;
    accepted(
        &mut desk.socket,
        json!({"SetGroup":{"name":"pixel","members":[{"Light":{"light":1}}]}}),
    )
    .await;
    accepted(
        &mut desk.socket,
        json!({"PutPattern":{"look":1,"name":"midpoints","pattern":{"Wave":{
            "target":{"Group":{"group":"pixel"}},"period":1,"palette":{"Inline":[
                {"red":255,"green":256,"blue":0},
                {"red":256,"green":255,"blue":0},
                {"red":0,"green":0,"blue":0}
            ]}
        }}}}),
    )
    .await;

    desk.wakes.set_time(desk.zero);
    accepted(&mut desk.socket, json!({"SetInternalTempo":{"bpm":100}})).await;
    start(&mut desk.socket, 1, 0, "Normal").await;
    let mut actual = [[0; 3]; 5];
    actual[0].copy_from_slice(&desk.frame(0).await[..3]);
    desk.wakes.set_time(desk.zero + Duration::from_millis(20));
    accepted(&mut desk.socket, json!({"SetInternalTempo":{"bpm":80}})).await;

    // 20 ms at 100 BPM and 100 ms at 80 BPM add up to 1/6 beat.
    // With three swatches, both channels then equal 255.5 before rounding.
    for (rgb, ms) in actual[1..].iter_mut().zip([100, 120, 140, 745]) {
        rgb.copy_from_slice(&desk.frame(ms).await[..3]);
    }
    desk.finish().await;
    assert_eq!(
        actual,
        [[0, 1, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0], [0, 1, 0]]
    );
}
