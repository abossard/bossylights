//! M3 T2 end to end over the API: a saved step played at Manual on a fake DMX USB Pro.
//! Generic RGB at 1:37, full intensity held, the step red: 255 0 0. Released: the
//! colour underneath (none): 0 0 0. Restarted from the show file without the library:
//! idle black, intensity restored by hand, the saved step played again: the same bytes.

mod common;

use common::{Heard, Socket, answer, command, connect, open, send};
use engine::{Engine, Universes};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

type Served = JoinHandle<std::io::Result<Engine>>;

/// An engine serving `show`, with the test definitions when `library`, its DMX USB Pro
/// heard into `heard`.
async fn serve(
    show: &Path,
    library: bool,
    heard: &Heard,
    stop: &Arc<AtomicBool>,
) -> (Served, Socket) {
    let opens = open(Some(Arc::clone(heard)), Arc::clone(stop));
    let (engine, sender) = Engine::start_with(Universes::new(1).unwrap(), opens).unwrap();
    let definitions = library.then(|| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixture-library/tests/definitions")
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let served = tokio::spawn(engine_server::serve(
        listener,
        engine,
        sender,
        definitions,
        Some(show.to_path_buf()),
    ));
    let mut socket = connect(address).await;
    let dmx = json!({"SetOutput": {
        "output": 1, "kind": {"DmxUsbPro": {"adapter": 1}},
        "universes": {"first": 1, "count": 1}, "rate": null,
    }});
    assert_eq!(command(&mut socket, dmx).await, json!("Accepted"));
    (served, socket)
}

/// Waits until the adapter's last frame has `bytes` from 37 on.
async fn until_dmx(heard: &Heard, bytes: [u8; 3]) {
    let started = Instant::now();
    loop {
        let dmx = heard.lock().unwrap().last().map(|c| [c[36], c[37], c[38]]);
        if dmx == Some(bytes) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "from 37: {dmx:?}, wanted {bytes:?}"
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

/// Waits until the picture at `pointer` is `wanted`: a picture may be a frame behind
/// the wire.
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

async fn stop(socket: &mut Socket, served: Served) {
    send(socket, &json!("Stop")).await;
    assert_eq!(answer(socket).await, json!("Stopping"));
    served.await.unwrap().unwrap();
}

fn show_file() -> PathBuf {
    let folder = std::env::temp_dir().join(format!("bossylights-m3-step-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder.join("show.json")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_saved_red_step_plays_255_0_0_releases_and_replays_after_a_restart() {
    let file = show_file();
    let heard: Heard = Arc::new(Mutex::new(Vec::new()));
    let stop_adapter = Arc::new(AtomicBool::new(false));
    let fixture = json!({"Fixture": {"fixture": 1}});
    let full = json!({"SetIntensity": {"target": fixture, "intensity": 65535}});
    let red = json!({"red": 65535, "green": 0, "blue": 0});
    let step = json!({"PutStep": {"look": 1, "name": "red", "step": {
        "target": fixture, "values": {"colour": red},
    }}});
    let start = json!({"StartLook": {"look": 1}});

    let (served, mut socket) = serve(&file, true, &heard, &stop_adapter).await;
    let par = json!({"Patch": {
        "name": "PAR 1", "maker": "Generic", "model": "Generic RGB", "mode": "RGB",
        "at": {"universe": 1, "address": 37},
    }});
    assert_eq!(
        once_read(&mut socket, &par).await,
        json!({"Patched": {"fixture": 1}})
    );
    assert_eq!(command(&mut socket, full.clone()).await, json!("Accepted"));
    // a look of nothing patched is refused, and nothing is saved
    let nowhere = json!({"PutStep": {"look": 2, "name": "x", "step": {
        "target": {"Fixture": {"fixture": 9}}, "values": {"colour": red},
    }}});
    assert_eq!(
        command(&mut socket, nowhere).await,
        json!({"Refused": {"reason": "NotPatched"}})
    );
    assert_eq!(
        command(&mut socket, json!({"StartLook": {"look": 2}})).await,
        json!({"Refused": {"reason": "NoSuchLook"}})
    );
    assert_eq!(command(&mut socket, step).await, json!("Accepted"));
    let started = command(&mut socket, start.clone()).await;
    let play = started["Started"]["play"].clone();
    assert!(play.is_object(), "{started}");
    until_dmx(&heard, [255, 0, 0]).await;
    until_picture(
        &mut socket,
        "/state/plays",
        &json!([{"play": play, "look": 1}]),
    )
    .await;
    until_picture(&mut socket, "/looks/0/look", &json!(1)).await;
    // the fixture a play shows cannot be unpatched, nor the look removed
    assert_eq!(
        command(&mut socket, json!({"Unpatch": {"fixture": 1}})).await,
        json!({"Refused": {"reason": {"LookInUse": {"look": 1}}}})
    );
    assert_eq!(
        command(&mut socket, json!({"RemoveLook": {"look": 1}})).await,
        json!({"Refused": {"reason": {"LookInUse": {"look": 1}}}})
    );

    let release = json!({"ReleaseHold": {"play": play}});
    assert_eq!(
        command(&mut socket, release.clone()).await,
        json!("Accepted")
    );
    until_dmx(&heard, [0, 0, 0]).await;
    // an ended play: a harmless no-op
    assert_eq!(command(&mut socket, release).await, json!("Accepted"));
    until_picture(&mut socket, "/state/plays", &json!([])).await;
    stop(&mut socket, served).await;

    // restarted from the file, without the library: performance idle
    heard.lock().unwrap().clear();
    let (served, mut socket) = serve(&file, false, &heard, &stop_adapter).await;
    until_dmx(&heard, [0, 0, 0]).await;
    until_picture(&mut socket, "/show", &json!("Kept")).await;
    until_picture(&mut socket, "/state/plays", &json!([])).await;
    until_picture(&mut socket, "/looks/0/name", &json!("red")).await;
    assert_eq!(command(&mut socket, full).await, json!("Accepted"));
    let started = command(&mut socket, start).await;
    assert!(started["Started"]["play"].is_object(), "{started}");
    until_dmx(&heard, [255, 0, 0]).await;
    stop(&mut socket, served).await;
    stop_adapter.store(true, Ordering::Relaxed);
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}
