import assert from "node:assert/strict";
import { test } from "node:test";
import { transition, versionEvent } from "./connection.ts";

// Every state and event pair: [from, event, to, effects]
const table = [
  ["connecting", "matched", "connecting", ["open"]],
  ["connecting", "mismatched", "other version", []],
  ["connecting", "opened", "connected", []],
  ["connecting", "lost", "disconnected", ["wait"]],
  ["connecting", "retry", "connecting", []],
  ["connecting", "close", "closed", ["shut"]],
  ["connected", "matched", "connected", []],
  ["connected", "mismatched", "connected", []],
  ["connected", "opened", "connected", []],
  ["connected", "lost", "disconnected", ["wait"]],
  ["connected", "retry", "connected", []],
  ["connected", "close", "closed", ["shut"]],
  ["disconnected", "matched", "disconnected", []],
  ["disconnected", "mismatched", "disconnected", []],
  ["disconnected", "opened", "disconnected", []],
  ["disconnected", "lost", "disconnected", []],
  ["disconnected", "retry", "connecting", ["check"]],
  ["disconnected", "close", "closed", ["shut"]],
  ["other version", "matched", "other version", []],
  ["other version", "mismatched", "other version", []],
  ["other version", "opened", "other version", []],
  ["other version", "lost", "other version", []],
  ["other version", "retry", "other version", []],
  ["other version", "close", "closed", ["shut"]],
  ["closed", "matched", "closed", []],
  ["closed", "mismatched", "closed", []],
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

// The engine's answer to GET /version, this UI's version, the event: [answer, mine, event]
const versions = [
  [undefined, 1, "lost"],
  ["1", 1, "matched"],
  ["1\n", 1, "matched"],
  ["2", 1, "mismatched"],
  ["", 1, "mismatched"],
  ["Not Found", 1, "mismatched"],
];

for (const [answer, mine, event] of versions) {
  test(`version ${JSON.stringify(answer)} for ${mine} = ${event}`, () => {
    assert.equal(versionEvent(answer, mine), event);
  });
}
