import assert from "node:assert/strict";
import { test } from "node:test";
import { codeLines, crateSource, escapeHatches, measurement, mutationReport, publicItems, scorecard, withoutTests } from "./scorecard.mjs";

// [source, code lines]
const lineCases = [
  ["", 0],
  ["fn a() {}\n\n  \n", 1],
  ["//! doc\n/// doc\n// note\nlet x = 1; // trailing\n", 1],
];
for (const [source, expected] of lineCases) {
  test(`code lines of ${JSON.stringify(source)}`, () => assert.equal(codeLines(source), expected));
}

// [source, public items]
const publicCases = [
  ["pub fn a() {}\npub(crate) fn b() {}\nfn c() {}", 1],
  ["pub struct S {\n    pub field: u8,\n}\npub enum E {}\npub type T = u8;", 3],
  ["pub const N: u8 = 1;\npub const fn f() {}\npub use x::Y;\npub mod m;\npub trait Q {}", 5],
  ["impl S {\n    pub fn method(&self) {}\n    pub const RATE: u32 = 44;\n}", 2],
];
publicCases.push(
  ["pub use a::{x, y, z};", 3],
  ["pub use a::{\n    x,\n    y as w,\n    b::{c, d},\n};\npub use e::*;", 5],
  ["pub(crate) use a::{x, y};\nuse b::{c, d};", 0],
);
for (const [source, expected] of publicCases) {
  test(`public items of ${JSON.stringify(source)}`, () => assert.equal(publicItems(source), expected));
}

// [source, without its test module]
const testCases = [
  ["fn a() {}\n", "fn a() {}\n"],
  ["fn a() {}\n\n#[cfg(test)]\nmod tests {\n    pub fn t() {}\n}\n", "fn a() {}\n\n"],
];
for (const [source, expected] of testCases) {
  test(`without tests ${JSON.stringify(source)}`, () => assert.equal(withoutTests(source), expected));
}

// [source, escape hatches]
const hatchCases = [
  ["fn a() {}", { allow: 0, expect: 0, lintDisable: 0, unsafe: 0 }],
  ["#[allow(x, reason = \"\")]\n#![allow(y)]", { allow: 2, expect: 0, lintDisable: 0, unsafe: 0 }],
  ["#![expect(unsafe_code, reason = \"needs unsafe\")]", { allow: 0, expect: 1, lintDisable: 0, unsafe: 0 }],
  ["unsafe impl A for B {}\nunsafe fn f() { unsafe { g() } }", { allow: 0, expect: 0, lintDisable: 0, unsafe: 3 }],
  ["// eslint-disable-next-line\n/* oxlint-disable */", { allow: 0, expect: 0, lintDisable: 2, unsafe: 0 }],
  ["#![forbid(unsafe_code)]", { allow: 0, expect: 0, lintDisable: 0, unsafe: 0 }],
];
for (const [source, expected] of hatchCases) {
  test(`escape hatches in ${JSON.stringify(source)}`, () => assert.deepEqual(escapeHatches(source), expected));
}

test("scorecard renders the numbers as tables", () => {
  const crates = [
    { name: "core", kind: "core", lines: 30, publicItems: 3 },
    { name: "app", kind: "action", lines: 70, publicItems: 0 },
  ];
  const hatches = { allow: 0, expect: 1, lintDisable: 0, unsafe: 2 };
  const expected = `## Scorecard

| Number | Now | Wants to go |
|---|---|---|
| Lines in calculation crates | 30% (30 of 100) | up |
| Escape hatches: allow, expect, lint-disable, unsafe | 0, 1, 0, 2 | flat |
| Lateness per output in 60 s (60 and 44 a second), worst p99 | 640 µs, worst 900 µs, 0 off by > 2 ms | flat |
| Send to frame in 10 s, p95 (under one frame + 5 ms) | 21700 µs | flat |

| Crate | Kind | Public items | Lines | Lines per public item |
|---|---|---|---|---|
| core | core | 3 | 30 | 10 |
| app | action | 0 | 70 | - |
`;
  assert.equal(scorecard(crates, hatches, { p99: 640, worst: 900, offBeat: 0 }, 21700), expected);
  assert.match(scorecard(crates, hatches, undefined, undefined), /Lateness per output in 60 s [^|]*\| not measured \|/);
  assert.match(scorecard(crates, hatches, undefined, undefined), /Send to frame in 10 s, p95 [^|]*\| not measured \|/);
  const onCi = scorecard(crates, hatches, undefined, undefined, "not measured on CI");
  assert.match(onCi, /Lateness per output in 60 s [^|]*\| not measured on CI \|/);
  assert.match(onCi, /Send to frame in 10 s, p95 [^|]*\| not measured on CI \|/);
});

// [outcomes, score cell]
const mutationCases = [
  [{ caught: 9, missed: 1, timeout: 0, unviable: 4 }, "| 90% | 9 | 1 | 0 | 4 |"],
  [{ caught: 2, missed: 0, timeout: 1, unviable: 0 }, "| 67% | 2 | 0 | 1 | 0 |"],
  [{ caught: 0, missed: 0, timeout: 0, unviable: 3 }, "| - | 0 | 0 | 0 | 3 |"],
];
for (const [outcomes, row] of mutationCases) {
  test(`mutation report ${row}`, () => {
    assert.ok(mutationReport(["engine-core"], outcomes).includes(`${row}\n`));
  });
}

// [files of one crate, code lines left after cutting each file's test module]
const crateCases = [
  [["fn a() {}\n"], 1],
  [["fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n", "fn b() {}\nfn c() {}\n"], 3],
  [["fn a() {}\n", "fn b() {}\n#[cfg(test)]\nmod tests {}\n", "fn c() {}\n#[cfg(test)]\nmod tests {}\n"], 3],
];
for (const [files, lines] of crateCases) {
  test(`crate source of ${files.length} files has ${lines} code lines`, () => assert.equal(codeLines(crateSource(files)), lines));
}

// [output of a measuring example, expected numbers or error]
const measurementCases = [
  ["640 900 0\n", 3, [640, 900, 0]],
  ["21700\n", 1, [21700]],
  ["", 1, "throws"],
  ["abc 1 2", 3, "throws"],
  ["1 2", 3, "throws"],
];
for (const [out, count, expected] of measurementCases) {
  test(`measurement of ${JSON.stringify(out)}`, () => {
    if (expected === "throws") assert.throws(() => measurement(out, count));
    else assert.deepEqual(measurement(out, count), expected);
  });
}
