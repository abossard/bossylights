//! Is the delay the engine reports true? `idle|show|biggest [seconds]` serves the engine
//! the way the app does, presses and releases one light at a time over the API, and reads
//! each DDP output on a UDP socket of its own. For every output and every 4 s window that
//! ends on a multiple of 4 s of engine time, it puts the press window Health reports
//! beside the same statistics measured from outside, and prints them as JSON, times in
//! microseconds. It exits 1 when they disagree or the measurement is incomplete.
//!
//! The stamps of one command: b before its send, a when its reply arrives, u when the system
//! received the first packet of the frame that carries it (its receive stamp, so a reader
//! that wakes late does not count). A thread of its own sends each reader's socket a probe
//! about every 10 ms; c is the slowest probe of the window, from its send to its receive
//! stamp: what the engine cannot see after its handoff.
//!
//! | Check | Rule |
//! |---|---|
//! | count | the window's samples are the commands its output carried, at least 20 |
//! | never more than outside | p50, p95 and worst are at most the upper bucket edge of the same statistic of u - b |
//! | never less than outside minus what it leaves out | each is at least the lower bucket edge of the same statistic of u - a - c |
//! | a mark is a press | lights 1 to 64 show B = 0 with G > 0 only while a press holds them, as [m, 255 - m, 0] |
//! | nothing lost | every DDP frame whole, its sequence one after the last |

use engine::{Color, Command, Engine, OutputKind, OutputNumber, Rate, RunNumbers, Universes};
use engine_core::LATENCY_BUCKETS;
use engine_core::health::WINDOW_DURATION as WINDOW;
use futures_util::{SinkExt, StreamExt};
use nix::errno::Errno;
use nix::sys::socket::{ControlMessageOwned, MsgFlags, SockaddrIn, recvmsg, setsockopt, sockopt};
use nix::sys::time::{TimeVal, TimeValLike};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::error::Error;
use std::io::IoSliceMut;
use std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use std::os::fd::AsRawFd;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio_tungstenite::tungstenite::Message;

#[path = "shared/rig.rs"]
#[allow(dead_code, reason = "send_to_frame uses the rest")]
mod rig;
use rig::Workload;

/// Sends keep this far from the end of a compared window, so the window of each is certain:
/// Health counts a command at the wake after the handoff of its frame, up to a period and
/// a wake after its send, and a late frame later still.
const MARGIN: Duration = Duration::from_millis(200);
/// Health shows its window to a grid end from its first read past that end until the
/// next end, 500 ms later: asked this long after the end, the window is the one shown.
const SHOWN: Duration = Duration::from_millis(250);
/// Health's statistics need this many samples in a window (its `MIN_SAMPLES`).
const FEWEST: usize = 20;
/// Health counts times in buckets this wide.
const BUCKET: Duration = Duration::from_micros(100);
/// The statistics Health reports, as nearest-rank percents.
const STATISTICS: [(&str, u64); 3] = [("p50", 50), ("p95", 95), ("worst", 100)];
/// Lights 1 to 64: all in the first packet of a frame.
const LIGHTS: usize = 64;
const HEADER: usize = 10;
const PUSH: u8 = 0x01;
/// A probe's first byte. A DDP packet's is 0x40 or 0x41.
const PROBE: u8 = 0;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let workload = match args.next().as_deref() {
        Some("idle") => Workload::Idle,
        Some("show") => Workload::Show,
        Some("biggest") => Workload::Biggest,
        _ => return usage(),
    };
    let seconds = match args.next().map(|s| s.parse::<u64>()) {
        None => 60,
        Some(Ok(seconds)) if seconds >= WINDOW.as_secs() => seconds,
        Some(_) => return usage(),
    };
    if args.next().is_some() {
        return usage();
    }
    let windows = u32::try_from(seconds / WINDOW.as_secs()).unwrap_or(u32::MAX);
    match agree(workload, windows) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("delay_agreement: {error}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("delay_agreement idle|show|biggest [seconds, at least 4]");
    ExitCode::FAILURE
}

fn rate(slot: usize) -> Rate {
    [Rate::FortyFour, Rate::Sixty][slot % 2]
}

fn next_seed(mut seed: u64) -> u64 {
    seed ^= seed << 13;
    seed ^= seed >> 7;
    seed ^ (seed << 17)
}

/// One command the plan sends: a press of `light` that shows `mark`, or its release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Planned {
    /// Engine time.
    at: Duration,
    light: u16,
    mark: Option<u8>,
}

/// Presses and releases of one light at a time, 90 to 130 ms apart, none within
/// [`MARGIN`] of the end of a window, in `windows` windows from `first` (engine time).
fn schedule(first: Duration, windows: u32) -> Vec<Planned> {
    let end = first + WINDOW * windows;
    let mut plan = Vec::new();
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut at = first;
    let mut held = None;
    loop {
        let into = Duration::from_nanos(((at - first).as_nanos() % WINDOW.as_nanos()) as u64);
        if into < MARGIN {
            at += MARGIN - into;
        } else if into > WINDOW - MARGIN {
            at += WINDOW - into + MARGIN;
        }
        if at + MARGIN > end {
            return plan;
        }
        seed = next_seed(seed);
        let command = match held.take() {
            Some(light) => Planned {
                at,
                light,
                mark: None,
            },
            None => {
                let light = 1 + ((seed >> 16) % LIGHTS as u64) as u16;
                held = Some(light);
                Planned {
                    at,
                    light,
                    mark: Some(1 + ((seed >> 32) % 254) as u8),
                }
            }
        };
        plan.push(command);
        at += Duration::from_millis(90 + seed % 41);
    }
}

