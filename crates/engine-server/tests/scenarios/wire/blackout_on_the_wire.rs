//! End to end over the API (#251): Blackout moves only the channels its rules name.
//!
//! Universe 1 on a fake DMX USB Pro. Every channel below is held at 200 first, then
//! Blackout goes on and off. The rules come from each fixture's file
//! (`docs/design/channel-model.md`, Rules):
//!
//! | At | Fixture (file, mode) | Goes dark | Stays at 200 |
//! |---|---|---|---|
//! | 1 to 6 | the Stairville AF-180 LED Fogger's channels less its colour macro (below: the macro is refused by the light model until #253) | 2 LED dimmer to 0 | 1 fog pump, 3 strobe, 4 to 6 R G B (the LED dimmer carries them) |
//! | 11 to 17 | two heads, a dimmer on the first (the definition below) | 11 dimmer, 15 to 17 head 2 R G B to 0 | 12 to 14 head 1 R G B |
//! | 21 to 32 | Vari-Lite VL2202 Spot, Standard 8bit | 21 intensity to 0, 28 strobe to 3 (its first Closed byte) | pan, tilt, wheels, iris, zoom, control |
//! | 41 to 43 | Generic RGB, RGB | R G B to 0 | |
//! | 100 | nothing patched | | a raw hold follows nothing |
//!
//! Master scales what follows it (owner, 2026-11-21, #251): `out = dark + (v - dark) x m /
//! 255`, one rounding. Master 50% is level 128. Worked by hand:
//!
//! | At | Fixture (file, mode) | Held | Under Master 128 |
//! |---|---|---|---|
//! | 1 to 19 | Cameo HydraBeam 4000, 19 Channel: 1 shared master dimmer, 3 to 6 RGBW in no head, 9, 12, 15, 18 head dimmers, 7 pan 1 | 255, 200, 179, 100 | 255 (keeps its value), 200 (the head dimmers carry it), 90 (179 x 128 / 255 = 89.85), 100 |
//! | 21 to 34 | Martin Rush MH6 Wash, Standard: 22 and 23 a 16-bit dimmer | 0xB3 0x33 | 0x59 0xF3: 45,875 x 128 / 255 = 23,027.45 (byte by byte 90 26) |
//! | 41 to 43 | Generic RGB, RGB: orange | 255 128 0 | 128 64 0 (64.25) |
//! | 51 | a reverse dimmer (below): dark at 255 | 76 | 165: 255 - round(179 x 128 / 255) |
//! | 61 | nothing patched | 200 | 200 |

use crate::wire_common as common;

use common::{Heard, Socket, command, connect, serve};
use engine::{Engine, Universes};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::{fs, process};

/// The Stairville AF-180 LED Fogger Co2 FX, 7 Channel, as its file has it, less channel 3
/// (Colour effect).
const FOGGER: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<!DOCTYPE FixtureDefinition>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Creator><Name>Q Light Controller Plus</Name><Version>4.12.0</Version><Author>test</Author></Creator>
 <Manufacturer>Test</Manufacturer>
 <Model>LED Fogger</Model>
 <Type>Smoke</Type>
 <Channel Name="Amount of Fog">
  <Group Byte="0">Intensity</Group>
  <Capability Min="0" Max="9">No fog output (0%)</Capability>
  <Capability Min="10" Max="255">Fog output (100%)</Capability>
 </Channel>
 <Channel Name="Brightness of All LEDs" Preset="IntensityDimmer"/>
 <Channel Name="Stroboscope Effect">
  <Group Byte="0">Effect</Group>
  <Capability Min="0" Max="9">Stroboscope effect off</Capability>
  <Capability Min="10" Max="255">Stroboscope effect from slow to fast</Capability>
 </Channel>
 <Channel Name="Intensity Colour Effect Red" Preset="IntensityRed"/>
 <Channel Name="Intensity Colour Effect Green" Preset="IntensityGreen"/>
 <Channel Name="Intensity Colour Effect Blue" Preset="IntensityBlue"/>
 <Mode Name="6 Channel">
  <Channel Number="0">Amount of Fog</Channel>
  <Channel Number="1">Brightness of All LEDs</Channel>
  <Channel Number="2">Stroboscope Effect</Channel>
  <Channel Number="3">Intensity Colour Effect Red</Channel>
  <Channel Number="4">Intensity Colour Effect Green</Channel>
  <Channel Number="5">Intensity Colour Effect Blue</Channel>
 </Mode>
 <Physical>
  <Bulb Type="LED" Lumens="0" ColourTemperature="0"/>
  <Dimensions Weight="1" Width="100" Height="100" Depth="100"/>
  <Lens Name="Other" DegreesMin="0" DegreesMax="0"/>
  <Focus Type="Fixed" PanMax="0" TiltMax="0"/>
  <Technical PowerConsumption="10" DmxConnector="3-pin"/>
 </Physical>
