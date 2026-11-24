import assert from "node:assert/strict";
import { test } from "node:test";
import {
  KEEP,
  crateOf,
  failedTests,
  isProductFile,
  markTests,
  matchFailure,
  mutantKills,
  multiplicity,
  rangeKeys,
  reachesSpan,
  removable,
  rustReach,
  stableTests,
  testCode,
  testCodeAt,
  uniques,
  unusableRun,
  v8Ranges,
} from "./test-reach.mjs";

// [file, source, test code]
const codeCases = [
  ["crates/x/src/a.rs", "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n", { ranges: [[2, 5]], files: [] }],
  ["crates/x/src/a.rs", "#[cfg(test)]\n#[allow(clippy::unwrap_used)]\nmod properties {\n}\nfn after() {}\n", { ranges: [[1, 4]], files: [] }],
  ["crates/x/src/a.rs", '#[cfg(test)]\nmod tests {\n    const S: &str = "}";\n    const R: &str = r#"}"#;\n    const C: char = \'}\';\n    // }\n    /* } */\n    fn f<\'a>(x: &\'a str) {}\n}\nfn after() {}\n', { ranges: [[1, 9]], files: [] }],
  ["crates/x/src/a.rs", "#[cfg(test)]\nmod tests {\n    const A: &str = \"a \\\" } \\\\\";\n    const B: &str = r##\"} \"# }\"##;\n    const C: &[u8] = b\"}\";\n    const D: char = '\\'';\n    const E: char = '\\u{7D}';\n    fn f<'a>(x: &'a str) -> &'a str { x }\n    /* } */\n}\nfn after() { let _ = \"{\"; }\n", { ranges: [[1, 10]], files: [] }],
  ["crates/x/src/a.rs", "#[cfg(test)]\npub(crate) mod handoff_tests;", { ranges: [], files: ["crates/x/src/a/handoff_tests.rs", "crates/x/src/a/handoff_tests/mod.rs"] }],
  ["crates/x/src/lib.rs", "#[cfg(test)]\nmod tests;", { ranges: [], files: ["crates/x/src/tests.rs", "crates/x/src/tests/mod.rs"] }],
  ["crates/x/src/a.rs", "mod tests {\n}\n", { ranges: [], files: [] }],
  ["crates/x/src/a.rs", "#[cfg(test)]\nuse std::fmt;", { ranges: [], files: [] }],
];
for (const [file, source, code] of codeCases) {
  test(`test code of ${JSON.stringify(source)}`, () => assert.deepEqual(testCode(file, source), code));
}

// [file, line, inside test code]
const atCases = [
  ["crates/x/src/a.rs", 1, false],
  ["crates/x/src/a.rs", 2, true],
  ["crates/x/src/a.rs", 5, true],
  ["crates/x/src/a.rs", 6, false],
  ["crates/x/src/a/tests.rs", 1, true],
  ["crates/x/src/b.rs", 3, false],
];
for (const [file, line, inside] of atCases) {
  test(`${file}:${line} in test code: ${inside}`, () => {
    const isTest = testCodeAt({ "crates/x/src/a.rs": { ranges: [[2, 5]], files: ["crates/x/src/a/tests.rs"] } });
    assert.equal(isTest(file, line), inside);
  });
}

// [file, product code]
const fileCases = [
  ["crates/rig/src/patch.rs", true],
  ["shell/src/main.rs", true],
  ["crates/rig/tests/patch.rs", false],
  ["crates/rig/examples/frame_lateness.rs", false],
  ["/rustc/abc/library/core/src/option.rs", false],
];
for (const [file, product] of fileCases) {
  test(`product file ${file}: ${product}`, () => assert.equal(isProductFile(file), product));
}

