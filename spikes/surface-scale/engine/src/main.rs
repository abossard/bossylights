//! Spike #24, throwaway. The product engine ticks at 44 Hz; this process adds what the
//! spike needs and the product does not have yet: 200 widget values, 100 fixtures, a
//! waveform, and commands from the UI. One JSON frame per product frame.
//!
//! | Route         | What                                                   |
//! |---------------|--------------------------------------------------------|
//! | `GET /health` | `ok`                                                   |
//! | `GET /ws`     | WebSocket: frames out, commands in (see `Command`)     |
//!
//! usage: surface-scale-engine [--listen 127.0.0.1:7745] [--results file.json]

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use engine::Engine;
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::time::MissedTickBehavior;

const WIDGETS: usize = 200;
const FIXTURES: usize = 100;
const WAVE_BINS: usize = 32;
const SAMPLE_RATE: f32 = 44_100.0;

/// What the UI sends. `seq` comes back in the frame that applied the command.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Command {
    Press {
        id: usize,
        down: bool,
        seq: u64,
    },
    Set {
        id: usize,
        x: f32,
        y: f32,
        seq: u64,
    },
    Animate {
        on: bool,
    },
    Results(serde_json::Value),
    /// A line for the engine's log, so a run can be followed from the terminal.
    Log(String),
}

/// One engine frame as the UI sees it.
#[derive(Serialize)]
struct Frame<'a> {
    frame: u64,
    /// (seq, µs from the command arriving here to this frame)
    acks: &'a [(u64, u64)],
    widgets: &'a [[f32; 2]],
    /// r, g, b, intensity, 0 to 255
    fixtures: &'a [[u8; 4]],
    /// peak level per bin, 0 to 1, oldest first
    wave: &'a [f32],
}

struct Shared {
    inbox: Mutex<Vec<(Command, Instant)>>,
    frames: broadcast::Sender<Arc<str>>,
    results: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    let (listen, results) = arguments();
    let engine = Engine::start().unwrap_or_else(|error| {
        eprintln!("surface-scale-engine: the tick thread did not start: {error}");
        std::process::exit(1);
    });
    let (frames, _) = broadcast::channel(64);
    let shared = Arc::new(Shared {
        inbox: Mutex::new(Vec::new()),
        frames,
        results,
    });
    tokio::spawn(produce_frames(engine, Arc::clone(&shared)));

    let api = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/ws", get(connect))
        .with_state(shared);
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .unwrap_or_else(|error| {
            eprintln!("surface-scale-engine: cannot listen on {listen}: {error}");
            std::process::exit(1);
        });
    println!("surface-scale-engine listening on http://{listen}");
    if let Err(error) = axum::serve(listener, api).await {
        eprintln!("surface-scale-engine: {error}");
    }
}

fn arguments() -> (SocketAddr, Option<PathBuf>) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut listen = "127.0.0.1:7745".to_string();
    let mut results = None;
    for pair in args.chunks(2) {
        match pair {
            [flag, value] if flag == "--listen" => listen = value.clone(),
            [flag, value] if flag == "--results" => results = Some(PathBuf::from(value)),
            _ => {
                eprintln!(
                    "usage: surface-scale-engine [--listen 127.0.0.1:7745] [--results file.json]"
                );
                std::process::exit(2);
            }
        }
    }
    let listen = listen.parse().unwrap_or_else(|_| {
        eprintln!("surface-scale-engine: not an address: {listen}");
        std::process::exit(2);
    });
    (listen, results)
}

/// Watches the product engine's frame counter every millisecond. Each new product frame
/// applies the commands that arrived since the last one and sends one spike frame.
async fn produce_frames(engine: Engine, shared: Arc<Shared>) {
    let mut poll = tokio::time::interval(Duration::from_millis(1));
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last = engine.query().frame;
    let mut widgets = [[0.0_f32; 2]; WIDGETS];
    let mut animate = false;
    loop {
        poll.tick().await;
        let frame = engine.query().frame;
        if frame == last {
            continue;
        }
        last = frame;
        let now = Instant::now();
        let commands = std::mem::take(&mut *shared.inbox.lock().unwrap_or_else(|e| e.into_inner()));
        let mut acks = Vec::new();
        for (command, arrived) in commands {
            let waited = u64::try_from(now.duration_since(arrived).as_micros()).unwrap_or(u64::MAX);
            match command {
                Command::Press { id, down, seq } => {
                    if let Some(widget) = widgets.get_mut(id) {
                        widget[0] = if down { 1.0 } else { 0.0 };
                    }
                    acks.push((seq, waited));
                }
                Command::Set { id, x, y, seq } => {
                    if let Some(widget) = widgets.get_mut(id) {
                        *widget = [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)];
                    }
                    acks.push((seq, waited));
                }
                Command::Animate { on } => animate = on,
                Command::Results(_) | Command::Log(_) => {}
            }
        }
        if animate {
            animate_widgets(&mut widgets, frame);
        }
        let fixtures = fixtures(&widgets, frame);
        let wave = wave(frame);
        let json = serde_json::to_string(&Frame {
            frame,
            acks: &acks,
            widgets: &widgets,
            fixtures: &fixtures,
            wave: &wave,
        });
        if let Ok(json) = json {
            let _ = shared.frames.send(json.into()); // no client: nobody to send to
        }
    }
}

