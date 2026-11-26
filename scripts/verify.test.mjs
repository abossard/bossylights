import assert from "node:assert/strict";
import { test } from "node:test";
import { firstFailure, leftovers, logPath, mutantsReport, mutantsSummary, parseArgs, reachOf, readsOf, recordOf, recordPath, rustCommands, stepsOf, suitesOf, testSummary } from "./verify.mjs";

test("suites are the npm run parts of the test script", () => {
  assert.deepEqual(suitesOf("npm run test:checks && npm run test:rust && npm run test:ui"), ["test:checks", "test:rust", "test:ui"]);
});

// [argv, parsed or null]
const argCases = [
  [["abc123"], { commit: "abc123", mutants: false }],
  [["abc123", "--mutants"], { commit: "abc123", mutants: true }],
  [["--mutants", "abc123"], { commit: "abc123", mutants: true }],
  [[], null],
  [["a", "b"], null],
  [["--mutants"], null],
  [["--bogus"], null],
];
for (const [argv, parsed] of argCases) {
  test(`args ${JSON.stringify(argv)}`, () => {
    if (parsed) assert.deepEqual(parseArgs(argv), parsed);
    else assert.throws(() => parseArgs(argv), /usage/);
  });
}

// [output, exit status, first failure]
const failureCases = [
  ["ok\nerror[E0308]: mismatched types\nmore", 1, "error[E0308]: mismatched types"],
  ["running\ntest foo ... FAILED\nend", 101, "test foo ... FAILED"],
  ["not ok 1 - thing", 1, "not ok 1 - thing"],
  ["test result: ok. 3 passed; 0 failed\nerror: process didn't exit successfully", 1, "error: process didn't exit successfully"],
  ["test result: ok. 3 passed; 0 failed", 1, "exit 1, see L.log"],
  ["thread 'main' panicked at src/lib.rs:3:5:\nboom", 101, "thread 'main' panicked at src/lib.rs:3:5:"],
  ["running\nsignal: 11 (SIGSEGV)", 1, "signal: 11 (SIGSEGV)"],
  ["test result: FAILED. 1 passed; 2 failed", 101, "test result: FAILED. 1 passed; 2 failed"],
  ["just\nthe tail", 0, "the tail"],
  ["", 0, "no output"],
  ["", 2, "exit 2, see L.log"],
];
for (const [output, status, expected] of failureCases) {
  test(`first failure of ${JSON.stringify(output)} exit ${status}`, () => assert.equal(firstFailure(output, status, "L.log"), expected));
}

// [suite, log file]
const logCases = [["test:rust", "/d/test-rust.log"], ["test checks", "/d/test-checks.log"], ["lint", "/d/lint.log"]];
for (const [suite, expected] of logCases) {
  test(`log of ${suite}`, () => assert.equal(logPath("/d", suite), expected));
}

test("a passing run is one line per suite with its log", () => {
  const text = testSummary("abc", [{ suite: "a", ok: true, log: "/d/a.log", output: "" }, { suite: "b", ok: true, log: "/d/b.log", output: "" }]);
  assert.equal(text, "verify abc: all suites passed\npass  a  /d/a.log\npass  b  /d/b.log");
});

test("a failing run names the first failure and stays within 10 lines", () => {
  const results = ["a", "b", "c", "d", "e"].map((suite, i) => ({ suite, ok: i !== 1 && i !== 3, status: 1, log: `/d/${suite}.log`, output: `error: broke ${suite}` }));
  const text = testSummary("abc", results);
  assert.match(text, /^verify abc: FAILED/);
  assert.match(text, /FAIL  b  \/d\/b\.log/);
  assert.match(text, /first failure \(b\): error: broke b$/);
  assert.ok(text.split("\n").length <= 10);
});

test("a failure with no matching line points at the log", () => {
  const text = testSummary("abc", [{ suite: "a", ok: false, status: 3, log: "/d/a.log", output: "fine\n" }]);
  assert.match(text, /first failure \(a\): exit 3, see \/d\/a\.log$/);
});

// [ps line, what is left over]: only a program started from the copy's own folder.
const copy = "/private/var/T/bossylights-verify-Co2KSx";
const leftoverCases = [
  [`1129 ${copy}/target/debug/bossylights`, [{ pid: 1129, args: `${copy}/target/debug/bossylights` }]],
  [`  9171 ${copy}/target/debug/bossylights-engine --listen 127.0.0.1:65313`, [{ pid: 9171, args: `${copy}/target/debug/bossylights-engine --listen 127.0.0.1:65313` }]],
  [`9143 node ${copy}/node_modules/.bin/playwright test`, []],
  ["9172 target/debug/bossylights-engine --listen 127.0.0.1:55127", []],
  ["1130 /private/var/T/bossylights-verify-OrmJmd/target/debug/bossylights", []],
  [`1131 ${copy}-other/target/debug/bossylights`, []],
  ["", []],
];
for (const [line, expected] of leftoverCases) {
  test(`left over: ${JSON.stringify(line)}`, () => assert.deepEqual(leftovers(`  PID ARGS\n${line}\n`, copy), expected));
}

