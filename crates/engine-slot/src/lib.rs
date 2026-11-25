//! The machine's one engine slot (#319). A test engine is as real as the product,
//! real-time priority included, and `nice` does not reach a real-time thread. So at
//! most one engine runs on the machine at a time: every engine a test starts takes this
//! slot first and waits while anyone else holds it. Threads of one test binary and
//! other processes wait alike.
//!
//! The slot is an OS file lock on one file with a fixed path. The OS frees it when its
//! holder drops it or the holder's process dies, so a killed run never leaves it taken.
//! The holder writes its process id into the file; a waiter says once a minute who holds it.

#![forbid(unsafe_code)]

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

/// The slot's file name. `scripts/release-shell.mjs` checks that no product program names it.
const NAME: &str = "bossylights-engine.slot";
/// How often a waiter looks whether the slot is free.
const LOOK: Duration = Duration::from_millis(10);
/// How often a waiter says who holds the slot.
const SAY_EVERY: Duration = Duration::from_secs(60);

/// The machine's one engine slot, held until dropped.
#[must_use = "the slot is free again once dropped"]
pub struct Slot {
    _lock: File,
}

/// The slot's file. Not the temp folder of the environment on Unix: a pane or a tool with
/// its own `TMPDIR` would get a slot of its own, and two engines would run.
fn path() -> PathBuf {
    if cfg!(unix) {
        PathBuf::from("/tmp").join(NAME)
    } else {
        std::env::temp_dir().join(NAME)
    }
}

/// Whether a waiter says again who holds the slot, `since` its last line.
fn says(since: Duration) -> bool {
    since >= SAY_EVERY
}

/// What a waiter says: the slot's file and the process id its holder wrote into it.
fn waiting(path: &Path, holder: &str) -> String {
    format!(
        "waiting for the engine slot {}, held by process {holder}\n",
        path.display()
    )
}

/// `error`, naming the slot's file.
fn named(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

/// Takes the machine's engine slot. Waits, with no time limit, while another thread or
/// process holds it: take it before any clock of the test runs. Fails, naming the
/// file, when the file cannot be opened or locked.
pub fn take() -> io::Result<Slot> {
    let path = path();
    // Opened for writing, to name its holder. Another user's file is opened for reading:
    // a lock needs no write access.
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .or_else(|_| File::open(&path))
        .map_err(|e| named(&path, e))?;
    let mut said = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) => {
                if says(said.elapsed()) {
                    let holder = std::fs::read_to_string(&path).unwrap_or_default();
                    // straight to stderr: a test's captured output shows only when it fails
                    let _ = io::stderr().write_all(waiting(&path, &holder).as_bytes());
                    said = Instant::now();
                }
                thread::sleep(LOOK);
            }
            Err(TryLockError::Error(e)) => return Err(named(&path, e)),
        }
    }
    // Names the holder. A file opened for reading stays unnamed.
    let _ = file
        .set_len(0)
        .and_then(|()| (&file).write_all(std::process::id().to_string().as_bytes()));
    Ok(Slot { _lock: file })
}

#[cfg(test)]
mod tests {
    use super::{named, says, waiting};
    use std::io;
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn a_waiter_says_who_holds_the_slot_once_a_minute() {
        // (since the last line, says again)
        let cases = [
            (Duration::ZERO, false),
            (Duration::from_millis(59_999), false),
            (Duration::from_secs(60), true),
            (Duration::from_secs(120), true),
        ];
        for (since, expected) in cases {
            assert_eq!(says(since), expected, "{since:?}");
        }
    }

    #[test]
    fn a_waiter_names_the_file_and_the_holders_process() {
        // (holder written in the file, line)
        let cases = [
            (
                "4242",
                "waiting for the engine slot /tmp/s, held by process 4242\n",
            ),
            ("", "waiting for the engine slot /tmp/s, held by process \n"),
        ];
        for (holder, line) in cases {
            assert_eq!(waiting(Path::new("/tmp/s"), holder), line);
        }
    }

    #[test]
    fn an_error_names_the_slots_file_and_keeps_its_kind() {
        // (kind, what the system said)
        let cases = [
            (io::ErrorKind::PermissionDenied, "denied"),
            (io::ErrorKind::NotFound, "no such folder"),
        ];
        for (kind, said) in cases {
            let error = named(Path::new("/tmp/s"), io::Error::new(kind, said));
            assert_eq!(error.kind(), kind);
            assert_eq!(error.to_string(), format!("/tmp/s: {said}"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_slot_is_one_file_for_the_whole_machine_whatever_tmpdir_says() {
        assert_eq!(super::path(), Path::new("/tmp/bossylights-engine.slot"));
    }

    #[test]
    fn the_holder_names_itself_in_the_slots_file() {
        let _slot = super::take().unwrap();
        let named = std::fs::read_to_string(super::path()).unwrap();
        assert_eq!(named, std::process::id().to_string());
    }
}
