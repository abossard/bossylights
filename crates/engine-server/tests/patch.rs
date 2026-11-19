//! End to end over the API: patch the Generic RGB PAR at universe 1, address 37, make it
//! red: 255 0 0 arrive at 37 to 39 on the DDP socket and on a fake DMX USB Pro (a pseudo
//! terminal). Unpatch it: 0 0 0. A group of three PARs made red: nine bytes.

mod common;

use common::{Heard, Socket, answer, command, connect, open, send, taken};
use engine::{Engine, Open, Universes};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

/// Waits until the channels of universe 1 from 37 on are `bytes` on the DDP socket and in
/// the last message of the fake adapter. Frames computed while a command travelled may
/// still lack it. With `before`, every DDP frame on the way is either `before` or `bytes`:
/// a command is never half in a frame.
async fn until_both<const N: usize>(
    udp: &UdpSocket,
    heard: &Heard,
    before: Option<[u8; N]>,
    bytes: [u8; N],
) {
    let started = Instant::now();
    let mut packet = vec![0; 1500];
    let mut ddp = None;
    loop {
        if let Ok(Ok(size)) =
            tokio::time::timeout(Duration::from_millis(100), udp.recv(&mut packet)).await
        {
            ddp = Some(packet[10 + 36..10 + 36 + N].to_vec()).filter(|_| size >= 10 + 36 + N);
            if let (Some(before), Some(seen)) = (before, &ddp) {
                assert!(
                    seen == &before || seen == &bytes,
                    "a partial frame: {seen:?} is neither {before:?} nor {bytes:?}"
                );
            }
        }
        let dmx = heard.lock().unwrap().last().map(|c| c[36..36 + N].to_vec());
        if ddp.as_deref() == Some(&bytes[..]) && dmx.as_deref() == Some(&bytes[..]) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "from 37: DDP {ddp:?}, DMX {dmx:?}, wanted {bytes:?}"
        );
    }
}

/// A served engine with the test definitions, its devices opened by `open`.
async fn serve(open: Open) -> SocketAddr {
    let (engine, sender) = Engine::start_with(Universes::new(2).unwrap(), open).unwrap();
    let definitions =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixture-library/tests/definitions");
    common::serve(engine, sender, Some(definitions)).await
}

fn par(name: &str, address: u16) -> Value {
    json!({"Patch": {
        "name": name, "maker": "Generic", "model": "Generic RGB", "mode": "RGB",
        "at": {"universe": 1, "address": address},
    }})
}

/// Patches `par`, again until the library is read. The reply.
async fn patch_once_read(socket: &mut Socket, par: &Value) -> Value {
    let started = Instant::now();
    loop {
        let reply = command(socket, par.clone()).await;
        if reply != json!({"Refused": {"reason": "NoSuchFixture"}}) {
            return reply;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "no library");
        tokio::task::yield_now().await;
    }
}

