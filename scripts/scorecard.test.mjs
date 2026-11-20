import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { test } from "node:test";
import { codeLines, crateSource, crateSurface, escapeHatches, measurement, mutationReport, publicItems, scorecard, withoutTests } from "./scorecard.mjs";

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
| At the limits: 16 outputs of 64 universes in 30 s (60 and 44 a second in turn), worst p99 | 700 µs, worst 1100 µs, 0 off by > 2 ms | flat |
| Send to frame in 10 s, p95 (under one frame + 5 ms) | 21700 µs | flat |

| Crate | Kind | Public items | Lines | Lines per public item |
|---|---|---|---|---|
| core | core | 3 | 30 | 10 |
| app | action | 0 | 70 | - |
`;
  const limits = { p99: 700, worst: 1100, offBeat: 0 };
  assert.equal(scorecard(crates, hatches, { p99: 640, worst: 900, offBeat: 0 }, 21700, limits), expected);
  assert.match(scorecard(crates, hatches, undefined, undefined), /Lateness per output in 60 s [^|]*\| not measured \|/);
  assert.match(scorecard(crates, hatches, undefined, undefined), /Send to frame in 10 s, p95 [^|]*\| not measured \|/);
  const onCi = scorecard(crates, hatches, undefined, undefined, undefined, "not measured on CI");
  assert.match(onCi, /Lateness per output in 60 s [^|]*\| not measured on CI \|/);
  assert.match(onCi, /Send to frame in 10 s, p95 [^|]*\| not measured on CI \|/);
});

// [the run at the limits, or none, what CI sets as missing, the row's cell]
const limitsCases = [
  [{ p99: 700, worst: 1100, offBeat: 0 }, undefined, "700 µs, worst 1100 µs, 0 off by > 2 ms"],
  [{ p99: 1600, worst: 2300, offBeat: 32 }, "not measured on CI", "1600 µs, worst 2300 µs, 32 off by > 2 ms"],
  [undefined, undefined, "not measured"],
  [undefined, "not measured on CI", "not measured on CI"],
];
for (const [limits, missing, cell] of limitsCases) {
  test(`the run at the limits reads ${cell}`, () => {
    const row = `| At the limits: 16 outputs of 64 universes in 30 s (60 and 44 a second in turn), worst p99 | ${cell} | flat |\n`;
    assert.ok(scorecard([], { allow: 0, expect: 0, lintDisable: 0, unsafe: 0 }, undefined, undefined, limits, missing).includes(row));
  });
}

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

// [files, public items]: a private module counts only through the root's `pub use`
const surfaceCases = [
  [[{ path: "lib.rs", source: "mod mix;\npub use mix::{A, B};" }, { path: "mix.rs", source: "pub enum A {}\npub fn B() {}\npub fn c() {}" }], 2],
  [[{ path: "lib.rs", source: "pub mod open;" }, { path: "open.rs", source: "pub fn a() {}" }], 2],
  [[{ path: "lib.rs", source: "mod deep;" }, { path: "deep/part.rs", source: "pub fn a() {}" }], 0],
];
for (const [files, expected] of surfaceCases) {
  test(`surface of ${files.length} files`, () => assert.equal(crateSurface(files), expected));
}

test("the performance scorecard keeps each output, rejects missing samples and uses strict timing targets", async () => {
  const { performanceRows, performanceTable, missedReferenceTarget } = await import("./scorecard.mjs");
  const run = (output, hz, p99, p95, drops = 0) => ({ output, hz, p99, p95, drops });
  const result = (outputs, pressSamples = 267) => JSON.stringify({ workload: "m3", seconds: 60, events: 400, pressSamples, outputs });
  const good = [run(1, 44, 999999, 27727271), run(2, 60, 999999, 21666665, 3)];
  const limits = good.map((row, i) => ({ ...row, p99: i ? 999999 : 1000000, p95: i ? 21666666 : 0 }));
  for (const [output, count, expected] of [
    [good, 2, [true, true]], [limits, 2, [false, false]], [good.slice(0, 1), 2, null],
    [[good[0], good[0]], 2, null], [[good[0], { ...good[1], p95: null }], 2, null],
    [[good[0], { ...good[1], hz: 44 }], 2, null],
  ]) {
    if (expected === null) assert.throws(() => performanceRows(result(output), count, 60, "m3"));
    else assert.deepEqual(performanceRows(result(output), count, 60, "m3").outputs.map((row) => row.pass), expected);
  }
  const rows = performanceRows(result(good), 2, 60, "m3");
  assert.match(performanceTable([["M3", rows]]), /M3 \| 2 \| 60 \| 999.999 \| 21666.665 \| 3 \| pass/);
  assert.throws(() => performanceRows(result(good), 2, 5, "m3"));
  assert.throws(() => performanceTable([["M2", rows], ["M3", { ...rows, events: 399 }]]));
  assert.throws(() => performanceTable([["M2", rows], ["M3", { ...rows, pressSamples: 266 }]]));
  for (const missing of [0, 401, null]) {
    assert.throws(() => performanceRows(result(good, missing), 2, 60, "m3"));
  }
  assert.throws(() => performanceRows(JSON.stringify({ workload: "m3", seconds: 60, events: 0, outputs: good }), 2, 60, "m3"), /positive event count/);
  assert.match(performanceTable([]), /Not measured on CI/);
  const smoke = performanceTable([["M3 at limits", { ...rows, seconds: 5 }]]);
  assert.match(smoke, /Each run took 5 s/);
  assert.match(smoke, /Runs of 60 s or longer check the caps/);
  assert.match(smoke, /267 Manual presses and releases per output/);
  assert.match(smoke, /10,922 RGB pixels, 32 roots, 65,536 reserved visits and 16 outputs/);
  const missed = performanceRows(result(limits), 2, 60, "m3");
  for (const [runs, fails] of [
    [[], false],
    [[["M2", rows], ["M3", rows]], false],
    [[["M2", rows], ["M3", missed]], true],
    [[["M3 at limits", { ...missed, seconds: 5 }]], false],
  ]) assert.equal(missedReferenceTarget(runs), fails);
});

test("a rig refuses a measurement from a different or unnamed workload", async () => {
  const { performanceRows } = await import("./scorecard.mjs");
  const outputs = [44, 60].map((hz, i) => ({ hz, output: i + 1, p99: 1, p95: 1, drops: 0 }));
  for (const workload of ["m2", undefined]) {
    const json = JSON.stringify({ workload, outputs, seconds: 60, events: 5, pressSamples: 4 });
    assert.throws(() => performanceRows(json, 2, 60, "m3"), /expected workload m3/);
  }
});

test("a burst row requires complete batches of raw channel sends and plan swaps", async () => {
  const { performanceRows } = await import("./scorecard.mjs");
  const outputs = Array.from({ length: 16 }, (_, i) => ({ output: i + 1, hz: i % 2 === 0 ? 44 : 60, p99: 1, p95: 1, drops: 0 }));
  for (const [bursts, pressSamples, events, error] of [
    [undefined, 452, 454, /measured burst count/],
    [0, 452, 454, /measured burst count/],
    [0.5, 113, 114, /measured burst count/],
    [2, 451, 453, /226 raw channel sends/],
    [2, 452, 453, /one plan swap batch/],
  ]) {
    const json = JSON.stringify({ workload: "burst", seconds: 60, bursts, pressSamples, events, outputs });
    assert.throws(() => performanceRows(json, 16, 60, "burst"), error);
  }
});

test("the scorecard command labels each workload and gates runs of at least 60 seconds", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "scorecard-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  // Feed the command recorded numbers in place of starting a real-time measurement on CI.
  writeFileSync(join(dir, "cargo"), `#!/bin/sh
