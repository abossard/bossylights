use outputs::{Color, OutputKind};
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

/// Listen at the other end of a real DDP output. The timeout only guards a missing packet.
pub fn receiver(color: Color) -> (UdpSocket, OutputKind) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        unreachable!("bound to IPv4 loopback");
    };
    (socket, OutputKind::Ddp { address, color })
}
