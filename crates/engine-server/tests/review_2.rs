mod common;

use common::{answer, command, connect, send, serve};
use engine::{Engine, Universes};
use serde_json::json;
use std::time::Duration;
use tokio::net::UdpSocket;

// Claim: A live gesture with UntilStop keeps overriding the show when its socket disappears.
// Source: docs/design/channel-model.md, "A press is in the very next frame"; docs/design/who-wins.md, rule 4.
// Severity: high. A crashed controller can leave its colour over the running show.
#[tokio::test]
async fn finding_2_01_live_grab_survives_socket_loss() {
    let (engine, sender) = Engine::start(Universes::ONE).unwrap();
    let address = serve(engine, sender, None).await;
    let mut show = connect(address).await;
    let mut hand = connect(address).await;
    let dmx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let target = json!({"Fixture": {"fixture": 1}});
    assert_eq!(
        command(
            &mut show,
            json!({"PatchStrip": {
                "name": "pixel", "pixels": 1, "color": "Rgb",
                "at": {"universe": 1, "address": 1}
            }}),
        )
        .await,
        json!({"Patched": {"fixture": 1}})
    );
    for edit in [
        json!({"SetOutput": {
            "output": 1, "kind": {"Ddp": {"address": dmx.local_addr().unwrap().to_string(), "color": "Rgb"}},
            "universes": {"first": 1, "count": 1}, "rate": null
        }}),
        json!({"SetMixRules": {"rules": {
            "brightness": "Latest", "manual_end": "UntilStop", "other_end": "Reveal"
        }}}),
        json!({"PutStep": {
            "look": 1, "name": "blue", "step": {
                "target": target,
                "values": {"colour": {"red": 0, "green": 0, "blue": 65535}}
            }
        }}),
    ] {
        assert_eq!(command(&mut show, edit).await, json!("Accepted"));
    }
    assert!(command(&mut show, json!({"StartLook": {"look": 1, "rank": "Show"}}))
        .await["Started"]["play"]
        .is_object());
    assert_eq!(next_rgb(&dmx, |rgb| rgb != [0, 0, 0]).await, [0, 0, 255]);

    let grab = command(
        &mut hand,
        json!({"SetProperty": {"target": target, "value": {"Colour": {
            "red": 65535, "green": 0, "blue": 0
        }}}}),
    )
    .await;
    assert!(grab["Grabbed"]["grab"].is_object(), "{grab}");
    next_rgb(&dmx, |rgb| rgb == [255, 0, 0]).await;
    assert_eq!(
        command(&mut hand, json!({"Blackout": {"on": true}})).await,
        json!("Accepted")
    );
    next_rgb(&dmx, |rgb| rgb == [0, 0, 0]).await;
    drop(hand);

    // The first non-dark frame proves that the server processed the lost socket.
    let after_disconnect = next_rgb(&dmx, |rgb| rgb != [0, 0, 0]).await;
    send(&mut show, &json!("Stop")).await;
    assert_eq!(answer(&mut show).await, json!("Stopping"));
    assert_eq!(
        after_disconnect,
        [0, 0, 255],
        "the first frame after the lost hand's Blackout clears must show the blue Show look, not its red grab"
    );
}

async fn next_rgb(socket: &UdpSocket, matches: impl Fn([u8; 3]) -> bool) -> [u8; 3] {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut packet = [0; 1024];
        loop {
            let size = socket.recv(&mut packet).await.unwrap();
            assert!(size >= 13, "expected a DDP packet with at least one pixel");
            let rgb = packet[10..13].try_into().unwrap();
            if matches(rgb) {
                return rgb;
            }
        }
    })
    .await
    .expect("the matching DDP frame within 5 s")
}
