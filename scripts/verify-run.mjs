// `npm run verify -- <commit> [--mutants]`: the check of what the commit can reach, in the worker's own worktree.
// The worktree must be clean and at the commit: nothing uncommitted, nothing untracked. Its build folder stays warm.
// What it leaves to CI it names; CI on main is the clean copy and checks everything.
// A passed run with mutants records its git tree in the shared git folder: `npm run land` trusts it for the same tree.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { logPath, mutantsReport, parseArgs, reachOf, readsOf, recordOf, recordPath, stepsOf, testSummary } from "./verify.mjs";
import { running, stopLeftovers } from "./verify-leftovers.mjs";

const run = (command, args, cwd, options = {}) =>
  spawnSync(command, args, { cwd, encoding: "utf8", maxBuffer: 1 << 28, ...options });
const env = { ...process.env };
delete env.CARGO_TARGET_DIR;
delete env.RUSTUP_TOOLCHAIN; // the pin in the commit's rust-toolchain.toml decides
delete env.UPDATE_TS; // check the committed API types, never write them
const stop = (message) => {
  console.error(`verify: ${message}`);
  process.exit(2);
};

let args;
try {
  args = parseArgs(process.argv.slice(2));
} catch (error) {
  stop(error.message);
}
const git = (...a) => run("git", a, process.cwd());
const sha = git("rev-parse", "--verify", `${args.commit}^{commit}`);
if (sha.status !== 0) stop(`no such commit: ${args.commit}`);
const commit = sha.stdout.trim();
const dir = git("rev-parse", "--show-toplevel").stdout.trim();
if (git("rev-parse", "HEAD").stdout.trim() !== commit) stop(`${dir} is not at ${commit.slice(0, 7)}: check out the commit you verify`);
if (git("status", "--porcelain").stdout !== "") stop(`${dir} has uncommitted or untracked files: commit them or remove them`);
const tree = git("rev-parse", `${commit}^{tree}`).stdout.trim();
// What the branch adds to main: from where it left origin/main. A commit on main itself: its own diff.
const forkPoint = git("merge-base", "refs/remotes/origin/main", commit).stdout.trim();
const base = forkPoint && forkPoint !== commit ? forkPoint : `${commit}^`;
const record = recordPath(git("rev-parse", "--path-format=absolute", "--git-common-dir").stdout.trim(), tree);
rmSync(record, { force: true }); // the last run of a tree decides: a run that ends early leaves none
const logDir = mkdtempSync(join(tmpdir(), "bossylights-verify-logs-")); // logs, the diff and mutants' output: never in the worktree
let failed = false;
try {
  const lock = createHash("sha256").update(readFileSync(join(dir, "package-lock.json"))).digest("hex");
  const stamp = join(dir, "node_modules", ".verify-lock");
  if (!existsSync(stamp) || readFileSync(stamp, "utf8") !== lock) {
    const install = run("npm", ["ci"], dir, { env });
    if (install.status !== 0) throw new Error(`npm ci failed: ${install.stderr.split("\n").find(Boolean)}`);
    writeFileSync(stamp, lock);
  }
  const files = git("diff", "--name-only", base, commit).stdout.split("\n").filter(Boolean);
  const metadata = run("cargo", ["metadata", "-q", "--format-version", "1", "--no-deps", "--locked"], dir, { env });
  if (metadata.status !== 0) throw new Error(`cargo metadata failed: ${metadata.stderr.split("\n").find(Boolean)}`);
  const crates = JSON.parse(metadata.stdout).packages.map((p) => {
    const crateDir = dirname(p.manifest_path).slice(dir.length + 1);
    const sources = git("ls-files", "--", `${crateDir}/*.rs`).stdout.split("\n").filter(Boolean)
      .map((path) => ({ path, text: readFileSync(join(dir, path), "utf8") }));
    return { name: p.name, dir: crateDir, deps: p.dependencies.filter((d) => d.path).map((d) => d.name), reads: readsOf(crateDir, sources) };
  });
  const reach = reachOf(files, crates);
  const plan = stepsOf(reach, JSON.parse(readFileSync(join(dir, "package.json"), "utf8")).scripts, crates.map((c) => c.name));
  console.log(reach.everything ? `verify: ${reach.everything} reaches everything` : `verify: reaches ${reach.crates.join(", ") || "no crate"}${reach.ui ? ", the UI" : ""}`);

  const results = plan.steps.map(({ name, command }) => {
    const before = running();
    const r = run(command[0], command.slice(1), dir, { env });
    const stopped = stopLeftovers(dir, before);
    const output = [`${r.stdout}\n${r.stderr}${r.error ? `\nerror: ${r.error.message}` : ""}`, ...stopped].join("\n");
    const log = logPath(logDir, name);
    writeFileSync(log, output);
    const status = r.status ?? 1; // killed by a signal or not started: status is null
    return { suite: name, ok: status === 0, status, log, output: r.signal ? `${output}\nsignal: ${r.signal}` : output };
  });
  failed = results.some((r) => !r.ok);
  console.log(testSummary(commit.slice(0, 7), results));
  console.log(`left to CI: ${plan.left.join("; ") || "nothing"}`);

  let mutants = null;
  if (args.mutants) {
    const diffFile = join(logDir, "verify.diff");
    writeFileSync(diffFile, git("diff", base, commit).stdout);
    const out = join(logDir, "mutants");
    const m = run("cargo", ["mutants", "--no-shuffle", "--in-diff", diffFile, "--output", out], dir, { env });
    const NO_MUTANTS = 0; // exit codes of cargo-mutants: 0 all caught, 2 missed, 3 timeout
    if (![NO_MUTANTS, 2, 3].includes(m.status)) throw new Error(`cargo mutants failed (exit ${m.status}): ${m.stderr.split("\n").filter(Boolean).at(-1)}`);
    const file = join(out, "mutants.out", "outcomes.json");
    const outcomes = existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : null;
    mutants = mutantsReport({ status: m.status, outcomes, stderr: m.stderr });
    console.log(mutants.text);
    if (!mutants.ok) failed = true;
  }
  const passed = recordOf({ commit, tree, base, results, left: plan.left, mutants });
  if (passed.record) {
    mkdirSync(dirname(record), { recursive: true });
    writeFileSync(record, `${JSON.stringify(passed.record, null, 2)}\n`);
    console.log(`verify: recorded tree ${tree.slice(0, 7)} in ${record}: land trusts it`);
  } else {
    console.log(`verify: no record (${passed.why}): land refuses this tree`);
  }
} catch (error) {
  console.error(`verify: ${error.message}`);
  failed = true;
}
process.exit(failed ? 1 : 0);