test("left over: a program that ran before the step is the worker's own, never stopped", () => {
  const ps = `1129 ${copy}/target/debug/bossylights-engine\n1130 ${copy}/target/debug/bossylights\n`;
  assert.deepEqual(leftovers(ps, copy, new Set([1129])), [{ pid: 1130, args: `${copy}/target/debug/bossylights` }]);
});

const mutant = (file, line, name, summary) => ({ summary, scenario: { Mutant: { file, name, span: { start: { line } } } } });
test("mutants list every missed and timeout line, never the caught ones", () => {
  const text = mutantsSummary({
    caught: 1, missed: 1, timeout: 1, unviable: 0,
    outcomes: [
      mutant("a.rs", 3, "a.rs:3:5: replace f with 0", "CaughtMutant"),
      mutant("b.rs", 7, "b.rs:7:9: replace g -> u8 with 1", "MissedMutant"),
      mutant("c.rs", 9, "c.rs:9:1: replace h with ()", "Timeout"),
    ],
  });
  assert.equal(text, [
    "mutants: caught 1, missed 1, timeout 1, unviable 0",
    "  missed  b.rs:7  replace g -> u8 with 1",
    "  timeout c.rs:9  replace h with ()",
  ].join("\n"));
});

test("no outcomes file: exit 0 is no mutants, any other exit is a failure line", () => {
  const none = mutantsReport({ status: 0, outcomes: null, stderr: "" });
  assert.deepEqual(none, { ok: true, text: "mutants: none in this diff", counts: { caught: 0, missed: 0, timeout: 0, unviable: 0 } });
  const bad = mutantsReport({ status: 1, outcomes: null, stderr: "a\nboom\n" });
  assert.equal(bad.ok, false);
  assert.match(bad.text, /no outcomes file \(cargo mutants exit 1\): boom/);
  assert.equal(bad.counts, null);
});

const counts = (missed, timeout = 0) => ({ caught: 4, missed, timeout, unviable: 1 });
const allPass = [{ suite: "test:checks", ok: true }, { suite: "test:rust", ok: true }];
const run = { commit: "c".repeat(40), tree: "f".repeat(40), base: "b".repeat(40), results: allPass, left: ["test:scorecard"], mutants: { ok: true, counts: counts(0) } };
// [name, what differs from a passed run with mutants, why there is no record, or null when it records]
const recordCases = [
  ["every suite passed, mutants none missed", {}, null],
  ["mutants timed out, none missed", { mutants: { ok: true, counts: counts(0, 2) } }, null],
  ["a red run never records", { results: [allPass[0], { suite: "test:rust", ok: false }] }, /a suite failed/],
  ["no suite ran", { results: [] }, /a suite failed/],
  ["mutants missed never records", { mutants: { ok: true, counts: counts(1) } }, /1 mutant\(s\) missed: kill each, or pin it/],
  ["mutants missed and timed out never records", { mutants: { ok: true, counts: counts(1, 2) } }, /1 mutant\(s\) missed/],
  ["mutants not asked", { mutants: null }, /verify with --mutants/],
  ["mutants did not run through", { mutants: { ok: false, counts: null } }, /did not run through/],
];
for (const [name, differs, why] of recordCases) {
  test(`record: ${name}`, () => {
    const made = recordOf({ ...run, ...differs });
    if (why) {
      assert.equal(made.record, null);
      assert.match(made.why, why);
    } else {
      assert.deepEqual(made, { record: { tree: run.tree, commit: run.commit, base: run.base, suites: ["test:checks", "test:rust"], left: ["test:scorecard"], mutants: (differs.mutants ?? run.mutants).counts }, why: null });
    }
  });
}

test("a record lives in the shared git folder, named by its tree", () => {
  assert.equal(recordPath("/p/bossylights/.git", "f".repeat(40)), `/p/bossylights/.git/bossylights-verified/${"f".repeat(40)}.json`);
});

