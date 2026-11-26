// `npm run verify -- <commit> [--mutants]`: the check of what the commit can reach, in the worker's own worktree.
// The worktree must be clean and at the commit: nothing uncommitted, nothing untracked. Its build folder stays warm.
// What it leaves to CI it names; CI on main is the clean copy and checks everything.
// --mutants: the mutants of the diff run on GitHub's machine (`.github/workflows/mutants-diff.yml`, #340) while the
// suites run here, so the commit must be pushed. The Mac runs no mutants.
// A passed run with mutants records its git tree in the shared git folder: `npm run land` trusts it for the same tree.
import { spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { logPath, mutantsReport, parseArgs, reachOf, readsOf, recordOf, recordPath, recordStep, runOf, stepsOf, testSummary } from "./verify.mjs";
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
const gh = (...a) => run("gh", a, process.cwd(), { env });
const firstLine = (r) => `${r.stderr}\n${r.stdout}`.split("\n").find(Boolean) ?? `exit ${r.status}`;
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
// The newest run of a tree decides its record: it names itself the newest (the name stays: a run that starts while
// this one ends keeps its claim), and a run that ends early leaves none.
const newest = join(dirname(record), `${tree}.newest`);
const me = randomUUID();
mkdirSync(dirname(record), { recursive: true });
writeFileSync(newest, me);
rmSync(record, { force: true });
const logDir = mkdtempSync(join(tmpdir(), "bossylights-verify-logs-")); // logs, the diff and mutants' output: never in the worktree
let failed = false;
let passed = { record: null, why: "the run ended early" };
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
  let job = null; // the mutants run on GitHub, started before the suites so both run at once
  if (args.mutants) {
    if (gh("api", `repos/{owner}/{repo}/commits/${commit}`, "--silent").status !== 0) throw new Error(`${commit.slice(0, 7)} is not on GitHub: push it, its mutants run there`);
    const fork = git("rev-parse", base).stdout.trim();
    // main's version of the workflow: a branch cannot change how it is checked
    const started = gh("workflow", "run", "mutants-diff.yml", "--ref", "main", "-f", `commit=${commit}`, "-f", `base=${fork}`);
    if (started.status !== 0) throw new Error(`gh workflow run failed: ${firstLine(started)}`);
    job = runOf(`${started.stdout}\n${started.stderr}`);
    if (!job) throw new Error(`gh workflow run printed no run: ${firstLine(started)}`);
    console.log(`verify: mutants of ${fork.slice(0, 7)}..${commit.slice(0, 7)} run on GitHub: ${job.url}`);
  }
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
  if (job) {
    gh("run", "watch", job.id, "--interval", "30"); // its end, not its verdict: the exit file says how cargo-mutants ended
    const out = join(logDir, "mutants");
    const got = gh("run", "download", job.id, "--name", "mutants", "--dir", out).status === 0 && existsSync(join(out, "exit"));
    const file = join(out, "mutants.out", "outcomes.json");
    const status = got ? Number(readFileSync(join(out, "exit"), "utf8").trim()) : null;
    mutants = mutantsReport({ status, outcomes: got && existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : null, run: job.url });
    console.log(mutants.text);
    if (!mutants.ok) failed = true;
  }
  passed = recordOf({ commit, tree, base, results, left: plan.left, mutants });
} catch (error) {
  console.error(`verify: ${error.message}`);
  failed = true;
}
if (args.mutants && !passed.record) failed = true; // a missed mutant fails the run, as land refuses its tree
const step = recordStep({ me, newest: existsSync(newest) ? readFileSync(newest, "utf8") : null, record: passed.record });
if (step === "leave") console.log(`verify: a newer run of tree ${tree.slice(0, 7)} started meanwhile: its end decides the record`);
else {
  if (step === "write") writeFileSync(record, `${JSON.stringify(passed.record, null, 2)}\n`);
  else rmSync(record, { force: true });
  console.log(step === "write" ? `verify: recorded tree ${tree.slice(0, 7)} in ${record}: land trusts it` : `verify: no record (${passed.why}): land refuses this tree`);
}
process.exit(failed ? 1 : 0);
