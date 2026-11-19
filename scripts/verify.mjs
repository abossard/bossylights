// Calculations for `npm run verify` (docs/agents/orchestration.md, step 4): no files, no processes.

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
export function mutantsReport({ status, outcomes, stderr }) {
  if (outcomes) return { ok: true, text: mutantsSummary(outcomes) };
  if (status === 0) return { ok: true, text: "mutants: none in this diff" };
  const last = stderr.split("\n").filter(Boolean).at(-1) ?? "no output";
  return { ok: false, text: `mutants: no outcomes file (cargo mutants exit ${status}): ${last}` };
}
