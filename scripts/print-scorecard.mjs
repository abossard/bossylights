// Prints the scorecard as Markdown. With GITHUB_STEP_SUMMARY set, also writes it there.
// Runs the engine for five seconds to measure tick lateness.
import { execFileSync } from "node:child_process";
import { appendFileSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { CRATES } from "./layers.mjs";
import { codeLines, escapeHatches, publicItems, scorecard, withoutTests } from "./scorecard.mjs";

const read = (dir, extension) =>
  readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile() && extension.test(entry.name))
    .map((entry) => readFileSync(join(entry.parentPath, entry.name), "utf8"))
    .join("\n");

const packages = JSON.parse(
  execFileSync("cargo", ["metadata", "-q", "--format-version", "1", "--no-deps", "--locked"], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  }),
).packages;

const crates = packages.map((p) => {
  const source = withoutTests(read(join(dirname(p.manifest_path), "src"), /\.rs$/));
  return { name: p.name, kind: CRATES[p.name]?.kind ?? "action", lines: codeLines(source), publicItems: publicItems(source) };
});

const everything = [
  ...packages.map((p) => read(dirname(p.manifest_path), /\.rs$/)),
  read("ui/src", /\.(ts|tsx|mjs)$/),
].join("\n");

const lateness = () => {
  try {
    const micros = execFileSync("cargo", ["run", "-q", "-p", "engine-server", "--example", "lateness"], { encoding: "utf8" });
    return Number(micros.trim());
  } catch {
    return undefined; // shown as "not measured"
  }
};

const markdown = scorecard(crates, escapeHatches(everything), lateness());
console.log(markdown);
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, markdown);