/// The command as JSON: a press sets the light to [m, 255 - m, 0] in 16 bits.
fn command(planned: Planned) -> Value {
    let target = json!({"Light": {"light": planned.light}});
    let command = match planned.mark {
        Some(mark) => {
            let red = u16::from(mark) << 8 | 0x80;
            json!({"SetColour": {"target": target, "red": red, "green": u16::MAX - red, "blue": 0}})
        }
        None => json!({"ReleaseColour": {"target": target}}),
    };
    json!({"Send": {"command": command}})
}

/// One command as the client sent it: b before the send, a when its reply arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sent {
    light: u16,
    mark: Option<u8>,
    before: Instant,
    after: Instant,
}

/// A frame as its reader saw it: when its first packet arrived, and lights 1 to 64.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Frame {
    at: Instant,
    lights: [u8; 3 * LIGHTS],
}

impl Frame {
    fn pixel(&self, light: u16) -> [u8; 3] {
        let at = 3 * (usize::from(light) - 1);
        [self.lights[at], self.lights[at + 1], self.lights[at + 2]]
    }

    /// Whether it shows `light` as `held`: the pixel of that press, or no mark.
    fn shows(&self, light: u16, held: Option<u8>) -> bool {
        let pixel = self.pixel(light);
        match held {
            Some(mark) => pixel == [mark, 255 - mark, 0],
            None => !(pixel[2] == 0 && pixel[1] > 0),
        }
    }
}

/// A probe: when it was sent (engine time), and how long until its reader stamped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Probe {
    sent: Duration,
    trip: Duration,
}

/// What one output's reader saw.
#[derive(Debug, Default, PartialEq, Eq)]
struct Seen {
    frames: Vec<Frame>,
    probes: Vec<Probe>,
    /// Places where packets went missing: a sequence step, a frame not whole.
    lost: u32,
}

/// Reads one output's datagrams in arrival order.
#[derive(Default)]
struct Reader {
    seen: Seen,
    sequence: Option<u8>,
    /// The offset of the next packet of the frame, `None` after its last.
    next: Option<u32>,
}

impl Reader {
    /// One datagram that arrived at `at`. `start` is the engine clock's zero.
    fn packet(&mut self, at: Instant, start: Instant, bytes: &[u8]) {
        if let [PROBE, rest @ ..] = bytes
            && let Ok(nanos) = <[u8; 8]>::try_from(rest)
        {
            let sent = Duration::from_nanos(u64::from_be_bytes(nanos));
            let trip = at.saturating_duration_since(start + sent);
            self.seen.probes.push(Probe { sent, trip });
            return;
        }
        let Some(&[flags, sequence, _, _, o0, o1, o2, o3, l0, l1]) = bytes.first_chunk() else {
            self.seen.lost += 1;
            return;
        };
        let offset = u32::from_be_bytes([o0, o1, o2, o3]);
        if offset == 0 {
            let skipped = self.sequence.is_some_and(|last| sequence != last % 15 + 1);
            self.seen.lost += u32::from(self.next.is_some()) + u32::from(skipped);
            match bytes.get(HEADER..HEADER + 3 * LIGHTS) {
                Some(lights) => self.seen.frames.push(Frame {
                    at,
                    lights: lights.try_into().unwrap_or([0; 3 * LIGHTS]),
                }),
                None => self.seen.lost += 1,
            }
        } else if self.next != Some(offset) || self.sequence != Some(sequence) {
            self.seen.lost += 1;
        }
        self.sequence = Some(sequence);
        let length = u32::from(u16::from_be_bytes([l0, l1]));
        self.next = (flags & PUSH == 0).then_some(offset + length);
    }
}

/// For each command, the arrival of the first frame that carries it: the first frame
/// after its send that shows its change. Every frame shows the presses held, nothing else.
fn carried(frames: &[Frame], sent: &[Sent]) -> Result<Vec<Instant>, String> {
    let mut held = [None; LIGHTS];
    let mut carried = Vec::with_capacity(sent.len());
    for frame in frames {
        while let Some(command) = sent.get(carried.len())
            && frame.at > command.before
            && frame.shows(command.light, command.mark)
        {
            held[usize::from(command.light) - 1] = command.mark;
            carried.push(frame.at);
        }
        let shown =
            (1..=LIGHTS as u16).find(|&light| !frame.shows(light, held[usize::from(light) - 1]));
        if let Some(light) = shown {
            return Err(format!(
                "light {light} shows {:?} after {} commands: mark without a press",
                frame.pixel(light),
                carried.len()
            ));
        }
    }
    match sent.get(carried.len()) {
        Some(command) => Err(format!(
            "command {} ({:?} on light {}) never reached a frame",
            carried.len() + 1,
            command.mark,
            command.light
        )),
        None => Ok(carried),
    }
}

/// One command of a window, measured from outside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Press {
    /// u - b.
    outside: Duration,
    /// a - b: the send and its reply.
    call: Duration,
    /// u - a - c, at least 0.
    least: Duration,
}

/// The press window as Health reports it, microseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Reported {
    statistics: [u32; 3],
    samples: u32,
    missing: u64,
}

impl Reported {
    /// A row's measurement on the wire: `{"Window": {"end_ms": ..., "press_to_light": ...}}`.
    fn of(window: &Value) -> Self {
        let press = &window["press_to_light"];
        let micros = |key: &str| press[key].as_u64().unwrap_or(0) as u32;
        Self {
            statistics: [micros("p50_us"), micros("p95_us"), micros("worst_us")],
            samples: micros("samples"),
            missing: window["press_missing"].as_u64().unwrap_or(0),
        }
    }
}

