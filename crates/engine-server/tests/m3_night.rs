//! A whole performance, heard on the adapter and DDP, then reopened and played again.
//! The second pixel shows where the sequence is while the first is held by hand.

mod common;

use common::{Heard, Socket, answer, command, connect, open, send};
use engine::{Engine, Event, Outcome, Universes, Wakes};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, UdpSocket};
use tokio::task::{JoinHandle, block_in_place};

type Rgb = [u8; 3];
const BLACK: Rgb = [0, 0, 0];
const GREEN: Rgb = [0, 255, 0];
const AMBER: Rgb = [255, 128, 0];
const RED: Rgb = [255, 0, 0];
const BLUE: Rgb = [0, 0, 255];
// DMX then DDP. Each deadline advances by its own integral-nanosecond period.
const PERIOD: [Duration; 2] = [
    Duration::from_nanos(1_000_000_000 / 44),
    Duration::from_nanos(1_000_000_000 / 60),
];

fn wide(rgb: Rgb) -> Value {
    let [red, green, blue] = rgb.map(|byte| u16::from(byte) * 257);
    json!({"red":red, "green":green, "blue":blue})
}

fn fixture() -> Value {
    json!({"Fixture":{"fixture":1}})
}

fn rules() -> Value {
    json!({"brightness":"Latest", "manual_end":"UntilStop", "other_end":"Reveal"})
}

struct Desk {
    client: Socket,
    served: JoinHandle<std::io::Result<Engine>>,
    stop: Arc<AtomicBool>,
    wakes: Wakes,
    heard: Heard,
    dmx_index: usize,
    udp: UdpSocket,
    next: [Duration; 2],
    launch: Duration,
}

impl Desk {
    async fn new(show: &Path) -> Self {
        let heard = Heard::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (engine, sender, wakes) =
            Engine::start_stepped(Universes::ONE, open(Some(heard.clone()), stop.clone())).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let served = tokio::spawn(engine_server::serve(
            listener,
            engine,
            sender,
            None,
            Some(show.into()),
        ));
        let client = connect(address).await;
        block_in_place(|| wakes.hold()).unwrap();
        let launch = block_in_place(|| wakes.wake()).unwrap() + Duration::from_secs(2);
        Self {
            client,
            served,
            stop,
            wakes,
            heard,
            dmx_index: 0,
            udp: UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            next: [Duration::MAX; 2],
            launch,
        }
    }

    async fn accepted(&mut self, cmd: Value) {
        assert_eq!(
            command(&mut self.client, cmd.clone()).await,
            json!("Accepted"),
            "{cmd}"
        );
    }

    async fn picture(&mut self) -> Value {
        send(&mut self.client, &json!("Query")).await;
        answer(&mut self.client).await["Picture"].clone()
    }

    async fn author(&mut self) {
        let patch = json!({"PatchStrip":{"name":"row","pixels":2,"color":"Rgb","at":{"universe":1,"address":1}}});
        assert_eq!(
            command(&mut self.client, patch).await,
            json!({"Patched":{"fixture":1}})
        );
        self.accepted(json!({"SetGroup":{"name":"row","members":[fixture()]}}))
            .await;
        self.accepted(json!({"PutStep":{"look":1,"name":"swatches","step":{
            "target":fixture(),"values":{},"swatches":[wide(RED), wide(BLUE)]
        }}}))
        .await;
        self.accepted(
            json!({"PutPattern":{"look":2,"name":"four beats","pattern":{"Wave":{
                "target":{"Group":{"group":"row"}},"period":4,"palette":{"Step":{"look":1}}
            }}}}),
        )
        .await;
        for (look, name, colour) in [(3, "green", GREEN), (4, "amber", AMBER)] {
            self.accepted(json!({"PutStep":{"look":look,"name":name,"step":{
                "target":fixture(),"values":{"colour":wide(colour)}
            }}}))
            .await;
        }
        self.accepted(json!({"PutSequence":{"look":5,"name":"show","sequence":{
            "items":[{"look":3},{"look":4}]
        }}}))
        .await;
        self.accepted(json!({"SetMixRules":{"rules":rules()}}))
            .await;
    }