test("rust reach keeps reached code regions of product code, outside test code, whatever a function is called", () => {
  const files = ["/r/crates/rig/src/a.rs", "/r/crates/rig/tests/t.rs"];
  const exported = {
    data: [
      {
        functions: [
          { name: "_ZN3rig1a5feeds17h0E", filenames: files, regions: [[1, 1, 2, 2, 3, 0, 0, 0], [3, 1, 3, 9, 0, 0, 0, 0], [4, 1, 4, 2, 5, 0, 0, 1]] },
          { name: "_ZN3rig5tests1g17h0E", filenames: files, regions: [[9, 1, 9, 2, 1, 0, 0, 0]] },
          { name: "_ZN1t1h17h0E", filenames: files, regions: [[1, 1, 1, 5, 1, 1, 0, 0]] },
        ],
      },
    ],
  };
  const isTest = (file, line) => file === "crates/rig/src/a.rs" && line >= 8;
  assert.deepEqual([...rustReach(exported, "/r", isTest)], ["crates/rig/src/a.rs:1:1-2:2"]);
});

test("v8 ranges: an inner zero count cuts a hole, other roots and node_modules are left out", () => {
  const coverage = {
    result: [
      { url: "file:///r/ui/src/a.ts", functions: [{ ranges: [{ startOffset: 0, endOffset: 10, count: 1 }, { startOffset: 3, endOffset: 5, count: 0 }] }] },
      { url: "file:///r/node_modules/x.js", functions: [{ ranges: [{ startOffset: 0, endOffset: 4, count: 1 }] }] },
      { url: "node:fs", functions: [{ ranges: [{ startOffset: 0, endOffset: 4, count: 1 }] }] },
      { url: "file:///elsewhere/b.js", functions: [{ ranges: [{ startOffset: 0, endOffset: 4, count: 1 }] }] },
    ],
  };
  assert.deepEqual(v8Ranges(coverage, "/r"), { "ui/src/a.ts": [[0, 3], [5, 10]] });
});

// [ranges of one function as V8 lists them, reached]
const holeCases = [
  [[{ startOffset: 3, endOffset: 5, count: 0 }, { startOffset: 0, endOffset: 10, count: 1 }], [[0, 3], [5, 10]]],
  [[{ startOffset: 0, endOffset: 4, count: 0 }, { startOffset: 0, endOffset: 10, count: 1 }], [[4, 10]]],
  [[{ startOffset: 0, endOffset: 10, count: 0 }, { startOffset: 2, endOffset: 4, count: 3 }], [[2, 4]]],
];
for (const [ranges, reached] of holeCases) {
  test(`v8 ranges ${JSON.stringify(ranges)}: inner wins whatever the order`, () => {
    assert.deepEqual(v8Ranges({ result: [{ url: "file:///r/ui/src/a.ts", functions: [{ ranges }] }] }, "/r"), { "ui/src/a.ts": reached });
  });
}

test("v8 ranges leave out the files keep refuses", () => {
  const coverage = { result: [{ url: "file:///r/ui/src/a.test.mjs", functions: [{ ranges: [{ startOffset: 0, endOffset: 4, count: 1 }] }] }] };
  assert.deepEqual(v8Ranges(coverage, "/r", (f) => !f.endsWith(".test.mjs")), {});
});

test("range keys cut each file at every boundary any test has", () => {
  const keys = rangeKeys({ a: { f: [[0, 10]] }, b: { f: [[5, 10]] } });
  assert.deepEqual([...keys.a], ["f:0-5", "f:5-10"]);
  assert.deepEqual([...keys.b], ["f:5-10"]);
});

test("failed tests name their binary", () => {
  const log = [
    "     Running unittests src/lib.rs (target/debug/deps/fixture_library-0123abcd)",
    "test tests::one ... ok",
    "test tests::two ... FAILED",
    "     Running tests/patch.rs (/tmp/m/target/debug/deps/patch-89ef)",
    "test refuses_overlap ... FAILED",
    "     Running `rustc --crate-name rig --edition=2024 crates/rig/src/lib.rs`",
    "     Running `/tmp/cargo-mutants-x.tmp/target/debug/deps/channels-26c9df5e`",
    "test a_range_does_what_its_preset_says ... FAILED",
  ].join("\n");
  assert.deepEqual(failedTests(log), [
    { src: "src/lib.rs", stem: "fixture_library", name: "tests::two" },
    { src: "tests/patch.rs", stem: "patch", name: "refuses_overlap" },
    { src: null, stem: "channels", name: "a_range_does_what_its_preset_says" },
  ]);
});

