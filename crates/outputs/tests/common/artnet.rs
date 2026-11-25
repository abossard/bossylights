//! A receiver of ArtDmx written from `docs/design/outputs-artnet.md`, not from our sender:
//! it decodes every field and checks every rule. An outside decoder, `artnet_protocol`
//! (MIT), reads the same packet and must agree, and its encoder must make the same bytes.
//! ArtPoll and ArtPollReply the same way; a fake node's reply is made by `artnet_protocol`.

#![allow(dead_code, reason = "each test file uses its own part")]

use artnet_protocol::{ArtCommand, Output, PortAddress};

/// One ArtDmx as a node sees it.
#[derive(Debug, PartialEq, Eq)]
pub struct ArtDmx {
    /// 1 to 255; 0 would mean "no sequence".
    pub sequence: u8,
    pub physical: u8,
    /// 15 bits: Net, Sub-Net, Universe.
    pub port_address: u16,
    /// The slots after the header, as many as the length field says.
    pub slots: Vec<u8>,
}

/// Decodes one packet and checks every rule of the spec. Panics on the first that breaks.
pub fn decode(packet: &[u8]) -> ArtDmx {
    assert!(
        packet.len() >= 18,
        "an ArtDmx has 18 header bytes: {packet:?}"
    );
    assert_eq!(&packet[..8], b"Art-Net\0", "id");
    assert_eq!(
        u16::from_le_bytes([packet[8], packet[9]]),
        0x5000,
        "opcode, low byte first"
    );
    assert_eq!(
        u16::from_be_bytes([packet[10], packet[11]]),
        14,
        "protocol version 14"
    );
    let port_address = u16::from_le_bytes([packet[14], packet[15]]);
    assert_eq!(port_address & 0x8000, 0, "bit 15 of a Port-Address is 0");
    assert_ne!(port_address, 0, "Port-Address 0 is deprecated");
    let length = usize::from(u16::from_be_bytes([packet[16], packet[17]]));
    assert!((2..=512).contains(&length), "length 2 to 512: {length}");
    assert_eq!(length % 2, 0, "length even: {length}");
    assert_eq!(
        packet.len(),
        18 + length,
        "the length field counts the slots"
    );
    let ours = ArtDmx {
        sequence: packet[12],
        physical: packet[13],
        port_address,
        slots: packet[18..].to_vec(),
    };
    assert_eq!(outside(packet), ours, "artnet_protocol reads the same");
    assert_eq!(
        made_outside(&ours),
        packet,
        "artnet_protocol makes the same bytes"
    );
    ours
}

/// The packet as `artnet_protocol` reads it.
fn outside(packet: &[u8]) -> ArtDmx {
    let ArtCommand::Output(output) = ArtCommand::from_buffer(packet).expect("an Art-Net packet")
    else {
        panic!("not an ArtDmx: {packet:?}");
    };
    assert_eq!(output.version, [0, 14], "artnet_protocol: version");
    assert_eq!(
        usize::from(*output.length),
        output.data.as_ref().len(),
        "artnet_protocol: length"
    );
    ArtDmx {
        sequence: output.sequence,
        physical: output.physical,
        port_address: output.port_address.into(),
        slots: output.data.as_ref().clone(),
    }
}

/// The bytes `artnet_protocol` makes for the same fields.
fn made_outside(dmx: &ArtDmx) -> Vec<u8> {
    let output = Output {
        sequence: dmx.sequence,
        physical: dmx.physical,
        port_address: PortAddress::try_from(dmx.port_address).expect("15 bits"),
        data: dmx.slots.clone().into(),
        ..Output::default()
    };
    ArtCommand::Output(output)
        .write_to_buffer()
        .expect("artnet_protocol encodes it")
}

/// One ArtPoll as a node sees it.
#[derive(Debug, PartialEq, Eq)]
pub struct ArtPoll {
    pub flags: u8,
    pub diagnostics: u8,
    pub esta: u16,
    pub oem: u16,
}

/// Decodes one ArtPoll of the current length (22 bytes) and checks every rule of the spec.
/// `artnet_protocol` reads its first 14 bytes, the oldest form, and must agree.
pub fn decode_poll(packet: &[u8]) -> ArtPoll {
    assert_eq!(
        packet.len(),
        22,
        "an ArtPoll of revision 1.4dp has 22 bytes"
    );
    assert_eq!(&packet[..8], b"Art-Net\0", "id");
    assert_eq!(
        u16::from_le_bytes([packet[8], packet[9]]),
        0x2000,
        "OpPoll, low byte first"
    );
    assert_eq!(
        u16::from_be_bytes([packet[10], packet[11]]),
        14,
        "protocol version 14"
    );
    assert_eq!(packet[12] & 0xC1, 0, "flags 7, 6 and 0 are 0");
    let top = u16::from_be_bytes([packet[14], packet[15]]);
    let bottom = u16::from_be_bytes([packet[16], packet[17]]);
    if packet[12] & 0x20 == 0 {
        assert_eq!(
            (top, bottom),
            (0, 0),
            "no target range without targeted mode"
        );
    }
    let ours = ArtPoll {
        flags: packet[12],
        diagnostics: packet[13],
        esta: u16::from_be_bytes([packet[18], packet[19]]),
        oem: u16::from_be_bytes([packet[20], packet[21]]),
    };
    let ArtCommand::Poll(poll) = ArtCommand::from_buffer(packet).expect("an Art-Net packet") else {
        panic!("artnet_protocol: not an ArtPoll: {packet:?}");
    };
    assert_eq!(poll.version, [0, 14], "artnet_protocol: version");
    assert_eq!(poll.talk_to_me.bits(), ours.flags, "artnet_protocol: flags");
    assert_eq!(
        poll.diagnostics_priority, ours.diagnostics,
        "artnet_protocol: priority"
    );
    ours
}