if [ "$1" = metadata ]; then echo '{"packages":[]}'; exit 0; fi
while [ "$#" -gt 0 ] && [ "$1" != -- ]; do shift; done
case "$2" in
  normal|limits) echo '1 1 0' ;;
  baseline) echo 1 ;;
  *) cat "$SCORECARD_FIXTURES/$2.json" ;;
esac
`, { mode: 0o755 });
  const env = { ...process.env, SCORECARD_FIXTURES: dir, PATH: dir + delimiter + process.env.PATH };
  delete env.CI;
  delete env.GITHUB_STEP_SUMMARY;
  for (const [seconds, missed, expectedExit] of [[59, true, 0], [60, false, 0], [60, true, 1], [120, true, 1]]) {
    for (const [workload, count, ns] of [["m2", 2, 1000], ["m3", 2, 2000], ["worst", 16, 3000], ["burst", 16, 4000]]) {
      const outputs = Array.from({ length: count }, (_, i) => ({
        output: i + 1, hz: i % 2 === 0 ? 44 : 60, p99: ns, p95: 1, drops: 0,
      }));
      const burst = workload === "burst";
      if (burst && missed) outputs[1].p99 = 1_000_000;
      const events = burst ? 454 : 2;
      const pressSamples = burst ? 452 : 1;
      writeFileSync(join(dir, workload + ".json"), JSON.stringify({ workload, seconds, events, pressSamples, outputs, bursts: burst ? 2 : undefined }));
    }
    const { stdout, stderr, status } = spawnSync(process.execPath, ["scripts/print-scorecard.mjs", "--seconds", String(seconds)], {
      encoding: "utf8", env,
    });
    assert.equal(status, expectedExit, `${seconds} s, missed=${missed}: ${stderr}`);
    for (const row of ["| M2 | 1 | 44 | 1 |", "| M3 | 1 | 44 | 2 |", "| M3 at limits | 1 | 44 | 3 |"]) {
      assert.ok(stdout.includes(row), row);
    }
    assert.ok(stdout.includes("| M3 at limits (burst) | 1 | 44 | 4 |"));
    assert.ok(stdout.includes(`| M3 at limits (burst) | 2 | 60 | ${missed ? 1000 : 4} | 0.001 | 0 | ${missed ? "fail" : "pass"} |`));
    assert.match(stdout, /2 bursts of 256 FIFO commands/);
    assert.match(stdout, /452 raw channel sends per output/);
    assert.match(stdout, /first accepted handoff/);
    assert.equal(stdout.includes("Short runs"), seconds < 60);
  }
});