test("a failure without its source file matches on binary and name", () => {
  const tests = [
    { id: "rig/tests/channels.rs::a", src: "tests/channels.rs", stem: "channels", name: "a" },
    { id: "fixture-library/tests/channels.rs::b", src: "tests/channels.rs", stem: "channels", name: "b" },
  ];
  assert.deepEqual(matchFailure({ src: null, stem: "channels", name: "a" }, tests).map((t) => t.id), ["rig/tests/channels.rs::a"]);
});

test("a failure matches every inventory test with its binary and name", () => {
  const tests = [
    { id: "rig/tests/patch.rs::a", src: "tests/patch.rs", stem: "patch", name: "a" },
    { id: "engine/tests/patch.rs::a", src: "tests/patch.rs", stem: "patch", name: "a" },
    { id: "rig/tests/patch.rs::b", src: "tests/patch.rs", stem: "patch", name: "b" },
  ];
  assert.deepEqual(matchFailure({ src: "tests/patch.rs", stem: "patch", name: "a" }, tests).map((t) => t.id), ["rig/tests/patch.rs::a", "engine/tests/patch.rs::a"]);
});

// [reach, file, lines, overlaps]
const spanCases = [
  [["crates/x/src/a.rs:10:1-12:2"], "crates/x/src/a.rs", [12, 20], true],
  [["crates/x/src/a.rs:10:1-12:2"], "crates/x/src/a.rs", [1, 10], true],
  [["crates/x/src/a.rs:10:1-12:2"], "crates/x/src/a.rs", [13, 20], false],
  [["crates/x/src/a.rs:10:1-12:2"], "crates/x/src/b.rs", [10, 12], false],
];
for (const [reach, file, lines, overlaps] of spanCases) {
  test(`${reach} reaches ${file} ${lines}: ${overlaps}`, () => assert.equal(reachesSpan(new Set(reach), file, lines), overlaps));
}

// [key, crate]
const crateCases = [
  ["crates/rig/src/a.rs:1:1-2:2", "rig"],
  ["ui/src/controls/fader.ts:0-5", "ui"],
  ["scripts/verify.mjs:0-5", "scripts"],
];
for (const [key, crate] of crateCases) {
  test(`crate of ${key}`, () => assert.equal(crateOf(key), crate));
}

const t = (id, reach, kills = []) => ({ id, reach: new Set(reach), kills: new Set(kills) });

test("multiplicity counts the tests on each piece, per crate", () => {
  const tests = [t("a", ["crates/x/src/a:1", "crates/x/src/a:2"]), t("b", ["crates/x/src/a:1"]), t("c", ["crates/x/src/a:1", "crates/y/src/b:1"])];
  assert.deepEqual(multiplicity(tests), {
    x: { pieces: 2, once: 1, twice: 0, thrice: 1, upTo9: 0, tenOrMore: 0 },
    y: { pieces: 1, once: 1, twice: 0, thrice: 0, upTo9: 0, tenOrMore: 0 },
  });
});

// [tests on one piece, bucket]
const bucketCases = [[2, "twice"], [4, "upTo9"], [9, "upTo9"], [10, "tenOrMore"]];
for (const [n, bucket] of bucketCases) {
  test(`a piece reached by ${n} tests counts as ${bucket}`, () => {
    const tests = Array.from({ length: n }, (_, i) => t(String(i), ["crates/x/src/a:1"]));
    assert.equal(multiplicity(tests).x[bucket], 1);
  });
}

