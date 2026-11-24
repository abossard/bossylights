//! A receiver of sACN data packets written from `docs/design/outputs-sacn.md`, not from our
//! sender: it decodes every field and checks every rule. An outside decoder, the `sacn`
//! crate (MIT OR Apache-2.0), reads the same packet and must agree, and its encoder must
//! make the same bytes.

use sacn::packet::{AcnRootLayerProtocol, E131RootLayerData};

/// One data packet as a receiver sees it.
#[derive(Debug, PartialEq, Eq)]
pub struct Data {
    pub cid: [u8; 16],
    /// The source name up to its first zero.
    pub name: String,
    pub priority: u8,
    pub sequence: u8,
    /// `0x40`: the stream is terminated.
    pub options: u8,
    /// 1 to 63,999.
    pub universe: u16,
    /// The slots after the start code.
    pub slots: Vec<u8>,
}

fn u16_at(packet: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([packet[at], packet[at + 1]])
}

fn u32_at(packet: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([packet[at], packet[at + 1], packet[at + 2], packet[at + 3]])
}

/// Flags `0x7` in the top 4 bits, then the length from the field to the end.
fn flags_and_length(packet: &[u8], at: usize, field: &str) {
    let value = u16_at(packet, at);
    assert_eq!(value >> 12, 0x7, "{field} flags");
    assert_eq!(
        usize::from(value & 0x0FFF),
        packet.len() - at,
        "{field} length"
    );
}

/// Decodes one packet and checks every rule of the spec. Panics on the first that breaks.
pub fn decode(packet: &[u8]) -> Data {
    assert!(
        (126..=638).contains(&packet.len()),
        "126 header bytes and 0 to 512 slots: {}",
        packet.len()
    );
    assert_eq!(u16_at(packet, 0), 0x0010, "preamble size");
    assert_eq!(u16_at(packet, 2), 0x0000, "postamble size");
    assert_eq!(&packet[4..16], b"ASC-E1.17\0\0\0", "packet id");
    flags_and_length(packet, 16, "root");
    assert_eq!(u32_at(packet, 18), 4, "root vector: data");
    flags_and_length(packet, 38, "framing");
    assert_eq!(u32_at(packet, 40), 2, "framing vector: data packet");
    let name = &packet[44..108];
    let end = name
        .iter()
        .position(|byte| *byte == 0)
        .expect("the source name ends with a zero");
    assert!(
        name[end..].iter().all(|byte| *byte == 0),
        "zero-padded name"
    );
    let priority = packet[108];
    assert!(priority <= 200, "priority 0 to 200: {priority}");
    assert_eq!(u16_at(packet, 109), 0, "sync address: not synchronised");
    let options = packet[112];
    assert_eq!(
        options & !0x40,
        0,
        "no preview, no force sync: {options:#x}"
    );
    let universe = u16_at(packet, 113);
    assert!(
        (1..=63_999).contains(&universe),
        "universe 1 to 63,999: {universe}"
    );
    flags_and_length(packet, 115, "DMP");
    assert_eq!(packet[117], 0x02, "DMP vector: set property");
    assert_eq!(packet[118], 0xA1, "address and data type");
    assert_eq!(u16_at(packet, 119), 0, "first address");
    assert_eq!(u16_at(packet, 121), 1, "address increment");
    assert_eq!(
        usize::from(u16_at(packet, 123)),
        packet.len() - 125,
        "value count: slots and the start code"
    );
    assert_eq!(packet[125], 0, "start code");
    let data = Data {
        cid: packet[22..38].try_into().unwrap(),
        name: String::from_utf8(name[..end].to_vec()).expect("UTF-8"),
        priority,
        sequence: packet[111],
        options,
        universe,
        slots: packet[126..].to_vec(),
    };
    agrees_with_the_sacn_crate(packet, &data);
    data
}

fn agrees_with_the_sacn_crate(packet: &[u8], data: &Data) {
    let parsed = AcnRootLayerProtocol::parse(packet).expect("the sacn crate reads it");
    assert_eq!(
        parsed.pack_alloc().expect("the sacn crate packs it"),
        packet,
        "sacn crate: the same bytes"
    );
    assert_eq!(parsed.pdu.cid.as_bytes(), &data.cid, "sacn crate: CID");
    let E131RootLayerData::DataPacket(framing) = parsed.pdu.data else {
        panic!("sacn crate: not a data packet");
    };
    assert_eq!(framing.source_name, data.name, "sacn crate: name");
    assert_eq!(framing.priority, data.priority, "sacn crate: priority");
    assert_eq!(framing.synchronization_address, 0, "sacn crate: sync");
    assert_eq!(
        framing.sequence_number, data.sequence,
        "sacn crate: sequence"
    );
    assert!(!framing.preview_data, "sacn crate: preview");
    assert_eq!(
        framing.stream_terminated,
        data.options == 0x40,
        "sacn crate: terminated"
    );
    assert!(!framing.force_synchronization, "sacn crate: force sync");
    assert_eq!(framing.universe, data.universe, "sacn crate: universe");
    let values = framing.data.property_values;
    assert_eq!(values.first(), Some(&0), "sacn crate: start code");
    assert_eq!(&values[1..], &data.slots[..], "sacn crate: slots");
}
