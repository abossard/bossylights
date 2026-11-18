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
 * lateness: { p99, worst } in µs, the worst of all outputs, and offBeat: intervals off by
 * more than 2 ms on all outputs. sendToFrame: p95 in µs. Each undefined when not measured, shown as `missing`.
 */
export function scorecard(crates, hatches, lateness, sendToFrame, missing = "not measured") {
  const total = crates.reduce((sum, c) => sum + c.lines, 0);
  const calculation = crates.filter((c) => CALCULATION_KINDS.includes(c.kind)).reduce((sum, c) => sum + c.lines, 0);
  const share = total === 0 ? 0 : Math.round((100 * calculation) / total);
  const late = lateness === undefined ? missing : `${lateness.p99} µs, worst ${lateness.worst} µs, ${lateness.offBeat} off by > 2 ms`;
  const press = sendToFrame === undefined ? missing : `${sendToFrame} µs`;
  const perItem = (c) => (c.publicItems === 0 ? "-" : `${Math.round((10 * c.lines) / c.publicItems) / 10}`);
  return [
    "## Scorecard",
    "",
    "| Number | Now | Wants to go |",
    "|---|---|---|",
    `| Lines in calculation crates | ${share}% (${calculation} of ${total}) | up |`,
    `| Escape hatches: allow, expect, lint-disable, unsafe | ${hatches.allow}, ${hatches.expect}, ${hatches.lintDisable}, ${hatches.unsafe} | flat |`,
    `| Lateness per output in 60 s (60 and 44 a second), worst p99 | ${late} | flat |`,
    `| Send to frame in 10 s, p95 (under one frame + 5 ms) | ${press} | flat |`,
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
