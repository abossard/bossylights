//! Starts a thread and asks the system for real-time priority for it. The only place
//! that asks, and the only `unsafe` code in the product outside tests. Three tests carry a counting allocator, which needs `unsafe` too.
//!
//! | System | Asks for | Refused when |
//! |---|---|---|
//! | macOS | the Mach time-constraint policy, as audio threads use it | rarely: the policy is open to every process |
//! | Linux | `SCHED_FIFO`, below the kernel's interrupt threads | the user has no `rtprio` limit, e.g. without the `audio` group |
//! | others | nothing | always: [`BestEffort::NotSupported`] |
//!
//! A refusal is a state, never an error: the thread runs best effort and says why.
//! With the feature `test-support` it also reads the CPU each thread used, for the
//! scorecard ([`cpu_by_thread`]).
//! The code runs on the output thread, so the same lock and I/O bans apply as in
//! `crates/engine` (`clippy.toml` is a link to that crate's).

#![deny(
    clippy::undocumented_unsafe_blocks,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub use engine_core::{BestEffort, Priority};

#[cfg(feature = "test-support")]
mod cpu;
#[cfg(feature = "test-support")]
pub use cpu::cpu_by_thread;

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

/// Starts a thread named `name` that wakes when it has work, asks for real-time
/// priority on it, then runs `run` with the answer. Returns once the answer is known.
/// Fails when the system refuses a thread, or the thread ends before it answers.
pub fn spawn<F, T>(name: &str, run: F) -> io::Result<(JoinHandle<T>, Priority)>
where
    F: FnOnce(Priority) -> T + Send + 'static,
    T: Send + 'static,
{
    spawn_asking(name, ask, run)
}

/// The question to the system: its answer code, or `None` when there is none to ask.
type Ask = fn() -> Option<i32>;

/// [`spawn`] with the question to the system as an input, so a test can make the
/// thread end before it answers.
fn spawn_asking<F, T>(name: &str, ask: Ask, run: F) -> io::Result<(JoinHandle<T>, Priority)>
where
    F: FnOnce(Priority) -> T + Send + 'static,
    T: Send + 'static,
{
    let answer = Arc::new(AtomicU64::new(UNANSWERED));
    let thread = thread::Builder::new().name(name.into()).spawn({
        let answer = Arc::clone(&answer);
        move || {
            let priority = priority(ask());
            answer.store(to_bits(priority), Ordering::Release);
            run(priority)
        }
    })?;
    loop {
        // Read the finish first: an answer stored before the thread ended is still read.
        let finished = thread.is_finished();
        if let Some(priority) = from_bits(answer.load(Ordering::Acquire)) {
            return Ok((thread, priority));
        }
        if finished {
            return Err(io::Error::other("the thread ended before it answered"));
        }
        thread::yield_now();
    }
}

/// The state for the system's answer: `Some(0)` granted, `Some(code)` refused,
/// `None` no policy to ask for on this system.
fn priority(answer: Option<i32>) -> Priority {
    match answer {
        Some(0) => Priority::RealTime,
        Some(code) => Priority::BestEffort {
            reason: BestEffort::Refused { code },
        },
        None => Priority::BestEffort {
            reason: BestEffort::NotSupported,
        },
    }
}

// The answer travels from the new thread to `spawn` as one atomic number.
const UNANSWERED: u64 = 0;
const REAL_TIME: u64 = 1;
const NOT_SUPPORTED: u64 = 2;
const REFUSED: u64 = 3; // the code in the upper 32 bits

fn to_bits(priority: Priority) -> u64 {
    match priority {
        Priority::RealTime => REAL_TIME,
        Priority::BestEffort {
            reason: BestEffort::NotSupported,
        } => NOT_SUPPORTED,
        Priority::BestEffort {
            reason: BestEffort::Refused { code },
        } => REFUSED | u64::from(code.cast_unsigned()) << 32,
    }
}

fn from_bits(bits: u64) -> Option<Priority> {
    let code = (bits >> 32) as u32;
    match bits & 0xffff_ffff {
        REAL_TIME => Some(priority(Some(0))),
        NOT_SUPPORTED => Some(priority(None)),
        REFUSED => Some(priority(Some(code.cast_signed()))),
        _ => None,
    }
}

