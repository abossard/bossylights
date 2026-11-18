// The scorecard: numbers that show whether the code follows docs/coding.md, tracked per CI run.
// Calculations only; scripts/print-scorecard.mjs reads the files and runs the engine.

/** Crate kinds from scripts/layers.mjs that hold calculations. */
export const CALCULATION_KINDS = ["core", "calc"];

/** Lines that hold code: not blank, not a comment line. */
export function codeLines(source) {
  return source.split("\n").filter((line) => line.trim() !== "" && !line.trim().startsWith("//")).length;
}

/** Items visible outside the crate: `pub` items, not `pub(crate)` and not fields. */
export function publicItems(source) {
  const items = /^\s*pub\s+(?:const\s+fn|async\s+fn|fn|struct|enum|trait|type|const|static|mod|use)\b/gm;
  return [...source.matchAll(items)].length;
}

/** The source without its `#[cfg(test)] mod`, which by convention ends the file. */
export function withoutTests(source) {
  const at = source.search(/^#\[cfg\(test\)\]\s*\nmod\s/m);
  return at === -1 ? source : source.slice(0, at);
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
 * lateness: { p99, worst } in µs. sendToFrame: p95 in µs. Each undefined when not measured.
 */
export function scorecard(crates, hatches, lateness, sendToFrame) {
  const total = crates.reduce((sum, c) => sum + c.lines, 0);
  const calculation = crates.filter((c) => CALCULATION_KINDS.includes(c.kind)).reduce((sum, c) => sum + c.lines, 0);
  const share = total === 0 ? 0 : Math.round((100 * calculation) / total);
  const late = lateness === undefined ? "not measured" : `${lateness.p99} µs, worst ${lateness.worst} µs`;
  const press = sendToFrame === undefined ? "not measured" : `${sendToFrame} µs`;
  const perItem = (c) => (c.publicItems === 0 ? "-" : `${Math.round((10 * c.lines) / c.publicItems) / 10}`);
  return [
    "## Scorecard",
    "",
    "| Number | Now | Wants to go |",
    "|---|---|---|",
    `| Lines in calculation crates | ${share}% (${calculation} of ${total}) | up |`,
    `| Escape hatches: allow, expect, lint-disable, unsafe | ${hatches.allow}, ${hatches.expect}, ${hatches.lintDisable}, ${hatches.unsafe} | flat |`,
    `| Tick lateness in 60 s, p99 | ${late} | flat |`,
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
