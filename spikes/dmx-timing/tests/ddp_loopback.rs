//! The DDP sink sends one well-formed packet per frame, and the rainbow moves one column every frame.
//! A UDP socket in the test stands in for WLED. It checks what is sent, not how fast:
//! CI runners wake late, timing is judged by the measured runs.

use dmx_timing::sink::{DDP_COLUMNS, DDP_PACKET, DDP_PIXELS, Sink, rainbow};
use dmx_timing::{Config, Priority, Wait, start};
use std::net::UdpSocket;
use std::time::Duration;

#[test]
fn every_frame_arrives_as_one_rgbw_packet_with_the_rainbow_one_column_further() {
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let sink = Sink::ddp(&receiver.local_addr().unwrap().to_string()).unwrap();
    let config = Config {
        priority: Priority::Default,
        wait: Wait::Spin,
        duration: Duration::from_secs(1),
    };
    let running = start(config, sink).unwrap();

    let mut packets = Vec::new();
    let mut buffer = [0_u8; 2048];
    // until the run has ended and nothing more comes
    loop {
        match receiver.recv(&mut buffer) {
            Ok(n) => packets.push(buffer[..n].to_vec()),
            Err(_) if running.is_finished() => break,
            Err(_) => {}
        }
    }
    let report = running.finish();

    assert_eq!(packets.len(), report.frames, "one packet per frame sent");
    assert!(report.frames > 1, "some frames: {}", report.frames);
    assert_eq!(report.send_errors, 0);
    for (n, packet) in packets.iter().enumerate() {
        let frame = n + 1;
        // (what, byte, expected)
        let header = [
            ("flags: version 1, push", packet[0], 0x41),
            ("sequence 1 to 15", packet[1], (frame % 15) as u8 + 1),
            ("data type RGBW, 8 bit", packet[2], 0x1B),
            ("device 1", packet[3], 1),
            ("offset", packet[4] | packet[5] | packet[6] | packet[7], 0),
        ];
        for (what, got, expected) in header {
            assert_eq!(got, expected, "frame {frame}: {what}");
        }
        assert_eq!(packet.len(), DDP_PACKET, "frame {frame}: length");
        let length = u16::from_be_bytes([packet[8], packet[9]]) as usize;
        assert_eq!(length, DDP_PIXELS * 4, "frame {frame}: length field");
        let pixels: Vec<&[u8]> = packet[10..].chunks_exact(4).collect();
        let shift = frame % DDP_COLUMNS;
        for (i, pixel) in pixels.iter().enumerate() {
            let column = i % DDP_COLUMNS;
            let [r, g, b] = rainbow((column + DDP_COLUMNS - shift) % DDP_COLUMNS);
            assert_eq!(*pixel, [r, g, b, 0], "frame {frame}: pixel {i}");
        }
    }
}
