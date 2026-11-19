import assert from "node:assert/strict";
import { test } from "node:test";
import { firstFailure, mutantsSummary, parseArgs, suitesOf, testSummary } from "./verify.mjs";

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

// [output, first failure]
const failureCases = [
  ["ok\nerror[E0308]: mismatched types\nmore", "error[E0308]: mismatched types"],
  ["running\ntest foo ... FAILED\nend", "test foo ... FAILED"],
  ["not ok 1 - thing", "not ok 1 - thing"],
  ["just\nthe tail", "the tail"],
  ["", "no output"],
];
for (const [output, expected] of failureCases) {
  test(`first failure of ${JSON.stringify(output)}`, () => assert.equal(firstFailure(output), expected));
}

test("a passing run is one line per suite", () => {
  const text = testSummary("abc", [{ suite: "a", ok: true, output: "" }, { suite: "b", ok: true, output: "" }]);
  assert.equal(text, "verify abc: all suites passed\npass  a\npass  b");
});

test("a failing run names the first failure and stays within 10 lines", () => {
  const results = ["a", "b", "c", "d", "e"].map((suite, i) => ({ suite, ok: i !== 1 && i !== 3, output: `error: broke ${suite}` }));
  const text = testSummary("abc", results);
  assert.match(text, /^verify abc: FAILED/);
  assert.match(text, /first failure \(b\): error: broke b$/);
  assert.ok(text.split("\n").length <= 10);
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
