//! The engine with a DMX USB Pro on a fake serial port (a pseudo terminal): the output
//! runs, goes to reconnecting when its port is "unplugged", and runs again by itself,
//! while another output never fails.

use engine::{
    ChannelNumbers, Command, Device, Engine, Levels, Open, Outcome, OutputKind, OutputNumber, Rate,
    Refusal, RunNumbers, Sender, Status, Universes,
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
    fn take(&mut self, _frame: &[Levels]) -> Outcome {
        Outcome::Accepted
    }
}

/// Reads every message on the test's ends of the fake ports; drops the current end when
/// told to unplug.
fn reader(mut ends: rtrb::Consumer<TTYPort>, unplug: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
    let mut end: Option<TTYPort> = None;
    let mut message = [0; MESSAGE];
    while !stop.load(Ordering::Relaxed) {
        if let Ok(new) = ends.pop() {
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

fn status_until(engine: &Engine, done: impl Fn(Status) -> bool) -> Status {
    let started = Instant::now();
    loop {
        // Output 1 is in the state from the wake after it was attached.
        let status = engine.query().outputs[0].map(|o| o.status);
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
    let (mut give, ends) = rtrb::RingBuffer::new(8);
    let find: Find = Box::new(move || {
        let (mut ours, port) = TTYPort::pair()?;
        never_wait(&port)?;
        ours.set_timeout(Duration::from_millis(20))?;
        give.push(ours)
            .map_err(|_| std::io::Error::other("too many ports"))?;
        Ok(port)
    });
    let (unplug, stop) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let reading = {
        let (unplug, stop) = (Arc::clone(&unplug), Arc::clone(&stop));
        thread::spawn(move || reader(ends, unplug, stop))
    };

    let (engine, mut sender) = Engine::start(Universes::ONE).unwrap();
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

    status_until(&engine, |s| s == Status::Running);
    unplug.store(true, Ordering::Relaxed);
    let lost = status_until(&engine, |s| s != Status::Running);
    assert!(
        matches!(lost, Status::Reconnecting { .. }),
        "lost: {lost:?}"
    );
    status_until(&engine, |s| s == Status::Running);

    let state = engine.query();
    let pro = state.outputs[0].unwrap().counters;
    let other = state.outputs[1].unwrap().counters;
    assert!(pro.failed > 0 && pro.accepted > 0, "{pro:?}");
    assert_eq!(other.failed, 0, "the other output never failed");
    stop.store(true, Ordering::Relaxed);
    reading.join().unwrap();
}

/// A fake adapter 1 that only one opener may hold, like the real port: a pseudo
/// terminal opened by its name, exclusive. Returns the engine's `open`, the test's end
/// and the port's name.
fn exclusive_adapter() -> (Open, TTYPort, String) {
    let (mut ours, theirs) = TTYPort::pair().unwrap();
    let name = theirs.name().unwrap();
    ours.set_timeout(Duration::from_millis(50)).unwrap();
    let path = name.clone();
    let open: Open = Box::new(move |kind| match kind {
        OutputKind::DmxUsbPro { adapter: 1 } => {
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

fn set_output(output: u8, rate: Option<Rate>) -> Command<ChannelNumbers, RunNumbers, OutputKind> {
    Command::SetOutput {
        output: OutputNumber::try_from(output).unwrap(),
        kind: OutputKind::DmxUsbPro { adapter: 1 },
        universes: RunNumbers { first: 1, count: 1 },
        rate,
    }
}

fn remove(output: u8) -> Command<ChannelNumbers, RunNumbers, OutputKind> {
    Command::RemoveOutput {
        output: OutputNumber::try_from(output).unwrap(),
    }
}

fn send_all(
    sender: &mut Sender,
    commands: &[Command<ChannelNumbers, RunNumbers, OutputKind>],
) -> Vec<Result<(), Refusal>> {
    commands
        .iter()
        .map(|command| sender.send(*command))
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
            vec![ok, Err(Refusal::CannotOpen)], // one adapter drives one output
            1,
        ),
    ];
    for (level, (commands, answers, drives)) in (1..).zip(cases) {
        let (open, mut end, _) = exclusive_adapter();
        let (engine, mut sender) = Engine::start_with(Universes::ONE, open).unwrap();
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
        let state = engine.query();
        let running = state.outputs.iter().position(Option::is_some);
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
