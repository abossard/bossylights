//! #197: engine health on the wire, checked against real DMX. The tick runs on a clock
//! the test steps one 30 Hz wake at a time (`engine/src/wake.rs`); a fake DMX USB Pro
//! writes each frame to a fake serial port the test decodes, and fails while told to.
//! The reader samples in real time, so the test waits for it after every wake (engine
//! time moves 33 ms at most between two reads during window measurements).
//!
//! ```text
//! attach --4 s--> first window E --4 s--> E + 4 s: RunningLate (two bad windows, apart)
//! on time --> clean window: Recovering --> next clean window: Recovered(WithinTargets)
//! device fails: OutputLost --> device takes frames again: Recovered(OutputAvailable)
//! ```

use crate::wire_common as common;

use common::{Heard, Socket, command, connect, send, serve};
use engine::{Adapter, Device, Engine, Event, Open, Outcome, OutputKind, Universes, Wakes};
use futures_util::StreamExt;
use outputs::dmx_usb_pro::DmxUsbPro;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::task::block_in_place;

const PERIOD: Duration = Duration::from_nanos(1_000_000_000 / 30);
const GRID: Duration = Duration::from_millis(500);
const WINDOW: Duration = Duration::from_secs(4);
const LATE: Duration = Duration::from_millis(2);
const ON_TIME: Duration = Duration::ZERO;
const LEVEL: u8 = 91;
/// How long the test waits for the engine or the reader.
const GIVE_UP: Duration = Duration::from_secs(10);

/// What every fake adapter did, by output slot, and whether they all fail now.
#[derive(Default)]
struct Devices {
    fail: AtomicBool,
    fail_only: AtomicUsize,
    took: [AtomicUsize; 16],
    accepted: [AtomicUsize; 16],
}

/// A fake DMX USB Pro for output `slot + 1`: adapter 1 writes to the fake port the test
/// reads, the others to nothing. Fails every frame while `fail` is set.
struct Flaky {
    port: Option<DmxUsbPro>,
    slot: usize,
    devices: Arc<Devices>,
}

impl Device for Flaky {
    fn take(&mut self, frame: &[u8]) -> Outcome {
        let fails = self.devices.fail.load(Ordering::SeqCst)
            || self.devices.fail_only.load(Ordering::SeqCst) == self.slot + 1;
        let outcome = if fails {
            Outcome::Failed
        } else {
            self.port
                .as_mut()
                .map_or(Outcome::Accepted, |port| port.take(frame))
        };
        if outcome == Outcome::Accepted {
            self.devices.accepted[self.slot].fetch_add(1, Ordering::SeqCst);
        }
        self.devices.took[self.slot].fetch_add(1, Ordering::SeqCst);
        outcome
    }
}

fn open(heard: Heard, stop: Arc<AtomicBool>, devices: Arc<Devices>) -> Open {
    Box::new(move |kind| {
        let OutputKind::DmxUsbPro { adapter } = kind else {
            return outputs::open(kind);
        };
        let port = (adapter == Adapter::FIRST)
            .then(|| DmxUsbPro::open(common::find(Some(heard.clone()), Arc::clone(&stop))))
            .transpose()?;
        Ok(Box::new(Flaky {
            port,
            slot: usize::from(adapter.number() - 1),
            devices: Arc::clone(&devices),
        }) as Box<dyn Device>)
    })
}

fn floor_grid(at: Duration) -> Duration {
    let grid = GRID.as_nanos();
    Duration::from_nanos(u64::try_from(at.as_nanos() / grid * grid).unwrap())
}

fn ceil_grid(at: Duration) -> Duration {
    let floor = floor_grid(at);
    if floor == at { at } else { floor + GRID }
}

fn ms(at: Duration) -> u64 {
    u64::try_from(at.as_millis()).unwrap()
}

/// The first row of a `Health` reply.
fn row(health: &Value) -> &Value {
    &health["Health"]["health"]["outputs"][0]
}

fn end_ms(health: &Value) -> Option<u64> {
    row(health)["measurement"]["Window"]["end_ms"].as_u64()
}

fn status(state: &Value, output: usize) -> &Value {
    &state["outputs"][output]["status"]
}

fn reconnecting(status: &Value) -> bool {
    status.get("Reconnecting").is_some()
}

/// A subscribed connection: the newest frame it was shown and every `Health` it got.
struct Watcher {
    socket: Socket,
    frame: u64,
    state: Value,
    pictures: usize,
    healths: Vec<Value>,
}

