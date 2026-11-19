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

/** First line of a suite's output that says why it failed, or the last line when none does. */
export function firstFailure(output) {
  const lines = output.split("\n").map((l) => l.trim()).filter(Boolean);
  const hit = lines.find((l) => /(^error|\bFAILED\b|\bfailed\b|✖|not ok|^\d+\) )/i.test(l));
  return (hit ?? lines.at(-1) ?? "no output").slice(0, 160);
}

/** results: [{ suite, ok, output }]. At most one line per suite plus one for the first failure. */
export function testSummary(commit, results) {
  const lines = results.map(({ suite, ok }) => `${ok ? "pass" : "FAIL"}  ${suite}`);
  const failed = results.find((r) => !r.ok);
  if (failed) lines.push(`first failure (${failed.suite}): ${firstFailure(failed.output)}`);
  return [`verify ${commit}: ${failed ? "FAILED" : "all suites passed"}`, ...lines].join("\n");
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
