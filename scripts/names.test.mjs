import assert from "node:assert/strict";
import { test } from "node:test";
import { datedTestNames, kindsOf, protocolNames } from "./names.mjs";

const FILE = "crates/engine/src/lib.rs";
const KINDS = ["Ddp", "DmxUsbPro"];

// [line, named]
const cases = [
  ["let kind = OutputKind::Ddp { address, color };", true],
  ["/// sends DDP packets", true],
  ["fn send_ddp() {}", true],
  ["struct DdpPacket;", true],
  ["use std::net::UdpSocket;", true],
  ["// over udp", true],
  ["OutputKind::DmxUsbPro { adapter: 1 }", true],
  ["fn dmx_usb_pro() {}", true],
  ["// a DMX USB Pro adapter", true],
  ["// a dmx-usb-pro adapter", true],
  ["// the serial port", true],
  ["let port = usb_serial();", true],
  ["let port = SerialPort::open();", true],
  ["OutputKind::ArtNet { .. } => {}", true], // a variant not on the list yet
  ["use engine_core::OutputKind::*;", false],
  ["fn open(kind: OutputKind) {}", false],
  ["#[derive(Serialize, Deserialize)]", false],
  ["serializer.collect_seq(levels)", false],
  ["use serde::Serialize;", false],
  ["let output = Output::new(run, rate, now);", false],
  ["fn add_pixel(updated: bool) {}", false],
  ["// a socket, a dmx universe, a usb hub", false],
];
for (const [line, named] of cases) {
  test(`${named ? "names" : "does not name"}: ${line}`, () => {
    assert.equal(protocolNames(FILE, line, KINDS).length, named ? 1 : 0);
  });
}

// [line, named with ArtNet on the list]
const newKind = [
  ["ArtNet { universe } => send(universe),", true],
  ["fn art_net_packet() {}", true],
  ["// over Art-Net", true],
  ["// art, then net", false],
];
for (const [line, named] of newKind) {
  test(`a new kind ArtNet: ${named ? "names" : "does not name"}: ${line}`, () => {
    assert.equal(protocolNames(FILE, line, [...KINDS, "ArtNet"]).length, named ? 1 : 0);
  });
}

test("the list of kinds may name them; the hit says where", () => {
  const source = "a\nOutputKind::Ddp";
  assert.deepEqual(protocolNames("crates/engine-core/src/kinds.rs", source, KINDS), []);
  assert.deepEqual(protocolNames(FILE, source, KINDS), [`${FILE}:2: OutputKind::Ddp`]);
});

// [source, kinds]
const kindCases = [
  [
    `/// What kind.
#[derive(Clone)]
pub enum OutputKind {
    /// DDP over UDP. \`address\` is {"a": 1}.
    Ddp {
        #[cfg_attr(test, ts(type = "string"))]
        address: SocketAddrV4,
        color: Color,
    },
    DmxUsbPro { adapter: Adapter },
    ArtNet(Universe),
    Sacn,
}

pub enum Color { Rgb }`,
    ["Ddp", "DmxUsbPro", "ArtNet", "Sacn"],
  ],
  ["pub enum Color { Rgb }", []],
];
for (const [source, kinds] of kindCases) {
  test(`kinds of ${JSON.stringify(source.slice(0, 40))}`, () => {
    assert.deepEqual(kindsOf(source), kinds);
  });
}

// [path, named by a milestone, a ticket or a review]
const testNameCases = [
  ["crates/engine-server/tests/m3_wake.rs", true],
  ["crates/engine/tests/t7_review_realtime.rs", true],
  ["crates/engine-server/tests/review_248_queued_press.rs", true],
  ["crates/engine-server/tests/review_261.rs", true],
  ["crates/engine-server/tests/t4_channels.rs", true],
  ["e2e/m4_status.spec.ts", true],
  ["crates/engine-server/tests/first_due_frame.rs", false],
  ["crates/engine/tests/mock/mod.rs", false],
  ["crates/engine/src/m3_thing.rs", false],
  ["crates/engine-server/tests/mover_step.rs", false],
];
for (const [path, dated] of testNameCases) {
  test(`test name ${path}: ${dated ? "dated" : "plain"}`, () => assert.deepEqual(datedTestNames([path]), dated ? [path] : []));
}
