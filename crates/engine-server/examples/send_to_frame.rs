//! The reference scorecard. `baseline [seconds]` prints the original one-output
//! send-to-frame p95 in microseconds. `m2|m3|worst [seconds]` prints JSON with
//! lateness p99 and command-to-device-handoff p95 in nanoseconds for each output.

use engine::{
    ChannelNumbers, Color, Command, Device, Engine, Levels, Outcome, OutputKind, OutputNumber,
    Rate, RunNumbers, Timing, Timings, Universes,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::error::Error;
use std::io::ErrorKind;
use std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use workload::{Performed, Workload};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "baseline".into());
    let seconds = args.next().as_deref().map(str::parse::<u64>).transpose();
    let Ok(seconds) = seconds else {
        eprintln!("seconds must be a positive integer");
        return ExitCode::FAILURE;
    };
    if args.next().is_some() || seconds == Some(0) {
        eprintln!("send_to_frame [baseline|m2|m3|worst] [seconds]");
        return ExitCode::FAILURE;
    }
    let profile = match mode.as_str() {
        "baseline" => return baseline(Duration::from_secs(seconds.unwrap_or(10))),
        "m2" => Workload::M2,
        "m3" => Workload::M3,
        "worst" => Workload::Worst,
        other => {
            eprintln!("unknown run {other:?}");
            return ExitCode::FAILURE;
        }
    };
    match benchmark(profile, Duration::from_secs(seconds.unwrap_or(60))) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("send_to_frame: {error}");
            ExitCode::FAILURE
        }
    }
}

fn baseline(duration: Duration) -> ExitCode {
    let Ok((mut engine, mut sender)) = Engine::start(Universes::ONE) else {
        eprintln!("send_to_frame: the engine did not start");
        return ExitCode::FAILURE;
    };
    let output = OutputNumber::try_from(1);
    let run = RunNumbers { first: 1, count: 1 }.check(Universes::ONE);
    let attached = output.ok().zip(run.ok()).is_some_and(|(output, run)| {
        sender
            .attach(output, Box::new(Nowhere), run, Rate::FortyFour)
            .is_ok()
    });
    if !attached {
        eprintln!("send_to_frame: the output was not attached");
        return ExitCode::FAILURE;
    }
    let started = Instant::now();
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut value = 0_u8;
    while started.elapsed() < duration {
        // Uneven gaps of 5 to 36 ms, so sends land everywhere within a frame.
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        thread::sleep(Duration::from_millis(5 + seed % 32));
        value = value.wrapping_add(1);
        let channel = ChannelNumbers {
            universe: 1,
            address: 1,
        };
        if let Err(refusal) = sender.send(Command::SetChannel { channel, value }) {
            eprintln!("send_to_frame: refused: {refusal:?}");
            return ExitCode::FAILURE;
        }
    }
    thread::sleep(Duration::from_millis(100)); // the last frame
    match engine.fresh().send_to_frame_p95() {
        Some(p95) => {
            println!("{}", p95.as_micros());
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("send_to_frame: no command reached a frame");
            ExitCode::FAILURE
        }
    }
}

/// Takes every frame and sends it nowhere.
struct Nowhere;

impl Device for Nowhere {
    fn take(&mut self, _frame: &[Levels]) -> Outcome {
        Outcome::Accepted
    }
}

// Resolve each accepted handoff against the FIFO prefix of that exact frame.
// A passed-over frame counts as a drop and leaves its commands for the next one.
#[derive(Default)]
struct OutputSamples {
    pending: BTreeMap<Duration, u64>,
    past_window: bool,
    latency: Vec<Duration>,
    lateness: Vec<Duration>,
    drops: usize,
}

struct Samples {
    outputs: Vec<OutputSamples>,
    setup: u64,
    window: std::ops::Range<Duration>,
    commands: Vec<Duration>,
}