/// The statistic at nearest rank `percent` of `times`, as Health ranks: ceil(percent / 100 x n).
fn ranked(times: &mut [Duration], percent: u64) -> Option<Duration> {
    times.sort_unstable();
    let rank = (percent as usize * times.len()).div_ceil(100).max(1);
    times.get(rank - 1).copied()
}

/// The upper edge of the bucket that holds `time`. The last bucket has none.
fn upper_edge(time: Duration) -> Option<Duration> {
    let bucket = engine_core::bucket::<LATENCY_BUCKETS>(time);
    (bucket + 1 < LATENCY_BUCKETS).then(|| BUCKET * (bucket as u32 + 1))
}

fn lower_edge(time: Duration) -> Duration {
    BUCKET * engine_core::bucket::<LATENCY_BUCKETS>(time) as u32
}

/// A window's statistics from outside, microseconds: p50, p95, worst.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Outside {
    /// Of u - b; each report is at most its upper bucket edge.
    most: [u32; 3],
    /// The lower bucket edge of u - a - c; each report is at least this.
    least: [u32; 3],
}

/// Checks one window: Health's report against the window's commands measured from
/// outside, `c` the slowest of its `probes`. The outside statistics, or what disagrees.
fn check(
    reported: Reported,
    presses: &[Press],
    c: Duration,
    probes: usize,
) -> Result<Outside, String> {
    if reported.samples as usize != presses.len() {
        return Err(format!(
            "Health counts {} commands ({} missing), the reader saw {} carried",
            reported.samples,
            reported.missing,
            presses.len()
        ));
    }
    if presses.len() < FEWEST {
        return Err(format!("{} commands, fewer than {FEWEST}", presses.len()));
    }
    let mut outside = Outside {
        most: [0; 3],
        least: [0; 3],
    };
    for (i, ((name, percent), reported)) in
        STATISTICS.into_iter().zip(reported.statistics).enumerate()
    {
        let micros = Duration::from_micros(u64::from(reported));
        let at = |key: fn(&Press) -> Duration| {
            let mut times: Vec<Duration> = presses.iter().map(key).collect();
            let time = ranked(&mut times, percent).unwrap_or_default();
            presses
                .iter()
                .find(|p| key(p) == time)
                .copied()
                .map(|p| (time, p))
        };
        let Some((most, press)) = at(|p| p.outside) else {
            return Err("no commands".into());
        };
        if upper_edge(most).is_some_and(|edge| micros > edge) {
            return Err(format!(
                "{name} reported {reported} µs is more than the outside {name} allows: {} µs (the press: u - b {} µs, a - b {} µs)",
                upper_edge(most).unwrap_or_default().as_micros(),
                press.outside.as_micros(),
                press.call.as_micros()
            ));
        }
        let Some((least, press)) = at(|p| p.least) else {
            return Err("no commands".into());
        };
        if micros < lower_edge(least) {
            return Err(format!(
                "{name} reported {reported} µs is less than outside minus what it leaves out: {} µs (the press: u - b {} µs, a - b {} µs, c {} µs of {probes} probes)",
                lower_edge(least).as_micros(),
                press.outside.as_micros(),
                press.call.as_micros(),
                c.as_micros()
            ));
        }
        outside.most[i] = most.as_micros() as u32;
        outside.least[i] = lower_edge(least).as_micros() as u32;
    }
    Ok(outside)
}

/// The run's outputs and windows, all checked: the report, or the first disagreement.
fn report(
    seen: &[Seen],
    sent: &[Sent],
    health: &BTreeMap<(u64, u64), Reported>,
    start: Instant,
    first: Duration,
    windows: u32,
) -> Result<Value, String> {
    let mut outputs = Vec::new();
    for (slot, seen) in seen.iter().enumerate() {
        let output = slot + 1;
        if seen.lost > 0 {
            return Err(format!(
                "output {output} lost packets in {} places: an incomplete measurement",
                seen.lost
            ));
        }
        if let Some(late) = sent.iter().find(|command| {
            let into =
                (command.before - start).saturating_sub(first).as_nanos() % WINDOW.as_nanos();
            into < MARGIN.as_nanos() || into > (WINDOW - MARGIN).as_nanos()
        }) {
            return Err(format!(
                "a send at {} ms came within {} ms of a window end: an incomplete measurement",
                (late.before - start).as_millis(),
                MARGIN.as_millis()
            ));
        }
        let carried = carried(&seen.frames, sent).map_err(|e| format!("output {output}: {e}"))?;
        let mut rows = Vec::new();
        for window in 1..=windows {
            let end = first + WINDOW * window;
            let within = |time: Duration| time > end - WINDOW && time <= end;
            let trips: Vec<Duration> = seen
                .probes
                .iter()
                .filter(|p| within(p.sent))
                .map(|p| p.trip)
                .collect();
            let end_ms = end.as_millis() as u64;
            let Some(&c) = trips.iter().max() else {
                return Err(format!("output {output}, window to {end_ms} ms: no probe"));
            };
            let presses: Vec<Press> = sent
                .iter()
                .zip(&carried)
                .filter(|(command, _)| within(command.before - start))
                .map(|(command, &u)| Press {
                    outside: u - command.before,
                    call: command.after - command.before,
                    least: u.saturating_duration_since(command.after).saturating_sub(c),
                })
                .collect();
            let reported = health.get(&(output as u64, end_ms)).ok_or_else(|| {
                format!("output {output}: Health showed no window to {end_ms} ms")
            })?;
            let outside = check(*reported, &presses, c, trips.len())
                .map_err(|e| format!("output {output}, window to {end_ms} ms: {e}"))?;
            rows.push(json!({
                "end_ms": end_ms,
                "presses": presses.len(),
                "probes": trips.len(),
                "left_out_us": c.as_micros() as u64,
                "reported_us": reported.statistics,
                "outside_us": outside.most,
                "least_us": outside.least,
            }));
        }
        outputs.push(json!({
            "output": output,
            "frames_per_second": rate(slot).per_second(),
            "windows": rows,
        }));
    }
    Ok(json!({ "outputs": outputs }))
}

