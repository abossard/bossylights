//! End to end over the API (#295): a brightness command never moves fog or haze. A
//! Stairville Hz-200 at 1:37 (Haze, Fan Speed), a Generic Smoke at 1:39 (Pump, Fan) and a
//! Generic RGB PAR in its mode RGB Dimmer at 1:41 all at half brightness, the PAR red: the
//! PAR sends 255 0 0 128 on the DDP socket while the Haze and the Pump stay at 0.

use crate::wire_common as common;

use common::{command, connect, serve, taken};
use engine::{Engine, Universes};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{fs, process};
use tokio::net::UdpSocket;

/// A folder of the hazer, the smoke machine and the Generic RGB PAR.
fn folder() -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fog-{}", process::id()));
    fs::create_dir_all(&folder).unwrap();
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (from, file) in [
        ("../rig/tests/definitions", "Stairville-Hz-200-DMX.qxf"),
        ("../rig/tests/definitions", "Generic-Generic-Smoke.qxf"),
        (
            "../fixture-library/tests/definitions",
            "Generic-Generic-RGB.qxf",
        ),
    ] {
        fs::copy(here.join(from).join(file), folder.join(file)).unwrap();
    }
    folder
}

/// Universe 1 from 37 to 44, once a packet of DDP shows the PAR at `par`.
async fn when_the_par_shows(udp: &UdpSocket, par: [u8; 4]) -> Vec<u8> {
    let started = Instant::now();
    let mut packet = vec![0; 1500];
    loop {
        let size = tokio::time::timeout(Duration::from_secs(5), udp.recv(&mut packet))
            .await
            .expect("a packet within 5 s")
            .unwrap();
        let seen = packet.get(10 + 36..10 + 44).filter(|_| size >= 10 + 44);
        if let Some(seen) = seen
            && seen[4..] == par
        {
            return seen.to_vec();
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "from 37: {seen:?}, the PAR never showed {par:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_brightness_never_moves_fog_or_haze() {
    let (engine, sender) = Engine::start(Universes::ONE).unwrap();
    let address = serve(engine, sender, Some(folder())).await;
    let mut socket = connect(address).await;
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ddp = json!({"Ddp": {"address": udp.local_addr().unwrap().to_string(), "color": "Rgb"}});
    let set = json!({"SetOutput": {
        "output": 1, "kind": ddp, "universes": {"first": 1, "count": 1}, "rate": null,
    }});
    assert_eq!(command(&mut socket, set).await, json!("Accepted"));
    let patch = |maker: &str, model: &str, mode: &str, address: u16| {
        json!({"Patch": {
            "name": model, "maker": maker, "model": model, "mode": mode,
            "at": {"universe": 1, "address": address},
        }})
    };
    let started = Instant::now();
    for (fixture, patch) in [
        patch("Stairville", "Hz-200 DMX", "Default", 37),
        patch("Generic", "Generic Smoke", "Normal", 39),
        patch("Generic", "Generic RGB", "RGB Dimmer", 41),
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
        assert_eq!(reply, json!({"Patched": {"fixture": fixture + 1}}));
    }

    let half = |fixture: u16| json!({"SetIntensity": {"target": {"Fixture": {"fixture": fixture}}, "intensity": 32768}});
    let red = json!({"SetColour": {
        "target": {"Fixture": {"fixture": 3}}, "red": 65535, "green": 0, "blue": 0,
    }});
    for set in [half(1), half(2), half(3), red] {
        let reply: Value = command(&mut socket, set).await;
        assert!(taken(&reply), "{reply}");
    }
    // Haze, Fan Speed, Pump, Fan, then the PAR: red, green, blue, its dimmer
    assert_eq!(
        when_the_par_shows(&udp, [255, 0, 0, 128]).await,
        [0, 0, 0, 0, 255, 0, 0, 128]
    );
}
