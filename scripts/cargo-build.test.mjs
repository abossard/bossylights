import assert from "node:assert/strict";
import { test } from "node:test";
import { executables } from "./cargo-build.mjs";

// Message shapes from the cargo book, "JSON messages": one JSON object a line.
const bin = (name, executable) =>
  JSON.stringify({ reason: "compiler-artifact", target: { name, kind: ["bin"] }, executable });
const lib = (name) => JSON.stringify({ reason: "compiler-artifact", target: { name, kind: ["lib"] }, executable: null });
const done = JSON.stringify({ reason: "build-finished", success: true });

// [cargo's lines, programs]
const cases = [
  [[bin("bossylights", "/t/debug/bossylights"), done], { bossylights: "/t/debug/bossylights" }],
  [[lib("engine"), bin("bossylights-engine", "/x/release/bossylights-engine"), done], { "bossylights-engine": "/x/release/bossylights-engine" }],
  [[lib("engine"), done], {}],
  [["   Compiling shell v0.1.0", bin("bossylights", "/t/b")], { bossylights: "/t/b" }],
  [[JSON.stringify({ reason: "compiler-message", message: {} }), done], {}],
];
for (const [lines, expected] of cases) {
  test(`programs of ${JSON.stringify(lines)}`, () => assert.deepEqual(executables(lines.join("\n")), expected));
}
