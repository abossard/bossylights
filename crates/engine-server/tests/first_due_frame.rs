//! M3 T1 (#165), the first due frame. The production tick runs on a clock the test steps
//! one wake at a time (`engine/src/wake.rs`). Raw level 91 is sent through `/api` before
//! a wake's cutoff, or after it while the tick is held there. Each due hand-over, what came
//! of it, every DDP frame and every complete message of a fake DMX USB Pro are recorded
//! in order. Each output is handed every slot of its schedule (its first due, then a
//! period apart, none skipped); its first slot at or after the wake that admitted 91 is
//! the first frame that carries it, and the outcome of every slot up to it is back.
//!
//! ```text
//! before: send 91 --> wake T (cutoff: 91 taken)        --> frames at T and later carry 91
//! after:  wake T, held at its cutoff --> send 91 --> on --> wake T' --> frames at T' and later
//! ```

mod common;

use common::{Heard, Socket, command, connect, open, send, serve};
use engine::{Engine, Event, Outcome, Rate, Universes, Wakes};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::task::block_in_place;

const LEVEL: u8 = 91;
/// Output 1 sends DDP, output 2 to the fake adapter: index 0 and 1 below.
const OUTPUTS: usize = 2;

#[derive(Clone, Copy, Debug)]
enum Side {
    /// Sent while the tick waits: in the next wake.
    Before,
    /// Sent while the tick is held at the cutoff of a wake: in the wake after it.
    After,
}

/// A due hand-over: when its frame was due, and the wake that handed it over.
#[derive(Clone, Copy, Debug)]
struct Handed {
    due: Duration,
    at: Duration,
}

/// What came of the frame due at `due`, and how many hand-overs its output had by then.
#[derive(Clone, Copy, Debug)]
struct Came {
    due: Duration,
    outcome: Outcome,
    handed: usize,
}

/// What the test saw, in order, per output (DDP, DMX).
struct Seen {
    udp: UdpSocket,
    heard: Heard,
    handed: [Vec<Handed>; OUTPUTS],
    /// In the order it came back.
    came: [Vec<Came>; OUTPUTS],
    /// Channel 1 of each DDP frame.
    ddp: Vec<u8>,
}

impl Seen {
    fn new(udp: UdpSocket, heard: Heard) -> Self {
        Self {
            udp,
            heard,
            handed: [vec![], vec![]],
            came: [vec![], vec![]],
            ddp: vec![],
        }
    }

    /// Channel 1 of each frame on the wire, per output.
    fn wire(&self) -> [Vec<u8>; OUTPUTS] {
        let dmx = self.heard.lock().unwrap().iter().map(|c| c[0]).collect();
        [self.ddp.clone(), dmx]
    }

    /// Takes the events of the wakes since the last call.
    fn take(&mut self, wakes: &mut Wakes) {
        for event in wakes.events() {
            match event {
                Event::Handed { output, due, at } => {
                    self.handed[output.slot()].push(Handed { due, at });
                }
                Event::Came {
                    output,
                    outcome,
                    due,
                } => {
                    let handed = self.handed[output.slot()].len();
                    self.came[output.slot()].push(Came {
                        due,
                        outcome,
                        handed,
                    });
                }
                Event::Counted { .. } => {}
            }
        }
    }

    /// What came of each frame of output `n` due by `until`, in order, once all are back.
    fn outcomes(&self, n: usize, until: Duration) -> Option<Vec<Outcome>> {
        self.handed[n]
            .iter()
            .filter(|handed| handed.due <= until)
            .map(|handed| {
                let came = self.came[n].iter().find(|came| came.due == handed.due);
                came.map(|came| came.outcome)
            })
            .collect()
    }

    /// Waits until every frame handed over so far is on the wire: each output drained.
    async fn drain(&mut self) {
        let started = Instant::now();
        let mut packet = vec![0; 1500];
        while self
            .wire()
            .iter()
            .map(Vec::len)
            .ne(self.handed.iter().map(Vec::len))
        {
            if let Ok(Ok(size)) =
                tokio::time::timeout(Duration::from_millis(20), self.udp.recv(&mut packet)).await
            {
                // one universe: one packet a frame, from offset 0, the push flag set
                assert_eq!(packet[0] & 0x01, 0x01, "{:?}", &packet[..10]);
                assert_eq!(packet[4..8], [0, 0, 0, 0]);
                assert!(size > 10);
                self.ddp.push(packet[10]);
            }
            let wire = self.wire();
            for (on_wire, handed) in wire.iter().zip(&self.handed) {
                assert!(on_wire.len() <= handed.len(), "more frames than hand-overs");
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "on the wire {:?}, handed over {:?}",
                wire.map(|w| w.len()),
                self.handed.each_ref().map(Vec::len)
            );
        }
    }