test("uniques count what only one test reaches or kills", () => {
  const u = uniques([t("a", [1, 2], ["m1"]), t("b", [1], ["m1", "m2"])]);
  assert.deepEqual(u, { a: { reach: 1, kills: 0 }, b: { reach: 0, kills: 1 } });
});

// [tests, kept ids, removable ids]
const removableCases = [
  ["a subset goes", [t("a", [1, 2]), t("b", [1])], [], ["b"]],
  ["of two equal tests, one goes", [t("a", [1]), t("b", [1])], [], ["a"]],
  ["a test that alone kills a mutant stays", [t("a", [1, 2]), t("b", [1], ["m"])], [], []],
  ["a kept test is never removed", [t("a", [1, 2]), t("b", [1])], ["b"], []],
  ["a kept test still covers others", [t("a", [1]), t("b", [1])], ["a"], ["b"]],
  ["three overlapping: the order decides, coverage stays", [t("a", [1, 2]), t("b", [2, 3]), t("c", [1, 3])], [], ["a"]],
  ["of two that share the only kill, one goes", [t("a", [1], ["m"]), t("b", [1], ["m"]), t("c", [1])], [], ["a", "c"]],
];
for (const [name, tests, keep, gone] of removableCases) {
  test(`removable: ${name}`, () => {
    assert.deepEqual(removable(tests, (x) => keep.includes(x.id)).map((g) => g.id), gone);
  });
}

test("removable names the staying tests that cover what goes", () => {
  const [g] = removable([t("a", [1, 2], ["m"]), t("b", [1]), t("c", [2], ["m"])], (x) => x.id !== "a");
  assert.deepEqual(g, { id: "a", reach: 2, kills: 1, coveredBy: ["c", "b"] });
});

// A test that passes every rule; each row changes one field. [changes, reason or null]
const plain = { package: "fixture-library", src: "src/lib.rs", name: "tests::x", ok: true, killsKnown: true, reachesUncertain: false, ambiguous: false, reach: new Set(["r"]) };
const keepCases = [
  [{}, null],
  [{ package: "outputs", src: "tests/no_alloc.rs", name: "take" }, "real-time: no allocation"],
  [{ src: "tests/search_allocations.rs" }, "real-time: no allocation"],
  [{ src: "tests/loop_budget.rs" }, "real-time: timing bound"],
  [{ name: "tests::lateness_p99" }, "real-time: timing bound"],
  [{ package: "engine-server", src: "tests/scenarios.rs", name: "x" }, "end to end: API in, DMX out"],
  [{ package: "engine-server", src: "src/main.rs", name: "tests::x" }, null],
  [{ package: "rig" }, "rewritten in #250 L3"],
  [{ package: "engine" }, "rewritten in #250 L3"],
  [{ package: "engine-core" }, "rewritten in #250 L3"],
  [{ name: "tests::every_row_of_the_table" }, "state machine walk"],
  [{ name: "tests::each_transition_once" }, "state machine walk"],
  [{ name: "tests::the_machine_refuses" }, "state machine walk"],
  [{ name: "tests::every_pair" }, "state machine walk"],
  [{ name: "tests::arrow_keys" }, null],
  [{ killsKnown: false }, "no kill matrix runs it for every crate it reaches"],
  [{ ok: false }, "fails when run alone"],
  [{ ambiguous: true }, "a failing name matched tests in two packages"],
  [{ reachesUncertain: true }, "reaches a mutant whose killers are not all known (timeout, crash)"],
  [{ reach: new Set() }, "reaches no measured code: a spawned process, or a fact the compiler checks"],
];
for (const [changes, reason] of keepCases) {
  test(`keep ${JSON.stringify(changes)}: ${reason}`, () => {
    const testCase = { ...plain, ...changes };
    assert.equal(KEEP.find(([rule]) => rule(testCase))?.[1] ?? null, reason);
  });
}

