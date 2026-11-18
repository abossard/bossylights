//! usage: dmx-timing [--sink mock | open-dmx=PORT | pro=PORT | ddp[=HOST:PORT]] [--priority default|qos|realtime]
//!                   [--wait sleep|spin] [--seconds 600] [--load THREADS] [--hands unplug|sleep]
//!                   [--api HOST:PORT] [--label NAME] [--json FILE]
//!
//! Sends full DMX frames at 44 Hz for the given time and prints the frame interval
//! statistics. `--load` keeps that many threads busy meanwhile. PORT is a serial port
//! such as /dev/cu.usbserial-XXXX (macOS only). DDP goes to 127.0.0.1:4048 unless given.
//! `--api` is the WLED HTTP API, checked (/json/info) before the run.
//! `--hands` prints when to unplug and replug the adapter, or when to sleep and wake the Mac.

use dmx_timing::sink::{Kind, Sink};
use dmx_timing::{Config, Priority, Report, Wait, start};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

struct Args {
    sink: String,
    config: Config,
    load: usize,
    hands: Option<Hands>,
    api: Option<String>,
    label: String,
    json: Option<String>,
}

#[derive(Clone, Copy)]
enum Hands {
    Unplug,
    Sleep,
}

/// What the person at the machine does, and when: seconds after the start, uptime.
/// The clock stops while the Mac sleeps, so the step after a sleep comes after waking.
fn steps(hands: Hands) -> &'static [(u64, &'static str)] {
    match hands {
        Hands::Unplug => &[
            (0, "Leave the adapter plugged in. First step in 2 minutes."),
            (120, ">>> UNPLUG the adapter now. <<<"),
            (135, ">>> PLUG the adapter back in now. <<<"),
            (
                150,
                "Done, thank you. Leave it running until it prints the result (about 7 min).",
            ),
        ],
        Hands::Sleep => &[
            (0, "Leave the Mac awake. First step in 2 minutes."),
            (
                120,
                ">>> Put the Mac to SLEEP now (Apple menu > Sleep). Wake it after about 1 minute. <<<",
            ),
            (
                150,
                "Once it has slept and woken: done, thank you. Leave it running until it prints the result.",
            ),
        ],
    }
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1).collect()) {
        Ok(args) => args,
        Err(error) => {
            eprintln!(
                "dmx-timing: {error}\n{}",
                include_str!("main.rs")
                    .lines()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            return ExitCode::from(2);
        }
    };
    let sink = match args.sink.split_once('=') {
        None if args.sink == "mock" => Ok(Sink::mock()),
        Some(("open-dmx", port)) => Sink::serial(Kind::OpenDmx, port),
        Some(("pro", port)) => Sink::serial(Kind::Pro, port),
        None if args.sink == "ddp" => Sink::ddp("127.0.0.1:4048"),
        Some(("ddp", address)) => Sink::ddp(address),
        _ => Err(format!("unknown sink: {}", args.sink)),
    };
    let sink = match sink {
        Ok(sink) => sink,
        Err(error) => {
            eprintln!("dmx-timing: {error}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(api) = &args.api
        && let Err(error) = check_api(api)
    {
        eprintln!("dmx-timing: the WLED API at {api} does not answer: {error}");
        return ExitCode::FAILURE;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let load: Vec<_> = (0..args.load).map(|_| busy(Arc::clone(&stop))).collect();
    eprintln!(
        "dmx-timing: {} for {:?} with {} busy threads",
        args.label, args.config.duration, args.load
    );
    let report = match start(args.config, sink) {
        Ok(running) => {
            // A hard deadline: whatever the device does, the run ends.
            let deadline =
                std::time::Instant::now() + args.config.duration + Duration::from_secs(10);
            if let Some(hands) = args.hands {
                prompt(hands, &running.looped);
            }
            while !running.is_finished() {
                if std::time::Instant::now() > deadline {
                    eprintln!(
                        "dmx-timing: the output thread did not end 10 s after the run; giving up"
                    );
                    std::process::exit(3);
                }
                thread::sleep(Duration::from_millis(100));
            }
            running.finish()
        }
        Err(error) => {
            eprintln!("dmx-timing: cannot start the output thread: {error}");
            return ExitCode::FAILURE;
        }
    };
    stop.store(true, Ordering::Relaxed);
    load.into_iter().for_each(|t| drop(t.join()));

    // the kind only: a serial port's name carries the adapter's serial number
    let kind = args.sink.split('=').next().unwrap_or_default();
    let json = json(&args.label, kind, args.load, &report);
    println!("{}", text(&args.label, &report));
    if let Some(file) = args.json
        && let Err(error) = std::fs::write(&file, format!("{json}\n"))
    {
        eprintln!("dmx-timing: cannot write {file}: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn parse(args: Vec<String>) -> Result<Args, String> {
    let mut parsed = Args {
        sink: "mock".into(),
        config: Config {
            priority: Priority::Default,
            wait: Wait::Sleep,
            duration: Duration::from_secs(600),
        },
        load: 0,
        hands: None,
        api: None,
        label: String::new(),
        json: None,
    };
    for pair in args.chunks(2) {
        let [flag, value] = pair else {
            return Err(format!("{} needs a value", pair[0]));
        };
        match flag.as_str() {
            "--sink" => parsed.sink = value.clone(),
            "--priority" => {
                parsed.config.priority = match value.as_str() {
                    "default" => Priority::Default,
                    "qos" => Priority::Qos,
                    "realtime" => Priority::Realtime,
                    _ => return Err(format!("unknown priority: {value}")),
                }
            }
            "--wait" => {
                parsed.config.wait = match value.as_str() {
                    "sleep" => Wait::Sleep,
                    "spin" => Wait::Spin,
                    _ => return Err(format!("unknown wait: {value}")),
                }
            }
            "--seconds" => {
                parsed.config.duration =
                    Duration::from_secs(value.parse().map_err(|_| format!("not seconds: {value}"))?)
            }
            "--load" => {
                parsed.load = value
                    .parse()
                    .map_err(|_| format!("not a thread count: {value}"))?
            }
            "--hands" => {
                parsed.hands = Some(match value.as_str() {
                    "unplug" => Hands::Unplug,
                    "sleep" => Hands::Sleep,
                    _ => return Err(format!("unknown hands step: {value}")),
                })
            }
            "--api" => parsed.api = Some(value.clone()),
            "--label" => parsed.label = value.clone(),
            "--json" => parsed.json = Some(value.clone()),
            _ => return Err(format!("unknown flag: {flag}")),
        }
    }
    if parsed.label.is_empty() {
        parsed.label = format!(
            "{} {:?} {:?}",
            parsed.sink.split('=').next().unwrap_or_default(),
            parsed.config.priority,
            parsed.config.wait
        )
        .to_lowercase();
    }
    Ok(parsed)
}

/// `GET /json/info` must answer 200 within a second.
fn check_api(address: &str) -> Result<(), String> {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    let timeout = Duration::from_secs(1);
    let socket = address
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let mut stream = TcpStream::connect_timeout(&socket, timeout).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    write!(stream, "GET /json/info HTTP/1.0\r\nHost: {address}\r\n\r\n")
        .map_err(|e| e.to_string())?;
    let mut status = [0_u8; 12];
    stream.read_exact(&mut status).map_err(|e| e.to_string())?;
    match &status[9..12] {
        b"200" => Ok(()),
        other => Err(format!("status {}", String::from_utf8_lossy(other))),
    }
}

/// Prints each step at its time, with a bell, on this thread (not the output thread).
/// Stops when the run ends.
fn prompt(hands: Hands, looped: &AtomicBool) {
    let start = std::time::Instant::now();
    for &(at, text) in steps(hands) {
        while start.elapsed() < Duration::from_secs(at) {
            if looped.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        eprintln!("\x07[{:>3} s] {text}", start.elapsed().as_secs());
    }
}

/// A thread that keeps one core busy with arithmetic and memory traffic until `stop`.
fn busy(stop: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut memory = vec![0_u64; 1 << 20];
        let mut x = 1_u64;
        while !stop.load(Ordering::Relaxed) {
            for (i, slot) in memory.iter_mut().enumerate().step_by(16) {
                x = x
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(i as u64);
                *slot ^= x;
            }
        }
        std::hint::black_box(&memory);
    })
}

fn ms(micros: u64) -> f64 {
    micros as f64 / 1000.0
}

fn text(label: &str, r: &Report) -> String {
    let i = &r.intervals;
    format!(
        "{label}: priority {}, {} frames, interval ms min {:.2} mean {:.2} p50 {:.2} p99 {:.2} max {:.2}, off by >2 ms: {} ({:.2}%), \
         late p99 {:.2} max {:.2} ms, resyncs {}, send errors {} (dropped, link busy: {}), reconnects {}",
        match &r.priority {
            Ok(()) => format!("{:?}", r.config.priority),
            Err(e) => format!("not set ({e})"),
        },
        r.frames,
        ms(i.min),
        i.mean / 1000.0,
        ms(i.p50),
        ms(i.p99),
        ms(i.max),
        i.off,
        if i.count == 0 {
            0.0
        } else {
            100.0 * i.off as f64 / i.count as f64
        },
        ms(r.late_p99),
        ms(r.late_max),
        r.resyncs,
        r.send_errors,
        r.dropped,
        r.reconnects,
    )
}

fn json(label: &str, sink: &str, load: usize, r: &Report) -> String {
    let i = &r.intervals;
    let priority = match &r.priority {
        Ok(()) => "\"ok\"".to_string(),
        Err(e) => format!("{e:?}"),
    };
    format!(
        "{{\"label\":{label:?},\"sink\":{sink:?},\"os\":{os:?},\"priority\":\"{p:?}\",\"prioritySet\":{priority},\"wait\":\"{w:?}\",\
         \"seconds\":{s},\"loadThreads\":{load},\"frames\":{f},\"intervalMs\":{{\"min\":{:.3},\"mean\":{:.3},\"p50\":{:.3},\"p99\":{:.3},\"max\":{:.3}}},\
         \"offBy2ms\":{off},\"lateMs\":{{\"p99\":{:.3},\"max\":{:.3}}},\"resyncs\":{rs},\"sendErrors\":{se},\"dropped\":{wb},\"reconnects\":{rc}}}",
        ms(i.min),
        i.mean / 1000.0,
        ms(i.p50),
        ms(i.p99),
        ms(i.max),
        ms(r.late_p99),
        ms(r.late_max),
        os = std::env::consts::OS,
        p = r.config.priority,
        w = r.config.wait,
        s = r.config.duration.as_secs(),
        f = r.frames,
        off = i.off,
        rs = r.resyncs,
        se = r.send_errors,
        rc = r.reconnects,
        wb = r.dropped,
    )
}
