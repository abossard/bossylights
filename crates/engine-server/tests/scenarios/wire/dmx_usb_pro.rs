//! The engine with a DMX USB Pro on a fake serial port (a pseudo terminal): the output
//! runs, goes to reconnecting when its port is "unplugged", and runs again by itself,
//! while another output never fails. An adapter plugged in after its output was set is
//! not found until then, and runs by itself once it is there.

use crate::output_common as common;

use common::Fake;
use engine::{
    Adapter, ChannelNumbers, Command, Device, Engine, Open, Outcome, OutputKind, OutputNumber,
    Rate, Refusal, RunNumbers, Sender, Status, Universes,
};
use outputs::dmx_usb_pro::{DmxUsbPro, Find, MESSAGE, never_wait};
use serialport::{SerialPort, TTYPort};
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

struct Nowhere;

impl Device for Nowhere {
    fn take(&mut self, _frame: &[u8]) -> Outcome {
        Outcome::Accepted
    }
}

/// Reads every message on the test's ends of the fake ports; drops the current end when
/// told to unplug.
fn reader(mut fake: Fake, unplug: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
    let mut end: Option<TTYPort> = None;
    let mut message = [0; MESSAGE];
    while !stop.load(Ordering::Relaxed) {
        if let Some(new) = fake.new_end() {
            end = Some(new);
        }
        if unplug.swap(false, Ordering::Relaxed) {
            end = None;
        }
        match &mut end {
            Some(port) => {
                let _ = port.read(&mut message);
            }
            None => thread::sleep(Duration::from_millis(5)), // poll
        }
    }
}

fn status_until(engine: &mut Engine, done: impl Fn(Status) -> bool) -> Status {
    let started = Instant::now();
    let one = OutputNumber::try_from(1).unwrap();
    loop {
        // Output 1 is in the state from the wake after it was attached.
        let status = engine.latest().output(one).map(|o| o.connection());
        if let Some(status) = status.filter(|s| done(*s)) {
            return status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "stuck at {status:?}"
        );
        thread::sleep(Duration::from_millis(10)); // poll
    }
}

#[test]
fn unplugged_it_reconnects_and_runs_again_by_itself() {
    let (find, fake) = Fake::new(true);
    let (unplug, stop) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let reading = {
        let (unplug, stop) = (Arc::clone(&unplug), Arc::clone(&stop));
        thread::spawn(move || reader(fake, unplug, stop))
    };

    let (mut engine, mut sender) = Engine::start(Universes::ONE).unwrap();
    let run = RunNumbers { first: 1, count: 1 }
        .check(Universes::ONE)
        .unwrap();
    let pro = Box::new(DmxUsbPro::open(find).unwrap());
    let one = OutputNumber::try_from(1).unwrap();
    let two = OutputNumber::try_from(2).unwrap();
    sender.attach(one, pro, run, Rate::FortyFour).unwrap();
    sender
        .attach(two, Box::new(Nowhere), run, Rate::Sixty)
        .unwrap();

    status_until(&mut engine, |s| s == Status::Running);
    unplug.store(true, Ordering::Relaxed);
    let lost = status_until(&mut engine, |s| s != Status::Running);
    assert!(
        matches!(lost, Status::Reconnecting { .. }),
        "lost: {lost:?}"
    );
    status_until(&mut engine, |s| s == Status::Running);

    let picture = engine.fresh().unwrap();
    let pro = picture.output(one).unwrap().counters;
    let other = picture.output(two).unwrap().counters;
    assert!(pro.failed > 0 && pro.accepted > 0, "{pro:?}");
    assert_eq!(other.failed, 0, "the other output never failed");
    stop.store(true, Ordering::Relaxed);
    reading.join().unwrap();
}

/// The statuses output 1 goes through, each once, until `done` holds for the latest.
fn statuses_until(engine: &mut Engine, done: impl Fn(&engine::Output) -> bool) -> Vec<Status> {
    let mut seen = Vec::new();
    let started = Instant::now();
    let one = OutputNumber::try_from(1).unwrap();
    loop {
        // A paused reader misses a status that lasts one frame: Connecting.
        if let Some(output) = engine.latest().output(one) {
            let status = output.connection();
            if seen.last() != Some(&status) {
                seen.push(status);
            }
            if done(&output) {
                return seen;
            }
        }
        assert!(started.elapsed() < Duration::from_secs(10), "{seen:?}");
        thread::sleep(Duration::from_millis(1)); // poll
    }
}

