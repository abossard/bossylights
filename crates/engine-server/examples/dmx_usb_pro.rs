//! The checks that need a real DMX USB Pro. Each one runs the engine with the adapter at
//! 44 a second and a second output at 60 a second, and says with a bell what to do and
//! when. The port is closed when it ends.
//!
//! | Check | Command |
//! |---|---|
//! | 10 minutes, no interval off, nothing dropped | `cargo run --release -p engine-server --example dmx_usb_pro -- <port> ten-minutes` |
//! | unplug and plug back in | `cargo run --release -p engine-server --example dmx_usb_pro -- <port> unplug` |
//! | sleep and wake | `cargo run --release -p engine-server --example dmx_usb_pro -- <port> sleep` |
//!
//! `<port>` is the adapter's port, such as `/dev/cu.usbserial-...` on macOS. The engine
//! finds the adapter by its USB identity; the port only says which adapter.

use engine::{
    ChannelNumbers, Command, Counters, Device, Engine, Levels, Outcome, OutputKind, OutputNumber,
    Rate, RunNumbers, State, Status, Universes,
};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

const USAGE: &str = "usage: dmx_usb_pro <port> ten-minutes|unplug|sleep";
const BELL: &str = "\x07";
/// Before and after the hands-on step.
const CALM: Duration = Duration::from_secs(15);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match (args.first(), args.get(1).map(String::as_str)) {
        (Some(port), Some(check @ ("ten-minutes" | "unplug" | "sleep"))) => run(port, check),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(problem) => {
            eprintln!("dmx_usb_pro: {problem}");
            ExitCode::FAILURE
        }
    }
}

/// Takes every frame and sends it nowhere: the other output, whose timing must hold.
struct Nowhere;

impl Device for Nowhere {
    fn take(&mut self, _frame: &[Levels]) -> Outcome {
        Outcome::Accepted
    }
}

fn number(n: u8) -> Result<OutputNumber, String> {
    OutputNumber::try_from(n).map_err(str::to_string)
}

fn run(port: &str, check: &str) -> Result<bool, String> {
    let adapter = outputs::dmx_usb_pro::adapter_of(port)
        .map_err(|e| format!("not a DMX USB Pro port: {e}"))?;
    let (engine, mut sender) = Engine::start(Universes::ONE).map_err(|e| e.to_string())?;
    let pro = Command::SetOutput {
        output: number(1)?,
        kind: OutputKind::DmxUsbPro { adapter },
        universes: RunNumbers { first: 1, count: 1 },
        rate: None,
    };
    sender.send(pro).map_err(|r| format!("refused: {r:?}"))?;
    let run = RunNumbers { first: 1, count: 1 }
        .check(Universes::ONE)
        .map_err(|r| format!("{r:?}"))?;
    sender
        .attach(number(2)?, Box::new(Nowhere), run, Rate::Sixty)
        .map_err(|r| format!("refused: {r:?}"))?;
    println!("adapter {adapter} at 44 a second, a second output at 60 a second.");

    let mut watch = Watch {
        engine: &engine,
        started: Instant::now(),
        level: 0,
    };
    let passed = match check {
        "ten-minutes" => ten_minutes(&mut watch, &mut sender),
        "unplug" => hands_on(&mut watch, &mut sender, Step::Unplug),
        _ => hands_on(&mut watch, &mut sender, Step::Sleep),
    };
    drop(sender);
    drop(engine); // stops the output thread; the port is closed with its device
    println!("port closed.");
    Ok(passed)
}

/// Watches the engine and keeps channel 1 moving, so every frame differs.
struct Watch<'a> {
    engine: &'a Engine,
    started: Instant,
    level: u8,
}

impl Watch<'_> {
    fn tick(&mut self, sender: &mut engine::Sender) -> State {
        thread::sleep(Duration::from_millis(250));
        self.level = self.level.wrapping_add(1);
        let channel = ChannelNumbers {
            universe: 1,
            address: 1,
        };
        let _ = sender.send(Command::SetChannel {
            channel,
            value: self.level,
        });
        self.engine.query()
    }

    fn line(&self, state: &State) {
        let [pro, other] = outputs(state);
        println!(
            "{:>5} s  adapter: {}  |  other: {}",
            self.started.elapsed().as_secs(),
            describe(pro),
            describe(other)
        );
    }
}

fn outputs(state: &State) -> [Option<engine::Output>; 2] {
    [state.outputs[0], state.outputs[1]]
}

fn describe(output: Option<engine::Output>) -> String {
    output.map_or("none".into(), |o| {
        let c = o.counters;
        format!(
            "{:?}, accepted {}, dropped {}, failed {}, off by > 2 ms {}",
            o.status, c.accepted, c.dropped, c.failed, c.off_beat
        )
    })
}

fn counters(output: Option<engine::Output>) -> Counters {
    output.map(|o| o.counters).unwrap_or_default()
}