/// When a datagram arrived, as an instant: `now`, read right after `recvmsg`, less how
/// long it waited, on the wall clock from its receive `stamp` to `wall` (read with `now`).
fn arrival(now: Instant, wall: Duration, stamp: Duration) -> Option<Instant> {
    now.checked_sub(wall.saturating_sub(stamp))
}

/// Reads one output's socket until `stop`. Each datagram is stamped when the system
/// received it: its receive stamp is on the wall clock, so `at` is the instant right after
/// `recvmsg`, less how long the datagram waited for it.
fn read(socket: &UdpSocket, start: Instant, stop: &AtomicBool) -> Result<Seen, String> {
    let fd = socket.as_raw_fd();
    setsockopt(fd, sockopt::ReceiveTimestamp, &true).map_err(|e| e.to_string())?;
    socket
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|e| e.to_string())?;
    let mut reader = Reader::default();
    let mut packet = [0; 1500];
    let mut control = nix::cmsg_space!(TimeVal);
    while !stop.load(Ordering::Relaxed) {
        let mut buffers = [IoSliceMut::new(&mut packet)];
        let received =
            match recvmsg::<SockaddrIn>(fd, &mut buffers, Some(&mut control), MsgFlags::empty()) {
                Ok(message) => {
                    let (now, wall) = (Instant::now(), SystemTime::now());
                    let stamp = message.cmsgs().find_map(|control| {
                        if let ControlMessageOwned::ScmTimestamp(stamp) = control {
                            Some(stamp)
                        } else {
                            None
                        }
                    });
                    let stamp = stamp.ok_or("a datagram without its receive stamp")?;
                    let stamp =
                        Duration::from_micros(stamp.num_microseconds().try_into().unwrap_or(0));
                    let wall = wall.duration_since(UNIX_EPOCH).unwrap_or_default();
                    (
                        arrival(now, wall, stamp).ok_or("a receive stamp before the clock")?,
                        message.bytes,
                    )
                }
                Err(Errno::EAGAIN | Errno::EINTR) => continue,
                Err(e) => return Err(e.to_string()),
            };
        let (at, length) = received;
        reader.packet(at, start, &packet[..length]);
    }
    Ok(reader.seen)
}

/// Sends every reader a probe about every 10 ms, at no fixed phase, until `stop`.
fn probe(targets: &[SocketAddrV4], start: Instant, stop: &AtomicBool) -> std::io::Result<()> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    while !stop.load(Ordering::Relaxed) {
        seed = next_seed(seed);
        thread::sleep(Duration::from_micros(5_000 + seed % 10_000));
        for target in targets {
            let mut packet = [PROBE; 9];
            let sent = start.elapsed().as_nanos() as u64;
            packet[1..].copy_from_slice(&sent.to_be_bytes());
            socket.send_to(&packet, target)?;
        }
    }
    Ok(())
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(address: SocketAddr) -> Result<Socket, String> {
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/api"))
        .await
        .map_err(|e| format!("connect: {e}"))?;
    Ok(socket)
}

/// The next message that is no subscription update, and when it arrived.
async fn answer(socket: &mut Socket) -> Result<(Value, Instant), String> {
    loop {
        let message = socket
            .next()
            .await
            .ok_or("the server closed")?
            .map_err(|e| e.to_string())?;
        let at = Instant::now();
        let reply: Value = serde_json::from_str(message.to_text().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if reply.get("Changed").is_none() && reply.get("Health").is_none() {
            return Ok((reply, at));
        }
    }
}

/// Sends each planned command at its time; stamps b before the send, a at its reply.
async fn press(socket: &mut Socket, plan: &[Planned], start: Instant) -> Result<Vec<Sent>, String> {
    let mut sent = Vec::with_capacity(plan.len());
    for planned in plan {
        tokio::time::sleep_until((start + planned.at).into()).await;
        let message = Message::text(command(*planned).to_string());
        let before = Instant::now();
        socket.send(message).await.map_err(|e| e.to_string())?;
        let (reply, after) = answer(socket).await?;
        let taken = match planned.mark {
            Some(_) => reply["Grabbed"]["grab"].is_object(),
            None => reply == "Accepted",
        };
        if !taken {
            return Err(format!("{planned:?} got {reply}"));
        }
        sent.push(Sent {
            light: planned.light,
            mark: planned.mark,
            before,
            after,
        });
    }
    Ok(sent)
}

/// Health as a caller gets it: a `Query` [`SHOWN`] after each of `ends` (ms), while each
/// output shows its window to that end. A subscriber gets Health at most once per 500 ms,
/// so it can miss a window.
async fn watch(
    mut socket: Socket,
    ends: Vec<u64>,
    start: Instant,
) -> Result<BTreeMap<(u64, u64), Reported>, String> {
    let mut seen = BTreeMap::new();
    for end in ends {
        tokio::time::sleep_until((start + Duration::from_millis(end) + SHOWN).into()).await;
        socket
            .send(Message::text(json!("Query").to_string()))
            .await
            .map_err(|e| e.to_string())?;
        let (picture, _) = answer(&mut socket).await?;
        let rows = picture["Picture"]["health"]["outputs"]
            .as_array()
            .ok_or_else(|| format!("a Query got {picture}"))?;
        for row in rows {
            let window = &row["measurement"]["Window"];
            if window["end_ms"].as_u64() == Some(end)
                && let Some(output) = row["output"].as_u64()
            {
                seen.insert((output, end), Reported::of(window));
            }
        }
    }
    Ok(seen)
}

/// Serves the engine, plans the commands from the next window, sends them and reads
/// Health. The commands as sent, Health's windows, and the first window's start.
async fn drive(
    engine: Engine,
    sender: engine::Sender,
    windows: u32,
    start: Instant,
) -> Result<(Vec<Sent>, BTreeMap<(u64, u64), Reported>, Duration), String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    let served = tokio::spawn(engine_server::serve(listener, engine, sender, None, None));
    let mut client = connect(address).await?;
    let asker = connect(address).await?;
    // The first window starts on the next multiple of 4 s, at least half a second ahead.
    let soon = (start.elapsed() + Duration::from_millis(500)).as_nanos();
    let first = Duration::from_nanos((soon.div_ceil(WINDOW.as_nanos()) * WINDOW.as_nanos()) as u64);
    let ends = (1..=windows)
        .map(|w| (first + WINDOW * w).as_millis() as u64)
        .collect();
    let watched = tokio::spawn(watch(asker, ends, start));
    let sent = press(&mut client, &schedule(first, windows), start).await?;
    let health = watched.await.map_err(|e| e.to_string())??;
    client
        .send(Message::text(json!("Stop").to_string()))
        .await
        .map_err(|e| e.to_string())?;
    let engine = served
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    drop(engine);
    Ok((sent, health, first))
}

