// `npm run verify -- <commit> [--mutants]`: a clean copy of the commit, outside the shared checkout.
// `git archive` reads the object store only: the working tree, index and stash stay as they are.
// The copy is built with the default target folder (CARGO_TARGET_DIR unset) and removed at the end.
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { mutantsReport, parseArgs, suitesOf, testSummary } from "./verify.mjs";

const run = (command, args, cwd, options = {}) =>
  spawnSync(command, args, { cwd, encoding: "utf8", maxBuffer: 1 << 28, ...options });
const env = { ...process.env };
delete env.CARGO_TARGET_DIR;

let args;
try {
  args = parseArgs(process.argv.slice(2));
} catch (error) {
  console.error(error.message);
  process.exit(2);
}
const sha = run("git", ["rev-parse", "--verify", `${args.commit}^{commit}`], process.cwd());
if (sha.status !== 0) {
  console.error(`verify: no such commit: ${args.commit}`);
  process.exit(2);
}
const commit = sha.stdout.trim();
const dir = mkdtempSync(join(tmpdir(), "bossylights-verify-"));
let failed = false;
try {
  const archive = spawnSync("sh", ["-c", `git archive ${commit} | tar -x -C ${dir}`], { stdio: "inherit" });
  if (archive.status !== 0) throw new Error("git archive failed");
  const install = run("npm", ["ci"], dir, { env });
  if (install.status !== 0) throw new Error(`npm ci failed: ${install.stderr.split("\n").find(Boolean)}`);

  const suites = suitesOf(JSON.parse(readFileSync(join(dir, "package.json"), "utf8")).scripts.test);
  const results = suites.map((suite) => {
    const r = run("npm", ["run", suite], dir, { env });
    return { suite, ok: r.status === 0, output: `${r.stdout}\n${r.stderr}` };
  });
  failed = results.some((r) => !r.ok);
  console.log(testSummary(commit.slice(0, 7), results));

  if (args.mutants) {
    const diff = run("git", ["diff", `${commit}^`, commit], process.cwd()).stdout;
    const diffFile = join(dir, "verify.diff");
    writeFileSync(diffFile, diff);
    const m = run("cargo", ["mutants", "--no-shuffle", "--in-diff", diffFile], dir, { env });
    const NO_MUTANTS = 0; // exit codes of cargo-mutants: 0 all caught, 2 missed, 3 timeout
    if (![NO_MUTANTS, 2, 3].includes(m.status)) throw new Error(`cargo mutants failed (exit ${m.status}): ${m.stderr.split("\n").filter(Boolean).at(-1)}`);
    const file = join(dir, "mutants.out", "outcomes.json");
    const outcomes = existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : null;
    const report = mutantsReport({ status: m.status, outcomes, stderr: m.stderr });
    console.log(report.text);
    if (!report.ok) failed = true;
  }
} catch (error) {
  console.error(`verify: ${error.message}`);
  failed = true;
} finally {
  rmSync(dir, { recursive: true, force: true });
}
process.exit(failed ? 1 : 0);
