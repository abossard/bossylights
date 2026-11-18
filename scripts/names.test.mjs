import assert from "node:assert/strict";
import { test } from "node:test";
import { protocolNames } from "./names.mjs";

const FILE = "crates/engine/src/lib.rs";

// [line, named]
const cases = [
  ["let kind = OutputKind::Ddp { address, color };", true],
  ["/// sends DDP packets", true],
  ["use std::net::UdpSocket;", true],
  ["// over udp", true],
  ["OutputKind::DmxUsbPro { adapter: 1 }", true],
  ["fn dmx_usb_pro() {}", true],
  ["// a DMX USB Pro adapter", true],
  ["// the serial port", true],
  ["let port = SerialPort::open();", true],
  ["#[derive(Serialize, Deserialize)]", false],
  ["serializer.collect_seq(levels)", false],
  ["use serde::Serialize;", false],
  ["let output = Output::new(run, rate, now);", false],
  ["// a socket", false],
];
for (const [line, named] of cases) {
  test(`${named ? "names" : "does not name"}: ${line}`, () => {
    assert.equal(protocolNames(FILE, line).length, named ? 1 : 0);
  });
}

test("the list of kinds may name them; the hit says where", () => {
  const source = "a\nOutputKind::Ddp";
  assert.deepEqual(protocolNames("crates/engine-core/src/kinds.rs", source), []);
  assert.deepEqual(protocolNames(FILE, source), [`${FILE}:2: OutputKind::Ddp`]);
});
