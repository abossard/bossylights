//! M3 T5 (#169) end to end over the API: launch on a beat or bar, read on a fake DMX USB
//! Pro. One Generic RGB PAR at 1:1 plays red; looks start green or blue on the grid. The
//! production tick runs on a clock the test sets wake by wake (`Wakes::wake_at`), so a
//! tap or a request is sent at an exact time: at the time of the last wake.
//!
//! ```text
//! 120 BPM --tap 0, tap 750--> 80 BPM, 1.5 beats --green on a beat--> green at 1500
//!         --release at 1500--> 120 BPM --blue on a beat--> blue at 2000
//! ```

mod common;

use common::{Heard, Socket, answer, command, connect, open, send};
use engine::{Device, Engine, Event, Levels, Open, Outcome, Universes, Wakes};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task::block_in_place;

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];

/// One desk at a time: frames on four fake adapters at once miss the wire under load.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A served engine on a stepped clock with the PAR, the fake adapter at 60 Hz, and
/// looks 1 red, 2 green, 3 blue on the PAR. Times are in ms from `zero`.
struct Desk {
    _turn: tokio::sync::MutexGuard<'static, ()>,
    socket: Socket,
    wakes: Wakes,
    heard: Heard,
    stop: Arc<AtomicBool>,
    /// Frames handed to the adapter so far, and those of them dropped: its thread had
    /// no spare box.
    handed: usize,
    dropped: usize,
    zero: Duration,
}

/// The adapter, but its thread drops every other frame it is handed.
struct EveryOther {
    device: Box<dyn Device>,
    handed: u64,
}

impl Device for EveryOther {
    fn take(&mut self, frame: &[Levels]) -> Outcome {
        self.handed += 1;
        if self.handed.is_multiple_of(2) {
            Outcome::Dropped
        } else {
            self.device.take(frame)
        }
    }
}

impl Desk {
    async fn new() -> Self {
        Self::opened(|open| open).await
    }

    /// A desk whose adapter's thread drops every other frame.
    async fn dropping() -> Self {
        Self::opened(|mut open| -> Open {
            Box::new(move |kind| {
                open(kind)
                    .map(|device| Box::new(EveryOther { device, handed: 0 }) as Box<dyn Device>)
            })
        })
        .await
    }

    async fn opened(adapter: impl FnOnce(Open) -> Open) -> Self {
        let turn = ONE_AT_A_TIME.lock().await;
        let heard: Heard = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let opens = adapter(open(Some(Arc::clone(&heard)), Arc::clone(&stop)));
        let (engine, sender, wakes) =
            Engine::start_stepped(Universes::new(1).unwrap(), opens).unwrap();
        let definitions = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixture-library/tests/definitions");
        let mut socket = connect(common::serve(engine, sender, Some(definitions)).await).await;
        let s = &mut socket;
        accepted(
            s,
            json!({"SetOutput": {
                "output": 1, "kind": {"DmxUsbPro": {"adapter": 1}},
                "universes": {"first": 1, "count": 1}, "rate": "60",
            }}),
        )
        .await;
        let patch = json!({"Patch": {
            "name": "PAR", "maker": "Generic", "model": "Generic RGB",
            "mode": "RGB", "at": {"universe": 1, "address": 1},
        }});
        let started = Instant::now();
        while command(s, patch.clone()).await != json!({"Patched": {"fixture": 1}}) {
            assert!(started.elapsed() < Duration::from_secs(10), "no library");
        }
        for (look, colour) in [(1, RED), (2, GREEN), (3, BLUE)] {
            let [red, green, blue] = colour.map(|byte| u16::from(byte) * 257);
            accepted(
                s,
                json!({"PutStep": {"look": look, "name": format!("look {look}"), "step": {
                    "target": {"Fixture": {"fixture": 1}},
                    "values": {
                        "intensity": 65535,
                        "colour": {"red": red, "green": green, "blue": blue},
                    },
                }}}),
            )
            .await;
        }
        accepted(s, json!({"SetInternalTempo": {"bpm": 120}})).await;
        block_in_place(|| wakes.hold()).unwrap();
        let mut desk = Self {
            _turn: turn,
            socket,
            wakes,
            heard,
            stop,
            handed: 0,
            dropped: 0,
            zero: Duration::ZERO,
        };
        // every command so far applied, then time zero a second on
        let now = block_in_place(|| desk.wakes.wake()).unwrap();
        desk.zero = now + Duration::from_secs(1);
        desk.count();
        desk
    }

    fn time(&self, ms: u64) -> Duration {
        self.zero + Duration::from_millis(ms)
    }