const fl = "crates/fixture-library/src/a.rs";
const mutant = (name, outcome, killers, more = {}) => ({ name, outcome, file: fl, lines: [10, 12], killers, unmatched: [], ambiguous: false, ...more });
const reaching = (id, pkg) => ({ id, package: pkg, reach: new Set([`${fl}:11:1-11:9`]) });
const elsewhere = (id, pkg) => ({ id, package: pkg, reach: new Set([`${fl}:40:1-40:9`]) });
const run = (mutants, packages = ["fixture-library", "rig"]) => [{ crate: "fixture-library", packages, mutants }];

// [name, test, runs, expected fields]
const markCases = [
  ["a killer gets the kill", reaching("a", "fixture-library"), run([mutant("m", "CaughtMutant", ["a"])]), { kills: ["m"], killsKnown: true, reachesUncertain: false, ambiguous: false }],
  ["a test of a package that ran is known", reaching("b", "rig"), run([]), { kills: [], killsKnown: true }],
  ["a test whose package did not run is not known", reaching("c", "looks"), run([]), { killsKnown: false }],
  ["a test reaching a crate with no run is not known", { id: "d", package: "fixture-library", reach: new Set(["crates/rig/src/b.rs:1:1-1:2"]) }, run([]), { killsKnown: false }],
  ["reaching nothing is known", { id: "e", package: "looks", reach: new Set() }, run([]), { killsKnown: true }],
  ["a timeout makes its reachers uncertain", reaching("f", "fixture-library"), run([mutant("m", "Timeout", [])]), { reachesUncertain: true }],
  ["a caught mutant with no named killer makes its reachers uncertain", reaching("g", "fixture-library"), run([mutant("m", "CaughtMutant", [])]), { reachesUncertain: true }],
  ["an unmatched failure makes its reachers uncertain", reaching("h", "fixture-library"), run([mutant("m", "CaughtMutant", ["z"], { unmatched: ["tests/x.rs::gone"] })]), { reachesUncertain: true }],
  ["a missed mutant is certain", reaching("i", "fixture-library"), run([mutant("m", "MissedMutant", [])]), { reachesUncertain: false }],
  ["an uncertain mutant elsewhere does not matter", elsewhere("j", "fixture-library"), run([mutant("m", "Timeout", [])]), { reachesUncertain: false }],
  ["an ambiguous kill marks its killers", reaching("k", "fixture-library"), run([mutant("m", "CaughtMutant", ["k"], { ambiguous: true })]), { ambiguous: true }],
];
for (const [name, testCase, runs, expected] of markCases) {
  test(`mark tests: ${name}`, () => {
    const [marked] = markTests([testCase], runs);
    for (const [field, value] of Object.entries(expected)) assert.deepEqual(field === "kills" ? [...marked.kills] : marked[field], value, field);
  });
}

// Two runs, a test killing in both. [name, test, kills, killsKnown, reachesUncertain]
const showFile = "crates/show-file/src/b.rs";
const twoRuns = [
  { crate: "fixture-library", packages: ["fixture-library"], mutants: [mutant("m1", "CaughtMutant", ["a"]), mutant("m2", "CaughtMutant", ["a", "b"])] },
  {
    crate: "show-file",
    packages: ["show-file", "fixture-library"],
    mutants: [mutant("m3", "CaughtMutant", ["a"], { file: showFile }), mutant("m4", "Timeout", [], { file: showFile })],
  },
];
const multiCases = [
  ["kills from every run and every mutant", { id: "a", package: "fixture-library", reach: new Set([`${fl}:11:1-11:9`, `${showFile}:1:1-1:2`]) }, ["m1", "m2", "m3"], true, false],
  ["an uncertain mutant in the second run", { id: "c", package: "fixture-library", reach: new Set([`${showFile}:11:1-11:2`]) }, [], true, true],
  ["known in one run, not run in the other", { id: "d", package: "show-file", reach: new Set([`${fl}:40:1-40:2`, `${showFile}:1:1-1:2`]) }, [], false, false],
];
for (const [name, testCase, kills, known, uncertain] of multiCases) {
  test(`mark tests over two runs: ${name}`, () => {
    const [marked] = markTests([testCase], twoRuns);
    assert.deepEqual([...marked.kills], kills);
    assert.equal(marked.killsKnown, known);
    assert.equal(marked.reachesUncertain, uncertain);
  });
}