fn animate_widgets(widgets: &mut [[f32; 2]; WIDGETS], frame: u64) {
    for (i, widget) in widgets.iter_mut().enumerate() {
        let t = frame as f32 * 0.05 + i as f32 * 0.37;
        *widget = [0.5 + 0.5 * t.sin(), 0.5 + 0.5 * (t * 1.3).cos()];
    }
}

/// A chase across the rig. Widget i (its first value) lifts fixture i % 100.
fn fixtures(widgets: &[[f32; 2]; WIDGETS], frame: u64) -> [[u8; 4]; FIXTURES] {
    let mut out = [[0_u8; 4]; FIXTURES];
    for (i, fixture) in out.iter_mut().enumerate() {
        let lift = widgets
            .get(i)
            .map_or(0.0, |w| w[0])
            .max(widgets.get(i + FIXTURES).map_or(0.0, |w| w[0]));
        let chase = 0.5 + 0.5 * (frame as f32 * 0.12 - i as f32 * 0.25).sin();
        let hue = (frame as f32 * 0.004 + i as f32 / FIXTURES as f32).fract();
        let [r, g, b] = hue_to_rgb(hue);
        let level = |v: f32| (v * 255.0).round() as u8;
        *fixture = [
            level(r),
            level(g),
            level(b),
            level((0.15 + 0.85 * chase).max(lift)),
        ];
    }
    out
}

fn hue_to_rgb(hue: f32) -> [f32; 3] {
    let channel = |offset: f32| (0.5 + 0.5 * (TAU * (hue + offset)).cos()).clamp(0.0, 1.0);
    [channel(0.0), channel(-1.0 / 3.0), channel(1.0 / 3.0)]
}

/// Peak levels of a made-up track for the audio this frame covers: a kick at 120 bpm,
/// a bass line and noise.
fn wave(frame: u64) -> [f32; WAVE_BINS] {
    let per_frame = SAMPLE_RATE / Engine::RATE_HZ as f32;
    let per_bin = (per_frame / WAVE_BINS as f32) as u64;
    let first = (frame as f32 * per_frame) as u64;
    let mut out = [0.0_f32; WAVE_BINS];
    for (bin, peak) in out.iter_mut().enumerate() {
        let start = first + bin as u64 * per_bin;
        *peak = (start..start + per_bin)
            .step_by(4)
            .map(|n| sample(n).abs())
            .fold(0.0, f32::max);
    }
    out
}

fn sample(n: u64) -> f32 {
    let t = n as f32 / SAMPLE_RATE;
    let beat = (t * 2.0).fract();
    let kick = (-beat * 18.0).exp() * (TAU * 55.0 * t).sin();
    let bass = 0.3 * (TAU * 110.0 * t).sin() * (0.5 + 0.5 * (TAU * 0.25 * t).sin());
    let noise = ((n.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1) >> 40) as f32
        / 16_777_216.0
        - 0.5)
        * 0.2;
    (kick * 0.8 + bass + noise).clamp(-1.0, 1.0)
}

async fn connect(socket: WebSocketUpgrade, State(shared): State<Arc<Shared>>) -> Response {
    socket.on_upgrade(move |socket| serve(socket, shared))
}

/// Frames out, commands in, until the client goes away.
async fn serve(mut socket: WebSocket, shared: Arc<Shared>) {
    let mut frames = shared.frames.subscribe();
    loop {
        let event = tokio::select! {
            message = socket.recv() => Event::In(message),
            frame = frames.recv() => Event::Out(frame),
        };
        match event {
            Event::In(Some(Ok(Message::Text(text)))) => receive(&text, &shared),
            Event::In(Some(Ok(_))) => {}
            Event::In(_) => return,
            Event::Out(Ok(frame)) => {
                if socket.send(Message::text(frame.as_ref())).await.is_err() {
                    return;
                }
            }
            Event::Out(Err(broadcast::error::RecvError::Lagged(_))) => {} // slow client skips frames
            Event::Out(Err(broadcast::error::RecvError::Closed)) => return,
        }
    }
}

enum Event {
    In(Option<Result<Message, axum::Error>>),
    Out(Result<Arc<str>, broadcast::error::RecvError>),
}

fn receive(text: &str, shared: &Shared) {
    let arrived = Instant::now();
    match serde_json::from_str::<Command>(text) {
        Ok(Command::Results(results)) => save(&results, shared),
        Ok(Command::Log(line)) => println!("page: {line}"),
        Ok(command) => shared
            .inbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((command, arrived)),
        Err(error) => eprintln!("surface-scale-engine: bad command: {error}"),
    }
}

fn save(results: &serde_json::Value, shared: &Shared) {
    let text = serde_json::to_string_pretty(results).unwrap_or_default();
    match &shared.results {
        Some(path) => match std::fs::write(path, &text) {
            Ok(()) => println!(
                "surface-scale-engine: results written to {}",
                path.display()
            ),
            Err(error) => eprintln!(
                "surface-scale-engine: cannot write {}: {error}",
                path.display()
            ),
        },
        None => println!("{text}"),
    }
}