    /// Counts the frames handed over and dropped since last asked. The frame due at the
    /// one handed over in the wake at `at`, if any, and the frames dropped.
    fn count_at(&mut self, at: Duration) -> (Option<Duration>, Vec<Duration>) {
        let (mut due_now, mut dropped) = (None, Vec::new());
        for event in self.wakes.events() {
            match event {
                Event::Handed {
                    at: handed, due, ..
                } => {
                    self.handed += 1;
                    if handed == at {
                        due_now = Some(due);
                    }
                }
                Event::Came {
                    outcome: Outcome::Dropped,
                    due,
                    ..
                } => {
                    self.dropped += 1;
                    dropped.push(due);
                }
                Event::Came { .. } => {}
            }
        }
        (due_now, dropped)
    }

    fn count(&mut self) {
        self.count_at(Duration::MAX);
    }

    /// Lets a wake through at `ms`. What the PAR shows in the frame it handed the
    /// adapter then, once it is on the wire; `None` if the adapter was not due or
    /// dropped it. A frame the adapter's thread dropped is counted by a later wake:
    /// until every frame is heard or dropped, wakes at `ms` again (nothing is due).
    async fn at(&mut self, ms: u64) -> Option<[u8; 3]> {
        let at = self.time(ms);
        assert_eq!(block_in_place(|| self.wakes.wake_at(at)).unwrap(), at);
        let (due, mut dropped) = self.count_at(at);
        let started = Instant::now();
        while self.heard.lock().unwrap().len() + self.dropped < self.handed {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "frames not on the wire"
            );
            tokio::time::sleep(Duration::from_millis(1)).await; // poll
            assert_eq!(block_in_place(|| self.wakes.wake_at(at)).unwrap(), at);
            dropped.extend(self.count_at(at).1);
        }
        let served = due.is_some_and(|due| !dropped.contains(&due));
        let last = self
            .heard
            .lock()
            .unwrap()
            .last()
            .map(|c| [c[0], c[1], c[2]]);
        served.then_some(last).flatten()
    }

    /// What the PAR shows at `ms`, in a frame handed over then.
    async fn shows(&mut self, ms: u64) -> [u8; 3] {
        let shown = self.at(ms).await;
        shown.unwrap_or_else(|| panic!("no frame at {ms} ms"))
    }

    /// The state of the picture painted last.
    async fn query(&mut self) -> Value {
        send(&mut self.socket, &json!("Query")).await;
        answer(&mut self.socket).await["Picture"]["state"].clone()
    }

    /// The picture at `ms`. The server paints the frame of the first wake after it
    /// asks for one: wakes at `ms` until a frame of one of them is painted.
    async fn picture(&mut self, ms: u64) -> Value {
        self.at(ms).await;
        let first = self.wakes.woke();
        let started = Instant::now();
        loop {
            let state = self.query().await;
            if state["frame"].as_u64().unwrap() >= first {
                return state;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "not painted: {state}"
            );
            self.at(ms).await;
        }
    }

    /// Sends `sent`: accepted.
    async fn send(&mut self, sent: Value) {
        accepted(&mut self.socket, sent).await;
    }

    /// Starts look `look` `when`: its play.
    async fn start(&mut self, look: u16, when: &str) -> Value {
        let sent = json!({"StartLook": {"look": look, "when": when}});
        let started = command(&mut self.socket, sent).await;
        let play = started["Started"]["play"].clone();
        assert!(play.is_object(), "look {look} {when}: {started}");
        play
    }

    async fn stop(mut self) {
        send(&mut self.socket, &json!("Stop")).await;
        self.wakes.run();
        self.stop.store(true, Ordering::Relaxed);
    }
}

async fn accepted(socket: &mut Socket, sent: Value) {
    assert_eq!(
        command(socket, sent.clone()).await,
        json!("Accepted"),
        "{sent}"
    );
}

/// The beats a play has run in a picture.
fn own_beat(state: &Value, look: u64) -> f64 {
    let play = state["plays"]
        .as_array()
        .unwrap()
        .iter()
        .find(|play| play["look"] == look)
        .unwrap_or_else(|| panic!("no play of look {look}: {state}"));
    state["clock"]["elapsed"].as_f64().unwrap() - play["start"].as_f64().unwrap()
}

fn close(seen: f64, want: f64) -> bool {
    (seen - want).abs() < 1e-6
}

