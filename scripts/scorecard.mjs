// The scorecard: numbers that show whether the code follows docs/coding.md, tracked per CI run.
// Calculations only; scripts/print-scorecard.mjs reads the files and runs the engine.

/** Crate kinds from scripts/layers.mjs that hold calculations. */
export const CALCULATION_KINDS = ["core", "calc"];

/** Lines that hold code: not blank, not a comment line. */
export function codeLines(source) {
  return source.split("\n").filter((line) => line.trim() !== "" && !line.trim().startsWith("//")).length;
}

/** Names inside one `use` tree, e.g. `a::{x, b::{c, d}}` has 3. A glob counts as one. */
function useNames(tree) {
  return tree
    .replace(/\s+as\s+\w+/g, "")
    .replace(/[{}]/g, ",")
    .split(",")
    .map((part) => part.trim())
    .filter((part) => part !== "" && !part.endsWith("::")).length;
}

/** Items visible outside the crate: `pub` items, not `pub(crate)` and not fields. Every name a `pub use` exports counts. */
export function publicItems(source) {
  const items = /^\s*pub\s+(?:const\s+fn|async\s+fn|fn|struct|enum|trait|type|const|static|mod)\b/gm;
  const uses = [...source.matchAll(/^\s*pub\s+use\s+([^;]*);/gm)];
  return [...source.matchAll(items)].length + uses.reduce((sum, m) => sum + useNames(m[1]), 0);
}

/**
 * Public items of a crate, from its files as [{ path, source }] (path relative to src). A module the crate
 * root declares without `pub` is not public: its `pub` items count only where the root re-exports them.
 */
export function crateSurface(files) {
  const root = files.find((f) => f.path === "lib.rs" || f.path === "main.rs");
  const hidden = new Set(
    [...(root?.source.matchAll(/^\s*(?:pub\(crate\)\s+)?mod\s+(\w+)\s*;/gm) ?? [])].map((m) => m[1]),
  );
  const inHidden = (path) => hidden.has(path.split("/")[0].replace(/\.rs$/, ""));
  return files.reduce((sum, f) => sum + (inHidden(f.path) ? 0 : publicItems(f.source)), 0);
}

