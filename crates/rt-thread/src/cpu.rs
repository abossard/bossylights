//! What the system counted of CPU for each thread of this process, read from outside
//! the threads it measures: for the scorecard (#318). Only with the feature
//! `test-support`.
//!
//! | System | Reads | Gives |
//! |---|---|---|
//! | macOS | `proc_pidinfo`: the list of threads, then each thread's info | each thread's name, user and system time |
//! | others | nothing | `None`: not measured, never a 0 |

use std::time::Duration;

/// Each thread of this process, its name (empty when it has none) and the CPU the
/// system counted for it so far: user and system time. A thread that ends while the
/// list is read is left out. `None` where the system is not measured.
pub fn cpu_by_thread() -> Option<Vec<(String, Duration)>> {
    system::cpu_by_thread()
}

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "proc_pidinfo has no safe wrapper; each block says why it is sound"
)]
mod system {
    use std::ffi::CStr;
    use std::time::Duration;

    /// `PROC_PIDLISTTHREADS` from `<sys/proc_info.h>`: a handle for each thread of a
    /// process, eight bytes each. libc does not declare it.
    const PROC_PIDLISTTHREADS: libc::c_int = 6;
    /// Room for this many threads at first. A full list is read again, twice as long.
    const FIRST: usize = 256;
    const HANDLE: usize = size_of::<u64>();

    pub(super) fn cpu_by_thread() -> Option<Vec<(String, Duration)>> {
        let pid = libc::c_int::try_from(std::process::id()).ok()?;
        let mut handles = vec![0_u64; FIRST];
        let count = loop {
            let room = libc::c_int::try_from(handles.len() * HANDLE).ok()?;
            // SAFETY: the system writes at most `room` bytes into `handles`, which holds
            // that many and lives across the call. Nothing keeps the pointer.
            let written = unsafe {
                libc::proc_pidinfo(
                    pid,
                    PROC_PIDLISTTHREADS,
                    0,
                    handles.as_mut_ptr().cast(),
                    room,
                )
            };
            let written = usize::try_from(written).ok().filter(|&w| w > 0)?;
            if written < handles.len() * HANDLE {
                break written / HANDLE;
            }
            handles.resize(handles.len() * 2, 0);
        };
        Some(
            handles
                .iter()
                .take(count)
                .filter_map(|&handle| thread(pid, handle))
                .collect(),
        )
    }

    /// One thread's name and CPU, or `None` once it has ended.
    fn thread(pid: libc::c_int, handle: u64) -> Option<(String, Duration)> {
        // SAFETY: the struct is integers and bytes only: all zeros is a valid value.
        let mut info: libc::proc_threadinfo = unsafe { std::mem::zeroed() };
        let size = libc::c_int::try_from(size_of::<libc::proc_threadinfo>()).ok()?;
        // SAFETY: the system writes at most `size` bytes into `info`, which is that big
        // and lives across the call. Nothing keeps the pointer.
        let written = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTHREADINFO,
                handle,
                (&raw mut info).cast(),
                size,
            )
        };
        if written != size {
            return None;
        }
        let bytes = info.pth_name.map(libc::c_char::cast_unsigned);
        let name = CStr::from_bytes_until_nul(&bytes).map_or_else(
            |_| String::from_utf8_lossy(&bytes).into_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        // Both in nanoseconds: the kernel converts each thread's times from seconds and
        // microseconds (`fill_taskthreadinfo`).
        let cpu = Duration::from_nanos(info.pth_user_time.saturating_add(info.pth_system_time));
        Some((name, cpu))
    }
}

#[cfg(not(target_os = "macos"))]
mod system {
    use std::time::Duration;

    pub(super) fn cpu_by_thread() -> Option<Vec<(String, Duration)>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::cpu_by_thread;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    /// The CPU the calling thread used so far, by its own clock: the outside source the
    /// system's list is checked against.
    #[expect(
        unsafe_code,
        reason = "clock_gettime has no safe wrapper; the block says why it is sound"
    )]
    fn own_cpu() -> Duration {
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: writes one timespec into `time`, which lives across the call.
        let result = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &raw mut time) };
        assert_eq!(result, 0, "the thread's own CPU clock");
        let seconds = u64::try_from(time.tv_sec).unwrap_or(0);
        let nanos = u32::try_from(time.tv_nsec).unwrap_or(0);
        Duration::new(seconds, nanos)
    }

    #[test]
    fn more_threads_than_the_first_room_are_all_listed() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        const NAME: &str = "cpu-many";
        const MANY: usize = 300; // more than the room for 256 the list starts with
        let stop = Arc::new(AtomicBool::new(false));
        let (running, ran) = mpsc::channel();
        let threads: Vec<_> = (0..MANY)
            .filter_map(|_| {
                let stop = Arc::clone(&stop);
                let running = running.clone();
                thread::Builder::new()
                    .name(NAME.into())
                    .spawn(move || {
                        let _ = running.send(());
                        drop(running);
                        while !stop.load(Ordering::Acquire) {
                            thread::park();
                        }
                    })
                    .ok()
            })
            .collect();
        assert_eq!(threads.len(), MANY, "the system gave every thread");
        // A thread names itself when it first runs, before its closure (std
        // `thread/lifecycle.rs`; macOS names only the calling thread): list them once
        // every one has sent. Each sender is dropped once used, ours too: a thread that
        // ends unsent ends the receive short, and the test fails.
        drop(running);
        assert_eq!(ran.iter().take(MANY).count(), MANY, "every thread ran");
        let listed =
            cpu_by_thread().map(|threads| threads.iter().filter(|(name, _)| name == NAME).count());
        stop.store(true, Ordering::Release);
        for thread in threads {
            thread.thread().unpark();
            assert!(thread.join().is_ok());
        }
        // Read where the system is measured, every one of them.
        assert_eq!(listed, cfg!(target_os = "macos").then_some(MANY));
    }

    #[test]
    fn a_thread_is_read_by_its_name_with_its_user_and_system_time_in_nanoseconds() {
        const NAME: &str = "cpu-by-thread";
        /// Reads of the system's list the thread makes first: CPU in user and system time.
        const READS: u32 = 1000;
        let (ran, read) = mpsc::channel();
        let busy = thread::Builder::new().name(NAME.into()).spawn(move || {
            let mut sum = 0_u64;
            for _ in 0..READS {
                let _ = cpu_by_thread();
                sum = sum.wrapping_add(std::hint::black_box(1));
            }
            // The system's reading lies between two readings of the thread's own clock.
            let before = own_cpu();
            let listed = cpu_by_thread().map(|threads| {
                threads
                    .into_iter()
                    .find(|(name, _)| name == NAME)
                    .map(|(_, cpu)| cpu)
            });
            let after = own_cpu();
            let _ = ran.send((before, listed, after, sum));
        });
        assert!(busy.is_ok(), "the system gave no thread");
        let got = read.recv();
        assert!(got.is_ok(), "the thread never read its CPU");
        if let Ok(busy) = busy {
            assert!(busy.join().is_ok());
        }
        let Ok((before, listed, after, _)) = got else {
            return;
        };
        // Read where the system is measured. It counts in microseconds: one below is fine.
        assert_eq!(listed.is_some(), cfg!(target_os = "macos"));
        if let Some(listed) = listed {
            assert!(listed.is_some(), "the thread is listed by its name");
            let cpu = listed.unwrap_or_default();
            assert!(
                before.saturating_sub(Duration::from_micros(1)) <= cpu && cpu <= after,
                "{before:?} <= {cpu:?} <= {after:?}"
            );
        }
    }
}