test("a test that kills two mutants, one of them alone, stays", () => {
  const marked = markTests(
    [reaching("a", "fixture-library"), reaching("b", "fixture-library")],
    run([mutant("m1", "CaughtMutant", ["a", "b"]), mutant("m2", "CaughtMutant", ["a"])]),
  );
  assert.deepEqual(removable(marked, () => null).map((g) => g.id), ["b"]);
});

const outcomeOf = (summary) => ({
  summary,
  scenario: { Mutant: { name: "crates/x/src/a.rs:3:5: replace f -> u8 with 0", file: "crates/x/src/a.rs", span: { start: { line: 3, column: 5 }, end: { line: 7, column: 2 } } } },
});
const inventory = [
  { id: "x/tests/t.rs::one", src: "tests/t.rs", stem: "t", name: "one" },
  { id: "y/tests/t.rs::one", src: "tests/t.rs", stem: "t", name: "one" },
  { id: "x/tests/u.rs::two", src: "tests/u.rs", stem: "u", name: "two" },
];
// [failures, killers, unmatched, ambiguous]
const killCases = [
  [[{ src: null, stem: "u", name: "two" }], ["x/tests/u.rs::two"], [], false],
  [[{ src: null, stem: "t", name: "one" }], ["x/tests/t.rs::one", "y/tests/t.rs::one"], [], true],
  [[{ src: null, stem: "v", name: "gone" }], [], ["v::gone"], false],
  [[{ src: "tests/u.rs", stem: "u", name: "two" }, { src: null, stem: "u", name: "two" }], ["x/tests/u.rs::two"], [], false],
  [[], [], [], false],
];
for (const [failures, killers, unmatched, ambiguous] of killCases) {
  test(`mutant kills of ${JSON.stringify(failures)}`, () => {
    assert.deepEqual(mutantKills(outcomeOf("CaughtMutant"), failures, inventory), {
      name: "crates/x/src/a.rs:3:5: replace f -> u8 with 0",
      file: "crates/x/src/a.rs",
      lines: [3, 7],
      outcome: "CaughtMutant",
      killers,
      unmatched,
      ambiguous,
    });
  });
}

const results = (endTime, n) => ({ end_time: endTime, outcomes: [{ scenario: "Baseline" }, ...Array.from({ length: n }, () => outcomeOf("CaughtMutant"))] });
const ended = "2026-11-24T20:00:00Z";
// [results, listed, run sources, reach sources, usable]
const runCases = [
  [results(ended, 2), [1, 2], "s1", "s1", true],
  [results(null, 2), [1, 2], "s1", "s1", false],
  [results(ended, 1), [1, 2], "s1", "s1", false],
  [results(ended, 2), [1, 2], "s1", "s2", false],
  [results(ended, 2), [1, 2], undefined, undefined, false],
];
for (const [res, listed, runSources, reachSources, usable] of runCases) {
  test(`a run ended ${res.end_time}, ${res.outcomes.length - 1} of ${listed.length} mutants, sources ${runSources} and ${reachSources}: usable ${usable}`, () => {
    assert.equal(unusableRun(res, listed, runSources, reachSources) === null, usable);
  });
}

test("stable tests leave out the packages under rewrite", () => {
  const tests = ["engine-core", "engine", "rig", "fixture-library", "engine-server"].map((p) => ({ id: p, package: p }));
  assert.deepEqual(stableTests(tests).map((x) => x.id), ["fixture-library", "engine-server"]);
});
