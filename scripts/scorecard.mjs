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
export function performanceRows(json, count, seconds, workload) {
  const run = JSON.parse(json);
  if (run.workload !== workload) throw new Error(`expected workload ${workload}, got ${run.workload}`);
  if (run.seconds !== seconds || !Number.isSafeInteger(run.events) || run.events < 1) {
    throw new Error(`expected ${seconds} measured seconds and a positive event count`);
  }
  if (!Number.isSafeInteger(run.pressSamples) || run.pressSamples < 1 || run.pressSamples > run.events) {
    throw new Error("expected a sample for each scheduled press and release");
  }
  if (workload === "burst") {
    if (!Number.isSafeInteger(run.bursts) || run.bursts < 1) throw new Error("expected a positive measured burst count");
    if (run.pressSamples !== 226 * run.bursts) throw new Error("expected 226 raw channel sends per burst");
    if (run.events !== run.pressSamples + run.bursts) throw new Error("expected one plan swap batch per burst alongside the raw channel sends");
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
  return { workload, seconds, events: run.events, pressSamples: run.pressSamples, bursts: run.bursts, outputs };
}

/** Compare the normal show and the biggest show on the same seeded schedule, without pooling any output's samples. */
export function performanceTable(runs) {
  if (runs.length === 0) return "## Performance\n\nNot measured on CI.\n";
  const seconds = runs[0][1].seconds;
  const matched = runs.map(([, run]) => run).filter((run) => run.workload !== "burst");
  const burst = runs.find(([, run]) => run.workload === "burst")?.[1];
  if (!runs.every(([, run]) => run.seconds === seconds)) throw new Error("performance run durations differ");
  if (!matched.every((run) => run.events === matched[0].events && run.pressSamples === matched[0].pressSamples)) {
    throw new Error("matched performance runs must have the same event count and Manual sample count");
  }
  return [
    "## Performance",
    "",
    ...(matched.length ? [
      `Each run took ${seconds} s. The matched schedule used ${matched[0].events} seeded events and a snapshot reader. Its p95 covers ${matched[0].pressSamples} Manual presses and releases per output while group edits run alongside.`,
      "",
    ] : []),
    ...(burst ? [
      `The biggest show plus burst applied ${burst.bursts} bursts of 256 FIFO commands. Each burst landed in one tick with 30 plan swaps and 226 raw channel sends. Its p95 covers ${burst.pressSamples} raw channel sends per output. Each output contributes one lateness sample at its first accepted handoff carrying new burst commands.`,
      "",
    ] : []),
    "The normal show ran a Wave and a sequence on 10,000 RGB pixels and 2 outputs. The biggest show used 10,922 RGB pixels, 32 roots, 65,536 reserved visits and 16 outputs.",
    "",
    "| Rig | Output | Hz | Handoff lateness p99 (µs) | Send to handoff p95 (µs) | Drops | Target |",
    "|---|---:|---:|---:|---:|---:|---|",
    ...runs.flatMap(([name, { outputs }]) => outputs.map(({ output, hz, p99, p95, drops, pass }) =>
      `| ${name} | ${output} | ${hz} | ${p99 / 1000} | ${p95 / 1000} | ${drops} | ${pass ? "pass" : "fail"} |`)),
    "",
    "Targets are p99 below 1,000 µs and p95 below the output period plus 5,000 µs.",
    ...(seconds >= 60 ? [] : ["Short runs exercise the measurements. Runs of 60 s or longer check the caps."]),
    "",
  ].join("\n");
}

/** Measured runs of 60 seconds or longer gate the caps. */
export function missedReferenceTarget(runs) {
  return runs.some(([, run]) => run.seconds >= 60 && run.outputs.some((row) => !row.pass));
}

/** Each output's thread wakes once at its frame and once every 200 µs from 3 ms before it (`crates/engine/src/lane.rs`, LOOK and LEAD). */
const OUTPUT_WAKES_PER_FRAME = 1 + 3000 / 200;
const isCount = (n) => Number.isSafeInteger(n) && n >= 0;

/**
 * One run of send_to_frame as the load tables read it: the rates of its outputs, the worst
 * lateness p99 of them, a press line per rate, and the load over `wall`. Throws when a part is missing.
 */
export function loadRow(json, workload) {
  const run = JSON.parse(json);
  if (run.workload !== workload) throw new Error(`expected workload ${workload}, got ${run.workload}`);
  const rates = (run.outputs ?? []).map((output) => output.hz);
  const distinct = [...new Set(rates)].sort((a, b) => a - b);
  const press = run.press ?? [];
  if (press.length !== distinct.length || press.some((line, i) => line.hz !== distinct[i])) {
    throw new Error("expected a press line for each rate of the outputs");
  }
  for (const line of press) {
    if (!(line.samples >= 1 && [line.samples, line.p50, line.p99, line.worst].every(isCount))) {
      throw new Error(`incomplete press line at ${line.hz}`);
    }
  }
  const { wall, cpu, wakes } = run.load ?? {};
  if (!(isCount(wall) && wall > 0)) throw new Error("expected the wall time of the load");
  if (cpu !== null && ![cpu?.tick, cpu?.outputs, cpu?.engine].every(isCount)) {
    throw new Error("expected the CPU of the tick, the output threads and the engine, or null");
  }
  if (![wakes?.tick, wakes?.outputs].every(isCount)) throw new Error("expected the wakes of the tick and the output threads");
  const lateness = Math.max(...run.outputs.map((output) => output.p99));
  return { workload, rates, lateness, press, wall, cpu, wakes };
}

/** The press to frame by rate, then the load on the Mac, for rows of [name, loadRow]. */
export function loadTables(rows) {
  if (rows.length === 0) return "## Press to frame and load\n\nNot measured on CI.\n";
  const ms = (ns) => (ns / 1e6).toFixed(1);
  const count = (n) => Math.round(n).toLocaleString("en-US");
  const period = (hz) => Math.floor(1e9 / hz);
  const share = (row, ns) => (row.cpu === null ? "not measured" : ((100 * ns) / row.wall).toFixed(1));
  const perSecond = (row, n) => count((n * 1e9) / row.wall);
  return [
    "## Press to frame and load",
    "",
    "How long a press takes to reach the lights, and what one Bossylights asks of the Mac. A release build that writes the scorecard's timing stream, which the product does not.",
    "",
    "Press to frame: from the moment the engine is handed the command to the moment an output's thread hands the frame that carries it to its device. It leaves out the trip from the screen to the engine, and all after the handoff: the adapter, about 23 ms for a whole DMX frame on the wire, the light itself. A press at a random moment waits for the next frame, so the median is near half a period.",
    "",
    "| Load | Hz | Presses | Median (ms) | Expected median, half a period (ms) | p99 (ms) | Worst (ms) | Worst stays under, a period + 5 ms (ms) |",
    "|---|---:|---:|---:|---:|---:|---:|---:|",
    ...rows.flatMap(([name, row]) => row.press.map((line) =>
      `| ${name} | ${line.hz} | ${line.samples} | ${ms(line.p50)} | ${ms(period(line.hz) / 2)} | ${ms(line.p99)} | ${ms(line.worst)} | ${ms(period(line.hz) + 5_000_000)} |`)),
    "",
    "CPU as the system counts it, in % of one core. The engine: every thread it starts. Wakes expected: the tick once a frame of each rate, at most; each output's thread about 16 times a frame (`crates/engine/src/lane.rs`).",
    "",
    "| Load | Outputs | Lateness p99 (µs) | CPU tick | CPU output threads | CPU engine | Tick wakes a second | Expected, at most | Output thread wakes a second | Expected, about |",
    "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ...rows.map(([name, row]) => {
      const rates = [...new Set(row.rates)].reduce((sum, hz) => sum + hz, 0);
      const frames = row.rates.reduce((sum, hz) => sum + hz, 0);
      return `| ${name} | ${row.rates.length} | ${count(row.lateness / 1000)} | ${share(row, row.cpu?.tick)} | ${share(row, row.cpu?.outputs)} | ${share(row, row.cpu?.engine)} | ${perSecond(row, row.wakes.tick)} | ${count(rates)} | ${perSecond(row, row.wakes.outputs)} | ${count(frames * OUTPUT_WAKES_PER_FRAME)} |`;
    }),
    "",
  ].join("\n");
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
