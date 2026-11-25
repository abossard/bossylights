//! The front prepares a command off its lock and admits it under it
//! (`docs/history/looks-slice-3.md`, "Prepare an edit" and "Admit"). A press is admitted
//! while another client's edit is still being prepared. A refused admission, for a stale
//! base, a full queue or an edit the project refuses, keeps the project, the patch and
//! the looks (what the show file saves) and the next DMX frame. The tick is stepped, so a
//! held tick takes no command and a full queue stays full. Over `/api`, a press needs no
//! thread of the blocking pool, and an edit admitted after `Stop` is refused.

mod common;

use common::{Heard, Socket, answer, command, connect, open, send, serve};
use engine::Unsent;
use engine::{
    Adapter, Begun, ChannelNumbers, Color, Command, Engine, Event, Fixture, Light, Member,
    MemberList, Members, Outcome, OutputKind, OutputNumber, PlayId, QUEUE, Refusal, Rgb,
    RunNumbers, Sender, Target, Text, Universes, Wakes,
};
use engine_core::{PLAYS, Step};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The test definition of a Generic RGB, read when the test is built.
const GENERIC_RGB: &[u8] =
    include_bytes!("../../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");

fn text(text: &str) -> Text {
    Text::new(text).unwrap()
}

fn par(address: u16) -> Command {
    par_in("RGB", address)
}

fn par_in(mode: &str, address: u16) -> Command {
    Command::Patch {
        name: text("PAR"),
        maker: text("Generic"),
        model: text("Generic RGB"),
        mode: text(mode),
        at: ChannelNumbers {
            universe: 1,
            address,
        },
    }
}

/// Look `look` saved as fixture 1 in `red green blue`.
fn step(look: u16, [red, green, blue]: [u16; 3]) -> Command {
    Command::PutStep {
        look,
        name: text("step"),
        step: Step {
            target: Target::Fixture { fixture: 1 },
            values: Light {
                colour: Some(Rgb { red, green, blue }),
                ..Light::DARK
            },
        }
        .into(),
    }
}

/// Group "g" of `members`.
fn group(members: &[Member]) -> Command {
    Command::SetGroup {
        name: text("g"),
        members: MemberList::Fits(Members::new(members).unwrap()),
    }
}

/// Look `look` saved as group "g" in red.
fn on_group(look: u16) -> Command {
    Command::PutStep {
        look,
        name: text("g"),
        step: Step {
            target: Target::Group { group: text("g") },
            values: Light {
                colour: Some(Rgb {
                    red: u16::MAX,
                    green: 0,
                    blue: 0,
                }),
                ..Light::DARK
            },
        }
        .into(),
    }
}

const RED: [u16; 3] = [u16::MAX, 0, 0];
const GREEN: [u16; 3] = [0, u16::MAX, 0];

/// The engine's front, its tick held, and the frames of its fake DMX USB Pro.
struct Front {
    _engine: Engine,
    sender: Sender,
    wakes: Wakes,
    library: Vec<Fixture>,
    heard: Heard,
    stop: Arc<AtomicBool>,
}