#[test]
fn an_adapter_plugged_in_after_its_output_was_set_is_not_found_then_runs() {
    let (find, fake) = Fake::new(false);
    let plugged = fake.plugged();
    let mut find = Some(find);
    let open: Open = Box::new(move |kind| match (kind, find.take()) {
        (
            OutputKind::DmxUsbPro {
                adapter: Adapter::FIRST,
            },
            Some(find),
        ) => Ok(Box::new(DmxUsbPro::open(find)?) as Box<dyn Device>),
        _ => Err(std::io::Error::other("one adapter in this test")),
    });
    let stop = Arc::new(AtomicBool::new(false));
    let reading = {
        let stop = Arc::clone(&stop);
        thread::spawn(move || reader(fake, Arc::new(AtomicBool::new(false)), stop))
    };
    let (mut engine, mut sender) = Engine::start_with(Universes::ONE, open).unwrap();
    assert_eq!(
        sender.send(set_output(1, None)),
        Ok(()),
        "no adapter is no refusal"
    );

    use Status::{Connecting, NotFound, Running};
    // (the adapter is plugged in, the statuses output 1 may go through until it settles)
    let cases = [
        (false, vec![vec![NotFound]]),
        (
            true,
            // Connecting lasts one frame: a reader that was paused then sees none.
            vec![vec![NotFound, Connecting, Running], vec![NotFound, Running]],
        ),
    ];
    for (is_plugged, may_see) in cases {
        plugged.store(is_plugged, Ordering::Relaxed);
        let settled = *may_see[0].last().unwrap();
        let seen = statuses_until(&mut engine, |output| {
            output.connection() == settled && (is_plugged || output.counters.failed >= 3)
        });
        assert!(
            may_see.contains(&seen),
            "plugged in: {is_plugged}: {seen:?}"
        );
    }
    stop.store(true, Ordering::Relaxed);
    reading.join().unwrap();
}

/// A fake adapter 1 that only one opener may hold, like the real port: a pseudo
/// terminal opened by its name, exclusive. Returns the engine's `open`, the test's end
/// and the port's name.
fn exclusive_adapter() -> (Open, TTYPort, String) {
    let (ours, theirs) = TTYPort::pair().unwrap();
    let name = theirs.name().unwrap();
    let ours = common::end(ours).unwrap();
    let path = name.clone();
    let open: Open = Box::new(move |kind| match kind {
        OutputKind::DmxUsbPro {
            adapter: Adapter::FIRST,
        } => {
            let path = path.clone();
            let find: Find = Box::new(move || {
                // Baud 0: a pseudo terminal takes no speed. Exclusive is the default.
                let port = serialport::new(&path, 0).open_native()?;
                never_wait(&port)?;
                Ok(port)
            });
            Ok(Box::new(DmxUsbPro::open(find)?) as Box<dyn Device>)
        }
        OutputKind::Ddp { .. } | OutputKind::DmxUsbPro { .. } => outputs::open(kind),
    });
    (open, ours, name)
}

/// Reads the test's end until a frame with `level` on channel 1 arrives.
fn hear(end: &mut TTYPort, level: u8) {
    let wanted = [0x7E, 6, 0x01, 0x02, 0, level];
    let mut heard = Vec::new();
    let mut chunk = [0; MESSAGE];
    let started = Instant::now();
    while !heard.windows(wanted.len()).any(|w| w == wanted) {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "never heard {level}"
        );
        if let Ok(read) = end.read(&mut chunk) {
            heard.extend_from_slice(chunk.get(..read).unwrap_or_default());
        }
    }
}

fn set_output(output: u8, rate: Option<Rate>) -> Command {
    Command::SetOutput {
        output: OutputNumber::try_from(output).unwrap(),
        kind: OutputKind::DmxUsbPro {
            adapter: Adapter::FIRST,
        },
        universes: RunNumbers { first: 1, count: 1 },
        rate,
    }
}

fn remove(output: u8) -> Command {
    Command::RemoveOutput {
        output: OutputNumber::try_from(output).unwrap(),
    }
}

