// Prints the scorecard as Markdown. With GITHUB_STEP_SUMMARY set, also writes it there.
// Keep the original engine timings and compare the 60 s performances on this machine.
// npm run scorecard -- --seconds 5 runs the same workloads as a short smoke check.
import { execFileSync } from "node:child_process";
import { appendFileSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { arch, cpus, release, type } from "node:os";
import { CRATES } from "./layers.mjs";
import { agreementTable, codeLines, crateSource, crateSurface, escapeHatches, loadRow, loadTables, measurement, performanceRows, performanceTable, scorecard, withoutTests } from "./scorecard.mjs";

const args = process.argv.slice(2);
const seconds = args.length === 0 ? undefined : Number(args[1]);
if (args.length > 0 && (args.length !== 2 || args[0] !== "--seconds" || !Number.isSafeInteger(seconds) || seconds < 1)) {
  throw new Error("usage: npm run scorecard -- [--seconds positive-integer]");
}

const entries = (dir, extension) =>
  readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile() && extension.test(entry.name))
    .map((entry) => ({
      path: relative(dir, join(entry.parentPath, entry.name)),
      source: readFileSync(join(entry.parentPath, entry.name), "utf8"),
    }));
const files = (dir, extension) => entries(dir, extension).map((entry) => entry.source);
const read = (dir, extension) => files(dir, extension).join("\n");

const packages = JSON.parse(
  execFileSync("cargo", ["metadata", "--offline", "-q", "--format-version", "1", "--no-deps", "--locked"], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  }),
).packages;

const crates = packages.map((p) => {
  const sources = entries(join(dirname(p.manifest_path), "src"), /\.rs$/).map((e) => ({ ...e, source: withoutTests(e.source) }));
  const source = crateSource(sources.map((e) => e.source));
  return { name: p.name, kind: CRATES[p.name]?.kind ?? "action", lines: codeLines(source), publicItems: crateSurface(sources) };
});

const everything = [
  ...packages.map((p) => read(dirname(p.manifest_path), /\.rs$/)),
  read("ui/src", /\.(ts|tsx|mjs)$/),
].join("\n");

// A measurement that fails fails the script. "Not measured" is only for CI.
const example = (name, ...args) =>
  execFileSync("cargo", ["run", "--offline", "-j", "4", "-q", "--release", "-p", "engine-server", "--example", name, "--", ...args], {
    encoding: "utf8",
  });

const lateness = (...args) => {
  const [p99, worst, offBeat] = measurement(example("lateness", ...args), 3);
  return { p99, worst, offBeat };
};

// CI judges what is sent, never how fast: on a runner the timing is left out.
const measured = process.env.CI === undefined;
const durationArg = seconds === undefined ? [] : [String(seconds)];
const summary = scorecard(
  crates,
  escapeHatches(everything),
  measured ? lateness("normal", ...durationArg) : undefined,
  measured ? measurement(example("send_to_frame", "baseline", ...durationArg), 1)[0] : undefined,
  measured ? lateness("limits", ...durationArg) : undefined,
  measured ? "not measured" : "not measured on CI",
  seconds,
);
const duration = seconds ?? 60;
const rigs = { show: ["normal show", 2], biggest: ["biggest show", 16], burst: ["biggest show plus burst", 16] };
const reports = measured ? Object.fromEntries(Object.keys(rigs).map((workload) => [workload, example("send_to_frame", workload, String(duration))])) : {};
const runs = measured ? Object.entries(rigs).map(([workload, [name, outputs]]) => [
  name, performanceRows(reports[workload], outputs, duration, workload),
]) : [];
// CPU and wakes settle in 10 s. A 10 s run ends 3 ms after its last press: 15 s leaves room.
const idle = measured ? example("send_to_frame", "idle", String(Math.min(duration, 15))) : undefined;
const loads = measured ? [
  ["full load", loadRow(reports.biggest, "biggest")],
  ["example rig", loadRow(reports.show, "show")],
  ["idle", loadRow(idle, "idle")],
] : [];
// The delay each output reports, checked from outside: a disagreement stops the scorecard.
const windowSeconds = String(Math.max(duration, 4));
const agreements = measured ? [["full load", "biggest"], ["example rig", "show"], ["idle", "idle"]].map(([name, rig]) => [
  name, JSON.parse(example("delay_agreement", rig, windowSeconds)),
]) : [];
const host = measured ? `\nMeasured on ${type()} ${release()}, ${arch()}, ${cpus()[0]?.model ?? "unknown CPU"}.\n` : "";
const markdown = `${summary}\n${performanceTable(runs)}\n${loadTables(loads)}\n${agreementTable(agreements)}${host}`;
console.log(markdown);
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, markdown);