/// One ArtPollReply as a controller sees it.
#[derive(Debug, PartialEq, Eq)]
pub struct ArtPollReply {
    pub ip: std::net::Ipv4Addr,
    pub oem: u16,
    pub esta: u16,
    pub short_name: String,
    pub long_name: String,
    pub ports: u16,
    pub style: u8,
    pub bind_index: u8,
    pub status_2: u8,
}

/// Decodes one ArtPollReply of the current length (239 bytes) and checks every rule of the
/// spec. `artnet_protocol` reads the same.
pub fn decode_reply(packet: &[u8]) -> ArtPollReply {
    assert_eq!(packet.len(), 239, "an ArtPollReply has 239 bytes");
    assert_eq!(&packet[..8], b"Art-Net\0", "id");
    assert_eq!(
        u16::from_le_bytes([packet[8], packet[9]]),
        0x2100,
        "OpPollReply, low byte first"
    );
    assert_eq!(
        u16::from_le_bytes([packet[14], packet[15]]),
        6454,
        "Port 0x1936, low byte first"
    );
    let name = |field: &[u8]| {
        let end = field
            .iter()
            .position(|b| *b == 0)
            .expect("a name ends with a 0");
        String::from_utf8(field[..end].to_vec()).expect("a name is text")
    };
    let ports = u16::from_be_bytes([packet[172], packet[173]]);
    assert!(ports <= 4, "at most 4 ports: {ports}");
    assert!(packet[229..].iter().all(|b| *b == 0), "filler is 0");
    let ours = ArtPollReply {
        ip: std::net::Ipv4Addr::new(packet[10], packet[11], packet[12], packet[13]),
        oem: u16::from_be_bytes([packet[20], packet[21]]),
        esta: u16::from_le_bytes([packet[24], packet[25]]),
        short_name: name(&packet[26..44]),
        long_name: name(&packet[44..108]),
        ports,
        style: packet[200],
        bind_index: packet[211],
        status_2: packet[212],
    };
    let ArtCommand::PollReply(reply) = ArtCommand::from_buffer(packet).expect("an Art-Net packet")
    else {
        panic!("artnet_protocol: not an ArtPollReply: {packet:?}");
    };
    let outside = ArtPollReply {
        ip: reply.address,
        oem: u16::from_be_bytes(reply.oem),
        esta: reply.esta_code,
        short_name: name(&reply.short_name),
        long_name: name(&reply.long_name),
        ports: u16::from_be_bytes(reply.num_ports),
        style: reply.style,
        bind_index: reply.bind_index,
        status_2: reply.status_2,
    };
    assert_eq!(reply.port, 6454, "artnet_protocol: port");
    assert_eq!(outside, ours, "artnet_protocol reads the same");
    ours
}

/// The ArtPollReply of a node at `ip` whose output ports take `outputs` (one Net and
/// Sub-Net, at most 4), made by `artnet_protocol`: an outside encoder.
pub fn node_reply(ip: std::net::Ipv4Addr, outputs: &[u16]) -> Vec<u8> {
    use artnet_protocol::PollReply;
    assert!(outputs.len() <= 4, "4 ports in one reply");
    let first = outputs.first().copied().unwrap_or(0);
    let mut short_name = [0; 18];
    short_name[..9].copy_from_slice(b"Fake node");
    let mut reply = PollReply {
        address: ip,
        version: [0, 1],
        port_address: [(first >> 8) as u8 & 0x7F, (first >> 4) as u8 & 0x0F],
        short_name,
        num_ports: [0, outputs.len() as u8],
        bind_index: 1,
        ..PollReply::default()
    };
    for (port, output) in outputs.iter().enumerate() {
        assert_eq!(output >> 4, first >> 4, "one Net and Sub-Net per reply");
        reply.port_types[port] = 0x80;
        reply.swout[port] = (output & 0x0F) as u8;
    }
    ArtCommand::PollReply(Box::new(reply))
        .write_to_buffer()
        .expect("artnet_protocol encodes it")
}

/// An ArtPoll of the oldest form (14 bytes), made by `artnet_protocol`.
pub fn old_poll() -> Vec<u8> {
    ArtCommand::Poll(artnet_protocol::Poll::default())
        .write_to_buffer()
        .expect("artnet_protocol encodes it")
}
