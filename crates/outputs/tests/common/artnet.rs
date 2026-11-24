//! A receiver of ArtDmx written from `docs/design/outputs-artnet.md`, not from our sender:
//! it decodes every field and checks every rule. An outside decoder, `artnet_protocol`
//! (MIT), reads the same packet and must agree, and its encoder must make the same bytes.

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