impl Samples {
    fn new(outputs: usize, setup: u64, start: Duration, end: Duration) -> Self {
        Self {
            outputs: (0..outputs).map(|_| OutputSamples::default()).collect(),
            setup,
            window: start..end,
            commands: Vec::new(),
        }
    }

    fn record(&mut self, event: Timing) -> Result<(), String> {
        match event {
            Timing::Applied { number, at } if number > self.setup => {
                if number != self.setup + self.commands.len() as u64 + 1 {
                    return Err(format!("missing command before {number}"));
                }
                if !self.window.contains(&at) {
                    return Err(format!(
                        "command {number} admitted outside the run at {at:?}"
                    ));
                }
                self.commands.push(at);
            }
            Timing::Applied { .. } => {}
            Timing::Due {
                output,
                due,
                applied,
            } => {
                let row = &mut self.outputs[output.slot()];
                if row.pending.insert(due, applied).is_some() {
                    return Err(format!("duplicate due time {due:?} on {output:?}"));
                }
                row.past_window |= due >= self.window.end;
            }
            Timing::Finished {
                output,
                due,
                at,
                outcome,
            } => {
                let row = &mut self.outputs[output.slot()];
                let Some(applied) = row.pending.remove(&due) else {
                    // discard() can split a setup handoff from its completion.
                    if due < self.window.start {
                        return Ok(());
                    }
                    return Err(format!("no due time {due:?} for {output:?}"));
                };
                let measured = self.window.contains(&due);
                match outcome {
                    Outcome::Dropped => row.drops += usize::from(measured),
                    Outcome::Failed => return Err(format!("{output:?} failed at {due:?}")),
                    Outcome::Accepted => {
                        if measured {
                            row.lateness.push(at.saturating_sub(due));
                        }
                        let carried = applied.saturating_sub(self.setup) as usize;
                        let new =
                            self.commands
                                .get(row.latency.len()..carried)
                                .ok_or_else(|| {
                                    format!("invalid {output:?} cutoff {applied} at {due:?}")
                                })?;
                        for &sent in new {
                            row.latency
                                .push(at.checked_sub(sent).ok_or("handoff before send")?);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn read(&mut self, timings: &mut Timings) -> Result<(), Box<dyn Error>> {
        while let Some(event) = timings.pop()? {
            self.record(event)?;
        }
        Ok(())
    }

    fn complete(&self, tail: u64) -> bool {
        self.setup + self.commands.len() as u64 == tail
            && self.outputs.iter().all(|row| {
                row.past_window
                    && row.pending.range(..self.window.end).next().is_none()
                    && row.latency.len() == self.commands.len()
            })
    }

    fn report(&mut self, performance: &Performed) -> Result<serde_json::Value, Box<dyn Error>> {
        fn percentile(values: &mut [Duration], percent: usize) -> Result<u128, &'static str> {
            if values.is_empty() {
                return Err("no samples");
            }
            values.sort_unstable();
            Ok(values[(values.len() * percent).div_ceil(100) - 1].as_nanos())
        }
        let mut outputs = Vec::new();
        for (slot, row) in self.outputs.iter_mut().enumerate() {
            let mut manual = Vec::with_capacity(performance.manual.len());
            for &number in &performance.manual {
                let index = number
                    .checked_sub(self.setup + 1)
                    .ok_or("press belongs to setup")?;
                manual.push(
                    *row.latency
                        .get(usize::try_from(index)?)
                        .ok_or("press never reached output")?,
                );
            }
            outputs.push(json!({
                "output": slot + 1,
                "hz": rate(slot).per_second(),
                "p99": percentile(&mut row.lateness, 99)?,
                "p95": percentile(&mut manual, 95)?,
                "drops": row.drops,
            }));
        }
        Ok(json!({
            "seconds": (self.window.end - self.window.start).as_secs(),
            "events": performance.events,
            "pressSamples": performance.manual.len(),
            "outputs": outputs,
        }))
    }
}

fn rate(slot: usize) -> Rate {
    [Rate::FortyFour, Rate::Sixty][slot % 2]
}

fn benchmark(profile: Workload, duration: Duration) -> Result<serde_json::Value, Box<dyn Error>> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    let SocketAddr::V4(address) = socket.local_addr()? else {
        return Err("expected a loopback IPv4 address".into());
    };
    socket.set_read_timeout(Some(Duration::from_millis(50)))?;
    thread::scope(|scope| {
        // Dropping the only sender also stops the receiver on an early return.
        let (stop, stopping) = mpsc::channel::<()>();
        let drain = scope.spawn(move || -> std::io::Result<u64> {
            let mut packets = 0;
            let mut packet = [0; 1500];
            while matches!(stopping.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                match socket.recv(&mut packet) {
                    Ok(_) => packets += 1,
                    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(packets)
        });
        let result = measure(profile, duration, address);
        drop(stop);
        let packets = drain.join().map_err(|_| "DDP reader panicked")??;
        let report = result?;
        if packets == 0 {
            return Err("no DDP datagrams arrived".into());
        }
        eprintln!("DDP datagrams read: {packets}");
        Ok(report)
    })
}

fn measure(
    profile: Workload,
    duration: Duration,
    address: SocketAddrV4,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let universes = Universes::new(engine::MAX_UNIVERSES).ok_or("invalid universe count")?;
    let (mut engine, mut sender, mut timings) = Engine::start_measured(universes)?;
    let outputs = if profile == Workload::Worst {
        engine::MAX_OUTPUTS
    } else {
        2
    };
    for slot in 0..outputs {
        sender
            .send(Command::SetOutput {
                output: OutputNumber::try_from((slot + 1) as u8)?,
                kind: OutputKind::Ddp {
                    address,
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
    workload::prepare(&mut sender, profile)?;
    let setup = sender.sent();
    let picture = engine.fresh();
    let mut last_frame = picture.state.frame;
    if picture.applied() != setup {
        return Err("setup did not reach the tick".into());
    }
    timings.discard();
    // A device may be handed its frame slightly before its nominal deadline.
    // Drain the trace for one full period before the first measured deadline.
    let lead = Rate::FortyFour.period();
    let since = timings.now() + lead;
    let started = Instant::now() + lead;
    let end = since
        .checked_add(duration)
        .ok_or("run duration exceeds clock range")?;
    let mut samples = Samples::new(outputs, setup, since, end);
    let mut pictures = 0_u64;
    let (performance, tail) = thread::scope(|scope| -> Result<_, Box<dyn Error>> {
        let player = scope.spawn(|| {
            let performance = workload::perform(&mut sender, started, duration)?;
            Ok::<_, String>((performance, sender.sent()))
        });
        while started.elapsed() < duration || !player.is_finished() {
            let frame = engine.latest().state.frame;
            if frame != last_frame {
                pictures += 1;
                last_frame = frame;
            }
            samples.read(&mut timings)?;
            thread::sleep(Duration::from_millis(1));
        }
        Ok(player.join().map_err(|_| "workload thread panicked")??)
    })?;
    // A late result can arrive after the last deadline of the run. Wait for
    // every in-window outcome and every measured command on every output.
    let draining = Instant::now();
    while !samples.complete(tail) {
        if draining.elapsed() > Duration::from_secs(10) {
            return Err(format!(
                "incomplete timing samples for commands {}..={tail}",
                setup + 1
            )
            .into());
        }
        engine.latest();
        samples.read(&mut timings)?;
        thread::sleep(Duration::from_millis(1));
    }
    if pictures == 0 {
        return Err("snapshot reader received no new frames".into());
    }
    eprintln!("snapshots read: {pictures}");
    samples.report(&performance)
}

mod workload {
    use engine::{
        ChannelNumbers, Color, Command, Light, Member, MemberList, Members, Palette, Pattern, Rank,
        Refusal, Rgb, Sender, Sequence, SequenceItem, Swatches, Target, Text, Use, WaveTarget,
        When,
    };
    use engine_core::{Pixels, Step};
    use std::sync::Mutex;
    use std::thread;
    use std::time::{Duration, Instant};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Workload {
        M2,
        M3,
        Worst,
    }

    impl Workload {
        /// The same 10,000-pixel rig for the matched runs. At the limit all 10,922 whole
        /// RGB pixels fit, and the three target sizes jointly fill the visit budget.
        fn strips(self) -> (u16, u16, u16) {
            match self {
                Self::M2 | Self::M3 => (1024, 256, 10_000 - 1024 - 256),
                Self::Worst => (77, 198, 10_922 - 77 - 198),
            }
        }

        fn waves(self) -> usize {
            match self {
                Self::M2 => 0,
                Self::M3 => 1,
                Self::Worst => engine_core::PLAYS - 2,
            }
        }
    }

    const BACKGROUND: Rgb = Rgb {
        red: 12000,
        green: 16000,
        blue: 32000,
    };
    const RED: Rgb = Rgb {
        red: u16::MAX,
        ..Rgb::BLACK
    };
    const BLUE: Rgb = Rgb {
        blue: u16::MAX,
        ..Rgb::BLACK
    };
    const ALL: &str = "All strips";
    const WAVE: &str = "Wave strips";

    fn text(value: &str) -> Text {
        Text::new(value).expect("short benchmark names")
    }

    fn group(name: &str, members: &[Member]) -> Command {
        Command::SetGroup {
            name: text(name),
            members: MemberList::Fits(Members::new(members).expect("small benchmark group")),
        }
    }

    fn wave_group(reverse: bool) -> Command {
        let order = if reverse { [2, 1] } else { [1, 2] };
        group(WAVE, &order.map(|fixture| Member::Fixture { fixture }))
    }

    fn colour(target: Target, rgb: Rgb) -> Command {
        Command::SetColour {
            target,
            red: rgb.red,
            green: rgb.green,
            blue: rgb.blue,
        }
    }

    fn send(sender: &mut Sender, command: Command) -> Result<(), String> {
        sender
            .send(command)
            .map_err(|r| format!("{command:?}: {r:?}"))
    }

    fn step(look: u16, name: &str, target: Target, rgb: Rgb, intensity: Option<u16>) -> Command {
        Command::PutStep {
            look,
            name: text(name),
            step: Step {
                target,
                values: Light {
                    intensity,
                    colour: Some(rgb),
                    ..Light::DARK
                },
            },
        }
    }

    /// Attach the streaming outputs before calling this, so all spanning strips can be admitted.
    pub(crate) fn prepare(sender: &mut Sender, workload: Workload) -> Result<(), String> {
        let (wave, sequence, background) = workload.strips();
        let mut channels: u16 = 0;
        for count in [wave / 2, wave - wave / 2, sequence, background] {
            send(
                sender,
                Command::PatchStrip {
                    name: text("RGB strip"),
                    pixels: Pixels::try_from(count)?,
                    color: Color::Rgb,
                    at: ChannelNumbers {
                        universe: channels / 512 + 1,
                        address: channels % 512 + 1,
                    },
                },
            )?;
            channels += count * 3;
        }
        let all = Target::Group { group: text(ALL) };
        send(
            sender,
            group(
                ALL,
                &[1, 2, 3, 4].map(|fixture| Member::Fixture { fixture }),
            ),
        )?;
        send(sender, wave_group(false))?;
        if workload == Workload::M2 {
            send(
                sender,
                Command::SetIntensity {
                    target: all,
                    intensity: u16::MAX,
                },
            )?;
            send(sender, colour(all, BACKGROUND))?;
            eprintln!("10,000 RGB pixels, direct colour and intensity, no playing looks");
            return Ok(());
        }

        let probe = "two lights";
        if workload == Workload::Worst {
            send(
                sender,
                group(
                    probe,
                    &[Member::Light { light: 1 }, Member::Light { light: 2 }],
                ),
            )?;
        }
        let target = Target::Fixture { fixture: 3 };
        send(
            sender,
            step(1, "Background", all, BACKGROUND, Some(u16::MAX)),
        )?;
        send(
            sender,
            step(
                2,
                "Green",
                target,
                Rgb {
                    green: u16::MAX,
                    ..Rgb::BLACK
                },
                None,
            ),
        )?;
        send(
            sender,
            step(
                3,
                "Amber",
                target,
                Rgb {
                    red: u16::MAX,
                    green: 32768,
                    blue: 0,
                },
                None,
            ),
        )?;
        let items = [2, 3].map(|look| SequenceItem {
            look,
            transition: 100,
            ..SequenceItem::default()
        });
        send(
            sender,
            Command::PutSequence {
                look: 4,
                name: text("Crossfade"),
                sequence: Sequence::new(&items).ok_or("sequence length")?,
            },
        )?;
        let palette = Palette::Inline(Swatches::new(&[RED, BLUE]).ok_or("palette length")?);
        let waves = workload.waves() as u16;
        for n in 0..waves {
            send(
                sender,
                Command::PutPattern {
                    look: 5 + n,
                    name: text("Wave"),
                    pattern: Pattern::Wave {
                        target: WaveTarget::Group { group: text(WAVE) },
                        period: if n == 0 { 4 } else { n as u8 + 1 },
                        palette,
                    },
                },
            )?;
        }
        // Measure what the compiler actually reserved, including an event sample.
        // Start the background before the other Autopilot roots, so their colour wins.
        let mut visits = 0;
        for (look, rank, spread) in [(1, Rank::Autopilot, 0), (4, Rank::Show, 0)]
            .into_iter()
            .chain((5..5 + waves).map(|look| (look, Rank::Autopilot, 100)))
        {
            visits += 2 * sender
                .looks()
                .plan(look)
                .map_err(|r| format!("{r:?}"))?
                .work();
            send(
                sender,
                Command::StartUse {
                    look,
                    when: When::Now,
                    rank,
                    usage: Use {
                        spread,
                        ..Use::default()
                    },
                },
            )?;
        }
        if workload == Workload::Worst {
            // With all variable visits booked, the desk guarantees a one-light grab.
            // Prove this is the admitted edge before measuring it.
            let refused = sender.send(colour(Target::Group { group: text(probe) }, RED));
            if refused != Err(Refusal::WorkLimit) || visits != 65_536 {
                return Err(format!(
                    "expected a full work budget, booked {visits} and two-light grab gave {refused:?}"
                ));
            }
        }
        eprintln!(
            "{} RGB pixels, {} roots ({waves} Waves on {wave} pixels, sequence on {sequence}, background), {visits} reserved visits",
            channels / 3,
            waves + 2
        );
        Ok(())
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Action {
        Press { light: u16, red: u16 },
        Release { light: u16 },
        Edit { reverse: bool },
    }

    impl Action {
        fn command(self) -> Command {
            match self {
                Self::Press { light, red } => colour(
                    Target::Light { light },
                    Rgb {
                        red,
                        green: u16::MAX - red,
                        blue: 0,
                    },
                ),
                Self::Release { light } => Command::ReleaseColour {
                    target: Target::Light { light },
                },
                Self::Edit { reverse } => wave_group(reverse),
            }
        }
    }

    struct Event {
        at: Duration,
        action: Action,
    }

    /// All runs use identical reproducible send slots and targets. Absolute offsets
    /// keep the next press independent of the preceding edit's compile time.
    fn schedule(duration: Duration) -> Vec<Event> {
        let mut events = Vec::new();
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut at = Duration::ZERO;
        let mut held = 1;
        let mut reverse = false;
        loop {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            at += Duration::from_millis(90 + seed % 41);
            if at >= duration {
                return events;
            }
            let action = match events.len() % 3 {
                0 => {
                    held = 1 + ((seed >> 16) % 64) as u16;
                    Action::Press {
                        light: held,
                        red: (seed >> 32) as u16,
                    }
                }
                1 => Action::Release { light: held },
                _ => {
                    reverse = !reverse;
                    Action::Edit { reverse }
                }
            };
            events.push(Event { at, action });
        }
    }

    pub(super) struct Performed {
        pub(super) events: usize,
        /// The one FIFO command for each Manual press and release, in send order.
        pub(super) manual: Vec<u64>,
    }

    /// One edit client and one performance client, mirroring the server's short
    /// sender lock. Heavy prepares may span a press. Both clients use the same epoch.
    pub(crate) fn perform(
        sender: &mut Sender,
        started: Instant,
        duration: Duration,
    ) -> Result<Performed, String> {
        let events = schedule(duration);
        let mut manual = Vec::new();
        let sender = Mutex::new(sender);
        thread::scope(|scope| -> Result<(), String> {
            let edits = scope.spawn(|| -> Result<(), String> {
                for event in &events {
                    if let Action::Edit { .. } = event.action {
                        thread::sleep(
                            (started + event.at).saturating_duration_since(Instant::now()),
                        );
                        let base = sender.lock().map_err(|e| e.to_string())?.base();
                        let prepared = base
                            .prepare(event.action.command(), &[])
                            .map_err(|r| format!("edit prepare: {r:?}"))?;
                        sender
                            .lock()
                            .map_err(|e| e.to_string())?
                            .admit(prepared)
                            .map_err(|r| format!("edit admit: {r:?}"))?;
                    }
                }
                Ok(())
            });
            for event in &events {
                if !matches!(event.action, Action::Edit { .. }) {
                    thread::sleep((started + event.at).saturating_duration_since(Instant::now()));
                    let mut sender = sender.lock().map_err(|e| e.to_string())?;
                    let number = sender.sent() + 1;
                    send(&mut sender, event.action.command())?;
                    if sender.sent() != number {
                        return Err(format!(
                            "expected one queued command for {:?}",
                            event.action
                        ));
                    }
                    manual.push(number);
                }
            }
            edits.join().map_err(|_| "edit client panicked")??;
            Ok(())
        })?;
        Ok(Performed {
            events: events.len(),
            manual,
        })
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn all_workloads_use_the_same_absolute_press_release_and_edit_slots() {
            let events = schedule(Duration::from_secs(5));
            let first: Vec<_> = events
                .iter()
                .take(4)
                .map(|event| (event.at.as_millis(), event.action))
                .collect();
            assert_eq!(
                first,
                [
                    (
                        93,
                        Action::Press {
                            light: 41,
                            red: 0x280b
                        }
                    ),
                    (204, Action::Release { light: 41 }),
                    (333, Action::Edit { reverse: true }),
                    (
                        424,
                        Action::Press {
                            light: 28,
                            red: 0x1ffa
                        }
                    ),
                ]
            );
            assert_eq!(events.len(), 45);
            assert_eq!(schedule(Duration::from_secs(60)).len(), 549);
        }

        #[test]
        fn measured_rigs_patch_all_the_pixels_and_run_the_expected_roots() {
            use engine::{
                Device, Engine, Levels, Outcome, OutputNumber, Rate, RunNumbers, Universes,
            };
            struct Sink;
            impl Device for Sink {
                fn take(&mut self, _: &[Levels]) -> Outcome {
                    Outcome::Accepted
                }
            }
            let universes = Universes::new(64).unwrap();
            for (mode, pixels, roots) in [
                (Workload::M2, 10_000, 0),
                (Workload::M3, 10_000, 3),
                (Workload::Worst, 10_922, engine_core::PLAYS),
            ] {
                let (mut engine, mut sender) = Engine::start(universes).unwrap();
                let run = RunNumbers {
                    first: 1,
                    count: 64,
                }
                .check(universes)
                .unwrap();
                sender
                    .attach(
                        OutputNumber::try_from(1).unwrap(),
                        Box::new(Sink),
                        run,
                        Rate::Sixty,
                    )
                    .unwrap();
                prepare(&mut sender, mode).unwrap();
                let patch = sender.patch();
                assert_eq!(
                    patch
                        .fixtures()
                        .iter()
                        .map(|fixture| fixture.lights)
                        .sum::<u16>(),
                    pixels
                );
                assert_eq!(engine.fresh().state.plays.iter().count(), roots);
                let target = Target::Light { light: 1 };
                send(&mut sender, colour(target, RED)).unwrap();
                assert_eq!(engine.fresh().lights()[0].colour, Some(RED));
                send(&mut sender, Command::ReleaseColour { target }).unwrap();
                // Exercise both clients' command paths at the admitted edge.
                send(&mut sender, wave_group(true)).unwrap();
                assert_eq!(engine.fresh().state.plays.iter().count(), roots);
            }
        }
    }
}

#[cfg(test)]
mod handoff_tests {
    use super::*;
    use engine::Timing::{Applied, Due, Finished};

    #[test]
    fn one_edit_swapping_thirty_plans_does_not_outweigh_the_two_manual_sends() {
        let ms = Duration::from_millis;
        let output = OutputNumber::try_from(1).unwrap();
        let mut run = Samples::new(1, 100, ms(0), ms(1000));
        // A group reorder publishes 30 swaps. The following press and release
        // each have their own FIFO number and reach the device in 1 and 3 ms.
        for number in 101..=130 {
            run.record(Applied { number, at: ms(10) }).unwrap();
        }
        for (number, sent, handed) in [(131, 49, 50), (132, 52, 55)] {
            run.record(Applied {
                number,
                at: ms(sent),
            })
            .unwrap();
            run.record(Due {
                output,
                due: ms(handed),
                applied: number,
            })
            .unwrap();
            run.record(Finished {
                output,
                due: ms(handed),
                at: ms(handed),
                outcome: Outcome::Accepted,
            })
            .unwrap();
        }
        let row = run
            .report(&Performed {
                events: 3,
                manual: vec![131, 132],
            })
            .unwrap();
        assert_eq!(row["outputs"][0]["p95"], 3_000_000);
        assert_eq!(row["pressSamples"], 2);
    }

    #[test]
    fn the_first_accepted_device_handoff_carries_only_its_own_cutoff() {
        let ms = Duration::from_millis;
        let a = OutputNumber::try_from(1).unwrap();
        let b = OutputNumber::try_from(2).unwrap();
        let mut run = Samples::new(2, 9, ms(1000), ms(2000));
        for event in [
            Applied {
                number: 10,
                at: ms(1010),
            },
            Due {
                output: a,
                due: ms(1020),
                applied: 10,
            },
            Applied {
                number: 11,
                at: ms(1021),
            },
            Due {
                output: b,
                due: ms(1022),
                applied: 11,
            },
            Finished {
                output: a,
                due: ms(1020),
                at: ms(1025),
                outcome: Outcome::Accepted,
            },
            Finished {
                output: b,
                due: ms(1022),
                at: ms(1028),
                outcome: Outcome::Dropped,
            },
            Due {
                output: a,
                due: ms(1040),
                applied: 11,
            },
            Due {
                output: b,
                due: ms(1044),
                applied: 11,
            },
            Finished {
                output: a,
                due: ms(1040),
                at: ms(1041),
                outcome: Outcome::Accepted,
            },
            Finished {
                output: b,
                due: ms(1044),
                at: ms(1047),
                outcome: Outcome::Accepted,
            },
        ] {
            run.record(event).unwrap();
        }
        assert_eq!(run.outputs[0].latency, [ms(15), ms(20)]);
        assert_eq!(run.outputs[1].latency, [ms(37), ms(26)]);
        assert_eq!(run.outputs[0].lateness, [ms(5), ms(1)]);
        assert_eq!(run.outputs[1].lateness, [ms(3)]);
        assert_eq!(run.outputs[1].drops, 1);
    }
}