</FixtureDefinition>
"#;

/// Two heads, each R G B; the first head also has a dimmer, which carries only its own.
const TWO_HEADS: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<!DOCTYPE FixtureDefinition>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Creator><Name>Q Light Controller Plus</Name><Version>4.12.0</Version><Author>test</Author></Creator>
 <Manufacturer>Test</Manufacturer>
 <Model>Two Heads</Model>
 <Type>LED Bar (Pixels)</Type>
 <Channel Name="Dimmer 1" Preset="IntensityDimmer"/>
 <Channel Name="Red 1" Preset="IntensityRed"/>
 <Channel Name="Green 1" Preset="IntensityGreen"/>
 <Channel Name="Blue 1" Preset="IntensityBlue"/>
 <Channel Name="Red 2" Preset="IntensityRed"/>
 <Channel Name="Green 2" Preset="IntensityGreen"/>
 <Channel Name="Blue 2" Preset="IntensityBlue"/>
 <Mode Name="7 Channel">
  <Channel Number="0">Dimmer 1</Channel>
  <Channel Number="1">Red 1</Channel>
  <Channel Number="2">Green 1</Channel>
  <Channel Number="3">Blue 1</Channel>
  <Channel Number="4">Red 2</Channel>
  <Channel Number="5">Green 2</Channel>
  <Channel Number="6">Blue 2</Channel>
  <Head><Channel>0</Channel><Channel>1</Channel><Channel>2</Channel><Channel>3</Channel></Head>
  <Head><Channel>4</Channel><Channel>5</Channel><Channel>6</Channel></Head>
 </Mode>
 <Physical>
  <Bulb Type="LED" Lumens="0" ColourTemperature="0"/>
  <Dimensions Weight="1" Width="100" Height="100" Depth="100"/>
  <Lens Name="Other" DegreesMin="0" DegreesMax="0"/>
  <Focus Type="Fixed" PanMax="0" TiltMax="0"/>
  <Technical PowerConsumption="10" DmxConnector="3-pin"/>
 </Physical>
</FixtureDefinition>
"#;

/// One dimmer that runs from full at 0 to dark at 255.
const REVERSE: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<!DOCTYPE FixtureDefinition>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Creator><Name>Q Light Controller Plus</Name><Version>4.12.0</Version><Author>test</Author></Creator>
 <Manufacturer>Test</Manufacturer>
 <Model>Reverse Dimmer</Model>
 <Type>Dimmer</Type>
 <Channel Name="Dimmer">
  <Group Byte="0">Intensity</Group>
  <Capability Min="0" Max="255" Preset="BigToSmall">Dimmer 100 to 0 %</Capability>
 </Channel>
 <Mode Name="1 Channel">
  <Channel Number="0">Dimmer</Channel>
 </Mode>
 <Physical>
  <Bulb Type="Other" Lumens="0" ColourTemperature="0"/>
  <Dimensions Weight="1" Width="100" Height="100" Depth="100"/>
  <Lens Name="Other" DegreesMin="0" DegreesMax="0"/>
  <Focus Type="Fixed" PanMax="0" TiltMax="0"/>
  <Technical PowerConsumption="10" DmxConnector="3-pin"/>
 </Physical>
</FixtureDefinition>
"#;