/// Asks until the picture shows `wanted` under `key` (`patch`, `groups`): a picture is of
/// a frame, and the frame after a command may not be taken yet.
async fn until_picture(socket: &mut Socket, key: &str, wanted: &Value) {
    let started = Instant::now();
    loop {
        send(socket, &json!("Query")).await;
        let seen = answer(socket).await["Picture"][key].clone();
        if seen == *wanted {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the {key}: {seen}, wanted {wanted}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_patched_par_made_red_sends_255_0_0_at_37_on_ddp_and_dmx_and_unpatched_0_0_0() {
    let heard: Heard = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let address = serve(open(Some(Arc::clone(&heard)), Arc::clone(&stop))).await;
    let mut socket = connect(address).await;
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ddp = json!({"Ddp": {"address": udp.local_addr().unwrap().to_string(), "color": "Rgb"}});
    let dmx = json!({"DmxUsbPro": {"adapter": 1}});
    for (output, kind) in [(1, ddp), (2, dmx)] {
        let set = json!({"SetOutput": {
            "output": output, "kind": kind, "universes": {"first": 1, "count": 1}, "rate": null,
        }});
        assert_eq!(command(&mut socket, set).await, json!("Accepted"));
    }

    // the library is read on a thread of its own: patch once it is there
    assert_eq!(
        patch_once_read(&mut socket, &par("PAR 3", 37)).await,
        json!({"Patched": {"fixture": 1}})
    );
    let fixture = json!({"Fixture": {"fixture": 1}});
    let full = json!({"SetIntensity": {"target": fixture, "intensity": 65535}});
    let red = json!({"SetColour": {"target": fixture, "red": 65535, "green": 0, "blue": 0}});
    assert!(taken(&command(&mut socket, full).await));
    assert!(taken(&command(&mut socket, red).await));
    until_both(&udp, &heard, None, [255, 0, 0]).await;

    until_picture(
        &mut socket,
        "patch",
        &json!([{
            "fixture": 1, "name": "PAR 3", "maker": "Generic", "model": "Generic RGB",
            "mode": "RGB", "universe": 1, "address": 37, "channels": 3, "first": 1,
            "lights": 1, "layout": null,
        }]),
    )
    .await;

    let unpatch = json!({"Unpatch": {"fixture": 1}});
    assert_eq!(command(&mut socket, unpatch).await, json!("Accepted"));
    until_both(&udp, &heard, None, [0, 0, 0]).await;
    until_picture(&mut socket, "patch", &json!([])).await;
    stop.store(true, Ordering::Relaxed);
}

/// Who wins, end to end: a raw channel held wins over the light that writes it until
/// released, a blackout wins over everything, a released colour goes back to black.
#[tokio::test(flavor = "multi_thread")]
async fn a_red_par_under_raw_38_at_9_sends_255_9_0_released_255_0_0_blackout_0_0_0() {
    let heard: Heard = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let address = serve(open(Some(Arc::clone(&heard)), Arc::clone(&stop))).await;
    let mut socket = connect(address).await;
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ddp = json!({"Ddp": {"address": udp.local_addr().unwrap().to_string(), "color": "Rgb"}});
    let dmx = json!({"DmxUsbPro": {"adapter": 1}});
    for (output, kind) in [(1, ddp), (2, dmx)] {
        let set = json!({"SetOutput": {
            "output": output, "kind": kind, "universes": {"first": 1, "count": 1}, "rate": null,
        }});
        assert_eq!(command(&mut socket, set).await, json!("Accepted"));
    }
    assert_eq!(
        patch_once_read(&mut socket, &par("PAR 1", 37)).await,
        json!({"Patched": {"fixture": 1}})
    );
    let fixture = json!({"Fixture": {"fixture": 1}});
    let channel_38 = json!({"universe": 1, "address": 38});
    // (command, bytes at 37 38 39 after it)
    let steps = [
        (
            json!({"SetIntensity": {"target": fixture, "intensity": 65535}}),
            [0, 0, 0],
        ),
        (
            json!({"SetColour": {"target": fixture, "red": 65535, "green": 0, "blue": 0}}),
            [255, 0, 0],
        ),
        (
            json!({"SetChannel": {"channel": channel_38, "value": 9}}),
            [255, 9, 0],
        ),
        (
            json!({"ReleaseChannel": {"channel": channel_38}}),
            [255, 0, 0],
        ),
        (json!({"Blackout": {"on": true}}), [0, 0, 0]),
        (json!({"Blackout": {"on": false}}), [255, 0, 0]),
        (
            json!({"SetColour": {"target": fixture, "red": 65535, "green": 65535, "blue": 0}}),
            [255, 255, 0],
        ),
        (
            json!({"SetChannel": {"channel": channel_38, "value": 0}}),
            [255, 0, 0],
        ),
        (
            json!({"ReleaseChannel": {"channel": channel_38}}),
            [255, 255, 0],
        ),
        (json!({"ReleaseColour": {"target": fixture}}), [0, 0, 0]),
        (json!({"ReleaseColour": {"target": fixture}}), [0, 0, 0]),
        (
            json!({"SetColour": {"target": fixture, "red": 65535, "green": 0, "blue": 0}}),
            [255, 0, 0],
        ),
        (json!({"ReleaseIntensity": {"target": fixture}}), [0, 0, 0]),
    ];
    for (step, bytes) in steps {
        assert!(taken(&command(&mut socket, step.clone()).await), "{step}");
        until_both(&udp, &heard, None, bytes).await;
    }
    let none = json!({"ReleaseColour": {"target": {"Fixture": {"fixture": 2}}}});
    assert_eq!(
        command(&mut socket, none).await,
        json!({"Refused": {"reason": "NotPatched"}})
    );
    stop.store(true, Ordering::Relaxed);
}

/// A group of three PARs set red: nine bytes. Its order is in the picture and survives a
/// move; an unpatched PAR leaves it, and the group then sets only the two left.
#[tokio::test(flavor = "multi_thread")]
async fn a_group_of_three_pars_made_red_sends_nine_bytes_on_ddp_and_dmx() {
    let heard: Heard = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let address = serve(open(Some(Arc::clone(&heard)), Arc::clone(&stop))).await;
    let mut socket = connect(address).await;
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ddp = json!({"Ddp": {"address": udp.local_addr().unwrap().to_string(), "color": "Rgb"}});
    let dmx = json!({"DmxUsbPro": {"adapter": 1}});
    for (output, kind) in [(1, ddp), (2, dmx)] {
        let set = json!({"SetOutput": {
            "output": output, "kind": kind, "universes": {"first": 1, "count": 1}, "rate": null,
        }});
        assert_eq!(command(&mut socket, set).await, json!("Accepted"));
    }
    assert_eq!(
        patch_once_read(&mut socket, &par("PAR 1", 37)).await,
        json!({"Patched": {"fixture": 1}})
    );
    for (n, at) in [(2, 40), (3, 43)] {
        assert_eq!(
            command(&mut socket, par(&format!("PAR {n}"), at)).await,
            json!({"Patched": {"fixture": n}})
        );
    }
    let f = |fixture: u16| json!({"Fixture": {"fixture": fixture}});
    let unpatched = json!({"SetGroup": {"name": "front", "members": [f(1), f(4)]}});
    assert_eq!(
        command(&mut socket, unpatched).await,
        json!({"Refused": {"reason": "NotPatched"}})
    );
    let mut subscriber = connect(address).await;
    send(&mut subscriber, &json!("Subscribe")).await;
    let front = json!({"SetGroup": {"name": "front", "members": [f(3), f(1), f(2)]}});
    assert_eq!(command(&mut socket, front).await, json!("Accepted"));
    // a subscriber gets the groups with the patch that has them, in their order
    let made = json!([{"name": "front", "members": [f(3), f(1), f(2)]}]);
    let started = Instant::now();
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), subscriber.next())
            .await
            .expect("a message within 5 s")
            .unwrap()
            .unwrap();
        let reply: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        if reply["Changed"]["groups"] == made {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no Changed with the groups"
        );
    }
    until_picture(&mut socket, "groups", &made).await;
    let group = json!({"Group": {"group": "front"}});
    // (command, bytes at 37 to 45 after it)
    let steps = [
        (
            json!({"SetIntensity": {"target": group, "intensity": 65535}}),
            [0, 0, 0, 0, 0, 0, 0, 0, 0],
        ),
        (
            json!({"SetColour": {"target": group, "red": 65535, "green": 0, "blue": 0}}),
            [255, 0, 0, 255, 0, 0, 255, 0, 0],
        ),
        (
            json!({"Move": {"fixture": 1, "at": {"universe": 1, "address": 100}}}),
            [0, 0, 0, 255, 0, 0, 255, 0, 0],
        ),
        (
            json!({"Unpatch": {"fixture": 2}}),
            [0, 0, 0, 0, 0, 0, 255, 0, 0],
        ),
        (
            json!({"Move": {"fixture": 1, "at": {"universe": 1, "address": 37}}}),
            [255, 0, 0, 0, 0, 0, 255, 0, 0],
        ),
        (
            json!({"SetColour": {"target": group, "red": 0, "green": 65535, "blue": 0}}),
            [0, 255, 0, 0, 0, 0, 0, 255, 0],
        ),
    ];
    let mut before = [0; 9];
    for (step, bytes) in steps {
        assert!(taken(&command(&mut socket, step.clone()).await), "{step}");
        until_both(&udp, &heard, Some(before), bytes).await;
        before = bytes;
    }
    let left = json!([{"name": "front", "members": [f(3), f(1)]}]);
    until_picture(&mut socket, "groups", &left).await;
    assert_eq!(
        command(&mut socket, json!({"RemoveGroup": {"name": "front"}})).await,
        json!("Accepted")
    );
    until_picture(&mut socket, "groups", &json!([])).await;
    let gone = json!({"SetColour": {"target": group, "red": 0, "green": 0, "blue": 0}});
    assert_eq!(
        command(&mut socket, gone).await,
        json!({"Refused": {"reason": "NoSuchGroup"}})
    );
    stop.store(true, Ordering::Relaxed);
}

