import assert from "node:assert/strict";
import { test } from "node:test";
import { codeLines, escapeHatches, publicItems, scorecard, withoutTests } from "./scorecard.mjs";

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
| Tick lateness, p99 | 640 µs | flat |

| Crate | Kind | Public items | Lines | Lines per public item |
|---|---|---|---|---|
| core | core | 3 | 30 | 10 |
| app | action | 0 | 70 | - |
`;
  assert.equal(scorecard(crates, hatches, 640), expected);
  assert.match(scorecard(crates, hatches, undefined), /Tick lateness, p99 \| not measured \|/);
});