#[tokio::test(flavor = "multi_thread")]
async fn taps_set_the_tempo_and_launches_land_on_the_tapped_and_the_fed_beat() {
    let mut desk = Desk::new().await;
    // a tap at 0 and red at once, both sent at 0
    desk.at(0).await;
    desk.start(1, "Now").await;
    desk.send(json!("TapTempo")).await;
    let state = desk.picture(0).await;
    assert_eq!(state["clock"]["bpm"], 120.0, "{state}");
    assert_eq!(state["clock"]["source"], "OneTap", "{state}");
    // the second tap at 750: 80 BPM, 1.5 beats on
    desk.at(750).await;
    desk.send(json!("TapTempo")).await;
    let state = desk.picture(750).await;
    assert_eq!(state["clock"]["bpm"], 80.0, "{state}");
    assert_eq!(state["clock"]["source"], "Tapped");
    assert!(close(own_beat(&state, 1), 1.5), "{state}");
    // green on the next beat, asked at 760: the tapped beat at 1500
    desk.at(760).await;
    desk.start(2, "Beat").await;
    assert_eq!(desk.shows(780).await, RED);
    assert_eq!(desk.shows(1480).await, RED);
    assert_eq!(desk.shows(1500).await, GREEN);
    // released at 1500: the feed's beat at 2000; blue asked at 1520
    desk.send(json!("ReleaseTempo")).await;
    let state = desk.picture(1520).await;
    assert_eq!(state["clock"]["bpm"], 120.0, "{state}");
    assert_eq!(state["clock"]["source"], "Feed");
    desk.start(3, "Beat").await;
    assert_eq!(desk.shows(1540).await, GREEN);
    assert_eq!(desk.shows(1980).await, GREEN);
    assert_eq!(desk.shows(2000).await, BLUE);
    desk.stop().await;
}

