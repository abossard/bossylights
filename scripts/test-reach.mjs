// Calculations for `npm run test-reach` (#300, docs/testing.md): what each test reaches, which mutants it
// kills, how many tests cover the same code, and which tests add nothing. No files, no processes.

const CFG_TEST_MOD = /#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*([{;])/g;

/**
 * Where a Rust source keeps its test code: the line ranges of `#[cfg(test)] mod x { … }` and the possible files of
 * `#[cfg(test)] mod x;`. `file` is the source's path from the repo root. By place, not by symbol name, so a product
 * function that shares a test module's name still counts.
 */
export function testCode(file, source) {
  const ranges = [];
  const files = [];
  for (const m of source.matchAll(CFG_TEST_MOD)) {
    if (m[2] === ";") files.push(...moduleFiles(file, m[1]));
    else ranges.push([lineAt(source, m.index), lineAt(source, matchingBrace(source, m.index + m[0].length - 1))]);
  }
  return { ranges, files };
}

function moduleFiles(file, name) {
  const slash = file.lastIndexOf("/");
  const base = file.slice(slash + 1);
  const home = /^(lib|main|mod)\.rs$/.test(base) ? file.slice(0, slash) : file.slice(0, -".rs".length);
  return [`${home}/${name}.rs`, `${home}/${name}/mod.rs`];
}

const lineAt = (source, index) => source.slice(0, index).split("\n").length;

