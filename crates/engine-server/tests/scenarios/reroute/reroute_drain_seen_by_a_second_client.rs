//! A second client observes the run while the adapter drains its last old write.

use crate::output_stream as stream;
use crate::wire_common as common;

use engine::{Color, Device, Engine, Outcome, Rate, Universes, Wakes};
use futures_util::{FutureExt, StreamExt};
use serde_json::{Value, json};
use std::io;
use std::sync::mpsc;
use std::time::Duration;

const GUARD: Duration = Duration::from_secs(10);

struct Drain {
    device: Box<dyn Device>,
    gate: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
}

impl Device for Drain {
    fn take(&mut self, frame: &[u8]) -> Outcome {
        self.device.take(frame)
    }

    fn finish_route(&mut self) {
        if let Some((entered, release)) = self.gate.take() {
            entered.send(()).unwrap();
            release.recv_timeout(GUARD).unwrap();
        }
        self.device.finish_route();
    }
}

async fn next_picture(client: &mut common::Socket, wakes: &Wakes, at: Duration) -> Value {
    let before = wakes.woke();
    tokio::time::timeout(GUARD, async {
        loop {
            wakes.wake_at(at).unwrap();
            common::send(client, &json!("Query")).await;
            let answer = common::answer(client).await;
            if answer["Picture"]["state"]["frame"].as_u64().unwrap() > before {
                return answer["Picture"].clone();
            }
        }
    })
    .await
    .expect("a picture from a new wake")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_clients_picture_keeps_the_old_run_until_the_reroute_is_accepted() {
    let (wire, kind) = stream::receiver(Color::Rgb);
    let (entered, draining) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut gate = Some((entered, gate));
    let (engine, sender, wakes) = Engine::start_stepped(
        Universes::new(2).unwrap(),
        Box::new(move |kind| -> io::Result<Box<dyn Device>> {
            Ok(Box::new(Drain {
                device: outputs::open(kind)?,
                gate: gate.take(),
            }))
        }),
    )
    .unwrap();
    let address = common::serve(engine, sender, None).await;
    let mut writer = common::connect(address).await;
    let mut reader = common::connect(address).await;
    wakes.hold().unwrap();

    let set = |first| {
        json!({"SetOutput": {
            "output": 1, "kind": kind, "universes": {"first": first, "count": 1}, "rate": "60"
        }})
    };
    let press = |universe, value| {
        json!({"SetChannel": {
            "channel": {"universe": universe, "address": 1}, "value": value
        }})
    };
    for (universe, value) in [(1, 11), (2, 91)] {
        assert_eq!(
            common::command(&mut writer, press(universe, value)).await,
            "Accepted"
        );
    }
    assert_eq!(common::command(&mut writer, set(1)).await, "Accepted");
    let first_slot = || {
        let mut packet = [0; 1500];
        assert_eq!(wire.recv(&mut packet).unwrap(), 520);
        assert_eq!(u16::from_be_bytes([packet[8], packet[9]]), 510);
        packet[10]
    };
    let at = wakes.wake().unwrap();
    assert_eq!(first_slot(), 11);

    common::send(&mut writer, &json!({"Send": {"command": set(2)}})).await;
    tokio::time::timeout(GUARD, async {
        while draining.try_recv().is_err() {
            wakes.wake_at(at).unwrap();
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the old adapter starts draining");
    let pending = next_picture(&mut reader, &wakes, at).await;
    let early = writer.next().now_or_never();

    release.send(()).unwrap();
    assert!(
        early.is_none(),
        "reply before the old adapter drained: {early:?}"
    );
    assert_eq!(common::answer(&mut writer).await, "Accepted");
    wakes.wake_at(at + Rate::Sixty.period()).unwrap();
    assert_eq!(first_slot(), 91);
    wakes.run();
    common::send(&mut writer, &json!("Stop")).await;

    let state_run = &pending["state"]["outputs"][0]["universes"]["first"];
    let project_run = &pending["project"]["outputs"][0]["Kind"]["universes"]["first"];
    assert_eq!(
        (state_run, project_run),
        (&json!(1), &json!(1)),
        "picture during drain, before the writer gets Accepted"
    );
}