// The workspace as cargo metadata gives it, dev dependencies included (a trimmed copy of the real one).
// In cargo's order: effects comes before rt-thread, which it tests with.
const workspace = [
  { name: "engine-core", dir: "crates/engine-core", deps: [], reads: [] },
  { name: "effects", dir: "crates/effects", deps: ["engine-slot", "rt-thread"], reads: [] },
  { name: "engine-slot", dir: "crates/engine-slot", deps: [], reads: [] },
  { name: "rt-thread", dir: "crates/rt-thread", deps: ["engine-core", "engine-slot"], reads: [] },
  { name: "fixture-library", dir: "crates/fixture-library", deps: [], reads: [] },
  { name: "rig", dir: "crates/rig", deps: ["engine-core", "fixture-library", "engine-slot", "rt-thread"], reads: ["crates/fixture-library/tests/definitions", "crates/outputs/tests/counting/mod.rs"] },
  { name: "wire", dir: "crates/wire", deps: ["engine-core", "fixture-library"], reads: ["ui/src/engine-api.ts"] },
  { name: "outputs", dir: "crates/outputs", deps: ["engine-core"], reads: [] },
  { name: "engine", dir: "crates/engine", deps: ["engine-core", "rig", "outputs"], reads: [] },
  { name: "audio-in", dir: "crates/audio-in", deps: [], reads: [] },
  { name: "engine-server", dir: "crates/engine-server", deps: ["engine", "wire"], reads: [] },
  { name: "shell", dir: "shell", deps: ["wire"], reads: [] },
];
const all = workspace.map((c) => c.name);
// [name, changed files, everything (pattern) or null, crates reached, UI suites]
const reachCases = [
  ["a leaf crate", ["crates/audio-in/src/lib.rs"], null, ["audio-in"], false],
  ["engine-core reaches its dependents", ["crates/engine-core/src/mix.rs"], null, ["engine-core", "effects", "rt-thread", "rig", "wire", "outputs", "engine", "engine-server", "shell"], true],
  ["a dev dependency reaches the crates that test with it", ["crates/engine-slot/src/lib.rs"], null, ["effects", "engine-slot", "rt-thread", "rig", "engine", "engine-server"], false],
  ["a crate's Cargo.toml is the crate", ["crates/rig/Cargo.toml"], null, ["rig", "engine", "engine-server"], false],
  ["docs only", ["docs/coding.md", "AGENTS.md", "mockups/a.html"], null, [], false],
  ["ui/ only", ["ui/src/picture.ts"], null, [], true],
  ["the generated types reach wire, which reads them", ["ui/src/engine-api.ts"], null, ["wire", "engine-server", "shell"], true],
  ["a file another crate's tests take in by #[path]", ["crates/outputs/tests/counting/mod.rs"], null, ["rig", "outputs", "engine", "engine-server"], false],
  ["a fixture another crate's tests read", ["crates/fixture-library/tests/definitions/A.qxf"], null, ["fixture-library", "rig", "wire", "engine", "engine-server", "shell"], true],
  ["a file beside a read one is not read", ["crates/outputs/tests/counting/other.rs"], null, ["outputs", "engine", "engine-server"], false],
  ["e2e/ only", ["e2e/press.spec.ts"], null, [], true],
  ["wire", ["crates/wire/src/lib.rs"], null, ["wire", "engine-server", "shell"], true],
  ["the shell", ["shell/src/main.rs"], null, ["shell"], true],
  ["Cargo.lock", ["Cargo.lock", "docs/a.md"], /^Cargo\.lock$/, all, true],
  ["the workspace Cargo.toml", ["Cargo.toml"], /^Cargo\.toml$/, all, true],
  ["the toolchain", ["rust-toolchain.toml"], /rust-toolchain/, all, true],
  ["cargo's config", [".cargo/mutants.toml"], /\.cargo/, all, true],
  ["a script", ["crates/audio-in/src/lib.rs", "scripts/verify.mjs"], /^scripts\/verify\.mjs$/, all, true],
  ["package.json", ["package.json"], /package\.json/, all, true],
  ["package-lock.json", ["package-lock.json"], /package-lock/, all, true],
  ["a CI file", [".github/workflows/ci.yml"], /\.github/, all, true],
  ["a file no row names", ["playwright.config.ts"], /playwright/, all, true],
  ["a folder named like a crate's prefix", ["crates/engine-corex/a.rs"], /engine-corex/, all, true],
];
for (const [name, files, everything, crates, ui] of reachCases) {
  test(`reach: ${name}`, () => {
    const reach = reachOf(files, workspace);
    if (everything) assert.match(reach.everything, everything);
    else assert.equal(reach.everything, null);
    assert.deepEqual(reach.crates, crates);
    assert.equal(reach.ui, ui);
  });
}

