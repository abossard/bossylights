// Calculations for `npm run verify` (docs/agents/orchestration.md, worker step 5): no files, no processes.

/** The suites `npm test` chains, in order: ["test:checks", ...]. */
export function suitesOf(testScript) {
  return testScript.split("&&").map((part) => part.trim().replace(/^npm run /, ""));
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

/** What still runs from the copy at `dir` once a suite is over, read from `ps -eo pid=,args=`: [{ pid, args }]. Only programs started from the copy itself (a test window, its engine), never another run's or the owner's. */
export function leftovers(ps, dir) {
  return ps
    .split("\n")
    .map((line) => line.trim().match(/^(\d+) +(.*)$/))
    .filter((row) => row && row[2].startsWith(`${dir}/`))
    .map(([, pid, args]) => ({ pid: Number(pid), args }));
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

// What the run of cargo-mutants left: its outcomes (or none) and its exit code. Exit 0 with no
// outcomes file means it found nothing to mutate; any other missing file is a failure.
// counts: caught, missed, timeout, unviable, or null when it did not run through.
export function mutantsReport({ status, outcomes, stderr }) {
  if (outcomes) {
    const { caught, missed, timeout, unviable } = outcomes;
    return { ok: true, text: mutantsSummary(outcomes), counts: { caught, missed, timeout, unviable } };
  }
  if (status === 0) return { ok: true, text: "mutants: none in this diff", counts: { caught: 0, missed: 0, timeout: 0, unviable: 0 } };
  const last = stderr.split("\n").filter(Boolean).at(-1) ?? "no output";
  return { ok: false, text: `mutants: no outcomes file (cargo mutants exit ${status}): ${last}`, counts: null };
}

/**
 * The record of a passed run, which `npm run land` trusts for the same tree, or why there is none.
 * Only a run where every suite passed and mutants ran on the diff with none missed records.
 * results: [{ suite, ok }]. mutants: what mutantsReport gave, or null when --mutants was not asked.
 */
export function recordOf({ commit, tree, base, results, mutants }) {
  if (results.length === 0 || results.some((r) => !r.ok)) return { record: null, why: "a suite failed" };
  if (!mutants) return { record: null, why: "mutants did not run: verify with --mutants" };
  if (!mutants.ok) return { record: null, why: "mutants did not run through" };
  if (mutants.counts.missed > 0) return { record: null, why: `${mutants.counts.missed} mutant(s) missed: kill each, or pin it where it lives with why (#[mutants::skip], .cargo/mutants.toml)` };
  return { record: { tree, commit, base, suites: results.map((r) => r.suite), mutants: mutants.counts }, why: null };
}

/** Where the record of a tree lives: in the git folder every worktree shares, outside any tree. */
export function recordPath(commonDir, tree) {
  return `${commonDir}/bossylights-verified/${tree}.json`;
}
