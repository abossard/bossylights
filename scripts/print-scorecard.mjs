// Prints the scorecard as Markdown. With GITHUB_STEP_SUMMARY set, also writes it there.
// Runs the engine for 60 seconds with two outputs to measure lateness per output, 30 seconds at the
// limits the API allows (16 outputs of 64 universes), then 10 seconds to time sends.
import { execFileSync } from "node:child_process";
import { appendFileSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { CRATES } from "./layers.mjs";
import { codeLines, crateSource, crateSurface, escapeHatches, measurement, scorecard, withoutTests } from "./scorecard.mjs";

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
  execFileSync("cargo", ["metadata", "-q", "--format-version", "1", "--no-deps", "--locked"], {
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
  execFileSync("cargo", ["run", "-q", "--release", "-p", "engine-server", "--example", name, "--", ...args], {
    encoding: "utf8",
  });

const lateness = (...args) => {
  const [p99, worst, offBeat] = measurement(example("lateness", ...args), 3);
  return { p99, worst, offBeat };
};

const sendToFrame = () => measurement(example("send_to_frame"), 1)[0];

// CI judges what is sent, never how fast: on a runner the timing is left out.
const measured = process.env.CI === undefined;
const markdown = scorecard(
  crates,
  escapeHatches(everything),
  measured ? lateness() : undefined,
  measured ? sendToFrame() : undefined,
  measured ? lateness("limits") : undefined,
  measured ? "not measured" : "not measured on CI",
);
console.log(markdown);
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, markdown);