impl Drop for Front {
    fn drop(&mut self) {
        self.wakes.run();
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// What a refused admission must keep.
#[derive(Debug, PartialEq)]
struct Kept {
    project: engine::Project,
    patch: usize,
    looks: usize,
    sent: u64,
}

impl Front {
    /// A PAR at 1:1 in its mode RGB Dimmer, its dimmer at full, look 1 red playing, look 2
    /// green saved. The tick held.
    fn start() -> (Self, PlayId) {
        let heard = Heard::default();
        let stop = Arc::new(AtomicBool::new(false));
        let opens = open(Some(Arc::clone(&heard)), Arc::clone(&stop));
        let (engine, sender, wakes) =
            Engine::start_stepped(Universes::new(1).unwrap(), opens).unwrap();
        wakes.hold().unwrap();
        let library = vec![fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB).unwrap()];
        let mut front = Self {
            _engine: engine,
            sender,
            wakes,
            library,
            heard,
            stop,
        };
        let dmx = Command::SetOutput {
            output: OutputNumber::try_from(1).unwrap(),
            kind: OutputKind::DmxUsbPro {
                adapter: Adapter::FIRST,
            },
            universes: RunNumbers { first: 1, count: 1 },
            rate: None,
        };
        let Ok(Begun::Opening(opening)) = front.begin(dmx) else {
            panic!("the adapter opens");
        };
        front.sender.finish(opening.open()).unwrap();
        front.begin(par_in("RGB Dimmer", 1)).unwrap();
        let full = Command::SetIntensity {
            target: Target::Fixture { fixture: 1 },
            intensity: u16::MAX,
        };
        front.begin(full).unwrap();
        front.begin(step(1, RED)).unwrap();
        front.begin(step(2, GREEN)).unwrap();
        let Ok(Begun::Started { play }) = front.begin(Command::StartLook {
            look: 1,
            rank: engine::Rank::Manual,
            when: engine::When::Now,
            usage: Default::default(),
        }) else {
            panic!("look 1 plays");
        };
        assert_eq!(front.frame(), [255, 0, 0], "look 1 plays red");
        (front, play)
    }

    fn begin(&mut self, command: Command) -> Result<Begun, Refusal> {
        self.sender.begin(command, &self.library)
    }

    /// The bytes at 1:1 to 1:3 the adapter got once the tick let three wakes through,
    /// each frame heard: every command admitted before is in it.
    fn frame(&mut self) -> [u8; 3] {
        let started = Instant::now();
        let mut last = None;
        for _ in 0..3 {
            let heard = self.heard.lock().unwrap().len();
            self.wakes.wake().unwrap();
            loop {
                let frames = self.heard.lock().unwrap();
                if frames.len() > heard {
                    last = frames.last().map(|c| [c[0], c[1], c[2]]);
                    break;
                }
                drop(frames);
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "no frame heard"
                );
                std::thread::yield_now();
            }
        }
        last.unwrap()
    }

    /// The dimmer at 1:4 in the last frame heard.
    fn dimmer(&self) -> u8 {
        self.heard.lock().unwrap().last().map_or(0, |c| c[3])
    }

    fn kept(&self) -> Kept {
        Kept {
            project: self.sender.project(),
            patch: Arc::as_ptr(&self.sender.patch()) as usize,
            looks: Arc::as_ptr(&self.sender.looks()) as usize,
            sent: self.sender.sent(),
        }
    }

    /// Takes every play slot left with red looks 3 and on: the frame stays red.
    fn fill_plays(&mut self) {
        for look in 3..3 + PLAYS as u16 - 1 {
            self.begin(step(look, RED)).unwrap();
            let started = self.begin(Command::StartLook {
                look,
                rank: engine::Rank::Manual,
                when: engine::When::Now,
                usage: Default::default(),
            });
            assert!(
                matches!(started, Ok(Begun::Started { .. })),
                "look {look} plays"
            );
        }
    }

    /// Fills the queue with presses that change no byte at 1:1 to 1:3: the next command
    /// that needs a place in it finds none.
    fn fill(&mut self) {
        let press = Command::SetChannel {
            channel: ChannelNumbers {
                universe: 1,
                address: 512,
            },
            value: 0,
        };
        for _ in 0..=QUEUE {
            if self.sender.send(press) == Err(Unsent::Refused(Refusal::QueueFull)) {
                return;
            }
        }
        panic!("the held tick took commands");
    }
}

/// Before the refused command: nothing, edits taken before its base, another client's
/// admitted edit (the base moves on), a full queue, or every play slot taken.
#[derive(Clone, Copy, Debug)]
enum Before {
    Nothing,
    Saved(fn() -> Vec<Command>),
    Admitted(fn() -> Command),
    Full,
    PlaysFull,
}

