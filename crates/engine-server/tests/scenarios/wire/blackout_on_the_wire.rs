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

use crate::wire_common as common;

use common::{Heard, command, connect, serve};
use engine::{Engine, Universes};
use serde_json::json;
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

/// A folder of the fogger, the spot, the PAR and the two heads.
fn folder() -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("blackout-{}", process::id()));
    fs::create_dir_all(&folder).unwrap();
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (from, file) in [
        ("../rig/tests/definitions", "Vari-Lite-VL2202-Spot.qxf"),
        (
            "../fixture-library/tests/definitions",
            "Generic-Generic-RGB.qxf",
        ),
    ] {
        fs::copy(here.join(from).join(file), folder.join(file)).unwrap();
    }
    fs::write(folder.join("Test-LED-Fogger.qxf"), FOGGER).unwrap();
    fs::write(folder.join("Test-Two-Heads.qxf"), TWO_HEADS).unwrap();
    folder
}

/// The held channels: each fixture's, then the unpatched one.
const HELD: [std::ops::RangeInclusive<u16>; 5] = [1..=6, 11..=17, 21..=32, 41..=43, 100..=100];

/// The bytes at every held channel, in order, once the adapter's last frame shows them.
fn until_heard(heard: &Heard, want: &[u8], step: &str) {
    let started = Instant::now();
    let at = |frame: &[u8; 512]| -> Vec<u8> {
        HELD.iter()
            .flat_map(|range| range.clone().map(|a| frame[usize::from(a) - 1]))
            .collect()
    };
    loop {
        let last = heard.lock().unwrap().last().map(at);
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
    let patch = |maker: &str, model: &str, mode: &str, address: u16| {
        json!({"Patch": {
            "name": model, "maker": maker, "model": model, "mode": mode,
            "at": {"universe": 1, "address": address},
        }})
    };
    let started = Instant::now();
    for (fixture, patch) in [
        patch("Test", "LED Fogger", "6 Channel", 1),
        patch("Test", "Two Heads", "7 Channel", 11),
        patch("Vari-Lite", "VL2202 Spot", "Standard 8bit", 21),
        patch("Generic", "Generic RGB", "RGB", 41),
    ]
    .into_iter()
    .enumerate()
    {
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
    for address in HELD.iter().flat_map(Clone::clone) {
        let hold =
            json!({"SetChannel": {"channel": {"universe": 1, "address": address}, "value": 200}});
        assert_eq!(command(&mut socket, hold).await, json!("Accepted"));
    }
    let lit = [200; 29];
    #[rustfmt::skip]
    let dark = [
        200, 0, 200, 200, 200, 200, // fogger: only the LED dimmer
        0, 200, 200, 200, 0, 0, 0, // two heads: the dimmer and the head without one
        0, 200, 200, 200, 200, 200, 200, 3, 200, 200, 200, 200, // spot: intensity, strobe closed
        0, 0, 0, // PAR
        200, // unpatched
    ];
    let blackout = |on: bool| json!({"Blackout": {"on": on}});
    let steps: [(&str, Option<bool>, &[u8]); 3] = [
        ("held at 200", None, &lit),
        ("blackout on", Some(true), &dark),
        ("blackout off", Some(false), &lit),
    ];
    for (step, on, want) in steps {
        if let Some(on) = on {
            assert_eq!(command(&mut socket, blackout(on)).await, json!("Accepted"));
        }
        let heard = Arc::clone(&heard);
        let want = want.to_vec();
        tokio::task::spawn_blocking(move || until_heard(&heard, &want, step))
            .await
            .unwrap();
    }
    stop.store(true, Ordering::Relaxed);
}
