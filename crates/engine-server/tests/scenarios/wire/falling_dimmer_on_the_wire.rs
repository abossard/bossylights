//! End to end over the API (#355): a falling dimmer (`BigToSmall`, "Dimmer 100% to 0%")
//! at 1:37 on DDP turns a light's intensity the way its file says.
//!
//! | Command | Channel 37 |
//! |---|---|
//! | none yet | 255, its default |
//! | `SetIntensity` 65535 | 0: full |
//! | `SetIntensity` 0 | 255: dark |

use crate::wire_common as common;

use common::{command, connect, serve, taken};
use engine::{Engine, Universes};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{fs, process};
use tokio::net::UdpSocket;

/// One dimmer, 255 at no light, as finding RIG-01 of #349 wrote it.
const FALLING_DIMMER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Test</Manufacturer>
 <Model>Inverted Dimmer</Model>
 <Type>Dimmer</Type>
 <Channel Name="Dimmer" Default="255">
  <Group Byte="0">Intensity</Group>
  <Capability Min="0" Max="255" Preset="BigToSmall">Dimmer 100% to 0%</Capability>
 </Channel>
 <Mode Name="1ch">
  <Channel Number="0">Dimmer</Channel>
 </Mode>
</FixtureDefinition>"#;

/// A folder of the falling dimmer alone.
fn folder() -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("falling-{}", process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("Test-Inverted-Dimmer.qxf"), FALLING_DIMMER).unwrap();
    folder
}

/// Waits for a packet of DDP whose channel 37 of universe 1 is `byte`.
async fn when_37_shows(udp: &UdpSocket, byte: u8) {
    let started = Instant::now();
    let mut packet = vec![0; 1500];
    loop {
        let size = tokio::time::timeout(Duration::from_secs(5), udp.recv(&mut packet))
            .await
            .expect("a packet within 5 s")
            .unwrap();
        let seen = packet.get(10 + 36).filter(|_| size > 10 + 36).copied();
        if seen == Some(byte) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "channel 37 is {seen:?}, never {byte}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_falling_dimmer_is_0_at_full_and_255_at_none() {
    let (engine, sender) = Engine::start(Universes::ONE).unwrap();
    let address = serve(engine, sender, Some(folder())).await;
    let mut socket = connect(address).await;
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ddp = json!({"Ddp": {"address": udp.local_addr().unwrap().to_string(), "color": "Rgb"}});
    let set = json!({"SetOutput": {
        "output": 1, "kind": ddp, "universes": {"first": 1, "count": 1}, "rate": null,
    }});
    assert_eq!(command(&mut socket, set).await, json!("Accepted"));
    let patch = json!({"Patch": {
        "name": "Inverted Dimmer", "maker": "Test", "model": "Inverted Dimmer", "mode": "1ch",
        "at": {"universe": 1, "address": 37},
    }});
    let started = Instant::now();
    // the library is read in the background: the first patches may find nothing
    let reply = loop {
        let reply = command(&mut socket, patch.clone()).await;
        if reply != json!({"Refused": {"reason": "NoSuchFixture"}}) {
            break reply;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "no library");
        tokio::task::yield_now().await;
    };
    assert_eq!(reply, json!({"Patched": {"fixture": 1}}));

    let intensity = |intensity: u16| json!({"SetIntensity": {"target": {"Fixture": {"fixture": 1}}, "intensity": intensity}});
    for (level, byte) in [(u16::MAX, 0), (0, 255)] {
        let reply: Value = command(&mut socket, intensity(level)).await;
        assert!(taken(&reply), "{reply}");
        when_37_shows(&udp, byte).await;
    }
}
