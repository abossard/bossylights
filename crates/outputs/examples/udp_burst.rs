//! Does a burst of UDP sends a frame fill the socket buffer (#288)? Sends `packets` of
//! `bytes` each frame, non-blocking as a UDP output does, at `rate` frames a second, and
//! prints: sends, busy sends (would block, no buffers), other failed sends, frames with a
//! busy send.
//!
//! `cargo run -q --release -p outputs --example udp_burst -- <ip:port> <packets> <bytes> <frames> <rate>`
//! Art-Net at 128 universes: `128 530`. DDP at 128 universes: `46 1450`.

#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_macros,
    reason = "a measuring tool, not the output thread: it waits between frames and prints"
)]

use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
const NO_BUFFERS: i32 = 105;
#[cfg(not(target_os = "linux"))]
const NO_BUFFERS: i32 = 55;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [to, packets, bytes, frames, rate] = args.as_slice() else {
        eprintln!("udp_burst <ip:port> <packets> <bytes> <frames> <rate>");
        std::process::exit(2);
    };
    let number = |text: &String| text.parse::<u64>().map_err(std::io::Error::other);
    let to: SocketAddr = to.parse().map_err(std::io::Error::other)?;
    let (packets, bytes, frames, rate) = (
        number(packets)?,
        number(bytes)?,
        number(frames)?,
        number(rate)?,
    );
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_nonblocking(true)?;
    let packet = vec![0; usize::try_from(bytes).map_err(std::io::Error::other)?];
    let period = Duration::from_secs(1) / u32::try_from(rate).map_err(std::io::Error::other)?;
    let (mut sent, mut busy, mut failed, mut busy_frames) = (0_u64, 0_u64, 0_u64, 0_u64);
    let started = Instant::now();
    for frame in 0..frames {
        let mut busy_now = false;
        for _ in 0..packets {
            match socket.send_to(&packet, to) {
                Ok(_) => sent += 1,
                Err(e)
                    if e.kind() == ErrorKind::WouldBlock
                        || e.raw_os_error() == Some(NO_BUFFERS) =>
                {
                    busy += 1;
                    busy_now = true;
                }
                Err(_) => failed += 1,
            }
        }
        busy_frames += u64::from(busy_now);
        let due = started + period * u32::try_from(frame + 1).map_err(std::io::Error::other)?;
        std::thread::sleep(due.saturating_duration_since(Instant::now())); // the frame rate
    }
    println!("{sent} {busy} {failed} {busy_frames}");
    Ok(())
}
