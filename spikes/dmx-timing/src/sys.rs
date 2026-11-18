//! The only `unsafe` in the spike: calls into the OS for thread priority and the serial
//! port. Each call checks its result; none keeps a pointer past the call.

use crate::Priority;
use std::ffi::CStr;
use std::io;
use std::time::Duration;

/// Raises the calling thread's priority. `Err` says why not; the thread then keeps its default.
pub fn raise_priority(priority: Priority, period: Duration) -> Result<(), String> {
    match priority {
        Priority::Default => Ok(()),
        Priority::Qos => qos(),
        Priority::Realtime => realtime(period),
    }
}

#[cfg(target_os = "macos")]
fn qos() -> Result<(), String> {
    // SAFETY: changes only the calling thread's QoS class; no pointers.
    let result = unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
    };
    if result == 0 {
        Ok(())
    } else {
        Err(format!("pthread_set_qos_class_self_np: {result}"))
    }
}

/// The Mach time-constraint policy, as audio threads use it: every `period` the thread
/// needs about `COMPUTATION` of CPU, finished within `CONSTRAINT` after it wakes.
#[cfg(target_os = "macos")]
fn realtime(period: Duration) -> Result<(), String> {
    const COMPUTATION: Duration = Duration::from_millis(2);
    const CONSTRAINT: Duration = Duration::from_millis(5);

    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    unsafe extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
    }
    let mut timebase = Timebase { numer: 0, denom: 0 };
    // SAFETY: writes two u32 into the struct we pass.
    if unsafe { mach_timebase_info(&mut timebase) } != 0 || timebase.numer == 0 {
        return Err("mach_timebase_info failed".into());
    }
    let ticks = |d: Duration| {
        let ticks = d.as_nanos() * u128::from(timebase.denom) / u128::from(timebase.numer);
        u32::try_from(ticks).unwrap_or(u32::MAX)
    };
    let mut policy = libc::thread_time_constraint_policy {
        period: ticks(period),
        computation: ticks(COMPUTATION),
        constraint: ticks(CONSTRAINT),
        preemptible: 1,
    };
    // SAFETY: `policy` is the struct this flavor expects and lives across the call;
    // the count is its size in integers, as the header defines it.
    let result = unsafe {
        libc::thread_policy_set(
            libc::pthread_mach_thread_np(libc::pthread_self()),
            libc::THREAD_TIME_CONSTRAINT_POLICY as libc::thread_policy_flavor_t,
            (&raw mut policy).cast(),
            libc::THREAD_TIME_CONSTRAINT_POLICY_COUNT,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(format!("thread_policy_set: {result}"))
    }
}

#[cfg(not(target_os = "macos"))]
fn qos() -> Result<(), String> {
    Err("QoS classes exist only on macOS".into())
}

#[cfg(not(target_os = "macos"))]
fn realtime(_: Duration) -> Result<(), String> {
    Err("the time-constraint policy exists only on macOS".into())
}

/// Opens a serial port raw, 8 data bits, 2 stop bits, no flow control, at `baud`,
/// exclusively. Writes block only if `blocking`.
#[cfg(target_os = "macos")]
pub fn open_serial(path: &CStr, baud: u32, blocking: bool) -> io::Result<i32> {
    /// `_IOW('T', 2, speed_t)` from IOKit/serial/ioss.h: any baud rate, here 250000.
    const IOSSIOSPEED: libc::c_ulong = 0x8008_5402;
    let check = |result: libc::c_int| {
        if result == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(result)
        }
    };
    // SAFETY: `path` is a valid C string; the descriptor is closed on every error below.
    let fd = check(unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK,
        )
    })?;
    let setup = || -> io::Result<()> {
        // SAFETY: plain calls on our own descriptor, with a termios we own.
        unsafe {
            check(libc::ioctl(fd, libc::TIOCEXCL as libc::c_ulong))?; // a second open fails with "busy"
            if blocking {
                check(libc::fcntl(fd, libc::F_SETFL, 0))?;
            }
            let mut tio: libc::termios = std::mem::zeroed();
            check(libc::tcgetattr(fd, &mut tio))?;
            libc::cfmakeraw(&mut tio);
            tio.c_cflag |= libc::CS8 | libc::CSTOPB | libc::CLOCAL | libc::CREAD;
            tio.c_cflag &= !(libc::PARENB | libc::CRTSCTS);
            check(libc::cfsetspeed(&mut tio, libc::B9600))?;
            check(libc::tcsetattr(fd, libc::TCSANOW, &tio))?;
            let speed: libc::speed_t = libc::speed_t::from(baud);
            check(libc::ioctl(fd, IOSSIOSPEED, &speed))?;
        }
        Ok(())
    };
    setup().inspect_err(|_| close(fd))?;
    Ok(fd)
}

#[cfg(target_os = "macos")]
pub fn set_break(fd: i32, on: bool) -> io::Result<()> {
    let request = if on { libc::TIOCSBRK } else { libc::TIOCCBRK };
    // SAFETY: a request without an argument on our own descriptor.
    if unsafe { libc::ioctl(fd, request as libc::c_ulong) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub fn drain(fd: i32) -> io::Result<()> {
    // SAFETY: waits for our own descriptor's output to be sent.
    if unsafe { libc::tcdrain(fd) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
pub fn open_serial(_: &CStr, _: u32, _: bool) -> io::Result<i32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "the serial sinks are macOS only in this spike",
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn set_break(_: i32, _: bool) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(not(target_os = "macos"))]
pub fn drain(_: i32) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

pub fn write_all(fd: i32, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        // SAFETY: reads `bytes.len()` bytes from a live slice.
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        bytes = &bytes[written as usize..];
    }
    Ok(())
}

/// One write on a non-blocking descriptor: how many bytes the driver took, 0 if none.
pub fn write_some(fd: i32, bytes: &[u8]) -> io::Result<usize> {
    loop {
        // SAFETY: reads `bytes.len()` bytes from a live slice.
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written >= 0 {
            return Ok(written as usize);
        }
        let error = io::Error::last_os_error();
        match error.kind() {
            io::ErrorKind::Interrupted => continue,
            io::ErrorKind::WouldBlock => return Ok(0),
            _ => return Err(error),
        }
    }
}

pub fn close(fd: i32) {
    // SAFETY: closes a descriptor this spike opened and no longer uses.
    unsafe { libc::close(fd) };
}
