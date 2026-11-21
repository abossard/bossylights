// `npm run peer -- <codex|copilot|both> <review|build|research|spec> <commit> <task-file> [--model ..] [--effort ..] [--name ..] [--out dir]`
// Runs Codex, Copilot or both in parallel, each in its own scratch copy of the commit (docs/agents/peers.md).
// The shared checkout is never touched; nothing is committed or pushed. Each run leaves a folder;
// `<base>/latest` names the folders of the last call, one per line.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runCli } from "./peer-cli.mjs";
import { parseArgs, patchFileOf, promptOf, report, runName, settingsOf, specFileOf, stampOf } from "./peer.mjs";

const git = (args, cwd) => spawnSync("git", args, { cwd, encoding: "utf8", maxBuffer: 1 << 29 });

let args;
try {
  args = parseArgs(process.argv.slice(2));
} catch (error) {
  console.error(error.message);
  process.exit(2);
}
const repo = process.cwd();
const sha = git(["rev-parse", "--verify", `${args.commit}^{commit}`], repo).stdout.trim();
if (!sha || !existsSync(args.task)) {
  console.error(`peer: no such commit or task file: ${args.commit} ${args.task}`);
  process.exit(2);
}
const brief = readFileSync(join(repo, "docs/agents/peer-brief.md"), "utf8");
const task = readFileSync(args.task, "utf8");
const base = args.out ?? join(tmpdir(), "bossylights-peer");
const date = new Date();

async function runOne(tool) {
  const settings = settingsOf(tool, args);
  const dir = join(base, runName({ name: args.name, mode: args.mode, sha, tool, stamp: stampOf(date) }));
  const copy = join(dir, "copy");
  const outDir = join(dir, "out");
  mkdirSync(outDir, { recursive: true });
  const added = git(["worktree", "add", "--detach", copy, sha], repo);
  if (added.status !== 0) return { status: 1, dir, text: `${tool}: no scratch copy: ${added.stderr.trim()}` };
  console.log(`${tool} ${settings.mode}: running in ${dir}`);

  const prompt = promptOf(brief, task, settings.mode, outDir);
  const run = await runCli(settings, { cwd: copy, dir, outDir, prompt, env: { ...process.env, CARGO_TARGET_DIR: join(dir, "target"), COPILOT_TASK_WAIT_TIMEOUT_SECONDS: "3600" } });

  const patchFile = patchFileOf(settings.mode);
  let diff = "";
  if (patchFile) {
    git(["add", "-A"], copy);
    diff = git(["diff", "--cached", "--binary"], copy).stdout;
    if (diff) writeFileSync(join(dir, patchFile), diff);
  }
  git(["worktree", "remove", "--force", copy], repo);
  rmSync(join(dir, "target"), { recursive: true, force: true });

  const hasSpec = existsSync(specFileOf(outDir));
  const outFiles = readdirSync(outDir, { recursive: true, withFileTypes: true }).filter((e) => e.isFile()).length;
  const text = report({ settings, sha, seconds: run.seconds, status: run.status, stats: run.stats, dir, hasPatch: Boolean(diff), hasSpec, outFiles, date, name: args.name });
  writeFileSync(join(dir, "report.md"), `${text}\n`);
  const specMissing = settings.mode === "spec" && !hasSpec;
  return { status: run.status === 0 && specMissing ? 1 : run.status, dir, text };
}

const results = await Promise.all(args.tools.map(runOne));
writeFileSync(join(base, "latest"), `${results.map((r) => r.dir).join("\n")}\n`);
console.log(`\n${results.map((r) => r.text).join("\n\n")}`);
process.exit(results.every((r) => r.status === 0) ? 0 : 1);