/// (what, the command, what came before it, why it is refused)
type Case = (&'static str, fn(PlayId) -> Command, Before, Refusal);

#[test]
fn a_refused_admission_keeps_the_project_the_looks_and_the_next_frame() {
    let rename = || Command::Rename {
        fixture: 1,
        name: text("renamed"),
    };
    let save_three = || step(3, GREEN);
    // RGB DDP sends 510 channels of universe 1: a PAR at 1:510 would lose two
    let ddp = || Command::SetOutput {
        output: OutputNumber::try_from(2).unwrap(),
        kind: OutputKind::Ddp {
            address: "127.0.0.1:4048".parse().unwrap(),
            color: Color::Rgb,
        },
        universes: RunNumbers { first: 1, count: 1 },
        rate: None,
    };
    // a PAR at 1:5 alone in group "g", idle look 3 saved on the group
    let grouped = || {
        vec![
            par(5),
            group(&[Member::Fixture { fixture: 2 }]),
            on_group(3),
        ]
    };
    let cases: [Case; 14] = [
        // stale base: another client's edit admitted while this one was prepared
        (
            "a step over a renamed patch",
            |_| step(1, GREEN),
            Before::Admitted(rename),
            Refusal::Stale,
        ),
        (
            "a start over new looks",
            |_| Command::StartLook {
                look: 2,
                rank: engine::Rank::Manual,
                when: engine::When::Now,
                usage: Default::default(),
            },
            Before::Admitted(save_three),
            Refusal::Stale,
        ),
        (
            "a move over new looks",
            |_| Command::Move {
                fixture: 1,
                at: ChannelNumbers {
                    universe: 1,
                    address: 4,
                },
            },
            Before::Admitted(save_three),
            Refusal::Stale,
        ),
        (
            "a patch over a new output",
            |_| par(510),
            Before::Admitted(ddp),
            Refusal::Stale,
        ),
        // exhausted credits: no place in the queue
        (
            "a step of a playing look",
            |_| step(1, GREEN),
            Before::Full,
            Refusal::QueueFull,
        ),
        (
            "a start",
            |_| Command::StartLook {
                look: 2,
                rank: engine::Rank::Manual,
                when: engine::When::Now,
                usage: Default::default(),
            },
            Before::Full,
            Refusal::QueueFull,
        ),
        (
            "a release",
            |play| Command::ReleaseHold { play },
            Before::Full,
            Refusal::QueueFull,
        ),
        // every play slot taken
        (
            "a start of another look",
            |_| Command::StartLook {
                look: 2,
                rank: engine::Rank::Manual,
                when: engine::When::Now,
                usage: Default::default(),
            },
            Before::PlaysFull,
            Refusal::PlayFull,
        ),
        // refused edits
        (
            "an unpatch under a play",
            |_| Command::Unpatch { fixture: 1 },
            Before::Nothing,
            Refusal::LookInUse { look: 1 },
        ),
        (
            "a step naming nothing",
            |_| Command::PutStep {
                look: 3,
                name: text("none"),
                step: Step {
                    target: Target::Fixture { fixture: 9 },
                    values: Light::DARK,
                }
                .into(),
            },
            Before::Nothing,
            Refusal::NotPatched,
        ),
        (
            "removing a playing look",
            |_| Command::RemoveLook { look: 1 },
            Before::Nothing,
            Refusal::LookInUse { look: 1 },
        ),
        (
            "emptying the group of an idle look",
            |_| group(&[]),
            Before::Saved(grouped),
            Refusal::LookInUse { look: 3 },
        ),
        (
            "unpatching the one member of the group of an idle look",
            |_| Command::Unpatch { fixture: 2 },
            Before::Saved(grouped),
            Refusal::LookInUse { look: 3 },
        ),
        (
            "a step on an empty group",
            |_| on_group(4),
            Before::Saved(|| vec![group(&[])]),
            Refusal::EmptyGroup,
        ),
    ];
    for (name, command, before, refusal) in cases {
        let (mut front, play) = Front::start();
        let command = command(play);
        // An opening dropped unfinished is undone by the next edit (output-cutover T4):
        // the admitted edit stays only while its opening is held.
        let mut opening = None;
        let prepared = match before {
            Before::Nothing => front.sender.base().prepare(command, &front.library),
            Before::Saved(edits) => {
                for edit in edits() {
                    front.begin(edit).unwrap();
                }
                front.sender.base().prepare(command, &front.library)
            }
            Before::Full => {
                front.fill();
                front.sender.base().prepare(command, &front.library)
            }
            Before::PlaysFull => {
                front.fill_plays();
                front.sender.base().prepare(command, &front.library)
            }
            Before::Admitted(edit) => {
                let base = front.sender.base();
                opening = Some(front.begin(edit()).unwrap());
                base.prepare(command, &front.library)
            }
        };
        let kept = front.kept();
        let admitted = prepared.and_then(|prepared| front.sender.admit(prepared));
        assert_eq!(admitted.err(), Some(refusal), "{name}");
        drop(opening);
        assert_eq!(
            front.kept(),
            kept,
            "{name}: the project, patch and looks stay"
        );
        assert_eq!(
            front.frame(),
            [255, 0, 0],
            "{name}: the next frame stays red"
        );
    }
}

#[test]
fn a_press_is_admitted_while_another_clients_edit_is_prepared() {
    let (mut front, _) = Front::start();
    // client A takes its base; its compile pauses here
    let base = front.sender.base();
    // client B's press goes through meanwhile, into the next frame
    let dim = Command::SetIntensity {
        target: Target::Fixture { fixture: 1 },
        intensity: 0x8080,
    };
    assert!(matches!(front.begin(dim), Ok(Begun::Grabbed { .. })));
    assert_eq!(
        (front.frame(), front.dimmer()),
        ([255, 0, 0], 128),
        "B's press is in the next frame"
    );
    // A's edit, prepared on a base the press did not move, is admitted after it
    let prepared = base.prepare(step(1, GREEN), &front.library).unwrap();
    assert!(matches!(front.sender.admit(prepared), Ok(Begun::Queued)));
    assert_eq!(
        (front.frame(), front.dimmer()),
        ([0, 255, 0], 128),
        "A's step plays green"
    );
}

/// (what, then what, the frame after)
type Then = (&'static str, fn(&mut Front, PlayId), [u8; 3]);

#[test]
fn with_every_play_slot_taken_a_release_and_a_fresh_start_need_only_room_in_the_queue() {
    let cases: [Then; 2] = [
        (
            "a release frees a slot for look 2",
            |front, play| {
                let release = front.begin(Command::ReleaseHold { play });
                assert!(matches!(release, Ok(Begun::Queued)));
                let started = front.begin(Command::StartLook {
                    look: 2,
                    rank: engine::Rank::Manual,
                    when: engine::When::Now,
                    usage: Default::default(),
                });
                assert!(matches!(started, Ok(Begun::Started { .. })));
            },
            [0, 255, 0],
        ),
        (
            "a fresh start of look 1 takes its own slot",
            |front, play| {
                let started = front.begin(Command::StartLook {
                    look: 1,
                    rank: engine::Rank::Manual,
                    when: engine::When::Now,
                    usage: Default::default(),
                });
                let Ok(Begun::Started { play: again }) = started else {
                    panic!("look 1 starts again: {:?}", started.err());
                };
                assert_eq!(again.slot(), play.slot());
            },
            [255, 0, 0],
        ),
    ];
    for (name, then, frame) in cases {
        let (mut front, play) = Front::start();
        front.fill_plays();
        then(&mut front, play);
        assert_eq!(front.frame(), frame, "{name}");
    }
}

/// A runtime whose blocking pool has one thread, which [`busy`] can take.
fn one_blocking_thread() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .unwrap()
}

/// Takes the one thread of the blocking pool, as a save, a device opening or another
/// client's edit being prepared does, until the returned sender is dropped.
async fn busy() -> std::sync::mpsc::Sender<()> {
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (started, running) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || {
        let _ = started.send(());
        let _ = gate.recv();
    });
    running.await.unwrap();
    release
}