fn ten_minutes(watch: &mut Watch, sender: &mut engine::Sender) -> bool {
    println!("10 minutes. Nothing to do; a line every 30 s.");
    let end = Duration::from_secs(600);
    let mut state = watch.engine.query();
    let mut shown = 0;
    while watch.started.elapsed() < end {
        state = watch.tick(sender);
        let seconds = watch.started.elapsed().as_secs();
        if seconds / 30 > shown {
            shown = seconds / 30;
            watch.line(&state);
        }
    }
    let [pro, _] = outputs(&state).map(counters);
    let lateness = OutputNumber::try_from(1)
        .ok()
        .and_then(|n| watch.engine.lateness(n));
    println!("{BELL}\nresult after 600 s:");
    println!("  adapter: {}", describe(state.outputs[0]));
    println!("  other:   {}", describe(state.outputs[1]));
    if let Some(l) = lateness {
        println!(
            "  adapter lateness: p99 {} µs, worst {} µs",
            l.p99.as_micros(),
            l.worst.as_micros()
        );
    }
    // The criterion is the adapter's; the other output is shown for comparison.
    let passed = pro.off_beat == 0 && pro.dropped == 0 && pro.failed == 0;
    println!("{}", if passed { "PASSED" } else { "FAILED" });
    passed
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Unplug,
    Sleep,
}

fn hands_on(watch: &mut Watch, sender: &mut engine::Sender, step: Step) -> bool {
    let what = match step {
        Step::Unplug => "unplug the adapter, then plug it back in",
        Step::Sleep => "put the Mac to sleep, then wake it",
    };
    println!(
        "Check: {what}. First {} s calm. Then a bell tells you what to do. Watch this window.",
        CALM.as_secs()
    );
    let calm_until = watch.started.elapsed() + CALM;
    let mut state = watch.engine.query();
    while watch.started.elapsed() < calm_until {
        state = watch.tick(sender);
    }
    watch.line(&state);

    match step {
        Step::Unplug => {
            println!("{BELL}\n>>> NOW: UNPLUG the DMX USB Pro. <<<\n");
            let lost = wait_until(watch, sender, Duration::from_secs(60), |s| {
                s.outputs[0].is_some_and(|o| o.status != Status::Running)
            });
            let Some(lost) = lost else {
                println!("FAILED: the adapter did not leave Running within 60 s.");
                return false;
            };
            watch.line(&lost);
            println!("{BELL}\n>>> NOW: PLUG the DMX USB Pro back in. <<<\n");
        }
        Step::Sleep => {
            println!(
                "{BELL}\n>>> NOW: put the Mac to sleep (Apple menu, Sleep). Wake it after about 30 s. <<<\n"
            );
            let (wall, mono) = (SystemTime::now(), Instant::now());
            let woke = wait_until(watch, sender, Duration::from_secs(600), |_| {
                let wall = wall.elapsed().unwrap_or_default();
                wall > mono.elapsed() + Duration::from_secs(5)
            });
            if woke.is_none() {
                println!("FAILED: no sleep seen within 10 minutes.");
                return false;
            }
            println!("{BELL}\nawake. Waiting for the adapter to run again.");
        }
    }
    let back = Instant::now();
    let running = wait_until(watch, sender, Duration::from_secs(120), |s| {
        s.outputs[0].is_some_and(|o| o.status == Status::Running)
    });
    let Some(running) = running else {
        println!("FAILED: the adapter did not run again within 120 s.");
        return false;
    };
    println!(
        "running again {} ms after that.",
        back.elapsed().as_millis()
    );
    watch.line(&running);

    println!("{} s calm to finish.", CALM.as_secs());
    let [pro_before, other_before] = outputs(&running).map(counters);
    let calm_until = watch.started.elapsed() + CALM;
    let mut state = running;
    while watch.started.elapsed() < calm_until {
        state = watch.tick(sender);
    }
    let [pro, other] = outputs(&state).map(counters);
    println!("{BELL}\nresult:");
    println!("  adapter: {}", describe(state.outputs[0]));
    println!("  other:   {}", describe(state.outputs[1]));
    let pro_ok = state.outputs[0].is_some_and(|o| o.status == Status::Running)
        && pro.accepted > pro_before.accepted
        && pro.off_beat == pro_before.off_beat;
    // Unplugging touches nothing else: the other output keeps its beat all along. A sleep
    // stops every clock once, so there it must keep its beat after the wake.
    let other_ok = match step {
        Step::Unplug => other.off_beat == 0,
        Step::Sleep => other.off_beat == other_before.off_beat,
    };
    let passed = pro_ok && other_ok;
    println!("{}", if passed { "PASSED" } else { "FAILED" });
    passed
}

fn wait_until(
    watch: &mut Watch,
    sender: &mut engine::Sender,
    limit: Duration,
    done: impl Fn(&State) -> bool,
) -> Option<State> {
    let until = Instant::now() + limit;
    while Instant::now() < until {
        let state = watch.tick(sender);
        if done(&state) {
            return Some(state);
        }
    }
    None
}
