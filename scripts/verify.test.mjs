import assert from "node:assert/strict";
import { test } from "node:test";
import { firstFailure, leftovers, logPath, mutantsReport, mutantsSummary, parseArgs, recordOf, recordPath, suitesOf, testSummary } from "./verify.mjs";

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
const run = { commit: "c".repeat(40), tree: "f".repeat(40), base: "b".repeat(40), results: allPass, mutants: { ok: true, counts: counts(0) } };
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
      assert.deepEqual(made, { record: { tree: run.tree, commit: run.commit, base: run.base, suites: ["test:checks", "test:rust"], mutants: (differs.mutants ?? run.mutants).counts }, why: null });
    }
  });
}

test("a record lives in the shared git folder, named by its tree", () => {
  assert.equal(recordPath("/p/bossylights/.git", "f".repeat(40)), `/p/bossylights/.git/bossylights-verified/${"f".repeat(40)}.json`);
});