/// A served engine on a stepped clock, its tick held, the frames of its fake DMX USB Pro
/// in `heard`. A PAR at 1:1 at full held blue, then look 1 red playing as `play`, look 2
/// green saved.
struct Served {
    socket: Socket,
    wakes: Wakes,
    heard: Heard,
    /// Frames handed to the adapter whose outcome is still due.
    out: usize,
    /// Frames the adapter accepted. The fake port hears them in order.
    accepted: usize,
    play: PlayId,
    stop: Arc<AtomicBool>,
}

impl Drop for Served {
    fn drop(&mut self) {
        self.wakes.run();
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Served {
    async fn start() -> Self {
        let heard = Heard::default();
        let stop = Arc::new(AtomicBool::new(false));
        let opens = open(Some(Arc::clone(&heard)), Arc::clone(&stop));
        let (engine, mut sender, wakes) =
            Engine::start_stepped(Universes::new(1).unwrap(), opens).unwrap();
        let library = [fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB).unwrap()];
        let dmx = Command::SetOutput {
            output: OutputNumber::try_from(1).unwrap(),
            kind: OutputKind::DmxUsbPro {
                adapter: Adapter::FIRST,
            },
            universes: RunNumbers { first: 1, count: 1 },
            rate: None,
        };
        let Ok(Begun::Opening(opening)) = sender.begin(dmx, &library) else {
            panic!("the adapter opens");
        };
        sender.finish(opening.open()).unwrap();
        let fixture = Target::Fixture { fixture: 1 };
        let setup = [
            par(1),
            Command::SetIntensity {
                target: fixture,
                intensity: u16::MAX,
            },
            Command::SetColour {
                target: fixture,
                red: 0,
                green: 0,
                blue: u16::MAX,
            },
            step(1, RED),
            step(2, GREEN),
        ];
        for command in setup {
            sender.begin(command, &library).unwrap();
        }
        let Ok(Begun::Started { play }) = sender.begin(
            Command::StartLook {
                look: 1,
                rank: engine::Rank::Manual,
                when: engine::When::Now,
                usage: Default::default(),
            },
            &library,
        ) else {
            panic!("look 1 plays");
        };
        // served while the clock runs: the server's first picture needs a wake
        let socket = connect(serve(engine, sender, None).await).await;
        wakes.hold().unwrap();
        let mut served = Self {
            socket,
            wakes,
            heard,
            out: 0,
            accepted: 0,
            play,
            stop,
        };
        served.count_frames();
        // wake_at never goes back, so zero keeps the clock at the last free wake.
        served.settle(Duration::ZERO);
        assert_eq!(served.first_frame(), [255, 0, 0], "look 1 plays red");
        served
    }

    /// Counts the outcomes and returns how many frames were handed over since the last read.
    fn count_frames(&mut self) -> usize {
        let mut handed = 0;
        for event in self.wakes.events() {
            match event {
                Event::Handed { .. } => {
                    self.out += 1;
                    handed += 1;
                }
                Event::Came { outcome, .. } => {
                    self.out -= 1;
                    self.accepted += usize::from(outcome == Outcome::Accepted);
                }
                Event::Counted { .. } => {}
            }
        }
        handed
    }

    /// Collects outcomes without advancing the clock, then waits to hear every accepted frame.
    fn settle(&mut self, at: Duration) {
        let started = Instant::now();
        while self.out != 0 {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "no frame outcome"
            );
            self.wakes.wake_at(at).unwrap();
            assert_eq!(self.count_frames(), 0, "an early wake handed a frame");
        }
        let started = Instant::now();
        while self.heard.lock().unwrap().len() != self.accepted {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "waiting to hear {} accepted frames",
                self.accepted
            );
            std::thread::yield_now();
        }
    }

    /// Reads slots 1 to 3 of the very next frame. The port is idle before each press.
    fn first_frame(&mut self) -> [u8; 3] {
        let before = self.accepted;
        let started = Instant::now();
        let at = loop {
            let at = self.wakes.wake().unwrap();
            let handed = self.count_frames();
            if handed != 0 {
                assert_eq!(handed, 1, "one frame at the first due wake");
                break at;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "no frame handed"
            );
        };
        self.settle(at);
        assert_eq!(
            self.accepted,
            before + 1,
            "the adapter must accept the first frame"
        );
        let frames = self.heard.lock().unwrap();
        let frame = &frames[self.accepted - 1];
        [frame[0], frame[1], frame[2]]
    }
}

