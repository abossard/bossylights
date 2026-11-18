// Mutation testing on the calculation crates (docs/coding.md, rule 8). Needs cargo-mutants.
// Prints the score; with GITHUB_STEP_SUMMARY set, also writes it there. Missed mutants are a
// number to watch, not a failure; the run fails only when cargo-mutants itself fails.
import { spawnSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import { CRATES } from "./layers.mjs";
import { CALCULATION_KINDS, mutationReport } from "./scorecard.mjs";

const crates = Object.keys(CRATES).filter((name) => CALCULATION_KINDS.includes(CRATES[name].kind));
const run = spawnSync("cargo", ["mutants", "--no-shuffle", ...crates.flatMap((c) => ["--package", c])], {
  stdio: "inherit",
});
const MISSED = 2;
const TIMEOUT = 3;
if (![0, MISSED, TIMEOUT].includes(run.status)) {
  console.error(`mutants: cargo mutants failed (exit ${run.status ?? run.error})`);
  process.exit(1);
}

const markdown = mutationReport(crates, JSON.parse(readFileSync("mutants.out/outcomes.json", "utf8")));
console.log(markdown);
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, markdown);
