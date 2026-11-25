//! `engine-slot <program>` holds the machine's engine slot while the program runs.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

#[test]
fn the_slot_is_held_while_the_program_runs_and_free_once_it_ended() {
    let mut holding = Command::new(env!("CARGO_BIN_EXE_engine-slot"))
        .args(["sh", "-c", "echo running; cat"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(holding.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line, "running\n");
    let taken = Arc::new(AtomicBool::new(false));
    let taking = thread::spawn({
        let taken = Arc::clone(&taken);
        move || {
            let _slot = engine_slot::take().unwrap();
            taken.store(true, Ordering::Release);
        }
    });
    thread::sleep(Duration::from_secs(1));
    assert!(
        !taken.load(Ordering::Acquire),
        "taken while the program ran"
    );
    drop(holding.stdin.take()); // `cat` ends
    assert!(holding.wait().unwrap().success());
    taking.join().unwrap();
    assert!(taken.load(Ordering::Acquire), "never taken");
}

#[test]
fn the_program_starts_only_once_the_slot_is_free() {
    let held = engine_slot::take().unwrap();
    let mut waiting = Command::new(env!("CARGO_BIN_EXE_engine-slot"))
        .args(["sh", "-c", "echo running"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    let reading = thread::spawn({
        let ran = Arc::clone(&ran);
        let stdout = waiting.stdout.take().unwrap();
        move || {
            let mut line = String::new();
            BufReader::new(stdout).read_line(&mut line).unwrap();
            ran.store(line == "running\n", Ordering::Release);
        }
    });
    thread::sleep(Duration::from_secs(1));
    assert!(
        !ran.load(Ordering::Acquire),
        "the program ran while the slot was held"
    );
    drop(held);
    reading.join().unwrap();
    assert!(waiting.wait().unwrap().success());
    assert!(ran.load(Ordering::Acquire), "never ran");
}

#[test]
fn the_exit_code_is_the_programs_or_says_what_went_wrong() {
    // (arguments, exit code)
    let cases: [(&[&str], i32); 4] = [
        (&["sh", "-c", "exit 0"], 0),
        (&["sh", "-c", "exit 3"], 3),
        (&["no-such-program-of-319"], 1),
        (&[], 2),
    ];
    for (args, code) in cases {
        let status = Command::new(env!("CARGO_BIN_EXE_engine-slot"))
            .args(args)
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(code), "{args:?}");
    }
}