    // The new client follows links from its own Query, including the palette's swatches.
    async fn discover(&mut self) -> (Value, Value) {
        let picture = self.picture().await;
        assert_eq!(picture["show"], "Kept");
        assert_eq!(picture["state"]["plays"], json!([]));
        assert_eq!(picture["state"]["clock"]["source"], "Feed");
        assert_eq!(picture["lights"], json!([]));
        assert_eq!(picture["universes"], json!([vec![0; 512]]));
        assert_eq!(picture["state"]["rules"], rules());
        let looks = picture["looks"].as_array().unwrap();
        let named = |name: &str| looks.iter().find(|row| row["name"] == name).unwrap();
        let palette = named("swatches");
        assert_eq!(palette["step"]["swatches"], json!([wide(RED), wide(BLUE)]));
        let wave = named("four beats");
        assert_eq!(
            wave["pattern"]["Wave"],
            json!({
                "target":{"Group":{"group":"row"}}, "period":4, "palette":{"Step":{"look":palette["look"]}}
            })
        );
        let sequence = named("show");
        let items = sequence["sequence"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        for (item, (name, rgb)) in items.iter().zip([("green", GREEN), ("amber", AMBER)]) {
            let child = named(name);
            assert_eq!(item["look"], child["look"]);
            assert_eq!(item["width"], 16);
            assert_eq!(child["step"]["values"]["colour"], wide(rgb));
        }
        (wave["look"].clone(), sequence["look"].clone())
    }

    // DMX is due exactly on the once end. DDP is due 8 ms later, once the Wave has
    // moved again. Set both phases by attaching at their preceding deadlines.
    async fn attach(&mut self) {
        let kinds = [
            (
                json!({"DmxUsbPro":{"adapter":1}}),
                "44",
                self.launch + Duration::from_secs(1) - PERIOD[0] * 45,
            ),
            (
                json!({"Ddp":{"address":self.udp.local_addr().unwrap().to_string(),"color":"Rgb"}}),
                "60",
                self.launch + Duration::from_millis(1008) - PERIOD[1] * 61,
            ),
        ];
        for (slot, (kind, rate, at)) in kinds.into_iter().enumerate() {
            self.accepted(json!({"SetOutput":{"output":slot+1,"kind":kind,
                "universes":{"first":1,"count":1},"rate":rate}}))
                .await;
            self.next[slot] = at;
            assert_eq!(block_in_place(|| self.wakes.wake_at(at)).unwrap(), at);
            self.check(at, [BLACK; 2]).await;
        }
    }

    // Compare the handoffs against both deadlines, and read the next complete wire frame
    // for every handoff. Skipping a stale leading packet would hide a one-frame regression.
    async fn check(&mut self, at: Duration, pixels: [Rgb; 2]) {
        let mut handed = [false; 2];
        for event in self.wakes.events() {
            match event {
                Event::Handed {
                    output,
                    due,
                    at: stamp,
                } => {
                    assert_eq!((due, stamp), (self.next[output.slot()], at));
                    assert!(!std::mem::replace(&mut handed[output.slot()], true));
                }
                Event::Came { outcome, .. } => assert_eq!(outcome, Outcome::Accepted),
            }
        }
        // The scheduler coalesces deadlines within its one-millisecond spin window.
        assert_eq!(
            handed,
            self.next.map(|due| due <= at + Duration::from_millis(1)),
            "handoffs at {at:?}"
        );
        for (slot, due) in self.next.iter_mut().enumerate() {
            if handed[slot] {
                *due += PERIOD[slot];
            }
        }
        let bytes = pixels.concat();
        if handed[0] {
            let waited = Instant::now();
            loop {
                if let Some(frame) = self.heard.lock().unwrap().get(self.dmx_index) {
                    assert_eq!(&frame[..6], &bytes, "DMX at {at:?}");
                    self.dmx_index += 1;
                    break;
                }
                assert!(
                    waited.elapsed() < Duration::from_secs(5),
                    "no DMX at {at:?}"
                );
                tokio::time::sleep(Duration::from_millis(1)).await; // poll
            }
        }
        if handed[1] {
            let mut packet = [0; 1500];
            let count = tokio::time::timeout(Duration::from_secs(5), self.udp.recv(&mut packet))
                .await
                .expect("DDP frame")
                .unwrap();
            // One universe of whole RGB pixels, from offset zero, with PUSH.
            assert_eq!(count, 10 + 510);
            assert_eq!(packet[0] & 1, 1);
            assert_eq!(packet[2..10], [0x0b, 1, 0, 0, 0, 0, 1, 254]);
            assert_eq!(&packet[10..16], &bytes, "DDP at {at:?}");
        }
    }

    async fn next_frame(&mut self, pixels: [Rgb; 2]) -> Duration {
        let at = block_in_place(|| self.wakes.wake()).unwrap();
        assert_eq!(at, self.next[0].min(self.next[1]));
        self.check(at, pixels).await;
        at
    }

    async fn before(&mut self, edge: Duration, pixels: [Rgb; 2]) {
        while self.next[0].min(self.next[1]) < edge {
            self.next_frame(pixels).await;
        }
    }

    async fn perform(&mut self, wave: Value, sequence: Value) {
        self.attach().await;
        let full = json!({"SetIntensity":{"target":fixture(),"intensity":65535}});
        assert!(command(&mut self.client, full).await["Grabbed"]["grab"].is_object());
        self.wakes.set_time(self.launch);
        for (look, rank, repeat) in [(wave, "Autopilot", "Loop"), (sequence, "Show", "Once")] {
            let start = json!({"StartLook":{"look":look,"rank":rank,"use":{"repeat":repeat}}});
            assert!(command(&mut self.client, start).await["Started"]["play"].is_object());
        }
        block_in_place(|| self.wakes.wake_at(self.launch)).unwrap();
        self.check(self.launch, [GREEN; 2]).await;

        let grab_at = self.launch + Duration::from_millis(125);
        let beat_one = self.launch + Duration::from_millis(500);
        let release_at = self.launch + Duration::from_millis(625);
        let beat_two = self.launch + Duration::from_secs(1);
        self.before(grab_at, [GREEN; 2]).await;
        self.wakes.set_time(grab_at);
        let red =
            json!({"SetProperty":{"target":{"Light":{"light":1}},"value":{"Colour":wide(RED)}}});
        let reply = command(&mut self.client, red).await;
        let grab = &reply["Grabbed"]["grab"];
        assert!(grab.is_object(), "{reply}");

        self.before(beat_one, [RED, GREEN]).await;
        // The 44 Hz period truncates to integer nanoseconds. Sample exactly on the
        // item edge, 16 ns before DMX is due, within its coalescing window.
        assert_eq!(
            block_in_place(|| self.wakes.wake_at(beat_one)).unwrap(),
            beat_one
        );
        self.check(beat_one, [RED, AMBER]).await;
        self.before(release_at, [RED, AMBER]).await;
        self.wakes.set_time(release_at);
        self.accepted(json!({"EndGrab":{"grab":grab}})).await;
        self.before(beat_two, [AMBER; 2]).await;
        assert_eq!(self.next_frame([BLUE; 2]).await, beat_two);
        // DDP was still amber on its preceding frame. Eight milliseconds later than
        // DMX, its own first eligible frame has advanced 0.8% from blue to red.
        assert_eq!(
            self.next_frame([[2, 0, 253]; 2]).await,
            beat_two + Duration::from_millis(8)
        );
    }

    // Take a picture of the live desk the new client will reopen. Channel 7 is
    // outside the two RGB pixels and makes a surviving raw hold visible on restart.
    async fn leave_live(&mut self) {
        self.accepted(json!({"SetChannel":{"channel":{"universe":1,"address":7},"value":91}}))
            .await;
        let at = self.launch + Duration::from_millis(1008);
        let waited = Instant::now();
        loop {
            block_in_place(|| self.wakes.wake_at(at)).unwrap();
            let picture = self.picture().await;
            if picture["universes"][0][6] == 91 {
                assert_eq!(picture["state"]["clock"]["source"], "Tapped");
                let plays = picture["state"]["plays"].as_array().unwrap();
                assert_eq!(plays.len(), 1);
                assert_eq!(plays[0]["rank"], "Autopilot");
                assert_eq!(plays[0]["look"], 2);
                assert_eq!(picture["lights"][0]["intensity"], 65535);
                return;
            }
            assert!(waited.elapsed() < Duration::from_secs(5), "no live picture");
            tokio::time::sleep(engine::BEAT).await; // poll
        }
    }

    async fn close(mut self) {
        send(&mut self.client, &json!("Stop")).await;
        assert_eq!(answer(&mut self.client).await, "Stopping");
        self.wakes.run();
        self.served.await.unwrap().unwrap();
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_night_and_its_replay_reach_each_outputs_first_due_frame() {
    let file = std::env::temp_dir().join(format!("bossylights-night-{}.json", std::process::id()));
    let mut desk = Desk::new(&file).await;
    desk.author().await;
    // Stop with the tapped clock, an Autopilot play and full intensity still live.
    for before in [1500, 1000] {
        let at = desk.launch - Duration::from_millis(before);
        desk.wakes.set_time(at);
        desk.accepted(json!("TapTempo")).await;
        block_in_place(|| desk.wakes.wake_at(at)).unwrap();
    }
    desk.perform(json!(2), json!(5)).await;
    desk.leave_live().await;
    desk.close().await;

    let mut reopened = Desk::new(&file).await;
    let (wave, sequence) = reopened.discover().await;
    reopened
        .accepted(json!({"SetInternalTempo":{"bpm":120}}))
        .await;
    reopened.perform(wave, sequence).await;
    reopened.close().await;
    std::fs::remove_file(file).unwrap();
}