fn send_all(sender: &mut Sender, commands: &[Command]) -> Vec<Result<(), Refusal>> {
    commands
        .iter()
        .map(|command| {
            sender.send(*command).map_err(|unsent| match unsent {
                engine::Unsent::Refused(reason) => reason,
                engine::Unsent::Stopping => panic!("the outputs never stop here"),
            })
        })
        .collect()
}

#[test]
fn a_dmx_usb_pro_output_is_set_again_replaced_and_moved_through_send() {
    let ok = Ok(());
    // (commands sent back to back, the answers, the output that drives the adapter)
    let cases = [
        (vec![set_output(1, None)], vec![ok], 1_u8),
        (
            vec![set_output(1, None), set_output(1, Some(Rate::Thirty))],
            vec![ok, ok],
            1,
        ),
        (
            vec![set_output(1, None), remove(1), set_output(1, None)],
            vec![ok, ok, ok],
            1,
        ),
        (
            vec![set_output(1, None), remove(1), set_output(2, None)],
            vec![ok, ok, ok],
            2,
        ),
        (
            vec![set_output(1, None), set_output(2, None)],
            vec![ok, ok], // one adapter drives one output: 2 waits for it, not found
            1,
        ),
    ];
    for (level, (commands, answers, drives)) in (1..).zip(cases) {
        let (open, mut end, _) = exclusive_adapter();
        let (mut engine, mut sender) = Engine::start_with(Universes::ONE, open).unwrap();
        assert_eq!(send_all(&mut sender, &commands), answers, "{commands:?}");
        let set = Command::SetChannel {
            channel: ChannelNumbers {
                universe: 1,
                address: 1,
            },
            value: level,
        };
        assert_eq!(sender.send(set), Ok(()), "{commands:?}");
        hear(&mut end, level);
        // The level is heard above; the status may come a wake later: what came of a frame is
        // counted when its box comes back from the output's thread (#174).
        let started = Instant::now();
        let running = loop {
            let picture = engine.latest();
            let running = (0..engine::MAX_OUTPUTS).find(|slot| {
                OutputNumber::from_slot(*slot)
                    .and_then(|output| picture.output(output))
                    .is_some_and(engine::Output::is_running)
            });
            if running.is_some() || started.elapsed() > Duration::from_secs(5) {
                break running;
            }
            thread::sleep(Duration::from_millis(1)); // poll
        };
        assert_eq!(running, Some(usize::from(drives) - 1), "{commands:?}");
    }
}

#[test]
fn a_removed_dmx_usb_pro_output_frees_its_port_without_another_command() {
    let (open, _end, name) = exclusive_adapter();
    let (_engine, mut sender) = Engine::start_with(Universes::ONE, open).unwrap();
    let answers = send_all(&mut sender, &[set_output(1, None), remove(1)]);
    assert_eq!(answers, vec![Ok(()), Ok(())]);
    let started = Instant::now();
    // A hang guard, not a duration: the port must come free at all.
    while serialport::new(&name, 0).open_native().is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the port stays held"
        );
        thread::sleep(Duration::from_millis(10)); // poll
    }
}

/// theory.md, idea 1: a USB DMX adapter sends at 44, lower only. DDP takes 60.
#[test]
fn usb_dmx_refuses_a_rate_above_44() {
    let (_engine, sender) = engine::Engine::start_with(
        engine::Universes::new(1).unwrap(),
        Box::new(|_| unreachable!("preparing an edit opens no device")),
    )
    .unwrap();
    let base = sender.base();
    let usb = engine::OutputKind::DmxUsbPro {
        adapter: engine_core::Adapter::FIRST,
    };
    let too_fast = Err(engine::Refusal::TooFast {
        most: engine::Rate::FortyFour,
    });
    let cases = [
        (None, Ok(())),
        (Some(engine::Rate::Thirty), Ok(())),
        (Some(engine::Rate::FortyFour), Ok(())),
        (Some(engine::Rate::Sixty), too_fast),
    ];
    for (rate, expected) in cases {
        let prepared = base.prepare(
            engine::Command::SetOutput {
                output: engine::OutputNumber::try_from(1).unwrap(),
                kind: usb,
                universes: engine::RunNumbers { first: 1, count: 1 },
                rate,
            },
            &[],
        );
        assert_eq!(prepared.map(|_| ()), expected, "{rate:?}");
    }
}