fn agree(workload: Workload, windows: u32) -> Result<Value, Box<dyn Error>> {
    let outputs = if workload == Workload::Biggest {
        engine::MAX_OUTPUTS
    } else {
        2
    };
    let sockets = (0..outputs)
        .map(|_| UdpSocket::bind("127.0.0.1:0"))
        .collect::<Result<Vec<_>, _>>()?;
    let addresses = sockets
        .iter()
        .map(|socket| match socket.local_addr()? {
            SocketAddr::V4(address) => Ok(address),
            SocketAddr::V6(_) => Err("expected a loopback IPv4 address".into()),
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let universes = Universes::new(engine::MAX_UNIVERSES).ok_or("invalid universe count")?;
    let (engine, mut sender, timings) = Engine::start_measured(universes)?;
    // The engine clock's zero: its time and an instant read together.
    let start = Instant::now() - timings.now();
    let stop = AtomicBool::new(false);
    thread::scope(|scope| {
        // Readers first: they see each output's first packet.
        let readers: Vec<_> = sockets
            .iter()
            .map(|socket| scope.spawn(|| read(socket, start, &stop)))
            .collect();
        let prober = scope.spawn(|| probe(&addresses, start, &stop));
        let driven = (|| -> Result<_, Box<dyn Error>> {
            for (slot, address) in addresses.iter().enumerate() {
                sender
                    .send(Command::SetOutput {
                        output: OutputNumber::try_from((slot + 1) as u8)?,
                        kind: OutputKind::Ddp {
                            address: *address,
                            color: Color::Rgb,
                        },
                        universes: RunNumbers {
                            first: 1,
                            count: universes.count(),
                        },
                        rate: Some(rate(slot)),
                    })
                    .map_err(|r| format!("attach {}: {r:?}", slot + 1))?;
            }
            rig::prepare(&mut sender, workload)?;
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            Ok(runtime.block_on(drive(engine, sender, windows, start))?)
        })();
        stop.store(true, Ordering::Relaxed);
        let seen = readers
            .into_iter()
            .map(|reader| {
                reader
                    .join()
                    .map_err(|_| "a reader panicked")?
                    .map_err(Into::into)
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
        prober.join().map_err(|_| "the prober panicked")??;
        let (sent, health, first) = driven?;
        Ok(report(&seen, &sent, &health, start, first, windows)?)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn health_counts_in_buckets_of_100_us() {
        for (time, bucket) in [
            (BUCKET - Duration::from_nanos(1), 0),
            (BUCKET, 1),
            (BUCKET * 499, 499),
            (BUCKET * 500, 500),
            (BUCKET * 9000, 500),
        ] {
            assert_eq!(
                engine_core::bucket::<LATENCY_BUCKETS>(time),
                bucket,
                "{time:?}"
            );
        }
    }

    #[test]
    fn every_window_gets_at_least_20_commands_and_none_near_its_ends() {
        let first = ms(8000);
        for windows in [1, 2, 15] {
            let plan = schedule(first, windows);
            for (i, pair) in plan.chunks(2).enumerate() {
                let press = pair[0];
                assert!(matches!(press.mark, Some(1..=254)), "{press:?}");
                assert!((1..=64).contains(&press.light), "{press:?}");
                if let Some(release) = pair.get(1) {
                    assert_eq!(
                        (release.light, release.mark),
                        (press.light, None),
                        "pair {i}"
                    );
                }
            }
            for (earlier, later) in plan.iter().zip(&plan[1..]) {
                assert!(later.at - earlier.at >= ms(90), "{earlier:?} {later:?}");
            }
            let mut counts = vec![0; windows as usize];
            for planned in &plan {
                let into = (planned.at - first).as_millis() % WINDOW.as_millis();
                assert!((200..=3800).contains(&into), "{planned:?}");
                counts[((planned.at - first).as_millis() / WINDOW.as_millis()) as usize] += 1;
            }
            assert!(
                counts.iter().all(|&n| n >= FEWEST),
                "{windows} windows: {counts:?}"
            );
        }
    }

    /// A DDP packet: sequence, offset, data length, push; its data byte n is n (mod 256).
    fn ddp(sequence: u8, offset: u32, length: u16, push: bool) -> Vec<u8> {
        let mut packet = vec![0x40 | u8::from(push), sequence, 0x0B, 1];
        packet.extend(offset.to_be_bytes());
        packet.extend(length.to_be_bytes());
        packet.extend((0..length).map(|byte| byte as u8));
        packet
    }

    #[test]
    fn the_reader_counts_each_place_where_packets_went_missing() {
        let whole = |sequence| {
            [
                ddp(sequence, 0, 1440, false),
                ddp(sequence, 1440, 100, true),
            ]
        };
        let cases: [(&str, Vec<Vec<u8>>, usize, u32); 8] = [
            ("two whole frames", [whole(1), whole(2)].concat(), 2, 0),
            (
                "the sequence wraps from 15 to 1",
                [whole(15), whole(1)].concat(),
                2,
                0,
            ),
            ("a frame skipped", [whole(1), whole(3)].concat(), 2, 1),
            (
                "a middle packet missing",
                vec![ddp(1, 0, 1440, false), ddp(1, 2880, 100, true)],
                1,
                1,
            ),
            (
                "a middle packet with another frame's sequence",
                vec![ddp(1, 0, 1440, false), ddp(2, 1440, 100, true)],
                1,
                1,
            ),
            (
                "the last packet missing",
                vec![ddp(1, 0, 1440, false), ddp(2, 0, 1440, true)],
                2,
                1,
            ),
            (
                "the first packet missing",
                vec![ddp(1, 1440, 100, true)],
                0,
                1,
            ),
            (
                "a datagram shorter than a header",
                vec![vec![0x41, 1, 0x0B]],
                0,
                1,
            ),
        ];
        let start = Instant::now();
        for (name, packets, frames, lost) in cases {
            let mut reader = Reader::default();
            for packet in &packets {
                reader.packet(start, start, packet);
            }
            assert_eq!(
                (reader.seen.frames.len(), reader.seen.lost),
                (frames, lost),
                "{name}"
            );
            // Light 1 is the first three data bytes, light 64 bytes 189 to 191.
            assert!(
                reader
                    .seen
                    .frames
                    .iter()
                    .all(|f| (f.pixel(1), f.pixel(64)) == ([0, 1, 2], [189, 190, 191])),
                "{name}"
            );
        }
    }

    #[test]
    fn a_probe_is_its_trip_from_its_send_to_its_reader() {
        let start = Instant::now();
        let mut packet = vec![PROBE];
        packet.extend(5_000_000_u64.to_be_bytes());
        let mut reader = Reader::default();
        reader.packet(start + ms(5) + Duration::from_micros(300), start, &packet);
        let probe = Probe {
            sent: ms(5),
            trip: Duration::from_micros(300),
        };
        assert_eq!(
            reader.seen,
            Seen {
                probes: vec![probe],
                ..Seen::default()
            }
        );
    }

    const DARK: [u8; 3] = [0, 0, 0];
    const MARK: [u8; 3] = [100, 155, 0];

    #[test]
    fn a_command_is_carried_by_the_first_frame_after_its_send_that_shows_it() {
        // (name, frames: (arrival ms, pixel of light 5), commands: (mark, b ms, a ms), carried ms or the error)
        type Case = (
            &'static str,
            Vec<(u64, [u8; 3])>,
            Vec<(Option<u8>, u64, u64)>,
            Result<Vec<u64>, &'static str>,
        );
        let cases: [Case; 8] = [
            (
                "a press and its release",
                vec![(10, DARK), (20, MARK), (30, MARK), (40, DARK)],
                vec![(Some(100), 12, 13), (None, 32, 33)],
                Ok(vec![20, 40]),
            ),
            (
                "a release waits for the frame without the press",
                vec![(10, MARK), (20, MARK), (30, DARK)],
                vec![(Some(100), 5, 6), (None, 15, 16)],
                Ok(vec![10, 30]),
            ),
            (
                "the background, a wave and dark are no marks",
                vec![
                    (10, [12, 16, 32]),
                    (20, [255, 0, 0]),
                    (30, [0, 0, 255]),
                    (40, DARK),
                ],
                vec![],
                Ok(vec![]),
            ),
            (
                "a mark before its send",
                vec![(10, DARK), (20, MARK)],
                vec![(Some(100), 25, 26)],
                Err("mark without a press"),
            ),
            (
                "a colour no press makes",
                vec![(10, [10, 20, 0])],
                vec![],
                Err("mark without a press"),
            ),
            (
                "another press's mark",
                vec![(10, [101, 154, 0])],
                vec![(Some(100), 5, 6)],
                Err("mark without a press"),
            ),
            (
                "a press never shown",
                vec![(10, DARK), (20, DARK)],
                vec![(Some(100), 5, 6)],
                Err("never reached a frame"),
            ),
            (
                "a colour that matches the press in red only",
                vec![(10, [100, 100, 0])],
                vec![(Some(100), 5, 6)],
                Err("mark without a press"),
            ),
        ];
        let start = Instant::now();
        for (name, frames, commands, expected) in cases {
            let frames: Vec<Frame> = frames
                .into_iter()
                .map(|(at, pixel)| {
                    let mut lights = [0; 3 * LIGHTS];
                    lights[12..15].copy_from_slice(&pixel);
                    Frame {
                        at: start + ms(at),
                        lights,
                    }
                })
                .collect();
            let sent: Vec<Sent> = commands
                .into_iter()
                .map(|(mark, before, after)| Sent {
                    light: 5,
                    mark,
                    before: start + ms(before),
                    after: start + ms(after),
                })
                .collect();
            let carried = carried(&frames, &sent).map(|u| {
                u.iter()
                    .map(|u| (*u - start).as_millis() as u64)
                    .collect::<Vec<_>>()
            });
            match expected {
                Ok(expected) => assert_eq!(carried, Ok(expected), "{name}"),
                Err(part) => assert!(
                    carried.as_ref().is_err_and(|e| e.contains(part)),
                    "{name}: {carried:?}"
                ),
            }
        }
    }

    fn reported(statistics: [u32; 3], samples: u32) -> Reported {
        Reported {
            statistics,
            samples,
            missing: 0,
        }
    }

    #[test]
    fn a_window_agrees_when_its_report_lies_between_the_outside_bounds() {
        let even = |n: usize, outside: u64, least: u64| -> Vec<Press> {
            let press = Press {
                outside: Duration::from_micros(outside),
                call: Duration::from_micros(200),
                least: Duration::from_micros(least),
            };
            vec![press; n]
        };
        // 1 to 20 ms: nearest rank p50 is the 10th, p95 the 19th, worst the 20th.
        let ranks: Vec<Press> = (1..=20)
            .map(|n| Press {
                outside: ms(n),
                call: Duration::from_micros(200),
                least: ms(n - 1),
            })
            .collect();
        // (name, report, presses, outside statistics or the error)
        type Case = (
            &'static str,
            Reported,
            Vec<Press>,
            Result<[u32; 3], &'static str>,
        );
        // 1 to 21 ms: p50 is the 11th, p95 the 20th: a rank rounded down gives the 10th, the 19th.
        let ranks_of_21: Vec<Press> = (1..=21)
            .map(|n| Press {
                outside: ms(n),
                call: Duration::from_micros(200),
                least: ms(n - 1),
            })
            .collect();
        let cases: [Case; 12] = [
            (
                "at the upper edges",
                reported([5100; 3], 20),
                even(20, 5050, 4500),
                Ok([5050; 3]),
            ),
            (
                "at the lower edges",
                reported([4500; 3], 20),
                even(20, 5050, 4550),
                Ok([5050; 3]),
            ),
            (
                "a count that differs",
                reported([5100; 3], 19),
                even(20, 5050, 4500),
                Err("Health counts 19 commands"),
            ),
            (
                "Health counts more than the reader saw",
                reported([5100; 3], 21),
                even(20, 5050, 4500),
                Err("Health counts 21 commands"),
            ),
            (
                "nearest ranks of 21",
                reported([11_000, 20_000, 21_000], 21),
                ranks_of_21,
                Ok([11_000, 20_000, 21_000]),
            ),
            (
                "fewer than 20",
                reported([5100; 3], 19),
                even(19, 5050, 4500),
                Err("19 commands, fewer than 20"),
            ),
            (
                "more than the outside allows",
                reported([5100, 5200, 5100], 20),
                even(20, 5050, 4500),
                Err("p95 reported 5200 µs is more"),
            ),
            (
                "less than outside minus what it leaves out",
                reported([4400, 5100, 5100], 20),
                even(20, 5050, 4500),
                Err("p50 reported 4400 µs is less"),
            ),
            (
                "in the last bucket nothing is more",
                reported([70_000; 3], 20),
                even(20, 60_000, 0),
                Ok([60_000; 3]),
            ),
            (
                "nearest ranks",
                reported([10_000, 19_000, 20_000], 20),
                ranks.clone(),
                Ok([10_000, 19_000, 20_000]),
            ),
            (
                "the 19th's upper edge",
                reported([10_000, 19_100, 20_000], 20),
                ranks.clone(),
                Ok([10_000, 19_000, 20_000]),
            ),
            (
                "past the 19th's upper edge",
                reported([10_000, 19_200, 20_000], 20),
                ranks,
                Err("p95 reported 19200 µs is more than the outside p95 allows: 19100 µs"),
            ),
        ];
        for (name, reported, presses, expected) in cases {
            let checked =
                check(reported, &presses, Duration::from_micros(300), 400).map(|o| o.most);
            match expected {
                Ok(expected) => assert_eq!(checked, Ok(expected), "{name}"),
                Err(part) => assert!(
                    checked.as_ref().is_err_and(|e| e.contains(part)),
                    "{name}: {checked:?}"
                ),
            }
        }
    }

    #[test]
    fn a_health_window_is_read_from_its_fields() {
        // (a row's window on the wire, what is read)
        let cases = [
            (
                json!({"end_ms": 8000, "press_to_light": {"p50_us": 1, "p95_us": 2, "worst_us": 3, "samples": 4}, "press_missing": 5}),
                Reported {
                    statistics: [1, 2, 3],
                    samples: 4,
                    missing: 5,
                },
            ),
            (
                json!({"end_ms": 8000, "press_to_light": null, "press_missing": 0}),
                reported([0; 3], 0),
            ),
        ];
        for (window, expected) in cases {
            assert_eq!(Reported::of(&window), expected, "{window}");
        }
    }

    #[test]
    fn a_datagram_arrived_as_long_before_now_as_it_waited_on_the_wall_clock() {
        let now = Instant::now();
        let wall = Duration::from_secs(1_800_000_000);
        let us = Duration::from_micros;
        // (receive stamp, how long before now it arrived)
        for (stamp, before) in [
            (wall - us(300), us(300)),
            (wall, Duration::ZERO),
            // the clocks read a microsecond apart: it arrived now, not later
            (wall + us(5), Duration::ZERO),
        ] {
            assert_eq!(arrival(now, wall, stamp), Some(now - before), "{stamp:?}");
        }
    }

    /// One output's run: in each of two windows from 4 s, 20 commands on light 1 (a press
    /// of mark 100, its release, ...) 150 ms apart from `from` into the window, each carried
    /// `outside` after its send with its reply `call` after the send; two probes in each
    /// window, its slowest trip from `slow` and one of 0.1 ms.
    fn run(
        start: Instant,
        outside: Duration,
        call: Duration,
        slow: [Duration; 2],
        from: Duration,
    ) -> (Seen, Vec<Sent>) {
        let first = ms(4000);
        let mut seen = Seen::default();
        let mut sent = Vec::new();
        for (window, slow) in (0..2).zip(slow) {
            let begin = first + WINDOW * window;
            for i in 0..20 {
                let before = start + begin + from + ms(150) * i;
                let mark = (i % 2 == 0).then_some(100);
                sent.push(Sent {
                    light: 1,
                    mark,
                    before,
                    after: before + call,
                });
                let mut lights = [0; 3 * LIGHTS];
                lights[..3].copy_from_slice(&mark.map_or(DARK, |m| [m, 255 - m, 0]));
                seen.frames.push(Frame {
                    at: before + outside,
                    lights,
                });
            }
            for (sent, trip) in [(ms(500), slow), (ms(1000), Duration::from_micros(100))] {
                seen.probes.push(Probe {
                    sent: begin + sent,
                    trip,
                });
            }
        }
        (seen, sent)
    }

    #[test]
    fn a_report_leaves_out_the_reply_and_the_slowest_probe_of_each_window_and_no_more() {
        let start = Instant::now();
        let us = Duration::from_micros;
        // (name, u - b, a - b, each window's slowest probe, first send into its window,
        //  Health's p50, p95 and worst, lost places, windows Health showed, each window's
        //  check 4 bound or the error)
        type Case = (
            &'static str,
            u64,
            u64,
            [u64; 2],
            u64,
            u32,
            u32,
            usize,
            Result<[u32; 2], &'static str>,
        );
        let cases: [Case; 8] = [
            // 20 ms - 0.2 ms - 3 ms = 16.8 ms, and with 1 ms 18.8 ms. With the run's slowest
            // probe the second would be 16.8 ms; with the fastest the first would be 19.7 ms
            (
                "agrees",
                20_000,
                200,
                [3_000, 1_000],
                200,
                19_000,
                0,
                2,
                Ok([16_800, 18_800]),
            ),
            // 20 ms - 5 ms - 3 ms = 12 ms; from b it would be 17 ms
            (
                "the reply is left out, from a",
                20_000,
                5_000,
                [3_000; 2],
                200,
                13_000,
                0,
                2,
                Ok([12_000; 2]),
            ),
            (
                "an engine that says less than it takes",
                20_000,
                200,
                [3_000; 2],
                200,
                10_000,
                0,
                2,
                Err(
                    "p50 reported 10000 µs is less than outside minus what it leaves out: 16800 µs",
                ),
            ),
            (
                "the slowest probe of its own window",
                20_000,
                200,
                [3_000, 1_000],
                200,
                17_000,
                0,
                2,
                Err(
                    "window to 12000 ms: p50 reported 17000 µs is less than outside minus what it leaves out: 18800 µs",
                ),
            ),
            (
                "lost packets",
                20_000,
                200,
                [3_000; 2],
                200,
                19_000,
                1,
                2,
                Err("output 1 lost packets in 1 places"),
            ),
            (
                "a send just after a window end",
                20_000,
                200,
                [3_000; 2],
                150,
                19_000,
                0,
                2,
                Err("a send at 4150 ms came within 200 ms of a window end"),
            ),
            (
                "a send just before a window end",
                20_000,
                200,
                [3_000; 2],
                3_850,
                19_000,
                0,
                2,
                Err("a send at 7850 ms came within 200 ms of a window end"),
            ),
            (
                "a window Health did not show",
                20_000,
                200,
                [3_000; 2],
                200,
                19_000,
                0,
                1,
                Err("Health showed no window to 12000 ms"),
            ),
        ];
        for (name, outside, call, slow, from, health, lost, shown, expected) in cases {
            let (mut seen, sent) = run(start, us(outside), us(call), slow.map(us), ms(from));
            seen.lost = lost;
            let windows: BTreeMap<_, _> = [8000, 12_000][..shown]
                .iter()
                .map(|&end| ((1, end), reported([health; 3], 20)))
                .collect();
            let report = report(&[seen], &sent, &windows, start, ms(4000), 2);
            match expected {
                Ok(least) => {
                    let rows: Vec<Value> = [8000, 12_000]
                        .into_iter()
                        .zip(slow.into_iter().zip(least))
                        .map(|(end_ms, (slow, least))| json!({"end_ms": end_ms, "presses": 20, "probes": 2, "left_out_us": slow, "reported_us": [health, health, health], "outside_us": [outside, outside, outside], "least_us": [least, least, least]}))
                        .collect();
                    assert_eq!(
                        report.map(|r| r["outputs"][0]["windows"].clone()),
                        Ok(Value::from(rows)),
                        "{name}"
                    );
                }
                Err(part) => assert!(
                    report.as_ref().is_err_and(|e| e.contains(part)),
                    "{name}: {report:?}"
                ),
            }
        }
    }
}
