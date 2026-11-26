// Calculations for `npm run verify` (docs/agents/orchestration.md, worker step 5): no files, no processes.
import { posix } from "node:path";

/** The suites `npm test` chains, in order: ["test:checks", ...]. */
export function suitesOf(testScript) {
  return testScript.split("&&").map((part) => part.trim().replace(/^npm run /, ""));
}

// What a change can reach (#315 G4). A path that matches no row reaches everything.
const EVERYTHING = [/^Cargo\.(toml|lock)$/, /^rust-toolchain/, /^\.cargo\//, /^scripts\//, /^package(-lock)?\.json$/, /^\.github\//];
const WORDS = [/^docs\//, /^[^/]+\.md$/, /^mockups\//, /^spikes\//, /^LICENSE$/];
const UI = [/^ui\//, /^e2e\//];
const UI_CRATES = ["shell", "wire"];

/**
 * What the changed files can reach. crates: [{ name, dir, deps, reads }] with every workspace dependency, dev ones too,
 * and the paths outside its folder it reads (readsOf).
 * everything: a reason when the change reaches every check, else null. crates: the changed crates and every crate
 * that depends on them. ui: the UI, e2e and catalog suites.
 */
export function reachOf(files, crates) {
  const under = (file, path) => file === path || file.startsWith(`${path}/`);
  const readers = (file) => crates.filter((c) => under(file, c.dir) || c.reads.some((r) => under(file, r)));
  const wide = files.find((f) => EVERYTHING.some((r) => r.test(f)) || !(WORDS.some((r) => r.test(f)) || UI.some((r) => r.test(f)) || readers(f).length));
  if (wide) return { everything: wide, crates: crates.map((c) => c.name), ui: true };
  const reached = new Set(files.flatMap(readers).map((c) => c.name));
  for (let grew = true; grew; ) {
    grew = false;
    for (const c of crates) {
      if (!reached.has(c.name) && c.deps.some((d) => reached.has(d))) grew = reached.add(c.name);
    }
  }
  const names = crates.map((c) => c.name).filter((n) => reached.has(n));
  return { everything: null, crates: names, ui: files.some((f) => UI.some((r) => r.test(f))) || names.some((n) => UI_CRATES.includes(n)) };
}

/**
 * The cargo commands of the test:rust script, narrowed to `crates`: --workspace becomes -p of each, a -p list keeps the
 * reached ones (none left: the command goes), any other command stays. [[program, ...args]].
 */
export function rustCommands(script, crates) {
  if (crates.length === 0) return [];
  return script.split("&&").flatMap((part) => {
    const words = part.trim().split(/\s+/);
    if (words.includes("--workspace")) return [words.flatMap((w) => (w === "--workspace" ? crates.flatMap((c) => ["-p", c]) : [w]))];
    if (!words.includes("-p")) return [words];
    const kept = words.filter((w, i) => w !== "-p" && words[i - 1] !== "-p");
    const named = words.filter((_, i) => words[i - 1] === "-p").filter((c) => crates.includes(c));
    return named.length ? [[...kept.slice(0, 2), ...named.flatMap((c) => ["-p", c]), ...kept.slice(2)]] : [];
  });
}

/**
 * The steps of a local check and what it leaves to CI. scripts: package.json scripts. steps: [{ name, command }],
 * left: the suites or crates CI checks alone.
 */
export function stepsOf(reach, scripts, allCrates) {
  const suites = suitesOf(scripts.test);
  const npm = (suite) => ({ name: suite, command: ["npm", "run", suite] });
  if (reach.everything) return { steps: suites.map(npm), left: [] };
  const rust = rustCommands(scripts["test:rust"], reach.crates).map((command) => ({ name: `test:rust: ${command.slice(0, 2).join(" ")}`, command }));
  const ui = ["test:ui", "test:e2e", "test:catalog"];
  const steps = suites.flatMap((s) => (s === "test:checks" ? [npm(s)] : s === "test:rust" ? rust : reach.ui && ui.includes(s) ? [npm(s)] : []));
  const otherCrates = allCrates.filter((c) => !reach.crates.includes(c));
  const left = suites.flatMap((s) => (s === "test:rust" ? (otherCrates.length ? [`test:rust for ${otherCrates.join(", ")}`] : []) : steps.some((t) => t.name === s) ? [] : [s]));
  return { steps, left };
}

/** Arguments: <commit> and an optional --mutants. Throws the usage text otherwise. */
export function parseArgs(argv) {
  const mutants = argv.includes("--mutants");
  const rest = argv.filter((a) => a !== "--mutants");
  if (rest.length !== 1 || rest[0].startsWith("-")) throw new Error("usage: npm run verify -- <commit> [--mutants]");
  return { commit: rest[0], mutants };
}

/** First line of a suite's output that says why it failed. With none, a non-zero exit points at the log; else the last line. */
export function firstFailure(output, status = 0, log = "the log") {
  const lines = output.split("\n").map((l) => l.trim()).filter(Boolean);
  const hit = lines.find((l) => /(^error|^signal:|\bsignal:|\bpanicked\b|\bFAILED\b|\b[1-9]\d* failed\b|✖|not ok|^\d+\) )/.test(l));
  if (hit) return hit.slice(0, 160);
  if (status !== 0) return `exit ${status}, see ${log}`;
  return (lines.at(-1) ?? "no output").slice(0, 160);
}

/** results: [{ suite, ok, status, log, output }]. One line per suite (with its log file) plus one for the first failure. */
export function testSummary(commit, results) {
  const lines = results.map(({ suite, ok, log }) => `${ok ? "pass" : "FAIL"}  ${suite}  ${log}`);
  const failed = results.find((r) => !r.ok);
  if (failed) lines.push(`first failure (${failed.suite}): ${firstFailure(failed.output, failed.status, failed.log)}`);
  return [`verify ${commit}: ${failed ? "FAILED" : "all suites passed"}`, ...lines].join("\n");
}

/** The log file of a suite inside the log folder: "test:rust" becomes "<dir>/test-rust.log". */
export function logPath(dir, suite) {
  return `${dir}/${suite.replace(/[^A-Za-z0-9._-]/g, "-")}.log`;
}

/**
 * What still runs from the worktree at `dir` once a step is over, read from `ps -eo pid=,args=`: [{ pid, args }].
 * Only programs started from the worktree itself (a test window, its engine) that were not running before the step
 * (`before`: their pids), never another run's, the owner's or the worker's own.
 */
export function leftovers(ps, dir, before = new Set()) {
  return ps
    .split("\n")
    .map((line) => line.trim().match(/^(\d+) +(.*)$/))
    .filter((row) => row && row[2].startsWith(`${dir}/`) && !before.has(Number(row[1])))
    .map(([, pid, args]) => ({ pid: Number(pid), args }));
}

/**
 * The paths outside its own folder that a crate's sources name: every string literal with "../" in it. In #[path] and
 * include!, include_bytes!, include_str! it is from the file's folder; anywhere else from the crate's folder, where
 * cargo runs its tests (CARGO_MANIFEST_DIR). sources: [{ path, text }], paths from the repository root.
 * A changed file under one of them reaches the crate.
 */
export function readsOf(dir, sources) {
  const reads = new Set();
  for (const { path, text } of sources) {
    for (const [, before, literal] of text.matchAll(/(#\[path\s*=\s*|include(?:_bytes|_str)?!\(\s*)?"([^"\n]*\.\.\/[^"\n]*)"/g)) {
      const target = posix.normalize(posix.join(before ? posix.dirname(path) : dir, literal.replace(/^\/+/, "")));
      if (!target.startsWith("../") && !target.startsWith(`${dir}/`)) reads.add(target);
    }
  }
  return [...reads].sort();
}

/** cargo-mutants' outcomes.json as caught, missed (file:line), timeouts. Missed lines are always listed. */
export function mutantsSummary({ caught, missed, timeout, unviable, outcomes }) {
  const where = (kind) =>
    outcomes
      .filter((o) => o.summary === kind)
      .map((o) => `${o.scenario.Mutant.file}:${o.scenario.Mutant.span.start.line}  ${o.scenario.Mutant.name.split(": ").slice(1).join(": ")}`);
  const lines = [`mutants: caught ${caught}, missed ${missed}, timeout ${timeout}, unviable ${unviable}`];
  for (const w of where("MissedMutant")) lines.push(`  missed  ${w}`);
  for (const w of where("Timeout")) lines.push(`  timeout ${w}`);
  return lines.join("\n");
}

// What the mutants job on GitHub's machine left (#340): cargo-mutants' exit code (null when the job left no
// result), its outcomes (or none), and the run's address. Exit 0 with no outcomes file means it found nothing to
// mutate. counts: caught, missed, timeout, unviable, or null when it did not run through.
export function mutantsReport({ status, outcomes, run }) {
  const RAN = [0, 2, 3]; // exit codes of cargo-mutants: 0 all caught, 2 missed, 3 timeout
  const fail = (why) => ({ ok: false, text: `mutants: ${why}: ${run}`, counts: null });
  if (status === null) return fail("the job left no result");
  if (!RAN.includes(status)) return fail(`cargo mutants failed (exit ${status})`);
  if (outcomes) {
    const { caught, missed, timeout, unviable } = outcomes;
    return { ok: true, text: `${mutantsSummary(outcomes)}\n  run     ${run}`, counts: { caught, missed, timeout, unviable } };
  }
  if (status === 0) return { ok: true, text: `mutants: none in this diff\n  run     ${run}`, counts: { caught: 0, missed: 0, timeout: 0, unviable: 0 } };
  return fail(`no outcomes file (cargo mutants exit ${status})`);
}

/** The run that `gh workflow run` started, from what it printed: { id, url }, or null. */
export function runOf(printed) {
  const found = printed.match(/https:\/\/\S+\/actions\/runs\/(\d+)/);
  return found ? { id: found[1], url: found[0] } : null;
}

/**
 * The record of a passed run, which `npm run land` trusts for the same tree, or why there is none.
 * Only a run where every suite passed and mutants ran on the diff with none missed records.
 * results: [{ suite, ok }], left: what the run left to CI. mutants: what mutantsReport gave, or null when --mutants was not asked.
 */
export function recordOf({ commit, tree, base, results, left, mutants }) {
  if (results.length === 0 || results.some((r) => !r.ok)) return { record: null, why: "a suite failed" };
  if (!mutants) return { record: null, why: "mutants did not run: verify with --mutants" };
  if (!mutants.ok) return { record: null, why: "mutants did not run through" };
  if (mutants.counts.missed > 0) return { record: null, why: `${mutants.counts.missed} mutant(s) missed: kill each, or pin it where it lives with why (#[mutants::skip], .cargo/mutants.toml)` };
  return { record: { tree, commit, base, suites: results.map((r) => r.suite), left, mutants: mutants.counts }, why: null };
}

/**
 * What a run does with the record of its tree at its end (#335): the newest run of a tree decides, so a run that a newer
 * one overtook leaves the record to it. me: this run, newest: the run that started last, or null. record: recordOf's.
 */
export function recordStep({ me, newest, record }) {
  if (newest !== me) return "leave";
  return record ? "write" : "remove";
}

/** Where the record of a tree lives: in the git folder every worktree shares, outside any tree. */
export function recordPath(commonDir, tree) {
  return `${commonDir}/bossylights-verified/${tree}.json`;
}