/** The source without its `#[cfg(test)] mod`, which by convention ends the file. */
export function withoutTests(source) {
  const at = source.search(/^#\[cfg\(test\)\]\s*\nmod\s/m);
  return at === -1 ? source : source.slice(0, at);
}

/** The code of a crate: every file without its own test module. */
export function crateSource(files) {
  return files.map(withoutTests).join("\n");
}

/** The `count` numbers a measuring example printed. Throws when there are not exactly that many. */
export function measurement(output, count) {
  const words = output.trim().split(/\s+/).filter((w) => w !== "");
  const numbers = words.map(Number);
  if (numbers.length !== count || numbers.some((n) => !Number.isFinite(n))) {
    throw new Error(`expected ${count} numbers, got ${JSON.stringify(output)}`);
  }
  return numbers;
}

/** One complete run, with nanoseconds from each output's own handoff. */
export function performanceRows(json, count, seconds) {
  const run = JSON.parse(json);
  if (run.seconds !== seconds || !Number.isSafeInteger(run.events) || run.events < 1) {
    throw new Error(`expected ${seconds} measured seconds and a positive event count`);
  }
  if (!Number.isSafeInteger(run.pressSamples) || run.pressSamples < 1 || run.pressSamples > run.events) {
    throw new Error("expected a sample for each scheduled press and release");
  }
  if (!Array.isArray(run.outputs) || run.outputs.length !== count) {
    throw new Error(`expected ${count} measured outputs`);
  }
  const outputs = run.outputs.map(({ output, hz, p99, p95, drops }, index) => {
    if (output !== index + 1 || hz !== (index % 2 === 0 ? 44 : 60)) {
      throw new Error(`unexpected output number or rate at position ${index + 1}`);
    }
    if (![p99, p95, drops].every((n) => Number.isSafeInteger(n) && n >= 0)) {
      throw new Error(`incomplete measurement for output ${output}`);
    }
    const period = Math.floor(1_000_000_000 / hz);
    return { output, hz, p99, p95, drops, pass: p99 < 1_000_000 && p95 < period + 5_000_000 };
  });
  return { seconds, events: run.events, pressSamples: run.pressSamples, outputs };
}

/** Compare the same seeded schedule on each rig without pooling either output's samples. */
export function performanceTable(runs) {
  if (runs.length === 0) return "## Performance\n\nNot measured on CI.\n";
  const { seconds, events, pressSamples } = runs[0][1];
  if (!runs.every(([, run]) => run.seconds === seconds && run.events === events && run.pressSamples === pressSamples)) {
    throw new Error("performance runs must have the same duration, event count and Manual sample count");
  }
  return [
    "## Performance",
    "",
    `Each run took ${seconds} s with ${events} seeded events and a snapshot reader. The p95 covers ${pressSamples} Manual presses and releases per output while group edits run alongside.`,
    "",
    "M2 used direct colour on 10,000 RGB pixels. M3 used the same rig with a Wave and a sequence. M3 at limits used 10,922 RGB pixels, 32 roots, 65,536 reserved visits and 16 outputs.",
    "",
    "| Rig | Output | Hz | Handoff lateness p99 (µs) | Send to handoff p95 (µs) | Drops | Target |",
    "|---|---:|---:|---:|---:|---:|---|",
    ...runs.flatMap(([name, { outputs }]) => outputs.map(({ output, hz, p99, p95, drops, pass }) =>
      `| ${name} | ${output} | ${hz} | ${p99 / 1000} | ${p95 / 1000} | ${drops} | ${pass ? "pass" : "fail"} |`)),
    "",
    "Targets are p99 below 1,000 µs and p95 below the output period plus 5,000 µs.",
    ...(seconds === 60 ? [] : ["Short runs exercise the measurements. The 60 s reference run checks the caps."]),
    "",
  ].join("\n");
}

/** Only the full reference measurement gates the caps. */
export function missedReferenceTarget(runs) {
  return runs.some(([, run]) => run.seconds === 60 && run.outputs.some((row) => !row.pass));
}

/** Places that switch a check off. */
export function escapeHatches(source) {
  const count = (pattern) => [...source.matchAll(pattern)].length;
  return {
    allow: count(/#!?\[allow\(/g),
    expect: count(/#!?\[expect\(/g),
    lintDisable: count(/\b(?:eslint|oxlint)-disable/g),
    unsafe: count(/\bunsafe\s*(?:\{|fn\b|impl\b|extern\b|trait\b)/g),
  };
}

/**
 * The scorecard as Markdown.
 * crates: [{ name, kind, lines, publicItems }]. hatches: from escapeHatches.
 * lateness and limits (the run at the limits the API allows): { p99, worst } in µs, the
 * worst of all outputs, and offBeat: intervals off by more than 2 ms on all outputs.
 * sendToFrame: p95 in µs. Each undefined when not measured, shown as `missing`.
 */
export function scorecard(crates, hatches, lateness, sendToFrame, limits, missing = "not measured", seconds) {
  const total = crates.reduce((sum, c) => sum + c.lines, 0);
  const calculation = crates.filter((c) => CALCULATION_KINDS.includes(c.kind)).reduce((sum, c) => sum + c.lines, 0);
  const share = total === 0 ? 0 : Math.round((100 * calculation) / total);
  const timing = (run) => (run === undefined ? missing : `${run.p99} µs, worst ${run.worst} µs, ${run.offBeat} off by > 2 ms`);
  const press = sendToFrame === undefined ? missing : `${sendToFrame} µs`;
  const perItem = (c) => (c.publicItems === 0 ? "-" : `${Math.round((10 * c.lines) / c.publicItems) / 10}`);
  return [
    "## Scorecard",
    "",
    "| Number | Now | Wants to go |",
    "|---|---|---|",
    `| Lines in calculation crates | ${share}% (${calculation} of ${total}) | up |`,
    `| Escape hatches: allow, expect, lint-disable, unsafe | ${hatches.allow}, ${hatches.expect}, ${hatches.lintDisable}, ${hatches.unsafe} | flat |`,
    `| Lateness per output in ${seconds ?? 60} s (60 and 44 a second), worst p99 | ${timing(lateness)} | flat |`,
    `| At the limits: 16 outputs of 64 universes in ${seconds ?? 30} s (60 and 44 a second in turn), worst p99 | ${timing(limits)} | flat |`,
    `| Send to frame in ${seconds ?? 10} s, p95 (under one frame + 5 ms) | ${press} | flat |`,
    "",
    "| Crate | Kind | Public items | Lines | Lines per public item |",
    "|---|---|---|---|---|",
    ...crates.map((c) => `| ${c.name} | ${c.kind} | ${c.publicItems} | ${c.lines} | ${perItem(c)} |`),
    "",
  ].join("\n");
}

/** Mutation testing as Markdown. counts: cargo-mutants' outcomes.json (caught, missed, timeout, unviable). */
export function mutationReport(crates, { caught, missed, timeout, unviable }) {
  const tested = caught + missed + timeout;
  const score = tested === 0 ? "-" : `${Math.round((100 * caught) / tested)}%`;
  return [
    "## Mutation testing",
    "",
    `Crates: ${crates.join(", ")}`,
    "",
    "| Score | Caught | Missed | Timeout | Unviable |",
    "|---|---|---|---|---|",
    `| ${score} | ${caught} | ${missed} | ${timeout} | ${unviable} |`,
    "",
  ].join("\n");
}
