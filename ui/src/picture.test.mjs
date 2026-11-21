import assert from "node:assert/strict";
import { test } from "node:test";
import { decode, merge, outputLine, outputLines, receive } from "./picture.ts";

const state = (frame) => ({ frame, priority: "RealTime", blackout: false });
const read = { Read: { read: 10, failed: 1 } };
const none = { outputs: [null, null] };
const ddp = { outputs: [{ Kind: { kind: { Ddp: { address: "127.0.0.1:4048", color: "Rgb" } } } }, null] };
const picture = { state: state(1), project: none, library: "Reading", universes: [[0, 0, 0], [0, 0, 0]] };

// [picture before, reply, picture after]
const table = [
  [undefined, "Accepted", undefined],
  [picture, "Accepted", picture],
  [picture, "Stopping", picture],
  [picture, { Refused: { reason: "QueueFull" } }, picture],
  [undefined, { Picture: picture }, picture],
  [picture, { Found: { fixtures: [] } }, picture],
  [picture, { Fixture: { maker: "Generic", model: "Generic RGB", kind: "ColorChanger", modes: [] } }, picture],
  [picture, { Failures: { failures: [] } }, picture],
  [undefined, { Changed: { state: state(2), project: none, library: read, channels: [] } }, undefined],
  [
    picture,
    { Changed: { state: state(2), project: ddp, library: read, channels: [] } },
    { state: state(2), project: ddp, library: read, universes: picture.universes },
  ],
  [
    picture,
    {
      Changed: {
        state: state(3),
        project: none,
        library: "Reading",
        channels: [{ universe: 2, address: 3, value: 9 }],
      },
    },
    { state: state(3), project: none, library: "Reading", universes: [[0, 0, 0], [0, 0, 9]] },
  ],
];
for (const [before, reply, after] of table) {
  test(`receive ${JSON.stringify(reply)}`, () => assert.deepEqual(receive(before, reply), after));
}

// [channels, universes after]
const merges = [
  [[], [[1, 2]]],
  [[{ universe: 1, address: 2, value: 7 }], [[1, 7]]],
  [[{ universe: 2, address: 1, value: 7 }], [[1, 2]]], // no such universe here
  [[{ universe: 1, address: 3, value: 7 }], [[1, 2]]], // no such address here
];
for (const [channels, expected] of merges) {
  test(`merge ${JSON.stringify(channels)}`, () => {
    const before = [[1, 2]];
    assert.deepEqual(merge(before, channels), expected);
    assert.deepEqual(before, [[1, 2]], "the old picture stays as it was");
  });
}

// A reply as text, whether this UI can read it. serde's externally tagged JSON: a unit variant is a string,
// any other an object of exactly one key. Picture and Changed carry a state with outputs, a project and a library.
const parts = { state: { outputs: [] }, project: { outputs: [] }, library: "Reading" };
const chan = { universe: 1, address: 2, value: 3 };
const texts = [
  ['"Accepted"', true],
  ['{"Refused":{"reason":"QueueFull"}}', true],
  [JSON.stringify({ Picture: { ...parts, universes: [] } }), true],
  [JSON.stringify({ Changed: { ...parts, channels: [] } }), true],
  ["null", false],
  ["[]", false],
  ["{}", false],
  ["7", false],
  ['{"Accepted":1,"Stopping":2}', false],
  ["not json", false],
  ['{"Picture":null}', false],
  ['{"Changed":null}', false],
  [JSON.stringify({ Picture: parts }), false], // no universes
  [JSON.stringify({ Changed: { ...parts, state: {} } }), false], // a state without outputs
  [JSON.stringify({ Picture: { ...parts, universes: [[0, 255]] } }), true],
  [JSON.stringify({ Changed: { ...parts, channels: [chan] } }), true],
  [JSON.stringify({ Picture: { ...parts, project: {}, universes: [] } }), false], // a project without outputs
  [JSON.stringify({ Changed: { ...parts, channels: [null] } }), false],
  [JSON.stringify({ Changed: { ...parts, channels: [{ ...chan, value: "3" }] } }), false],
  [JSON.stringify({ Picture: { ...parts, universes: [null] } }), false],
];
for (const [text, readable] of texts) {
  test(`decode ${text}: ${readable ? "read" : "unreadable"}`, () => assert.equal(decode(text) !== undefined, readable));
}

// An output in plain words: where it sends, its state, and the rate while it runs. Written by hand (owner, #266).
const run = { first: 1, count: 1 };
const counters = { accepted: 9, dropped: 1, failed: 2, off_beat: 3 };
const usb = (adapter) => ({ Kind: { kind: { DmxUsbPro: { adapter } }, universes: run } });
const ddpTo = (address) => ({ Kind: { kind: { Ddp: { address, color: "Rgb" } }, universes: run } });
// [status, rate, set up as, line]
const lines = [
  ["Running", "44", usb(1), "DMX USB Pro, running, 44 a second"],
  ["Running", "30", usb(2), "DMX USB Pro 2, running, 30 a second"],
  ["NotFound", "44", usb(1), "DMX USB Pro, not found"],
  ["Connecting", "44", usb(1), "DMX USB Pro, connecting"],
  [{ Reconnecting: { failures: 4 } }, "60", ddpTo("10.0.0.5:4048"), "DDP to 10.0.0.5:4048, reconnecting"],
  ["Running", "60", ddpTo("10.0.0.5:4048"), "DDP to 10.0.0.5:4048, running, 60 a second"],
  ["Running", "30", { Attached: { universes: run } }, "a test device, running, 30 a second"],
  ["Running", "30", null, "an output, running, 30 a second"],
];
for (const [status, rate, setUp, line] of lines) {
  test(`output line: ${line}`, () => assert.equal(outputLine({ universes: run, rate, status, counters }, setUp, "now"), line));
}

// The last picture before the connection went: where it sends, and that its state is not known now.
// [status, set up as, line]
const before = [
  ["Running", usb(1), "DMX USB Pro, state not known"],
  [{ Reconnecting: { failures: 4 } }, ddpTo("10.0.0.5:4048"), "DDP to 10.0.0.5:4048, state not known"],
];
for (const [status, setUp, line] of before) {
  test(`output line seen before: ${line}`, () => assert.equal(outputLine({ universes: run, rate: "44", status, counters }, setUp, "before"), line));
}

test("output lines: one per output the engine runs, in order", () => {
  const output = (status) => ({ universes: run, rate: "44", status, counters });
  const shown = { ...picture, state: { ...picture.state, outputs: [output("Running"), null, output("NotFound")] }, project: { outputs: [usb(1), null, ddpTo("10.0.0.5:4048")] } };
  assert.deepEqual(outputLines(shown, "now"), ["DMX USB Pro, running, 44 a second", "DDP to 10.0.0.5:4048, not found"]);
});