    /// Lets one wake through and waits until its frames are on the wire. Its frame time.
    async fn wake(&mut self, wakes: &mut Wakes) -> Duration {
        let at = block_in_place(|| wakes.wake()).unwrap();
        self.take(wakes);
        self.drain().await;
        at
    }
}

/// A served engine on a stepped clock, held before its first wake with outputs: DDP at
/// `ddp` frames a second to `udp`, the fake adapter at `dmx`, its port read into `heard`
/// or not at all.
async fn start(
    heard: Option<Heard>,
    stop: &Arc<AtomicBool>,
    udp: &UdpSocket,
    ddp: &str,
    dmx: &str,
) -> (Socket, Wakes) {
    let (engine, sender, wakes) =
        Engine::start_stepped(Universes::new(1).unwrap(), open(heard, Arc::clone(stop))).unwrap();
    let mut socket = connect(serve(engine, sender, None).await).await;
    block_in_place(|| wakes.hold()).unwrap();
    let address = udp.local_addr().unwrap().to_string();
    let kinds = [
        (1, json!({"Ddp": {"address": address, "color": "Rgb"}}), ddp),
        (2, json!({"DmxUsbPro": {"adapter": 1}}), dmx),
    ];
    for (output, kind, rate) in kinds {
        let set = json!({"SetOutput": {
            "output": output, "kind": kind, "universes": {"first": 1, "count": 1}, "rate": rate,
        }});
        assert_eq!(command(&mut socket, set).await, json!("Accepted"));
    }
    (socket, wakes)
}

fn period(rate: &str) -> Duration {
    serde_json::from_value::<Rate>(json!(rate))
        .unwrap()
        .period()
}

fn level_91() -> Value {
    json!({"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": LEVEL}})
}

async fn first_frame(side: Side, ddp: &str, dmx: &str) {
    let case = format!("{side:?}, DDP at {ddp}, DMX at {dmx}");
    let heard: Heard = Arc::default();
    let stop = Arc::new(AtomicBool::new(false));
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (mut socket, mut wakes) = start(Some(Arc::clone(&heard)), &stop, &udp, ddp, dmx).await;
    let mut seen = Seen::new(udp, heard);
    // healthy and drained: each device took two frames, each on the wire
    while seen.came.iter().any(|came| came.len() < 2) {
        seen.wake(&mut wakes).await;
    }
    let admitted = match side {
        Side::Before => {
            assert_eq!(command(&mut socket, level_91()).await, json!("Accepted"));
            seen.wake(&mut wakes).await
        }
        Side::After => {
            let cut = block_in_place(|| wakes.cut()).unwrap();
            assert_eq!(command(&mut socket, level_91()).await, json!("Accepted"));
            block_in_place(|| wakes.finish()).unwrap();
            seen.take(&mut wakes);
            seen.drain().await;
            assert!(
                seen.handed.iter().flatten().any(|handed| handed.at == cut),
                "{case}: the wake held at its cutoff served no output"
            );
            seen.wake(&mut wakes).await
        }
    };
    // Each output's first slot carrying 91, from its schedule: its first due, a period
    // apart. A slot due before the admitting wake had a wake of its own; one due in it,
    // or within the spin after it, goes out in it.
    let periods = [ddp, dmx].map(period);
    let first: [Duration; OUTPUTS] = std::array::from_fn(|n| {
        let start = seen.handed[n][0].due;
        let k = (admitted - start)
            .as_nanos()
            .div_ceil(periods[n].as_nanos());
        start + periods[n] * u32::try_from(k).unwrap()
    });
    // until each output was handed that slot, and what came of every slot up to it is back
    let started = Instant::now();
    while (0..OUTPUTS).any(|n| {
        seen.handed[n].last().is_none_or(|last| last.due < first[n])
            || seen.outcomes(n, first[n]).is_none()
    }) {
        seen.wake(&mut wakes).await;
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{case}: first slots {first:?}, handed {:?}, came {:?}",
            seen.handed,
            seen.came
        );
    }
    let wire = seen.wire();
    for (n, on_wire) in wire.iter().enumerate() {
        let output = n + 1;
        let (handed, came) = (&seen.handed[n], &seen.came[n]);
        let dues: Vec<Duration> = handed.iter().map(|handed| handed.due).collect();
        let schedule: Vec<Duration> = (0..u32::try_from(dues.len()).unwrap())
            .map(|k| dues[0] + periods[n] * k)
            .collect();
        assert_eq!(dues, schedule, "{case}, output {output}: a slot skipped");
        assert_eq!(
            seen.outcomes(n, first[n]),
            Some(vec![
                Outcome::Accepted;
                handed.iter().filter(|h| h.due <= first[n]).count()
            ]),
            "{case}, output {output}: came {came:?}"
        );
        assert!(
            came.iter().all(|came| came.outcome == Outcome::Accepted),
            "{case}, output {output}: {came:?}"
        );
        let expected: Vec<u8> = handed
            .iter()
            .map(|handed| if handed.due >= first[n] { LEVEL } else { 0 })
            .collect();
        assert_eq!(
            on_wire, &expected,
            "{case}, output {output}: admitted in the wake at {admitted:?}, handed over {handed:?}"
        );
    }
    send(&mut socket, &json!("Stop")).await;
    stop.store(true, Ordering::Relaxed);
}