/** Index of the brace that closes the one at `open`. Skips comments, strings, raw strings and char literals. */
function matchingBrace(src, open) {
  let depth = 0;
  for (let i = open; i < src.length; i += 1) {
    const rest = src.slice(i, i + 16);
    if (rest.startsWith("//")) {
      i = src.indexOf("\n", i);
      if (i === -1) break;
    } else if (rest.startsWith("/*")) {
      i = src.indexOf("*/", i + 2) + 1;
      if (i === 0) break;
    } else if (/^r#*"/.test(rest) && !/\w/.test(src[i - 1] ?? "")) {
      const hashes = rest.match(/^r(#*)/)[1];
      i = src.indexOf(`"${hashes}`, i + hashes.length + 2) + hashes.length;
    } else if (src[i] === '"') {
      for (i += 1; i < src.length && src[i] !== '"'; i += 1) if (src[i] === "\\") i += 1;
    } else if (src[i] === "'") {
      const char = rest.match(/^'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'\n])'/);
      if (char) i += char[0].length - 1;
    } else if (src[i] === "{") depth += 1;
    else if (src[i] === "}" && --depth === 0) return i;
  }
  return src.length;
}

/** The test code of many sources as one predicate: (file, line) → true inside test code. `code`: { file: testCode(...) }. */
export function testCodeAt(code) {
  const files = new Set(Object.values(code).flatMap((c) => c.files));
  return (file, line) => files.has(file) || (code[file]?.ranges ?? []).some(([from, to]) => line >= from && line <= to);
}

/** Product code: a source file of a crate, not its tests, examples or benches. `file` is relative to the repo root. */
export function isProductFile(file) {
  return /^(crates\/[^/]+|shell)\/src\//.test(file);
}

const CODE_REGION = 0;

/**
 * Regions one `llvm-cov export` reached in product code, as keys "file:l1:c1-l2:c2".
 * A region is a stretch of code with one count: both sides of a branch are separate regions.
 * `isTest(file, line)`: true inside test code, which is never reach.
 */
export function rustReach(exported, root, isTest) {
  const reached = new Set();
  for (const fn of exported.data.flatMap((d) => d.functions)) {
    for (const [l1, c1, l2, c2, count, fileId, , kind] of fn.regions) {
      if (count === 0 || kind !== CODE_REGION) continue;
      const file = relativeTo(root, fn.filenames[fileId]);
      if (isProductFile(file) && !isTest(file, l1)) reached.add(`${file}:${l1}:${c1}-${l2}:${c2}`);
    }
  }
  return reached;
}

function relativeTo(root, path) {
  const base = root.endsWith("/") ? root : `${root}/`;
  return path.startsWith(base) ? path.slice(base.length) : path;
}

/**
 * One process's V8 coverage as reached [start, end) byte ranges per file under root.
 * V8 nests ranges: an inner range overrides the outer one, so a zero count inside cuts a hole.
 */
export function v8Ranges(coverage, root, keep = (file) => true) {
  const out = {};
  for (const script of coverage.result) {
    if (!script.url.startsWith("file://")) continue;
    const file = relativeTo(root, decodeURIComponent(script.url.slice("file://".length)));
    if (file.startsWith("/") || file.includes("node_modules/") || !keep(file)) continue;
    const ranges = script.functions.flatMap((f) => f.ranges);
    if (ranges.length === 0) continue;
    const end = Math.max(...ranges.map((r) => r.endOffset));
    const counts = new Array(end).fill(0);
    // Outer before inner: wider ranges first, so the narrower one writes last.
    for (const r of [...ranges].sort((a, b) => b.endOffset - b.startOffset - (a.endOffset - a.startOffset))) {
      counts.fill(r.count > 0 ? 1 : 0, r.startOffset, r.endOffset);
    }
    const reached = [];
    for (let i = 0; i < end; i += 1) {
      if (!counts[i]) continue;
      const start = i;
      while (i < end && counts[i]) i += 1;
      reached.push([start, i]);
    }
    if (reached.length) out[file] = (out[file] ?? []).concat(reached);
  }
  return out;
}

/**
 * Byte ranges of many tests as comparable keys. Every range boundary of any test cuts a file into pieces;
 * a test reaches the pieces inside its ranges. `ranges`: { test: { file: [[start, end]] } } → { test: Set }.
 */
export function rangeKeys(ranges) {
  const cuts = {};
  for (const files of Object.values(ranges)) {
    for (const [file, list] of Object.entries(files)) {
      cuts[file] ??= new Set();
      for (const [s, e] of list) cuts[file].add(s).add(e);
    }
  }
  const sorted = Object.fromEntries(Object.entries(cuts).map(([f, c]) => [f, [...c].sort((a, b) => a - b)]));
  return Object.fromEntries(
    Object.entries(ranges).map(([test, files]) => {
      const keys = new Set();
      for (const [file, list] of Object.entries(files)) {
        const points = sorted[file];
        for (const [s, e] of list) {
          for (let i = points.indexOf(s); i >= 0 && points[i] < e; i += 1) keys.add(`${file}:${points[i]}-${points[i + 1]}`);
        }
      }
      return [test, keys];
    }),
  );
}

/**
 * Failed tests in the output of `cargo test`, each with the test binary it ran in:
 * [{ src: "tests/patch.rs" | "src/lib.rs" | null, stem: "patch", name: "case::one" }].
 * `cargo test --verbose` (as cargo-mutants runs it) names only the binary's path, so src is null.
 */
export function failedTests(log) {
  const failed = [];
  let binary = null;
  for (const line of log.split("\n")) {
    const plain = line.match(/^\s*Running (?:unittests )?(\S+) \((?:.*\/)?([A-Za-z0-9_]+)-[0-9a-f]+\)/);
    const verbose = line.match(/^\s*Running `(?:.*\/)?([A-Za-z0-9_]+)-[0-9a-f]+`$/);
    if (plain) binary = { src: plain[1], stem: plain[2] };
    if (verbose) binary = { src: null, stem: verbose[1] };
    const fail = line.match(/^test (\S+) \.\.\. FAILED$/);
    if (fail && binary) failed.push({ ...binary, name: fail[1] });
  }
  return failed;
}

/** The inventory tests a failure names. More than one means the name is ambiguous across packages. */
export function matchFailure(failure, tests) {
  return tests.filter((t) => (failure.src === null || t.src === failure.src) && t.stem === failure.stem && t.name === failure.name);
}

/** True when a reach key "file:l1:c1-l2:c2" of the file overlaps lines [from, to]. */
export function reachesSpan(reach, file, [from, to]) {
  for (const key of reach) {
    const m = key.match(/^(.*):(\d+):\d+-(\d+):\d+$/);
    if (m && m[1] === file && Number(m[2]) <= to && Number(m[3]) >= from) return true;
  }
  return false;
}

/** The crate a reach key belongs to: "crates/rig/src/x.rs:1:2-3:4" → "rig". */
export function crateOf(key) {
  const m = key.match(/^crates\/([^/]+)\//);
  if (m) return m[1];
  return key.split("/")[0];
}

/**
 * How many tests reach each piece of code, per crate: { crate: { pieces, once, twice, thrice, upTo9, tenOrMore } }.
 * `tests`: [{ reach: Set }]. Only reached pieces are counted.
 */
export function multiplicity(tests) {
  const hits = new Map();
  for (const t of tests) for (const key of t.reach) hits.set(key, (hits.get(key) ?? 0) + 1);
  const table = {};
  for (const [key, n] of hits) {
    const row = (table[crateOf(key)] ??= { pieces: 0, once: 0, twice: 0, thrice: 0, upTo9: 0, tenOrMore: 0 });
    row.pieces += 1;
    row[n === 1 ? "once" : n === 2 ? "twice" : n === 3 ? "thrice" : n < 10 ? "upTo9" : "tenOrMore"] += 1;
  }
  return table;
}

/** For each test, what only it reaches and what only it kills. `tests`: [{ id, reach: Set, kills: Set }]. */
export function uniques(tests) {
  const count = (field) => {
    const n = new Map();
    for (const t of tests) for (const k of t[field]) n.set(k, (n.get(k) ?? 0) + 1);
    return n;
  };
  const reachN = count("reach");
  const killN = count("kills");
  return Object.fromEntries(
    tests.map((t) => [
      t.id,
      { reach: [...t.reach].filter((k) => reachN.get(k) === 1).length, kills: [...t.kills].filter((k) => killN.get(k) === 1).length },
    ]),
  );
}

/**
 * The tests that can go together without losing a reached piece or a killed mutant.
 * Greedy: candidates in the given order (slowest first is a good one); a candidate goes only when every piece it reaches
 * and every mutant it kills is still reached and killed by a test that stays. `kept(test)` returns a reason to keep, or null.
 * Returns [{ id, reach, kills, coveredBy: [ids of the staying tests that share most with it] }].
 */
export function removable(tests, kept, order = tests) {
  const reachN = new Map();
  const killN = new Map();
  for (const t of tests) {
    for (const k of t.reach) reachN.set(k, (reachN.get(k) ?? 0) + 1);
    for (const k of t.kills) killN.set(k, (killN.get(k) ?? 0) + 1);
  }
  const gone = new Set();
  for (const t of order) {
    if (kept(t)) continue;
    if ([...t.reach].some((k) => reachN.get(k) < 2) || [...t.kills].some((k) => killN.get(k) < 2)) continue;
    gone.add(t.id);
    for (const k of t.reach) reachN.set(k, reachN.get(k) - 1);
    for (const k of t.kills) killN.set(k, killN.get(k) - 1);
  }
  const staying = tests.filter((t) => !gone.has(t.id));
  return tests
    .filter((t) => gone.has(t.id))
    .map((t) => ({ id: t.id, reach: t.reach.size, kills: t.kills.size, coveredBy: closest(t, staying) }));
}

/** The staying tests that share the most reach and kills with t, until they cover all of it (at most 3). */
function closest(t, staying) {
  const has = (s, [field, key]) => s[field].has(key);
  let left = [...[...t.reach].map((k) => ["reach", k]), ...[...t.kills].map((k) => ["kills", k])];
  const picked = [];
  while (left.length && picked.length < 3) {
    let best = null;
    let bestN = 0;
    for (const s of staying) {
      if (picked.includes(s.id)) continue;
      const n = left.filter((item) => has(s, item)).length;
      if (n > bestN) [best, bestN] = [s, n];
    }
    if (!best) break;
    picked.push(best.id);
    left = left.filter((item) => !has(best, item));
  }
  return picked;
}

/**
 * Kill matrices onto the tests. `runs`: [{ crate, packages, mutants: [{ name, outcome, file, lines, killers, unmatched, ambiguous }] }],
 * one per crate whose mutants ran, `packages` the packages whose tests ran on each.
 * Returns the tests with `kills` (Set of mutant names) and three flags for KEEP:
 * - killsKnown: for every crate the test reaches, a run mutated that crate and ran the test's package
 * - reachesUncertain: it reaches a mutant whose killers the log cannot fully name (a timeout, a caught mutant with
 *   no failing test found, a failing name matching no test)
 * - ambiguous: a failing name matched it and a test of another package
 */
export function markTests(tests, runs) {
  const uncertain = runs.flatMap((r) =>
    r.mutants.filter((m) => m.outcome === "Timeout" || (m.outcome === "CaughtMutant" && m.killers.length === 0) || m.unmatched.length > 0),
  );
  const kills = new Map();
  const ambiguous = new Set();
  for (const m of runs.flatMap((r) => r.mutants)) {
    for (const id of m.killers) {
      if (!kills.has(id)) kills.set(id, new Set());
      kills.get(id).add(m.name);
      if (m.ambiguous) ambiguous.add(id);
    }
  }
  return tests.map((t) => ({
    ...t,
    kills: kills.get(t.id) ?? new Set(),
    killsKnown: [...t.reach].every((k) => runs.some((r) => r.crate === crateOf(k) && r.packages.includes(t.package))),
    reachesUncertain: uncertain.some((m) => reachesSpan(t.reach, m.file, m.lines)),
    ambiguous: ambiguous.has(t.id),
  }));
}

/** Packages whose tests are being rewritten (#250 L3): they neither go nor count as a test that stays. */
export const REWRITTEN = ["engine-core", "engine", "rig"];

/** Why a test is never a candidate, or null. Each row is a rule of docs/testing.md. */
export const KEEP = [
  [(t) => /no_alloc|allocation|counting/.test(`${t.src}${t.name}`), "real-time: no allocation"],
  [(t) => /loop_budget|stalled_free|lateness/.test(`${t.src}${t.name}`), "real-time: timing bound"],
  [(t) => t.package === "engine-server" && t.src.startsWith("tests/"), "end to end: API in, DMX out"],
  [(t) => REWRITTEN.includes(t.package), "rewritten in #250 L3"],
  [(t) => /machine|transition|every_pair|(^|_|:)rows?(_|$)/.test(t.name), "state machine walk"],
  [(t) => !t.killsKnown, "no kill matrix runs it for every crate it reaches"],
  [(t) => !t.ok, "fails when run alone"],
  [(t) => t.ambiguous, "a failing name matched tests in two packages"],
  [(t) => t.reachesUncertain, "reaches a mutant whose killers are not all known (timeout, crash)"],
  [(t) => t.reach.size === 0, "reaches no measured code: a spawned process, or a fact the compiler checks"],
];

/**
 * One mutant of a cargo-mutants run as a row of the kill matrix: who killed it, and whether the log names them all.
 * `outcome`: an entry of outcomes.json; `failures`: failedTests of its log; `tests`: the inventory of the packages that ran.
 */
export function mutantKills(outcome, failures, tests) {
  const { name, file, span } = outcome.scenario.Mutant;
  const matches = failures.map((f) => matchFailure(f, tests));
  return {
    name,
    file,
    lines: [span.start.line, span.end.line],
    outcome: outcome.summary,
    killers: [...new Set(matches.flat().map((t) => t.id))],
    unmatched: failures.filter((_, i) => matches[i].length === 0).map((f) => `${f.src ?? f.stem}::${f.name}`),
    ambiguous: matches.some((m) => m.length > 1),
  };
}

/**
 * Why a kill run cannot be used yet, or null. A run counts only when cargo-mutants finished every mutant it listed,
 * and it ran on the same Rust sources as the reach measurement (mutant spans are line numbers).
 * `results`: outcomes.json; `listed`: mutants.json; `runSources`, `reachSources`: stamps of the sources each was measured on.
 */
export function unusableRun(results, listed, runSources, reachSources) {
  const done = results.outcomes.filter((o) => o.scenario.Mutant).length;
  if (!results.end_time || done !== listed.length) return `not finished: ${done} of ${listed.length} mutants`;
  if (!runSources || runSources !== reachSources) return `mutants on sources ${runSources ?? "unknown"}, reach on ${reachSources ?? "unknown"}`;
  return null;
}

/** The tests that may go or count as staying: none of a package under rewrite. */
export const stableTests = (tests) => tests.filter((t) => !REWRITTEN.includes(t.package));