/// A folder of every fixture both tests patch.
fn folder() -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("blackout-{}", process::id()));
    fs::create_dir_all(&folder).unwrap();
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (from, file) in [
        ("../rig/tests/definitions", "Vari-Lite-VL2202-Spot.qxf"),
        ("../rig/tests/definitions", "Cameo-HydraBeam-4000.qxf"),
        ("../rig/tests/definitions", "Martin-Rush-MH6-Wash.qxf"),
        (
            "../fixture-library/tests/definitions",
            "Generic-Generic-RGB.qxf",
        ),
    ] {
        fs::copy(here.join(from).join(file), folder.join(file)).unwrap();
    }
    fs::write(folder.join("Test-LED-Fogger.qxf"), FOGGER).unwrap();
    fs::write(folder.join("Test-Two-Heads.qxf"), TWO_HEADS).unwrap();
    fs::write(folder.join("Test-Reverse-Dimmer.qxf"), REVERSE).unwrap();
    folder
}

/// A served engine on universe 1 of a fake DMX USB Pro, these fixtures patched.
struct Desk {
    socket: Socket,
    heard: Heard,
    stop: Arc<AtomicBool>,
}

impl Desk {
    /// Patches each `(maker, model, mode, address)` in order: fixtures 1, 2, ...
    async fn new(fixtures: &[(&str, &str, &str, u16)]) -> Self {
        let (heard, stop) = (Heard::default(), Arc::new(AtomicBool::new(false)));
        let open = common::open(Some(Arc::clone(&heard)), Arc::clone(&stop));
        let (engine, sender) = Engine::start_with(Universes::ONE, open).unwrap();
        let address = serve(engine, sender, Some(folder())).await;
        let mut socket = connect(address).await;
        let usb = json!({"SetOutput": {
            "output": 1, "kind": {"DmxUsbPro": {"adapter": 1}},
            "universes": {"first": 1, "count": 1}, "rate": null,
        }});
        assert_eq!(command(&mut socket, usb).await, json!("Accepted"));
        let started = Instant::now();
        for (fixture, (maker, model, mode, address)) in fixtures.iter().enumerate() {
            let patch = json!({"Patch": {
                "name": model, "maker": maker, "model": model, "mode": mode,
                "at": {"universe": 1, "address": address},
            }});
            // the library is read in the background: the first patches may find nothing
            let reply = loop {
                let reply = command(&mut socket, patch.clone()).await;
                if reply != json!({"Refused": {"reason": "NoSuchFixture"}}) {
                    break reply;
                }
                assert!(started.elapsed() < Duration::from_secs(10), "no library");
                tokio::task::yield_now().await;
            };
            assert_eq!(
                reply,
                json!({"Patched": {"fixture": fixture + 1}}),
                "{patch}"
            );
        }
        Self {
            socket,
            heard,
            stop,
        }
    }

    /// Holds each `(address, byte)` of universe 1 raw.
    async fn hold(&mut self, holds: &[(u16, u8)]) {
        for (address, value) in holds {
            let hold = json!({"SetChannel": {
                "channel": {"universe": 1, "address": address}, "value": value,
            }});
            assert_eq!(command(&mut self.socket, hold).await, json!("Accepted"));
        }
    }

    async fn accepted(&mut self, command_: Value) {
        assert_eq!(command(&mut self.socket, command_).await, json!("Accepted"));
    }

    /// Waits until the adapter's last frame shows `want` at the addresses `at`, in order.
    async fn heard(&self, at: Vec<u16>, want: Vec<u8>, step: &'static str) {
        let heard = Arc::clone(&self.heard);
        tokio::task::spawn_blocking(move || until_heard(&heard, &at, &want, step))
            .await
            .unwrap();
    }
}