/// The Mach time-constraint policy: on each wake the thread needs about `COMPUTATION`
/// of CPU, done within `CONSTRAINT`. Period 0: it wakes when an output is due, at no
/// steady period (#125). Spike #23 measured these values: 0 of 26 399 frames off at 44
/// a second. Answers a `kern_return_t`.
#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "the Mach thread policy has no safe wrapper; each block says why it is sound"
)]
fn ask() -> Option<i32> {
    use std::time::Duration;

    /// Sleep, then spin the last millisecond, then compute the frame.
    const COMPUTATION: Duration = Duration::from_millis(2);
    const CONSTRAINT: Duration = Duration::from_millis(5);

    // libc marks its own declaration deprecated in favour of another crate; the
    // function itself is stable in macOS.
    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    unsafe extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
    }

    let mut timebase = Timebase { numer: 0, denom: 0 };
    // SAFETY: writes two u32 into the struct we pass, which lives across the call.
    let result = unsafe { mach_timebase_info(&raw mut timebase) };
    if result != 0 {
        return Some(result);
    }
    // A value that does not fit becomes u32::MAX, which the system refuses.
    let ticks = |d: Duration| {
        (d.as_nanos() * u128::from(timebase.denom))
            .checked_div(u128::from(timebase.numer))
            .map_or(u32::MAX, |t| u32::try_from(t).unwrap_or(u32::MAX))
    };
    let mut policy = libc::thread_time_constraint_policy {
        period: 0, // no steady period
        computation: ticks(COMPUTATION),
        constraint: ticks(CONSTRAINT),
        preemptible: 1,
    };
    // SAFETY: changes only the calling thread. `policy` is the struct this flavour
    // expects and lives across the call; the count is its size in integers, as the
    // header defines it. Nothing keeps the pointer.
    let result = unsafe {
        libc::thread_policy_set(
            libc::pthread_mach_thread_np(libc::pthread_self()),
            libc::THREAD_TIME_CONSTRAINT_POLICY as libc::thread_policy_flavor_t,
            (&raw mut policy).cast(),
            libc::THREAD_TIME_CONSTRAINT_POLICY_COUNT,
        )
    };
    Some(result)
}

/// `SCHED_FIFO` at this priority: below the kernel's interrupt threads (50 on a
/// real-time kernel), above every normal thread. Answers an `errno`.
#[cfg(target_os = "linux")]
#[expect(
    unsafe_code,
    reason = "pthread_setschedparam has no safe wrapper; the block says why it is sound"
)]
fn ask() -> Option<i32> {
    const FIFO_PRIORITY: libc::c_int = 40;
    let param = libc::sched_param {
        sched_priority: FIFO_PRIORITY,
    };
    // SAFETY: changes only the calling thread. `param` lives across the call and is
    // only read. Nothing keeps the pointer.
    Some(unsafe { libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_FIFO, &param) })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn ask() -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_becomes_a_state() {
        // (the system's answer, the state)
        let cases = [
            (Some(0), Priority::RealTime),
            (
                Some(1), // macOS KERN_INVALID_ARGUMENT, Linux EPERM
                Priority::BestEffort {
                    reason: BestEffort::Refused { code: 1 },
                },
            ),
            (
                Some(-1),
                Priority::BestEffort {
                    reason: BestEffort::Refused { code: -1 },
                },
            ),
            (
                None,
                Priority::BestEffort {
                    reason: BestEffort::NotSupported,
                },
            ),
        ];
        for (answer, state) in cases {
            assert_eq!(priority(answer), state, "{answer:?}");
            assert_eq!(
                from_bits(to_bits(state)),
                Some(state),
                "{state:?} survives the hand-over"
            );
        }
    }

    #[test]
    fn a_thread_that_ends_before_it_answers_is_an_error_and_one_that_answers_is_not() {
        #[expect(clippy::panic, reason = "the thread unwinds before it answers")]
        fn unwinds() -> Option<i32> {
            panic!("asking unwound");
        }
        fn granted() -> Option<i32> {
            Some(0)
        }
        // (the system's answer, spawn's result: the priority, or None for an error)
        let cases: [(Ask, Option<Priority>); 2] =
            [(unwinds, None), (granted, Some(Priority::RealTime))];
        // A spawn that never returns fails here instead of hanging the test run.
        const NEVER: std::time::Duration = std::time::Duration::from_secs(10);
        for (ask, expected) in cases {
            let (answer, answered) = std::sync::mpsc::channel();
            thread::spawn(move || {
                let spawned = spawn_asking("rt-test", ask, |priority| priority);
                let joined = spawned.map(|(thread, priority)| (thread.join().ok(), priority));
                let _ = answer.send(joined.ok());
            });
            let got = answered.recv_timeout(NEVER);
            assert!(got.is_ok(), "spawn never returned");
            let got = got.ok().flatten();
            assert_eq!(got.map(|(_, priority)| priority), expected);
            if let Some((ran, priority)) = got {
                assert_eq!(
                    ran,
                    Some(priority),
                    "the thread ran with the reported priority"
                );
            }
        }
    }

    #[test]
    fn only_an_answer_reads_as_one() {
        // (bits, what spawn reads)
        let cases = [
            (UNANSWERED, None),
            (4, None),
            (u64::MAX, None),
            (REAL_TIME, Some(Priority::RealTime)),
        ];
        for (bits, read) in cases {
            assert_eq!(from_bits(bits), read, "{bits:#x}");
        }
    }
}