const rustScript = "cargo fmt --all --check && cargo check -p engine-core -p fixture-library -p wire && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace";
// [name, crates reached, cargo commands]
const rustCases = [
  ["no crate: no cargo", [], []],
  ["a leaf", ["audio-in"], [
    ["cargo", "fmt", "--all", "--check"],
    ["cargo", "clippy", "-p", "audio-in", "--all-targets", "--", "-D", "warnings"],
    ["cargo", "test", "-p", "audio-in"],
  ]],
  ["wire keeps its no_std check", ["wire", "shell"], [
    ["cargo", "fmt", "--all", "--check"],
    ["cargo", "check", "-p", "wire"],
    ["cargo", "clippy", "-p", "wire", "-p", "shell", "--all-targets", "--", "-D", "warnings"],
    ["cargo", "test", "-p", "wire", "-p", "shell"],
  ]],
];
for (const [name, crates, commands] of rustCases) {
  test(`rust commands: ${name}`, () => assert.deepEqual(rustCommands(rustScript, crates), commands));
}

const scripts = {
  test: "npm run test:checks && npm run test:rust && npm run test:release-shell && npm run test:ui && npm run test:e2e && npm run test:catalog && npm run test:scorecard",
  "test:rust": rustScript,
};
// [name, reach, step names, left to CI]
const stepCases = [
  ["everything: every suite, nothing left", { everything: "Cargo.lock", crates: all, ui: true }, suitesOf(scripts.test), []],
  ["docs only: test:checks", { everything: null, crates: [], ui: false }, ["test:checks"],
    [`test:rust for ${all.join(", ")}`, "test:release-shell", "test:ui", "test:e2e", "test:catalog", "test:scorecard"]],
  ["ui only: checks and the UI suites", { everything: null, crates: [], ui: true }, ["test:checks", "test:ui", "test:e2e", "test:catalog"],
    [`test:rust for ${all.join(", ")}`, "test:release-shell", "test:scorecard"]],
  ["a leaf crate", { everything: null, crates: ["audio-in"], ui: false }, ["test:checks", "test:rust: cargo fmt", "test:rust: cargo clippy", "test:rust: cargo test"],
    [`test:rust for ${all.filter((c) => c !== "audio-in").join(", ")}`, "test:release-shell", "test:ui", "test:e2e", "test:catalog", "test:scorecard"]],
];
for (const [name, reach, steps, left] of stepCases) {
  test(`steps: ${name}`, () => {
    const plan = stepsOf(reach, scripts, all);
    assert.deepEqual(plan.steps.map((s) => s.name), steps);
    assert.deepEqual(plan.left, left);
  });
}

// [name, crate folder, its sources, the paths outside it they name]
const readCases = [
  ["#[path] from the file's folder", "crates/rig", [{ path: "crates/rig/tests/no_alloc.rs", text: '#[path = "../../outputs/tests/counting/mod.rs"]' }],
    ["crates/outputs/tests/counting/mod.rs"]],
  ["CARGO_MANIFEST_DIR from the crate's folder", "crates/show-file", [{ path: "crates/show-file/tests/file.rs", text: 'Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixture-library/tests/definitions")' }],
    ["crates/fixture-library/tests/definitions"]],
  ["a path up to the repository root", "crates/wire", [{ path: "crates/wire/src/lib.rs", text: 'const TYPESCRIPT: &str = "../../ui/src/engine-api.ts";' }],
    ["ui/src/engine-api.ts"]],
  ["include_bytes! from the file's folder", "crates/engine", [{ path: "crates/engine/tests/patch.rs", text: 'include_bytes!("../../fixture-library/tests/definitions/A.qxf")' }],
    ["crates/fixture-library/tests/definitions/A.qxf"]],
  ["include! from the file's folder", "crates/engine", [{ path: "crates/engine/src/front/tests.rs", text: 'include!("../../../outputs/tests/common/stream.rs");' }],
    ["crates/outputs/tests/common/stream.rs"]],
  ["inside its own folder: not a read", "crates/engine-core", [{ path: "crates/engine-core/src/clock.rs", text: '#[path = "../tests/unit/clock.rs"]' }], []],
  ["above the repository: not a read", "shell", [{ path: "shell/src/main.rs", text: '"../../../x"' }], []],
  ["no ../ at all", "crates/rig", [{ path: "crates/rig/src/lib.rs", text: 'let a = "rig/x"; // ../not a literal' }], []],
];
for (const [name, dir, sources, reads] of readCases) {
  test(`reads: ${name}`, () => assert.deepEqual(readsOf(dir, sources), reads));
}
