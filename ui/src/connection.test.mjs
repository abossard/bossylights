import assert from "node:assert/strict";
import { test } from "node:test";
import { pickEngine, start, transition, versionEvent } from "./connection.ts";

// Every state and event pair, as in docs/design/ui-states.md: [from, event, to, effects]
const table = [
  ["connecting", "matched", "connecting", ["open"]],
  ["connecting", "mismatched", "other version", ["wait"]],
  ["connecting", "opened", "connected", []],
  ["connecting", "lost", "disconnected", ["wait"]],
  ["connecting", "retry", "connecting", []],
  ["connecting", "garbled", "connecting", []],
  ["connecting", "close", "closed", ["shut"]],
  ["connected", "matched", "connected", []],
  ["connected", "mismatched", "connected", []],
  ["connected", "opened", "connected", []],
  ["connected", "lost", "disconnected", ["wait"]],
  ["connected", "retry", "connected", []],
  ["connected", "garbled", "unreadable reply", ["shut",  "wait"]],
  ["connected", "close", "closed", ["shut"]],
  ["disconnected", "matched", "disconnected", []],
  ["disconnected", "mismatched", "disconnected", []],
  ["disconnected", "opened", "disconnected", []],
  ["disconnected", "lost", "disconnected", []],
  ["disconnected", "retry", "connecting", ["check"]],
  ["disconnected", "garbled", "disconnected", []],
  ["disconnected", "close", "closed", ["shut"]],
  ["other version", "matched", "connecting", ["open"]],
  ["other version", "mismatched", "other version", ["wait"]],
  ["other version", "opened", "other version", []],
  ["other version", "lost", "other version", ["wait"]],
  ["other version", "retry", "other version", ["check"]],
  ["other version", "garbled", "other version", []],
  ["other version", "close", "closed", ["shut"]],
  ["unreadable reply", "matched", "unreadable reply", []],
  ["unreadable reply", "mismatched", "unreadable reply", []],
  ["unreadable reply", "opened", "unreadable reply", []],
  ["unreadable reply", "lost", "unreadable reply", []],
  ["unreadable reply", "retry", "connecting", ["check"]],
  ["unreadable reply", "garbled", "unreadable reply", []],
  ["unreadable reply", "close", "closed", ["shut"]],
  ["no address", "matched", "no address", []],
  ["no address", "mismatched", "no address", []],
  ["no address", "opened", "no address", []],
  ["no address", "lost", "no address", []],
  ["no address", "retry", "no address", []],
  ["no address", "garbled", "no address", []],
  ["no address", "close", "closed", []],
  ["closed", "matched", "closed", []],
  ["closed", "mismatched", "closed", []],
  ["closed", "opened", "closed", []],
  ["closed", "lost", "closed", []],
  ["closed", "retry", "closed", []],
  ["closed", "garbled", "closed", []],
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

// The page's query string, what the shell told the window, the address used or the unusable text.
// Usable: a ws: or wss: URL without a fragment (the WHATWG WebSocket constructor refuses the rest).
const engines = [
  ["", undefined, { url: "ws://127.0.0.1:7744/api" }],
  ["", "ws://127.0.0.1:7798/api", { url: "ws://127.0.0.1:7798/api" }],
  ["?engine=ws://example:1/api", "ws://127.0.0.1:7798/api", { url: "ws://example:1/api" }],
  ["?engine=wss://example/api", undefined, { url: "wss://example/api" }],
  ["?other=1", undefined, { url: "ws://127.0.0.1:7744/api" }],
  ["?engine=", undefined, { unusable: "" }],
  ["?engine=nonsense", undefined, { unusable: "nonsense" }],
  ["?engine=http://example/api", undefined, { unusable: "http://example/api" }],
  ["?engine=ws://example:1/api%23x", undefined, { unusable: "ws://example:1/api#x" }],
  ["", "not a url", { unusable: "not a url" }],
];
for (const [search, told, used] of engines) {
  test(`engine for ${JSON.stringify(search)} told ${told}: ${JSON.stringify(used)}`, () => {
    const address = pickEngine(search, told);
    assert.deepEqual("url" in address ? { url: address.url.href } : address, used);
  });
}

// Where the machine starts: [address, state, effects]
const starts = [
  [{ url: new URL("ws://127.0.0.1:7744/api") }, "connecting", ["check"]],
  [{ unusable: "" }, "no address", []],
];
for (const [address, state, effects] of starts) {
  test(`starts ${state} for ${JSON.stringify(address)}`, () => assert.deepEqual(start(address), [state, effects]));
}