/// Beat `n` of the grid after `zero`, in ms: the grid was at `first` then, 500 ms a beat.
fn beat_at(first: u64, n: u64) -> u64 {
    (n - first) * 500
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bar_waits_two_crossings_in_one_wake_start_in_order_and_a_tap_moves_one_once() {
    let mut desk = Desk::new().await;
    // a tap at 0 puts a beat there; red at once
    desk.at(0).await;
    desk.start(1, "Now").await;
    desk.send(json!("TapTempo")).await;
    let first = desk.picture(0).await["clock"]["beat"].as_f64().unwrap() as u64;
    let bar = first.div_ceil(4) * 4 + 4; // a bar at least four beats on
    // a bar launch, asked off the grid: red through every beat before it
    let asked = beat_at(first, bar - 4) + 100;
    desk.at(asked).await;
    let blue = desk.start(3, "Bar").await;
    for beat in bar - 3..bar {
        assert_eq!(desk.shows(beat_at(first, beat)).await, RED, "beat {beat}");
    }
    assert_eq!(desk.shows(beat_at(first, bar)).await, BLUE, "bar {bar}");
    desk.send(json!({"ReleaseHold": {"play": blue}})).await;
    // blue on the next bar asked first, green on the next beat after: one late wake
    // crosses both; green started first, so blue, started later, wins
    let asked = beat_at(first, bar) + 100;
    desk.at(asked).await;
    let blue = desk.start(3, "Bar").await;
    let green = desk.start(2, "Beat").await;
    assert_eq!(desk.shows(beat_at(first, bar + 1) - 20).await, RED);
    assert_eq!(desk.shows(beat_at(first, bar + 4) + 20).await, BLUE);
    // green launched too: blue let go, green is on the wire
    desk.send(json!({"ReleaseHold": {"play": blue}})).await;
    assert_eq!(desk.shows(beat_at(first, bar + 4) + 40).await, GREEN);
    desk.send(json!({"ReleaseHold": {"play": green}})).await;
    // green on the next bar, at 4 bars on; a tap 200 ms past a beat moves the grid on
    // by 0.6 beats once: green comes 300 ms early, and red's own beat runs on
    let bar = bar + 4;
    let asked = beat_at(first, bar) + 100;
    desk.at(asked).await;
    let green = desk.start(2, "Bar").await;
    let tap = beat_at(first, bar + 2) + 200;
    let before = own_beat(&desk.picture(tap).await, 1);
    desk.send(json!("TapTempo")).await;
    let after = own_beat(&desk.picture(tap).await, 1);
    assert!(
        close(before, after),
        "red's beat jumped: {before} to {after}"
    );
    let moved = beat_at(first, bar + 4) - 300;
    assert_eq!(desk.shows(moved - 20).await, RED);
    assert_eq!(desk.shows(moved).await, GREEN);
    // once: at the bar it waited for, the same play from the same start
    let greens = |state: &Value| -> Vec<_> {
        state["plays"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|play| play["look"] == 2)
            .map(|play| {
                (
                    play["play"].clone(),
                    play["state"].clone(),
                    play["start"].clone(),
                )
            })
            .collect()
    };
    let launched = greens(&desk.picture(moved).await);
    let state = desk.picture(beat_at(first, bar + 4)).await;
    assert_eq!(greens(&state), launched, "{state}");
    assert_eq!(launched.first().map(|play| &play.0), Some(&green));
    assert_eq!(desk.shows(beat_at(first, bar + 4) + 20).await, GREEN);
    desk.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn two_taps_drained_in_one_wake_count_at_the_times_they_were_sent() {
    let mut desk = Desk::new().await;
    desk.at(0).await;
    desk.start(1, "Now").await;
    desk.send(json!("TapTempo")).await;
    // the second tap sent at 750, before any wake: one wake at 800 drains both
    desk.wakes.set_time(desk.time(750));
    desk.send(json!("TapTempo")).await;
    let state = desk.picture(800).await;
    assert_eq!(state["clock"]["bpm"], 80.0, "{state}");
    assert_eq!(state["clock"]["source"], "Tapped");
    // the beat the second tap marked was at 750: the next at 1500
    desk.start(2, "Beat").await;
    assert_eq!(desk.shows(1480).await, RED);
    assert_eq!(desk.shows(1500).await, GREEN);
    desk.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stop_cancels_a_waiting_play_and_frees_its_slot() {
    let mut desk = Desk::new().await;
    // red at once on a tapped beat at 0, and 31 more looks on the PAR
    desk.at(0).await;
    desk.start(1, "Now").await;
    desk.send(json!("TapTempo")).await;
    let waiting: Vec<u16> = (4..35).collect();
    for &look in &waiting {
        desk.send(
            json!({"PutStep": {"look": look, "name": format!("look {look}"), "step": {
                "target": {"Fixture": {"fixture": 1}},
                "values": {"intensity": 65535, "colour": {"red": 0, "green": 0, "blue": 65535}},
            }}}),
        )
        .await;
    }
    // each asked for the next bar, then stopped before it: every slot is taken now
    desk.at(100).await;
    for &look in &waiting {
        let play = desk.start(look, "Bar").await;
        desk.send(json!({"StopLook": {"play": play}})).await;
    }
    let full = command(
        &mut desk.socket,
        json!({"StartLook": {"look": 2, "when": "Now"}}),
    )
    .await;
    assert_eq!(
        full,
        json!({"Refused": {"reason": "PlayFull"}}),
        "before the wake"
    );
    // a wake cancels them: gone from the picture, their slots and looks free
    let state = desk.picture(120).await;
    let plays: Vec<_> = state["plays"]
        .as_array()
        .unwrap()
        .iter()
        .map(|play| play["look"].clone())
        .collect();
    assert_eq!(plays, [json!(1)], "{state}");
    desk.send(json!({"RemoveLook": {"look": 4}})).await;
    desk.start(2, "Now").await;
    assert_eq!(desk.shows(140).await, GREEN);
    // and none launches on the bar
    assert_eq!(desk.shows(2000).await, GREEN);
    desk.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_frame_the_adapter_drops_on_its_own_thread_shows_nothing() {
    let mut desk = Desk::dropping().await;
    desk.at(0).await;
    desk.start(1, "Now").await;
    // a frame due at each, 60 a second: every other one dropped, the others red
    let mut shown = Vec::new();
    for ms in [20, 40, 60, 80] {
        shown.push(desk.at(ms).await);
    }
    for pair in shown.windows(2) {
        assert_ne!(pair[0].is_some(), pair[1].is_some(), "{shown:?}");
    }
    assert!(
        shown.iter().flatten().all(|&shown| shown == RED),
        "{shown:?}"
    );
    desk.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_feed_edit_off_the_beat_launches_on_the_beat_worked_out_from_it() {
    let mut desk = Desk::new().await;
    // a tap at 0 puts a beat there; red at once
    desk.at(0).await;
    desk.start(1, "Now").await;
    desk.send(json!("TapTempo")).await;
    // 120 to 60 BPM at 100 ms: 0.2 beats, then 0.8 of a second: beat 1 at 900 ms
    desk.at(100).await;
    desk.send(json!({"SetInternalTempo": {"bpm": 60}})).await;
    desk.start(2, "Beat").await;
    assert_eq!(desk.shows(880).await, RED);
    assert_eq!(desk.shows(900).await, GREEN);
    desk.stop().await;
}
