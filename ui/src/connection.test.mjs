import assert from "node:assert/strict";
import { test } from "node:test";
import { transition } from "./connection.ts";

// Every state and event pair: [from, event, to, effects]
const table = [
  ["connecting", "opened", "connected", []],
  ["connecting", "lost", "disconnected", ["wait"]],
  ["connecting", "retry", "connecting", []],
  ["connecting", "close", "closed", ["shut"]],
  ["connected", "opened", "connected", []],
  ["connected", "lost", "disconnected", ["wait"]],
  ["connected", "retry", "connected", []],
  ["connected", "close", "closed", ["shut"]],
  ["disconnected", "opened", "disconnected", []],
  ["disconnected", "lost", "disconnected", []],
  ["disconnected", "retry", "connecting", ["open"]],
  ["disconnected", "close", "closed", ["shut"]],
  ["closed", "opened", "closed", []],
  ["closed", "lost", "closed", []],
  ["closed", "retry", "closed", []],
  ["closed", "close", "closed", []],
];

for (const [from, event, to, effects] of table) {
  test(`${from} + ${event} = ${to} ${JSON.stringify(effects)}`, () => {
    assert.deepEqual(transition(from, event), [to, effects]);
  });
}