impl Drop for Desk {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The bytes at `at` (addresses of universe 1), in order, once the adapter's last frame
/// shows them.
fn until_heard(heard: &Heard, at: &[u16], want: &[u8], step: &str) {
    let started = Instant::now();
    let bytes =
        |frame: &[u8; 512]| -> Vec<u8> { at.iter().map(|a| frame[usize::from(*a) - 1]).collect() };
    loop {
        let last = heard.lock().unwrap().last().map(bytes);
        if last.as_deref() == Some(want) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{step}: last frame {last:?}, want {want:?}"
        );
        std::thread::sleep(Duration::from_millis(10)); // poll
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn blackout_moves_only_the_channels_its_rules_name() {
    let mut desk = Desk::new(&[
        ("Test", "LED Fogger", "6 Channel", 1),
        ("Test", "Two Heads", "7 Channel", 11),
        ("Vari-Lite", "VL2202 Spot", "Standard 8bit", 21),
        ("Generic", "Generic RGB", "RGB", 41),
    ])
    .await;
    // each fixture's channels, then the unpatched one
    let at: Vec<u16> = [1..=6, 11..=17, 21..=32, 41..=43, 100..=100]
        .into_iter()
        .flatten()
        .collect();
    let holds: Vec<(u16, u8)> = at.iter().map(|a| (*a, 200)).collect();
    desk.hold(&holds).await;
    let lit = vec![200; 29];
    #[rustfmt::skip]
    let dark = vec![
        200, 0, 200, 200, 200, 200, // fogger: only the LED dimmer
        0, 200, 200, 200, 0, 0, 0, // two heads: the dimmer and the head without one
        0, 200, 200, 200, 200, 200, 200, 3, 200, 200, 200, 200, // spot: intensity, strobe closed
        0, 0, 0, // PAR
        200, // unpatched
    ];
    desk.heard(at.clone(), lit.clone(), "held at 200").await;
    desk.accepted(json!({"Blackout": {"on": true}})).await;
    desk.heard(at.clone(), dark, "blackout on").await;
    desk.accepted(json!({"Blackout": {"on": false}})).await;
    desk.heard(at, lit, "blackout off").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn master_scales_only_what_follows_it() {
    let mut desk = Desk::new(&[
        ("Cameo", "HydraBeam 4000", "19 Channel", 1),
        ("Martin", "Rush MH6 Wash", "Standard", 21),
        ("Generic", "Generic RGB", "RGB", 41),
        ("Test", "Reverse Dimmer", "1 Channel", 51),
    ])
    .await;
    // (address, held): the HydraBeam's shared dimmer, RGBW, pan 1, head dimmers; the
    // MH6's 16-bit dimmer and its RGBW; the PAR's orange; the reverse dimmer; unpatched
    #[rustfmt::skip]
    let holds: Vec<(u16, u8)> = vec![
        (1, 255), (3, 200), (4, 200), (5, 200), (6, 200), (7, 100),
        (9, 179), (12, 179), (15, 179), (18, 179),
        (22, 0xB3), (23, 0x33), (24, 200), (27, 200),
        (41, 255), (42, 128), (43, 0),
        (51, 76),
        (61, 200),
    ];
    desk.hold(&holds).await;
    let at: Vec<u16> = holds.iter().map(|(a, _)| *a).collect();
    let held: Vec<u8> = holds.iter().map(|(_, b)| *b).collect();
    #[rustfmt::skip]
    let half = vec![
        255, 200, 200, 200, 200, 100, // shared dimmer and colour keep, pan stays
        90, 90, 90, 90, // head dimmers: 179 x 128 / 255
        0x59, 0xF3, 200, 200, // one word; the dimmer carries the RGBW
        128, 64, 0, // orange, hue kept
        165, // reverse: toward 255
        200, // unpatched
    ];
    #[rustfmt::skip]
    let zero = vec![
        255, 200, 200, 200, 200, 100,
        0, 0, 0, 0,
        0, 0, 200, 200,
        0, 0, 0,
        255,
        200,
    ];
    desk.heard(at.clone(), held.clone(), "held").await;
    let master = |level: u8| json!({"SetMaster": {"level": level}});
    desk.accepted(master(128)).await;
    desk.heard(at.clone(), half, "master half").await;
    desk.accepted(master(0)).await;
    desk.heard(at.clone(), zero, "master at 0").await;
    desk.accepted(master(255)).await;
    desk.heard(at, held, "master full").await;
}
