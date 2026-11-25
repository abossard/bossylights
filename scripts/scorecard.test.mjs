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
| Send to frame in 10 s, p95 | 21700 µs | flat |

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

test("the performance scorecard keeps each output and rejects missing samples, with no time limit", async () => {
  const { performanceRows, performanceTable } = await import("./scorecard.mjs");
  const run = (output, hz, p99, p95, drops = 0) => ({ output, hz, p99, p95, drops });
  const result = (outputs, pressSamples = 267) => JSON.stringify({ workload: "show", seconds: 60, events: 400, pressSamples, outputs });
  const good = [run(1, 44, 999999, 27727271), run(2, 60, 999999, 21666665, 3)];
  // Late by the old limits (1 ms, a period + 5 ms): kept as measured, judged by nobody.
  const late = good.map((row) => ({ ...row, p99: 2_000_000, p95: 30_000_000 }));
  for (const [output, count, expected] of [
    [good, 2, good], [late, 2, late], [good.slice(0, 1), 2, null],
    [[good[0], good[0]], 2, null], [[good[0], { ...good[1], p95: null }], 2, null],
    [[good[0], { ...good[1], hz: 44 }], 2, null],
  ]) {
    if (expected === null) assert.throws(() => performanceRows(result(output), count, 60, "show"));
    else assert.deepEqual(performanceRows(result(output), count, 60, "show").outputs, expected);
  }
  const rows = performanceRows(result(good), 2, 60, "show");
  const table = performanceTable([["normal show", rows], ["biggest show", performanceRows(result(late), 2, 60, "show")]]);
  assert.match(table, /^\| normal show \| 2 \| 60 \| 999.999 \| 21666.665 \| 3 \|$/m);
  assert.match(table, /^\| biggest show \| 1 \| 44 \| 2000 \| 30000 \| 0 \|$/m);
  assert.doesNotMatch(table, /Target|pass|fail|caps/);
  assert.throws(() => performanceRows(result(good), 2, 5, "show"));
  assert.throws(() => performanceTable([["normal show", rows], ["biggest show", { ...rows, events: 399 }]]));
  assert.throws(() => performanceTable([["normal show", rows], ["biggest show", { ...rows, pressSamples: 266 }]]));
  for (const missing of [0, 401, null]) {
    assert.throws(() => performanceRows(result(good, missing), 2, 60, "show"));
  }
  assert.throws(() => performanceRows(JSON.stringify({ workload: "show", seconds: 60, events: 0, outputs: good }), 2, 60, "show"), /positive event count/);
  assert.match(performanceTable([]), /Not measured on CI/);
  const smoke = performanceTable([["biggest show", { ...rows, seconds: 5 }]]);
  assert.match(smoke, /Each run took 5 s/);
  assert.match(smoke, /267 Manual presses and releases per output/);
  assert.match(smoke, /10,922 RGB pixels, 32 roots, 65,536 reserved visits and 16 outputs/);
});

// A run of send_to_frame as the load tables read it: its outputs, its press lines, its load.
const loadRun = ({ workload = "idle", outputs = [44, 60], press, cpu = { tick: 1_600_000_000, outputs: 300_000_000, engine: 2_000_000_000 }, wakes = { tick: 1500, outputs: 25_000 }, load } = {}) => JSON.stringify({
  workload, seconds: 15, events: 60, pressSamples: 60,
  outputs: outputs.map((hz, i) => ({ output: i + 1, hz, p99: 100_000 * (i + 1), p95: 1, drops: 0 })),
  press: press ?? [{ hz: 44, samples: 92, p50: 11_503_709, p99: 22_575_458, worst: 22_575_458 }, { hz: 60, samples: 92, p50: 7_988_708, p99: 16_545_625, worst: 16_545_625 }],
  load: load ?? { wall: 15_000_000_000, cpu, wakes },
});

// [what the run says, a line the tables print]
const loadCases = [
  [{}, "| idle | 44 | 92 | 11.5 | 11.4 | 22.6 | 22.6 | 22.7 |"],
  [{}, "| idle | 60 | 92 | 8.0 | 8.3 | 16.5 | 16.5 | 16.7 |"],
  [{}, "| idle | 2 | 200 | 10.7 | 2.0 | 13.3 | 100 | 104 | 1,667 | 1,664 |"],
  [{ cpu: null }, "| idle | 2 | 200 | not measured | not measured | not measured | 100 | 104 | 1,667 | 1,664 |"],
  [{ workload: "biggest", outputs: Array.from({ length: 16 }, (_, i) => (i % 2 ? 60 : 44)) }, "| full load | 16 | 1,600 | 10.7 | 2.0 | 13.3 | 100 | 104 | 1,667 | 13,312 |"],
  [{ outputs: [44, 44], press: [{ hz: 44, samples: 184, p50: 1, p99: 1, worst: 1 }] }, "| idle | 2 | 200 | 10.7 | 2.0 | 13.3 | 100 | 44 | 1,667 | 1,408 |"],
];
for (const [run, line] of loadCases) {
  test(`the load tables print ${line}`, async () => {
    const { loadRow, loadTables } = await import("./scorecard.mjs");
    const workload = run.workload ?? "idle";
    const name = workload === "idle" ? "idle" : "full load";
    const tables = loadTables([[name, loadRow(loadRun(run), workload)]]);
    assert.ok(tables.split("\n").includes(line), tables);
  });
}