#[tokio::test(flavor = "multi_thread")]
async fn raw_91_before_or_after_the_cutoff_is_in_each_outputs_first_frame_after_it() {
    // (side of the cutoff, DDP rate, DMX rate)
    let cases = [
        (Side::Before, "60", "44"),
        (Side::After, "60", "44"),
        (Side::Before, "44", "30"),
        (Side::After, "44", "30"),
    ];
    for (side, ddp, dmx) in cases {
        first_frame(side, ddp, dmx).await;
    }
}

/// Nobody reads the fake adapter's port: its queue fills up and the frame it cannot take
/// is `Dropped` (not one passed over by its output's thread), counted on the picture. The DDP output beside it takes every frame.
#[tokio::test(flavor = "multi_thread")]
async fn a_full_adapter_queue_drops_the_frame_and_the_output_beside_it_takes_every_one() {
    let stop = Arc::new(AtomicBool::new(false));
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (mut socket, mut wakes) = start(None, &stop, &udp, "60", "44").await;
    let mut seen = Seen::new(udp, Heard::default());
    let started = Instant::now();
    let dropped = loop {
        block_in_place(|| wakes.wake()).unwrap();
        seen.take(&mut wakes);
        if let Some(at) = seen.came[1]
            .iter()
            .position(|came| came.outcome == Outcome::Dropped)
        {
            break at;
        }
        thread::sleep(Duration::from_millis(2)); // poll: the output's thread takes the frame
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the adapter took every frame: {:?}",
            seen.came[1]
        );
    };
    // The adapter took frames until its queue was full, then dropped the newest at once:
    // no newer frame was handed over before it came back, so none was passed over.
    let drop = seen.came[1][dropped];
    let (handed, came) = (&seen.handed[1][..drop.handed], &seen.came[1][..dropped]);
    assert_eq!(
        handed.last().map(|handed| handed.due),
        Some(drop.due),
        "passed over: handed {handed:?}, came {:?}",
        seen.came[1]
    );
    assert!(
        !came.is_empty() && came.iter().all(|came| came.outcome == Outcome::Accepted),
        "{came:?}"
    );
    // the DDP output took every frame handed over by then, each outcome back
    let until = seen.handed[0].last().unwrap().due;
    let started = Instant::now();
    while seen.outcomes(0, until).is_none() {
        block_in_place(|| wakes.wake()).unwrap();
        seen.take(&mut wakes);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            seen.came[0]
        );
    }
    let ddp = seen.outcomes(0, until).unwrap();
    assert!(
        !ddp.is_empty() && ddp.iter().all(|outcome| *outcome == Outcome::Accepted),
        "{ddp:?}"
    );
    // the picture counts it
    block_in_place(|| wakes.run());
    let started = Instant::now();
    loop {
        send(&mut socket, &json!("Query")).await;
        let picture = common::answer(&mut socket).await;
        if picture["Picture"]["state"]["outputs"][1]["counters"]["dropped"].as_u64() > Some(0) {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "{picture}");
    }
    send(&mut socket, &json!("Stop")).await;
    stop.store(true, Ordering::Relaxed);
}