/// The name of a reply: `"Accepted"`, or its one key.
fn named(reply: &Value) -> String {
    match reply {
        Value::String(name) => name.clone(),
        Value::Object(fields) => fields.keys().next().cloned().unwrap_or_default(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => reply.to_string(),
    }
}

/// (what, the press, the reply's name, the bytes at 1:1 to 1:3 in the first frame after it)
type Press = (&'static str, fn(PlayId) -> Value, &'static str, [u8; 3]);

#[test]
fn a_press_over_the_api_is_in_the_first_frame_after_it_while_the_blocking_pool_is_busy() {
    let cases: [Press; 5] = [
        (
            "a colour, newer than the play",
            |_| {
                json!({"SetColour": {"target": {"Fixture": {"fixture": 1}},
                    "red": 0, "green": 65535, "blue": 0}})
            },
            "Grabbed",
            [0, 255, 0],
        ),
        (
            "a channel held",
            |_| json!({"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": 91}}),
            "Accepted",
            [91, 0, 0],
        ),
        (
            "a start of look 2",
            |_| json!({"StartLook": {"look": 2}}),
            "Started",
            [0, 255, 0],
        ),
        (
            "a release of look 1",
            |play| json!({"ReleaseHold": {"play": play}}),
            "Accepted",
            [0, 0, 255],
        ),
        (
            "a blackout",
            |_| json!({"Blackout": {"on": true}}),
            "Accepted",
            [0, 0, 0],
        ),
    ];
    let runtime = one_blocking_thread();
    for (name, press, reply, frame) in cases {
        runtime.block_on(async {
            let mut served = Served::start().await;
            let release = busy().await;
            let press = json!({"Send": {"command": press(served.play)}});
            send(&mut served.socket, &press).await;
            let answered = tokio::time::timeout(Duration::from_secs(2), answer(&mut served.socket))
                .await
                .unwrap_or_else(|_| panic!("{name}: no answer while the blocking pool is busy"));
            assert_eq!(named(&answered), reply, "{name}: {answered}");
            assert_eq!(served.first_frame(), frame, "{name}");
            drop(release);
            send(&mut served.socket, &json!("Stop")).await;
        });
    }
}