/// Every client sees one patch and one project per frame: a subscriber, which gets the patch
/// only when it changed, and a client that patches and asks again and again see the same
/// for the same frame.
#[tokio::test(flavor = "multi_thread")]
async fn every_client_sees_the_same_patch_and_project_in_the_same_frame() {
    const FIXTURES: u16 = 20;
    let open: Open = Box::new(outputs::open);
    let address = serve(open).await;
    let (mut subscriber, mut asker) = (connect(address).await, connect(address).await);
    assert_eq!(
        patch_once_read(&mut asker, &par("PAR 1", 1)).await,
        json!({"Patched": {"fixture": 1}})
    );
    let subscribed = async {
        send(&mut subscriber, &json!("Subscribe")).await;
        // (frame, patch, project), the patch carried on from the last message that had it
        let mut frames: Vec<(u64, Value, Value)> = Vec::new();
        while frames
            .iter()
            .filter(|(_, p, _)| p.as_array().map(Vec::len) == Some(usize::from(FIXTURES)))
            .count()
            < 5
        {
            let message = tokio::time::timeout(Duration::from_secs(5), subscriber.next())
                .await
                .expect("a message within 5 s")
                .unwrap()
                .unwrap();
            let reply: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            let picture = reply
                .get("Picture")
                .or_else(|| reply.get("Changed"))
                .unwrap();
            let patch = if picture["patch"].is_null() {
                frames.last().unwrap().1.clone()
            } else {
                picture["patch"].clone()
            };
            let frame = picture["state"]["frame"].as_u64().unwrap();
            frames.push((frame, patch, picture["project"].clone()));
        }
        frames
    };
    let asked = async {
        let mut frames = Vec::new();
        for n in 2..=FIXTURES {
            let name = format!("PAR {n}");
            assert_eq!(
                command(&mut asker, par(&name, 3 * n)).await,
                json!({"Patched": {"fixture": n}})
            );
            for _ in 0..10 {
                send(&mut asker, &json!("Query")).await;
                let picture = answer(&mut asker).await["Picture"].clone();
                let frame = picture["state"]["frame"].as_u64().unwrap();
                frames.push((frame, picture["patch"].clone(), picture["project"].clone()));
            }
        }
        frames
    };
    let (subscribed, asked) = tokio::join!(subscribed, asked);
    let mut same = 0;
    for (frame, patch, project) in &asked {
        if let Some((_, seen, seen_project)) = subscribed.iter().find(|(f, _, _)| f == frame) {
            assert_eq!(patch, seen, "frame {frame}");
            assert_eq!(project, seen_project, "frame {frame}");
            same += 1;
        }
    }
    assert!(same >= 10, "too few frames both saw: {same}");
}