impl Watcher {
    /// Subscribes `socket`. Its first `Picture`.
    async fn subscribe(mut socket: Socket) -> (Self, Value) {
        send(&mut socket, &json!("Subscribe")).await;
        let mut watcher = Self {
            socket,
            frame: 0,
            state: Value::Null,
            pictures: 0,
            healths: vec![],
        };
        loop {
            let reply = watcher.next().await;
            if reply.get("Picture").is_some() {
                return (watcher, reply);
            }
        }
    }

    async fn next(&mut self) -> Value {
        self.within(GIVE_UP).await.expect("a message within 10 s")
    }

    /// The next message, if one comes within `limit`.
    async fn within(&mut self, limit: Duration) -> Option<Value> {
        let message = tokio::time::timeout(limit, self.socket.next())
            .await
            .ok()?
            .unwrap()
            .unwrap();
        let reply: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        if let Some(picture) = reply.get("Picture") {
            self.pictures += 1;
            self.frame = picture["state"]["frame"].as_u64().unwrap();
            self.state = picture["state"].clone();
        } else if let Some(changed) = reply.get("Changed") {
            self.frame = changed["state"]["frame"].as_u64().unwrap();
            self.state = changed["state"].clone();
        } else if reply.get("Health").is_some() {
            self.healths.push(reply.clone());
        }
        Some(reply)
    }

    /// Reads until `done`, at most [`GIVE_UP`].
    async fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let started = Instant::now();
        while !done(self) {
            let left = GIVE_UP.saturating_sub(started.elapsed());
            if self.within(left).await.is_none() {
                panic!(
                    "{what}: frame {}, last Health {:?}",
                    self.frame,
                    self.healths.last()
                );
            }
        }
    }

    fn last(&self) -> &Value {
        self.healths.last().expect("a Health")
    }

    /// The events of every `Health` from the `from`th on, in order.
    fn events(&self, from: usize) -> Vec<Value> {
        self.healths[from..]
            .iter()
            .flat_map(|health| health["Health"]["events"].as_array().unwrap().clone())
            .collect()
    }

    fn missed(&self) -> u64 {
        self.healths
            .iter()
            .map(|health| health["Health"]["missed"].as_u64().unwrap())
            .sum()
    }

    /// A round trip on this socket also reads any earlier subscription messages.
    async fn query(&mut self) -> Value {
        send(&mut self.socket, &json!("Query")).await;
        loop {
            let reply = self.next().await;
            if reply.get("Picture").is_some() {
                return reply;
            }
        }
    }
}

/// A served engine on a stepped clock with fake USB Pro outputs at 30 Hz.
struct Rig {
    address: SocketAddr,
    wakes: Wakes,
    /// Sends the commands; never subscribed.
    desk: Socket,
    watcher: Watcher,
    devices: Arc<Devices>,
    heard: Heard,
    stop: Arc<AtomicBool>,
    outputs: usize,
    /// Each output's due hand-overs, `(due, at)`.
    handed: Vec<Vec<(Duration, Duration)>>,
    /// The newest frame time, and the due time of output 1's last frame.
    now: Duration,
    due: Duration,
    /// A Query captures each covered grid window even when the socket coalesces replies.
    windows_from: Duration,
    windows: Vec<Value>,
}

impl Rig {
    /// Held before its first wake; the watcher subscribed. Its first `Picture`.
    async fn start() -> (Self, Value) {
        let heard = Heard::default();
        let stop = Arc::new(AtomicBool::new(false));
        let devices = Arc::new(Devices::default());
        let (engine, sender, wakes) = Engine::start_stepped(
            Universes::new(1).unwrap(),
            open(Arc::clone(&heard), Arc::clone(&stop), Arc::clone(&devices)),
        )
        .unwrap();
        let address = serve(engine, sender, None).await;
        let desk = connect(address).await;
        block_in_place(|| wakes.hold()).unwrap();
        let (watcher, picture) = Watcher::subscribe(connect(address).await).await;
        let rig = Self {
            address,
            wakes,
            desk,
            watcher,
            devices,
            heard,
            stop,
            outputs: 0,
            handed: vec![],
            now: Duration::ZERO,
            due: Duration::ZERO,
            windows_from: Duration::MAX,
            windows: vec![],
        };
        (rig, picture)
    }