#[test]
fn an_edit_admitted_after_stop_is_answered_stopping_so_every_accepted_one_is_saved() {
    // (what, the step's fixture): a step the patch takes, and one it refuses
    let cases = [("a red step", 1), ("a step of nothing patched", 9)];
    let runtime = one_blocking_thread();
    for (name, fixture) in cases {
        runtime.block_on(async {
            let file = std::env::temp_dir().join(format!(
                "bossylights-admit-{}-stop-{fixture}.json",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&file);
            let definitions =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixture-library/tests/definitions");
            let (engine, sender) = Engine::start(Universes::new(1).unwrap()).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let served = tokio::spawn(engine_server::serve(
                listener,
                engine,
                sender,
                Some(definitions),
                Some(file.clone()),
            ));
            let (mut a, mut b) = (connect(address).await, connect(address).await);
            let par = json!({"Patch": {
                "name": "PAR", "maker": "Generic", "model": "Generic RGB", "mode": "RGB",
                "at": {"universe": 1, "address": 1},
            }});
            let started = Instant::now();
            loop {
                let reply = command(&mut a, par.clone()).await;
                if reply != json!({"Refused": {"reason": "NoSuchFixture"}}) {
                    assert_eq!(reply, json!({"Patched": {"fixture": 1}}));
                    break;
                }
                assert!(started.elapsed() < Duration::from_secs(10), "no library");
            }
            while !std::fs::read_to_string(&file).is_ok_and(|text| text.contains("\"PAR\"")) {
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "the patch not saved"
                );
                tokio::task::yield_now().await;
            }
            // A's step is prepared on the blocking pool, which is busy: it waits there
            let release = busy().await;
            // a name of its own: the test waits until this edit reached the pool
            let look = format!("red, {name}");
            let red = json!({"PutStep": {"look": 1, "name": look, "step": {
                "target": {"Fixture": {"fixture": fixture}},
                "values": {"intensity": null, "colour": {"red": 65535, "green": 0, "blue": 0},
                    "position": null, "zoom": null, "gobos": [null, null, null]},
            }}});
            send(&mut a, &json!({"Send": {"command": red}})).await;
            let waiting = Instant::now();
            while !engine_server::handed_to_the_pool(&look) {
                assert!(
                    waiting.elapsed() < Duration::from_secs(10),
                    "{name}: A's step never reached the blocking pool"
                );
                tokio::task::yield_now().await;
            }
            // B stops the engine: the last save is taken now
            send(&mut b, &json!("Stop")).await;
            assert_eq!(answer(&mut b).await, json!("Stopping"));
            drop(release);
            let replied = answer(&mut a).await;
            drop(served.await.unwrap().unwrap());
            let saved: Value =
                serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            assert_eq!(
                (replied, saved["looks"].clone()),
                (json!("Stopping"), json!([])),
                "{name}: an edit answered after the last save is answered Stopping"
            );
            let _ = std::fs::remove_file(&file);
        });
    }
}
