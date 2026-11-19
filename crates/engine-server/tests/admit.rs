//! The front prepares a command off its lock and admits it under it
//! (`docs/design/looks-slice-3.md`, "Prepare an edit" and "Admit"). A press is admitted
//! while another client's edit is still being prepared. A refused admission, for a stale
//! base, a full queue or an edit the project refuses, keeps the project, the patch and
//! the looks (what the show file saves) and the next DMX frame. The tick is stepped, so a
//! held tick takes no command and a full queue stays full.

mod common;

use common::{Heard, open};
use engine::{
    Adapter, Begun, ChannelNumbers, Command, Engine, Fixture, Light, OutputKind, OutputNumber,
    PlayId, QUEUE, Refusal, Rgb, RunNumbers, Sender, Target, Text, Universes, Wakes,
};
use engine_core::{PLAYS, Step};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// The test definition of a Generic RGB, read when the test is built.
const GENERIC_RGB: &[u8] =
    include_bytes!("../../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");

fn text(text: &str) -> Text {
    Text::new(text).unwrap()
}

fn par(address: u16) -> Command {
    Command::Patch {
        name: text("PAR"),
        maker: text("Generic"),
        model: text("Generic RGB"),
        mode: text("RGB"),
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
        },
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
    /// A PAR at 1:1 at full, look 1 red playing, look 2 green saved. The tick held.
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
        front.begin(par(1)).unwrap();
        let full = Command::SetIntensity {
            target: Target::Fixture { fixture: 1 },
            intensity: u16::MAX,
        };
        front.begin(full).unwrap();
        front.begin(step(1, RED)).unwrap();
        front.begin(step(2, GREEN)).unwrap();
        let Ok(Begun::Started { play }) = front.begin(Command::StartLook { look: 1 }) else {
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
            let started = self.begin(Command::StartLook { look });
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
            if self.sender.send(press) == Err(Refusal::QueueFull) {
                return;
            }
        }
        panic!("the held tick took commands");
    }
}

/// Before the refused command: nothing, another client's admitted edit (the base moves
/// on), a full queue, or every play slot taken.
#[derive(Clone, Copy, Debug)]
enum Before {
    Nothing,
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
    let cases: [Case; 10] = [
        // stale base: another client's edit admitted while this one was prepared
        (
            "a step over a renamed patch",
            |_| step(1, GREEN),
            Before::Admitted(rename),
            Refusal::Stale,
        ),
        (
            "a start over new looks",
            |_| Command::StartLook { look: 2 },
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
        // exhausted credits: no place in the queue
        (
            "a step of a playing look",
            |_| step(1, GREEN),
            Before::Full,
            Refusal::QueueFull,
        ),
        (
            "a start",
            |_| Command::StartLook { look: 2 },
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
            |_| Command::StartLook { look: 2 },
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
                },
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
    ];
    for (name, command, before, refusal) in cases {
        let (mut front, play) = Front::start();
        let command = command(play);
        let prepared = match before {
            Before::Nothing => front.sender.base().prepare(command, &front.library),
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
                front.begin(edit()).unwrap();
                base.prepare(command, &front.library)
            }
        };
        let kept = front.kept();
        let admitted = prepared.and_then(|prepared| front.sender.admit(prepared));
        assert_eq!(admitted.err(), Some(refusal), "{name}");
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
    assert!(matches!(front.begin(dim), Ok(Begun::Queued)));
    assert_eq!(front.frame(), [128, 0, 0], "B's press is in the next frame");
    // A's edit, prepared on a base the press did not move, is admitted after it
    let prepared = base.prepare(step(1, GREEN), &front.library).unwrap();
    assert!(matches!(front.sender.admit(prepared), Ok(Begun::Queued)));
    assert_eq!(front.frame(), [0, 128, 0], "A's step plays green");
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
                let started = front.begin(Command::StartLook { look: 2 });
                assert!(matches!(started, Ok(Begun::Started { .. })));
            },
            [0, 255, 0],
        ),
        (
            "a fresh start of look 1 takes its own slot",
            |front, play| {
                let started = front.begin(Command::StartLook { look: 1 });
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
