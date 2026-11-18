import assert from "node:assert/strict";
import { test } from "node:test";
import { merge, receive } from "./picture.ts";

const state = (frame) => ({ frame, priority: "RealTime", blackout: false });
const picture = { state: state(1), universes: [[0, 0, 0], [0, 0, 0]] };

// [picture before, reply, picture after]
const table = [
  [undefined, "Accepted", undefined],
  [picture, "Accepted", picture],
  [picture, { Refused: { reason: "QueueFull" } }, picture],
  [undefined, { Picture: picture }, picture],
  [undefined, { Changed: { state: state(2), channels: [] } }, undefined],
  [picture, { Changed: { state: state(2), channels: [] } }, { state: state(2), universes: picture.universes }],
  [
    picture,
    { Changed: { state: state(3), channels: [{ universe: 2, address: 3, value: 9 }] } },
    { state: state(3), universes: [[0, 0, 0], [0, 0, 9]] },
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
