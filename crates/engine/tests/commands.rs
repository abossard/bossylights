use engine::{
    ADDRESSES, ChannelNumbers, Command, Engine, Levels, QUEUE, Refusal, Sender, Universes,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// The smallest output: keeps the last frame of two universes in atomics.
struct Mock {
    frames: AtomicU64,
    levels: [AtomicU8; 2 * ADDRESSES],
}

impl Mock {
    fn level(&self, universe: u16, address: u16) -> u8 {
        let at = usize::from(universe - 1) * ADDRESSES + usize::from(address - 1);
        self.levels[at].load(Ordering::Relaxed)
    }

    /// Waits until the output has received two more frames: the one that may have
    /// been computed while the commands were sent, and the next one.
    fn two_more_frames(&self) {
        let before = self.frames.load(Ordering::Relaxed);
        let started = Instant::now();
        while self.frames.load(Ordering::Relaxed) < before + 2 {
            assert!(started.elapsed() < Duration::from_secs(2), "no frames");
            thread::sleep(Duration::from_millis(1));
        }
    }
}

fn start() -> (Engine, Sender, Arc<Mock>) {
    let mock = Arc::new(Mock {
        frames: AtomicU64::new(0),
        levels: [const { AtomicU8::new(0) }; 2 * ADDRESSES],
    });
    let output = {
        let mock = Arc::clone(&mock);
        move |frame: &[Levels]| {
            let levels = frame.iter().flat_map(|universe| universe.0.iter());
            for (cell, level) in mock.levels.iter().zip(levels) {
                cell.store(*level, Ordering::Relaxed);
            }
            mock.frames.fetch_add(1, Ordering::Relaxed);
        }
    };
    let (engine, sender) =
        Engine::start(Universes::new(2).unwrap(), output).expect("the tick thread starts");
    (engine, sender, mock)
}

fn channel(universe: u16, address: u16) -> ChannelNumbers {
    ChannelNumbers { universe, address }
}

fn set(universe: u16, address: u16, value: u8) -> Command<ChannelNumbers> {
    Command::SetChannel {
        channel: channel(universe, address),
        value,
    }
}

#[test]
fn a_command_reaches_the_frame_and_the_state() {
    let release = |universe, address| Command::ReleaseChannel {
        channel: channel(universe, address),
    };
    let blackout = |on| Command::Blackout { on };
    // (commands, universe, address, held level, level in the frame)
    let cases = [
        (vec![set(1, 1, 255)], 1, 1, 255, 255),
        (vec![set(2, 512, 7)], 2, 512, 7, 7),
        (vec![set(1, 5, 9), release(1, 5)], 1, 5, 0, 0),
        (
            vec![set(1, 5, 9), set(2, 5, 9), Command::ReleaseAll],
            2,
            5,
            0,
            0,
        ),
        (vec![set(1, 5, 9), blackout(true)], 1, 5, 9, 0),
        (
            vec![set(1, 5, 9), blackout(true), blackout(false)],
            1,
            5,
            9,
            9,
        ),
    ];
    for (commands, universe, address, held, out) in cases {
        let (engine, mut sender, mock) = start();
        for command in &commands {
            assert_eq!(sender.send(*command), Ok(()), "{command:?}");
        }
        mock.two_more_frames();
        assert_eq!(mock.level(universe, address), out, "{commands:?} out");
        let mut universes = [Levels::ZERO; 2];
        engine.read(&mut universes);
        let level = universes[usize::from(universe - 1)].0[usize::from(address - 1)];
        assert_eq!(level, held, "{commands:?} held");
        assert!(engine.send_to_frame_p95().is_some(), "the send was timed");
    }
}

#[test]
fn a_command_that_cannot_be_right_is_refused_and_changes_nothing() {
    // (command, reason)
    let cases = [
        (set(3, 1, 9), Refusal::NoSuchUniverse { universes: 2 }),
        (set(0, 1, 9), Refusal::NoSuchUniverse { universes: 2 }),
        (set(1, 0, 9), Refusal::NoSuchAddress),
        (set(1, 513, 9), Refusal::NoSuchAddress),
        (Command::Tick, Refusal::NotACommand),
    ];
    let (engine, mut sender, mock) = start();
    for (command, reason) in cases {
        assert_eq!(sender.send(command), Err(reason), "{command:?}");
    }
    mock.two_more_frames();
    let mut universes = [Levels::ZERO; 2];
    let state = engine.read(&mut universes);
    assert_eq!(universes, [Levels::ZERO; 2]);
    assert!(!state.blackout);
    assert_eq!(engine.send_to_frame_p95(), None, "nothing was queued");
}

#[test]
fn a_full_queue_refuses_and_says_so() {
    let (_engine, mut sender, _mock) = start();
    // A frame takes at most QUEUE commands; four queues' worth in a few microseconds
    // cannot all fit.
    let refused = (0..4 * QUEUE)
        .map(|_| sender.send(set(1, 1, 1)))
        .filter(|sent| *sent == Err(Refusal::QueueFull))
        .count();
    assert!(refused > 0, "nothing was refused");
}
