//! M3 T1 (#165), the first due frame. The production tick runs on a clock the test steps
//! one wake at a time (`engine/src/wake.rs`). Raw level 91 is sent through `/api` before
//! a wake's cutoff, or after it while the tick is held there. Each due hand-over, what came
//! of it, every DDP frame and every complete message of a fake DMX USB Pro are recorded
//! in order: each output's first hand-over at or after the wake that admitted 91 carries
//! it, and no earlier one does.
//!
//! ```text
//! before: send 91 --> wake T (cutoff: 91 taken)        --> frames at T and later carry 91
//! after:  wake T, held at its cutoff --> send 91 --> on --> wake T' --> frames at T' and later
//! ```

mod common;

use common::{Heard, Socket, command, connect, open, send, serve};
use engine::{Engine, Event, Outcome, Universes, Wakes};
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

/// What the test saw, in order, per output (DDP, DMX).
struct Seen {
    udp: UdpSocket,
    heard: Heard,
    /// The wake of each due hand-over.
    handed: [Vec<Duration>; OUTPUTS],
    /// What came of each frame, in the order it came back.
    came: [Vec<Outcome>; OUTPUTS],
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
                Event::Handed { output, at, .. } => self.handed[output.slot()].push(at),
                Event::Came {
                    output, outcome, ..
                } => self.came[output.slot()].push(outcome),
                Event::Counted { .. } => {}
            }
        }
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
                seen.handed.iter().flatten().any(|at| *at == cut),
                "{case}: the wake held at its cutoff served no output"
            );
            seen.wake(&mut wakes).await
        }
    };
    // until each output was handed a frame in or after that wake, and what came of it is back
    let mut eligible = [None; OUTPUTS];
    while eligible
        .iter()
        .zip(&seen.came)
        .any(|(first, came)| first.is_none_or(|first| came.len() <= first))
    {
        seen.wake(&mut wakes).await;
        for (first, handed) in eligible.iter_mut().zip(&seen.handed) {
            *first = handed.iter().position(|at| *at >= admitted);
        }
    }
    let wire = seen.wire();
    for (n, ((handed, came), on_wire)) in seen.handed.iter().zip(&seen.came).zip(&wire).enumerate()
    {
        let output = n + 1;
        assert!(
            came.iter().all(|outcome| *outcome == Outcome::Accepted),
            "{case}, output {output}: {came:?}"
        );
        let expected: Vec<u8> = handed
            .iter()
            .map(|at| if *at >= admitted { LEVEL } else { 0 })
            .collect();
        assert_eq!(
            on_wire, &expected,
            "{case}, output {output}: admitted in the wake at {admitted:?}, handed over at {handed:?}"
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
        (Side::Before, "44", "60"),
        (Side::After, "44", "60"),
    ];
    for (side, ddp, dmx) in cases {
        first_frame(side, ddp, dmx).await;
    }
}

/// Nobody reads the fake adapter's port: its queue fills up and the frame it cannot take
/// is `Dropped`, counted on the picture. The DDP output beside it takes every frame.
#[tokio::test(flavor = "multi_thread")]
async fn a_full_adapter_queue_drops_the_frame_and_the_output_beside_it_takes_every_one() {
    let stop = Arc::new(AtomicBool::new(false));
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (mut socket, mut wakes) = start(None, &stop, &udp, "60", "44").await;
    let mut seen = Seen::new(udp, Heard::default());
    let started = Instant::now();
    while !seen.came[1].contains(&Outcome::Dropped) {
        block_in_place(|| wakes.wake()).unwrap();
        seen.take(&mut wakes);
        thread::sleep(Duration::from_millis(2)); // poll: the output's thread takes the frame
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the adapter took every frame: {:?}",
            seen.came[1]
        );
    }
    assert!(
        seen.came[0]
            .iter()
            .all(|outcome| *outcome == Outcome::Accepted),
        "{:?}",
        seen.came[0]
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