    async fn level(&mut self, level: u8) {
        let set = json!({"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": level}});
        assert_eq!(command(&mut self.desk, set).await, json!("Accepted"));
    }

    /// Outputs 1 to `outputs` on adapters 1 to `outputs`, then wakes on time until each
    /// was handed its first frame.
    async fn attach(&mut self, outputs: usize) {
        for output in 1..=outputs {
            let set = json!({"SetOutput": {
                "output": output, "kind": {"DmxUsbPro": {"adapter": output}},
                "universes": {"first": 1, "count": 1}, "rate": "30",
            }});
            assert_eq!(command(&mut self.desk, set).await, json!("Accepted"));
        }
        self.outputs = outputs;
        self.handed = vec![vec![]; outputs];
        let started = Instant::now();
        while self.handed.iter().any(Vec::is_empty) {
            assert!(started.elapsed() < GIVE_UP, "no first frame"); // guard
            let at = block_in_place(|| self.wakes.wake()).unwrap();
            self.after(at).await;
            if self.windows_from == Duration::MAX && !self.watcher.state["outputs"][0].is_null() {
                // the reader's first sample of this output: its first window is 4 s on
                self.windows_from = ceil_grid(at + WINDOW);
            }
        }
        self.due = self.handed[0][0].0;
    }

    /// The next wake of output 1, `late` after its due time. Its frame time.
    async fn step(&mut self, late: Duration) -> Duration {
        self.due += PERIOD;
        let at = block_in_place(|| self.wakes.wake_at(self.due + late)).unwrap();
        assert_eq!(at, self.due + late);
        self.after(at).await;
        assert_eq!(self.handed[0].last(), Some(&(self.due, at)));
        at
    }

    async fn wake_at(&mut self, at: Duration) {
        let at = block_in_place(|| self.wakes.wake_at(at)).unwrap();
        self.after(at).await;
    }

    /// One snapshot can already be in flight at delivery. A second wake lets the
    /// reader sample the send time before the stepped clock moves again.
    async fn sample_delivery_time(&mut self) {
        self.wake_at(self.now).await;
        self.wake_at(self.now).await;
    }

    /// Finish the send and let this subscriber see the fresh sample before time advances.
    async fn sample_delivery_for(&mut self, subscriber: &mut Watcher) {
        subscriber.query().await;
        self.sample_delivery_time().await;
        subscriber
            .until("a fresh sample at the delivery time", |w| {
                w.frame == self.wakes.woke()
            })
            .await;
    }

    /// Wait for the DMX writes and the reader, then save any newly covered window.
    async fn after(&mut self, at: Duration) {
        let before = self.now;
        self.now = at;
        for event in self.wakes.events() {
            if let Event::Handed { output, due, at } = event {
                self.handed[output.slot()].push((due, at));
            }
        }
        self.drain().await;
        let woke = self.wakes.woke();
        self.watcher
            .until(&format!("the reader read wake {woke}"), |w| w.frame >= woke)
            .await;
        let endpoint = floor_grid(at);
        if endpoint > before && endpoint >= self.windows_from {
            let picture = self.watcher.query().await;
            self.windows.push(picture["Picture"]["health"].clone());
        }
    }

    fn window(&self, end: Duration) -> &Value {
        self.windows
            .iter()
            .find(|h| h["outputs"][0]["measurement"]["Window"]["end_ms"] == ms(end))
            .unwrap_or_else(|| panic!("no window ending at {end:?}"))
    }

    /// Waits until every adapter took each frame it was handed, and the fake port got
    /// each one adapter 1 accepted.
    async fn drain(&mut self) {
        let started = Instant::now();
        loop {
            let took = (0..self.outputs).all(|slot| {
                self.devices.took[slot].load(Ordering::SeqCst) == self.handed[slot].len()
            });
            let accepted = self.devices.accepted[0].load(Ordering::SeqCst);
            if took && self.heard.lock().unwrap().len() == accepted {
                return;
            }
            assert!(
                started.elapsed() < GIVE_UP, // guard
                "handed {:?}, on the wire {} of {accepted}",
                self.handed.iter().map(Vec::len).collect::<Vec<_>>(),
                self.heard.lock().unwrap().len()
            );
            tokio::time::sleep(Duration::from_millis(1)).await; // poll until the adapters drain
        }
    }

    fn fail(&self, fail: bool) {
        self.devices.fail.store(fail, Ordering::SeqCst);
    }

    /// Channel 1 of each frame on the wire.
    fn wire(&self) -> Vec<u8> {
        self.heard.lock().unwrap().iter().map(|c| c[0]).collect()
    }

    async fn stop(mut self) {
        send(&mut self.desk, &json!("Stop")).await;
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn frames_2_ms_late_warn_once_at_the_second_bad_window_then_recover_once_on_time() {
    let (mut rig, _) = Rig::start().await;
    rig.level(LEVEL).await;
    rig.attach(1).await;
    let first = rig.windows_from;
    let warned = first + WINDOW;
    while rig.now < warned + 2 * GRID {
        rig.step(LATE).await;
    }
    // the first window, and every one before the second bad one apart from it: no warning
    assert_eq!(rig.watcher.healths.iter().find_map(end_ms), Some(ms(first)));
    let mut end = first;
    while end < warned {
        assert_eq!(rig.window(end)["outputs"][0]["state"], "Measuring");
        end += GRID;
    }
    let health = rig.window(warned);
    let samples = &health["outputs"][0]["measurement"]["Window"]["lateness"]["samples"];
    assert!((119..=121).contains(&samples.as_u64().unwrap()), "{health}");
    assert_eq!(
        health["outputs"][0],
        json!({"output": 1, "generation": 1, "status": "Running",
            "state": {"Late": {"reason": "HandoffLateness"}},
            "measurement": {"Window": {"end_ms": ms(warned),
                "lateness": {"p99_us": 2000, "worst_us": 2000, "samples": samples},
                "press_to_light": null, "press_missing": 0, "dropped": 0, "failed": 0,
                "off_beat": 0, "frames_handed": samples}}})
    );
    assert_eq!(health["window_ms"], 4000);
    assert_eq!(rig.watcher.missed(), 0);
    let running_late = json!({"output": 1, "generation": 1, "at_ms": ms(warned),
        "event": {"RunningLate": {"reason": "HandoffLateness"}}});

    let warning = rig
        .watcher
        .healths
        .iter()
        .find(|reply| reply["Health"]["events"] == json!([running_late]))
        .expect("a Health carrying the warning");
    let delivered = Duration::from_millis(end_ms(warning).expect("a covered window"));
    assert_eq!(&warning["Health"]["health"], rig.window(delivered));
    assert_eq!(rig.watcher.events(0), vec![running_late]);
    assert_eq!(
        row(rig.watcher.last())["state"],
        json!({"Late": {"reason": "HandoffLateness"}})
    );

    // a late subscriber sees the warning in its row, and future events only
    let (mut late, picture) = Watcher::subscribe(connect(rig.address).await).await;
    assert_eq!(picture["Picture"]["health"]["window_ms"], 4000);
    assert_eq!(
        picture["Picture"]["health"]["outputs"][0]["state"],
        json!({"Late": {"reason": "HandoffLateness"}})
    );

    let seen = rig.watcher.healths.len();
    let on_time = rig.now;
    let recovered = |w: &Watcher| {
        w.events(0)
            .iter()
            .any(|e| e["event"].get("Recovered").is_some())
    };
    while !recovered(&rig.watcher) {
        assert!(rig.now < on_time + 3 * WINDOW, "no recovery");
        rig.step(ON_TIME).await;
    }
    let back = rig.now;
    while rig.now < back + 2 * GRID {
        rig.step(ON_TIME).await;
    }
    let events = rig.watcher.events(seen);
    assert_eq!(events.len(), 1, "{events:?}");
    let at = events[0]["at_ms"].as_u64().unwrap();
    assert_eq!(
        events[0],
        json!({"output": 1, "generation": 1, "at_ms": at,
            "event": {"Recovered": {"reason": "WithinTargets"}}})
    );
    let at = Duration::from_millis(at);
    assert_eq!(
        rig.window(at - GRID)["outputs"][0]["state"],
        json!({"Recovering": {"reason": "HandoffLateness"}})
    );
    assert_eq!(rig.window(at)["outputs"][0]["state"], json!("Healthy"));
    assert_eq!(row(rig.watcher.last())["state"], json!("Healthy"));

    late.until("the late subscriber hears the recovery", recovered)
        .await;
    assert_eq!(late.events(0), events, "the late subscriber's events");

    // every frame handed over reached the adapter, each with the level
    let wire = rig.wire();
    assert_eq!(wire.len(), rig.handed[0].len());
    assert!(wire.iter().all(|level| *level == LEVEL), "{wire:?}");
    rig.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_press_1_ms_before_a_deadline_served_2_ms_after_is_in_the_first_eligible_frame_and_its_window()
 {
    let (mut rig, _) = Rig::start().await;
    rig.attach(1).await;
    while rig.now < rig.windows_from {
        rig.step(ON_TIME).await;
    }
    assert_eq!(
        rig.window(rig.windows_from)["outputs"][0]["state"],
        "Healthy"
    );

    let deadline = rig.due + PERIOD;
    rig.wakes.set_time(deadline - Duration::from_millis(1));
    rig.level(LEVEL).await;
    let pressed = rig.handed[0].len();
    let at = rig.step(LATE).await;
    assert_eq!(rig.handed[0][pressed], (deadline, deadline + LATE));

    let window = |w: &Watcher| {
        w.healths
            .last()
            .is_some_and(|h| !row(h)["measurement"]["Window"]["press_to_light"].is_null())
    };
    while !window(&rig.watcher) {
        assert!(rig.now < at + 3 * GRID, "no window shows the press");
        rig.step(ON_TIME).await;
    }
    let health = rig.watcher.last();
    let measured = &row(health)["measurement"]["Window"];
    assert_eq!(
        measured["press_to_light"],
        json!({"p50_us": 3000, "p95_us": 3000, "worst_us": 3000, "samples": 1}),
        "{health}"
    );
    assert_eq!(measured["press_missing"], 0);
    assert_eq!(row(health)["state"], json!("Healthy"));
    assert_eq!(rig.watcher.events(0), Vec::<Value>::new());

    // the decoded DMX: the pressed level from the first eligible frame on, never before
    let wire = rig.wire();
    assert_eq!(wire.len(), rig.handed[0].len());
    let expected: Vec<u8> = (0..wire.len())
        .map(|frame| if frame < pressed { 0 } else { LEVEL })
        .collect();
    assert_eq!(wire, expected, "pressed in frame {pressed}");
    rig.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_that_fails_then_takes_frames_is_lost_once_then_back_once() {
    let (mut rig, _) = Rig::start().await;
    rig.level(LEVEL).await;
    rig.attach(1).await;
    while rig.watcher.state["outputs"][0]["status"] != json!("Running") {
        rig.step(ON_TIME).await;
    }

    rig.fail(true);
    while !reconnecting(status(&rig.watcher.state, 0)) {
        rig.step(ON_TIME).await;
    }
    let lost = rig.now;
    let lost_event =
        json!({"output": 1, "generation": 1, "at_ms": ms(lost), "event": "OutputLost"});
    while !rig.watcher.events(0).contains(&lost_event) {
        assert!(rig.now < lost + 2 * GRID, "OutputLost not delivered");
        rig.step(ON_TIME).await;
    }
    assert!(
        reconnecting(&row(rig.watcher.last())["status"]),
        "{}",
        rig.watcher.last()
    );

    rig.fail(false);
    while status(&rig.watcher.state, 0) != "Running" {
        rig.step(ON_TIME).await;
    }
    let back = rig.now;
    let back_event = json!({"output": 1, "generation": 1, "at_ms": ms(back),
        "event": {"Recovered": {"reason": "OutputAvailable"}}});
    while !rig.watcher.events(0).contains(&back_event) {
        assert!(rig.now < back + 2 * GRID, "Recovered not delivered");
        rig.step(ON_TIME).await;
    }
    assert_eq!(row(rig.watcher.last())["status"], "Running");
    assert_eq!(rig.watcher.events(0), vec![lost_event, back_event]);

    // the frames the adapter took are on the wire, each with the level
    let wire = rig.wire();
    assert_eq!(wire.len(), rig.devices.accepted[0].load(Ordering::SeqCst));
    assert!(wire.len() < rig.handed[0].len(), "no frame failed");
    assert!(wire.iter().all(|level| *level == LEVEL), "{wire:?}");
    rig.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_query_keeps_pending_events_and_health_waits_500_ms_of_engine_time() {
    let (mut rig, _) = Rig::start().await;
    rig.level(LEVEL).await;
    rig.attach(1).await;
    while status(&rig.watcher.state, 0) != "Running" {
        rig.step(ON_TIME).await;
    }

    let (mut sub, _) = Watcher::subscribe(connect(rig.address).await).await;
    let mut sent = rig.now;
    // Start at the subscription Picture, then at the first Health delivery.
    for previous in 0..2 {
        rig.sample_delivery_for(&mut sub).await;
        let due = sent + GRID;
        rig.fail(true);
        while !reconnecting(status(&rig.watcher.state, 0)) {
            assert!(rig.now < due, "no failed handoff");
            rig.wake_at(rig.now + PERIOD).await;
        }
        let lost = ms(rig.now);
        let picture = sub.query().await;
        assert!(reconnecting(
            &picture["Picture"]["health"]["outputs"][0]["status"]
        ));

        rig.fail(false);
        while status(&rig.watcher.state, 0) != "Running" {
            assert!(rig.now < due, "no successful handoff");
            rig.wake_at(rig.now + PERIOD).await;
        }
        let back = ms(rig.now);
        rig.wake_at(due - Duration::from_nanos(1)).await;
        sub.until("the last frame before delivery is due", |w| {
            w.frame == rig.wakes.woke()
        })
        .await;
        // Keep engine time fixed while more than a whole delivery interval passes on the host.
        tokio::time::sleep(Duration::from_secs(1)).await;
        let picture = sub.query().await;
        assert_eq!(
            picture["Picture"]["health"]["outputs"][0]["status"],
            "Running"
        );
        assert_eq!(sub.healths.len(), previous, "early Health at {:?}", rig.now);

        rig.wake_at(due).await;
        sub.until("the pending events at 500 ms", |w| {
            w.healths.len() > previous
        })
        .await;
        assert_eq!(
            sub.last()["Health"]["events"],
            json!([
                {"output": 1, "generation": 1, "at_ms": lost, "event": "OutputLost"},
                {"output": 1, "generation": 1, "at_ms": back, "event": {"Recovered": {"reason": "OutputAvailable"}}},
            ])
        );
        assert_eq!(sub.last()["Health"]["missed"], 0);
        sent = due;
    }
    assert_eq!(rig.wire().last(), Some(&LEVEL));
    rig.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn first_health_waits_after_a_subscribe_picture_with_an_old_snapshot() {
    let (mut rig, _) = Rig::start().await;
    rig.level(LEVEL).await;
    rig.attach(1).await;
    while status(&rig.watcher.state, 0) != "Running" {
        rig.step(ON_TIME).await;
    }

    // Advance the engine clock without waking the tick. The subscription gets
    // the previous frame, even though delivery happens a quarter second later.
    let delivered = rig.now + GRID / 2;
    rig.wakes.set_time(delivered);
    let (mut sub, _) = Watcher::subscribe(connect(rig.address).await).await;
    assert_eq!(sub.frame, rig.watcher.frame);

    rig.fail(true);
    rig.wake_at(delivered).await;
    rig.sample_delivery_for(&mut sub).await;
    assert!(reconnecting(status(&rig.watcher.state, 0)));
    assert_eq!(rig.wire().last(), Some(&LEVEL));

    let due = delivered + GRID;
    rig.wake_at(due - Duration::from_nanos(1)).await;
    sub.until("the last frame before Health is due", |w| {
        w.frame == rig.wakes.woke()
    })
    .await;
    sub.query().await;
    assert!(sub.healths.is_empty(), "early Health: {:?}", sub.healths);

    rig.wake_at(due).await;
    sub.until("the loss at 500 ms after the subscription Picture", |w| {
        !w.healths.is_empty()
    })
    .await;
    assert_eq!(
        sub.events(0),
        [json!({"output": 1, "generation": 1, "at_ms": ms(delivered), "event": "OutputLost"})]
    );
    rig.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unchanged_health_revision_is_not_resent_while_frames_keep_arriving() {
    let (mut rig, _) = Rig::start().await;
    rig.level(LEVEL).await;
    rig.attach(1).await;
    while status(&rig.watcher.state, 0) != "Running" {
        rig.step(ON_TIME).await;
    }

    let (mut sub, picture) = Watcher::subscribe(connect(rig.address).await).await;
    let initial = &picture["Picture"]["health"];
    assert_eq!(initial["outputs"][0]["measurement"], "Measuring");
    // Finish the subscription send before advancing the clock.
    assert_eq!(sub.query().await["Picture"]["health"], *initial);

    let until = rig.now + 2 * GRID;
    while rig.now < until {
        rig.step(ON_TIME).await;
    }
    sub.until("the last Changed frame", |w| w.frame == rig.wakes.woke())
        .await;
    assert_eq!(sub.query().await["Picture"]["health"], *initial);
    let wire = rig.wire();
    assert_eq!(wire, vec![LEVEL; rig.handed[0].len()]);
    assert!(
        sub.healths.is_empty(),
        "unchanged Health: {:?}",
        sub.healths
    );
    rig.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_loss_at_the_last_grid_time_resets_the_timing_state() {
    let (mut rig, _) = Rig::start().await;
    rig.level(LEVEL).await;
    rig.attach(1).await;
    let end = rig.windows_from;
    while rig.due + 2 * PERIOD <= end {
        rig.step(ON_TIME).await;
    }

    // The failed frame is handed at the endpoint, and the next wake reads its result
    // at the same engine time. The first read still has the preceding accepted frame.
    rig.fail(true);
    rig.wake_at(end).await;
    assert_eq!(rig.window(end)["outputs"][0]["state"], "Healthy");
    assert_eq!(rig.wire().last(), Some(&LEVEL));

    let started = Instant::now();
    while !reconnecting(status(&rig.watcher.state, 0)) {
        assert!(started.elapsed() < GIVE_UP, "no failed frame result"); // guard
        rig.wake_at(end).await;
    }
    let loss = json!({"output": 1, "generation": 1, "at_ms": ms(end), "event": "OutputLost"});
    let picture = rig.watcher.query().await;
    let current = &picture["Picture"]["health"]["outputs"][0];
    assert!(reconnecting(&current["status"]));
    assert_eq!(current["state"], "Measuring", "{current}");
    rig.sample_delivery_time().await;
    rig.wake_at(end + GRID).await;
    rig.watcher
        .until("the loss at the same grid point", |w| {
            w.events(0).contains(&loss)
        })
        .await;
    let current = row(rig.watcher.last());
    assert!(reconnecting(&current["status"]));
    assert_eq!(current["state"], "Measuring", "{current}");
    rig.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unavailable_repeat_cannot_confirm_lateness_on_the_wire() {
    let (mut rig, _) = Rig::start().await;
    rig.level(LEVEL).await;
    rig.attach(2).await;
    let first = rig.windows_from;
    while rig.due + 2 * PERIOD + LATE <= first {
        rig.step(LATE).await;
    }

    // Both outputs have bad timing, but only output 1 loses its device at the first
    // endpoint. The reader sees the failure on a second wake at that exact time.
    rig.devices.fail_only.store(1, Ordering::SeqCst);
    rig.wake_at(first).await;
    assert_eq!(rig.window(first)["outputs"][0]["status"], "Running");
    let started = Instant::now();
    while !reconnecting(status(&rig.watcher.state, 0)) {
        assert!(started.elapsed() < GIVE_UP, "no failed frame result"); // guard
        rig.wake_at(first).await;
    }
    let picture = rig.watcher.query().await;
    assert!(reconnecting(
        &picture["Picture"]["health"]["outputs"][0]["status"]
    ));
    rig.devices.fail_only.store(0, Ordering::SeqCst);
    rig.due = rig.handed[0].last().unwrap().0;

    let confirm = first + WINDOW;
    while rig.now < confirm + 2 * GRID {
        rig.step(LATE).await;
    }
    let wire = rig.wire();
    assert_eq!(wire.len() + 1, rig.handed[0].len());
    assert!(wire.iter().all(|level| *level == LEVEL));
    let warnings: Vec<_> = rig
        .watcher
        .events(0)
        .into_iter()
        .filter(|e| e["at_ms"] == ms(confirm) && e["event"].get("RunningLate").is_some())
        .collect();
    assert_eq!(
        warnings,
        [json!({"output": 2, "generation": 1, "at_ms": ms(confirm),
            "event": {"RunningLate": {"reason": "HandoffLateness"}}})]
    );
    rig.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_return_after_the_grid_does_not_warn_at_the_unavailable_endpoint() {
    let (mut rig, _) = Rig::start().await;
    rig.level(LEVEL).await;
    rig.attach(2).await;
    let end = rig.windows_from + WINDOW;
    while rig.due + 3 * PERIOD + LATE < end - 2 * GRID {
        rig.step(LATE).await;
    }
    rig.devices.fail_only.store(1, Ordering::SeqCst);
    while rig.due + 2 * PERIOD + LATE < end {
        rig.step(LATE).await;
    }
    assert!(reconnecting(status(&rig.watcher.state, 0)));

    // This success reaches the reader only on the first wake after the endpoint.
    rig.devices.fail_only.store(0, Ordering::SeqCst);
    assert!(rig.step(LATE).await < end);
    assert!(reconnecting(status(&rig.watcher.state, 0)));
    assert_eq!(rig.wire().last(), Some(&LEVEL));
    let after = end + Duration::from_millis(1);
    let (mut subscriber, _) = Watcher::subscribe(connect(rig.address).await).await;
    rig.wake_at(after).await;
    assert_eq!(status(&rig.watcher.state, 0), "Running");

    assert_eq!(rig.window(end)["outputs"][0]["state"], "Measuring");
    rig.sample_delivery_for(&mut subscriber).await;
    rig.wake_at(after + GRID).await;
    subscriber
        .until("warning and return", |w| !w.healths.is_empty())
        .await;
    let expected = json!([
        {"output": 2, "generation": 1, "at_ms": ms(end),
            "event": {"RunningLate": {"reason": "HandoffLateness"}}},
        {"output": 1, "generation": 1, "at_ms": ms(after),
            "event": {"Recovered": {"reason": "OutputAvailable"}}}
    ]);
    assert_eq!(subscriber.last()["Health"]["events"], expected);
    rig.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_engine_has_no_health_and_a_slow_socket_counts_what_it_missed_and_stays_open() {
    assert_eq!(wire::API_VERSION, 12);
    let (mut rig, picture) = Rig::start().await;
    let empty = json!({"window_ms": 4000, "outputs": []});
    assert_eq!(picture["Picture"]["health"], empty);
    send(&mut rig.desk, &json!("Query")).await;
    assert_eq!(
        common::answer(&mut rig.desk).await["Picture"]["health"],
        empty
    );

    rig.level(LEVEL).await;
    rig.attach(16).await;
    let running = |w: &Watcher| (0..16).all(|n| status(&w.state, n) == "Running");
    while !running(&rig.watcher) {
        rig.step(ON_TIME).await;
    }
    let (mut slow, picture) = Watcher::subscribe(connect(rig.address).await).await;
    assert_eq!(
        picture["Picture"]["health"]["outputs"][15],
        json!({"output": 16, "generation": 1, "status": "Running", "state": "Measuring",
            "measurement": "Measuring"})
    );

    // 10 losses and returns on 16 outputs: 320 events, faster than one Health per 500 ms
    let mut expected = Vec::new();
    let mut lost = [false; 16];
    for _ in 0..10 {
        for (fail, event) in [
            (true, json!("OutputLost")),
            (false, json!({"Recovered": {"reason": "OutputAvailable"}})),
        ] {
            rig.fail(fail);
            while lost != [fail; 16] {
                rig.step(ON_TIME).await;
                for (slot, was_lost) in lost.iter_mut().enumerate() {
                    let now_lost = reconnecting(status(&rig.watcher.state, slot));
                    if now_lost != *was_lost {
                        expected.push(json!({"output": slot + 1, "generation": 1,
                            "at_ms": ms(rig.now), "event": event}));
                        *was_lost = now_lost;
                    }
                }
            }
        }
    }
    rig.sample_delivery_for(&mut slow).await;
    rig.wake_at(rig.now + GRID).await;
    slow.until("every event heard or counted missed", |w| {
        w.events(0).len() as u64 + w.missed() >= expected.len() as u64
    })
    .await;
    assert!(slow.missed() > 0, "the log must overflow");
    let mut pending = expected.as_slice();
    for reply in &slow.healths {
        let health = &reply["Health"];
        let missed = usize::try_from(health["missed"].as_u64().unwrap()).unwrap();
        let events = health["events"].as_array().unwrap();
        if missed > 0 {
            assert_eq!(events.len(), 64, "the log retains the newest 64 events");
        }
        let (delivered, rest) = pending[missed..].split_at(events.len());
        assert_eq!(events, delivered, "events since the last delivery");
        pending = rest;
    }
    assert!(pending.is_empty());

    // the slow socket is still open
    let pictures = slow.pictures;
    send(&mut slow.socket, &json!("Query")).await;
    slow.until("a Picture after Query", |w| w.pictures > pictures)
        .await;

    let wire = rig.wire();
    assert_eq!(wire.len(), rig.devices.accepted[0].load(Ordering::SeqCst));
    assert!(wire.iter().all(|level| *level == LEVEL), "{wire:?}");
    rig.stop().await;
}