test("the load tables say what the press to frame leaves out, and are not measured on CI", async () => {
  const { loadRow, loadTables } = await import("./scorecard.mjs");
  const tables = loadTables([["idle", loadRow(loadRun(), "idle")]]);
  for (const words of [/screen to the engine/, /after the handoff/, /23 ms/, /the product does not/, /The worst is near a period, plus the frame's work and its handoff/]) assert.match(tables, words);
  assert.ok(tables.split("\n").includes("| Load | Hz | Presses | Median (ms) | Expected median, half a period (ms) | p99 (ms) | Worst (ms) | Expected worst, a period (ms) |"), tables);
  assert.match(loadTables([]), /## Press to frame and load\n\nNot measured on CI/);
});

// [what is wrong with the run, the error]
const brokenLoads = [
  [{ workload: "show" }, /expected workload idle/],
  [{ press: [] }, /a press line for each rate/],
  [{ press: [{ hz: 44, samples: 0, p50: 1, p99: 1, worst: 1 }, { hz: 60, samples: 1, p50: 1, p99: 1, worst: 1 }] }, /press line at 44/],
  [{ press: [{ hz: 44, samples: 1, p50: 1, p99: 1, worst: 1 }, { hz: 60, samples: 1, p50: -1, p99: 1, worst: 1 }] }, /press line at 60/],
  [{ cpu: { tick: 1, outputs: 1 } }, /CPU of the tick, the output threads and the engine/],
  [{ wakes: { tick: 1 } }, /wakes of the tick and the output threads/],
  [{ load: { wall: 15_000_000_000, wakes: { tick: 1, outputs: 1 } } }, /CPU of the tick, the output threads and the engine, or null/],
  [{ press: [30, 60].map((hz) => ({ hz, samples: 1, p50: 1, p99: 1, worst: 1 })) }, /a press line for each rate/],
  [{ load: { wall: 0, cpu: null, wakes: { tick: 1, outputs: 1 } } }, /wall time/],
];
for (const [run, error] of brokenLoads) {
  test(`a load run is refused: ${error.source}`, async () => {
    const { loadRow } = await import("./scorecard.mjs");
    assert.throws(() => loadRow(loadRun(run), "idle"), error);
  });
}

test("a rig refuses a measurement from a different or unnamed workload", async () => {
  const { performanceRows } = await import("./scorecard.mjs");
  const outputs = [44, 60].map((hz, i) => ({ hz, output: i + 1, p99: 1, p95: 1, drops: 0 }));
  for (const workload of ["biggest", undefined]) {
    const json = JSON.stringify({ workload, outputs, seconds: 60, events: 5, pressSamples: 4 });
    assert.throws(() => performanceRows(json, 2, 60, "show"), /expected workload show/);
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

test("the scorecard command prints the normal show, the biggest show, the burst and the load at full load, the example rig and idle, and fails only on an incomplete measurement", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "scorecard-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  // Feed the command recorded numbers in place of starting a real-time measurement on CI.
  // A workload without a recorded file prints nothing, so an extra rig fails the command.
  writeFileSync(join(dir, "cargo"), `#!/bin/sh
if [ "$1" = metadata ]; then echo '{"packages":[]}'; exit 0; fi
while [ "$#" -gt 0 ] && [ "$1" != -- ]; do shift; done
case "$2" in
  normal|limits) cat "$SCORECARD_FIXTURES/lateness" ;;
  baseline) cat "$SCORECARD_FIXTURES/baseline" ;;
  *) cat "$SCORECARD_FIXTURES/$2.json" ;;
esac
`, { mode: 0o755 });
  const env = { ...process.env, SCORECARD_FIXTURES: dir, PATH: dir + delimiter + process.env.PATH };
  delete env.CI;
  delete env.GITHUB_STEP_SUMMARY;
  const rigRow = /^\| ([^|]+) \| \d+ \| (?:44|60) \|/;
  // [seconds, the burst's output 2: as measured, 2 ms late or missing a number, every time over the old limits, exit]
  for (const [seconds, second, late, expectedExit] of [
    [59, { p99: 2_000_000 }, false, 0], [60, {}, false, 0], [60, { p99: 2_000_000 }, false, 0], [120, { p99: 2_000_000 }, false, 0],
    [60, {}, true, 0], [60, { p95: null }, false, 1],
  ]) {
    // Late: lateness 1 to 4 ms (the old limit was 1 ms), send to handoff p95 and press worst 30 ms (a period + 5 ms is 27.7 ms at most).
    const scale = late ? 1000 : 1;
    const p95 = late ? 30_000_000 : 1;
    writeFileSync(join(dir, "lateness"), late ? "2000 2000 0" : "1 1 0");
    writeFileSync(join(dir, "baseline"), late ? "30000" : "1");
    for (const [workload, count, base] of [["show", 2, 2000], ["biggest", 16, 3000], ["burst", 16, 4000]]) {
      const ns = base * scale;
      const outputs = Array.from({ length: count }, (_, i) => ({
        output: i + 1, hz: i % 2 === 0 ? 44 : 60, p99: ns, p95, drops: 0,
      }));
      const burst = workload === "burst";
      if (burst) Object.assign(outputs[1], second);
      const events = burst ? 454 : 2;
      const pressSamples = burst ? 452 : 1;
      const press = [44, 60].map((hz) => ({ hz, samples: 1, p50: ns, p99: ns, worst: late ? 30_000_000 : ns }));
      const load = { wall: 1e9, cpu: { tick: ns, outputs: ns, engine: ns }, wakes: { tick: 100, outputs: 1600 } };
      writeFileSync(join(dir, workload + ".json"), JSON.stringify({ workload, seconds, events, pressSamples, outputs, press, load, bursts: burst ? 2 : undefined }));
    }
    const idleNs = 1000 * scale;
    const idle = { workload: "idle", seconds: Math.min(seconds, 15), events: 1, pressSamples: 1, outputs: [44, 60].map((hz, i) => ({ output: i + 1, hz, p99: idleNs, p95, drops: 0 })) };
    writeFileSync(join(dir, "idle.json"), JSON.stringify({ ...idle, press: [44, 60].map((hz) => ({ hz, samples: 1, p50: idleNs, p99: idleNs, worst: late ? 30_000_000 : idleNs })), load: { wall: 1e9, cpu: null, wakes: { tick: 100, outputs: 1600 } } }));
    const { stdout, stderr, status } = spawnSync(process.execPath, ["scripts/print-scorecard.mjs", "--seconds", String(seconds)], {
      encoding: "utf8", env,
    });
    assert.equal(status, expectedExit, `${seconds} s, ${JSON.stringify(second)}, late=${late}: ${stderr}`);
    if (expectedExit) {
      assert.match(stderr, /incomplete measurement for output 2/);
      continue;
    }
    const rigs = new Set(stdout.split("\n").map((line) => rigRow.exec(line)?.[1]).filter(Boolean));
    assert.deepEqual([...rigs], ["normal show", "biggest show", "biggest show plus burst"]);
    // The machine line names the CPU, and "Apple M2 Pro" is a machine, not a milestone.
    const machineLine = /^Measured on .*$/m;
    assert.match(stdout, machineLine);
    assert.doesNotMatch(stdout.replace(machineLine, ""), /\bM[23]\b|earlier/);
    const thousands = (n) => (n * scale).toLocaleString("en-US");
    if (late) {
      assert.ok(stdout.includes("| Lateness per output in 60 s (60 and 44 a second), worst p99 | 2000 µs, worst 2000 µs"), stdout);
      assert.ok(stdout.includes("| Send to frame in 60 s, p95 | 30000 µs |"), stdout);
    }
    for (const row of [`| normal show | 1 | 44 | ${2 * scale} |`, `| biggest show | 1 | 44 | ${3 * scale} |`, `| biggest show plus burst | 1 | 44 | ${4 * scale} |`]) {
      assert.ok(stdout.includes(row), row);
    }
    assert.ok(stdout.includes(`| biggest show plus burst | 2 | 60 | ${(second.p99 ?? 4000 * scale) / 1000} | ${p95 / 1000} | 0 |\n`));
    assert.match(stdout, /2 bursts of 256 FIFO commands/);
    assert.match(stdout, /452 raw channel sends per output/);
    assert.match(stdout, /first accepted handoff/);
    assert.doesNotMatch(stdout, /Target|pass|fail|caps/);
    for (const row of ["| full load | 44 |", "| example rig | 60 |", "| idle | 44 |", `| full load | 16 | ${thousands(3)} |`, `| example rig | 2 | ${thousands(2)} |`, `| idle | 2 | ${thousands(1)} | not measured |`]) {
      assert.ok(stdout.includes(row), row);
    }
  }
});
