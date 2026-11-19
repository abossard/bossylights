// `npm run codex -- <review|build> <commit> <task-file> [--model astra] [--effort ultra] [--name label]`
// Runs the Codex CLI on a scratch copy of the commit (docs/agents/codex.md). The shared
// checkout is never touched. A build leaves `change.patch`; nothing is committed or pushed.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execArgs, parseArgs, promptOf, report, summarize } from "./codex.mjs";

const run = (command, args, cwd, options = {}) =>
  spawnSync(command, args, { cwd, encoding: "utf8", maxBuffer: 1 << 29, ...options });

let args;
try {
  args = parseArgs(process.argv.slice(2));
} catch (error) {
  console.error(error.message);
  process.exit(2);
}
const sha = run("git", ["rev-parse", "--verify", `${args.commit}^{commit}`], process.cwd()).stdout.trim();
if (!sha || !existsSync(args.task)) {
  console.error(`codex: no such commit or task file: ${args.commit} ${args.task}`);
  process.exit(2);
}

const dir = join(tmpdir(), "bossylights-codex", `${args.name ?? `${args.mode}-${sha.slice(0, 7)}`}-${Date.now()}`);
const copy = join(dir, "copy");
mkdirSync(dir, { recursive: true });
run("git", ["worktree", "add", "--detach", copy, sha], process.cwd());

const brief = readFileSync("docs/agents/codex-brief.md", "utf8");
const prompt = promptOf(brief, readFileSync(args.task, "utf8"), args.mode);
const lastMessage = join(dir, "last-message.md");
const started = Date.now();
const result = run("codex", execArgs(args, { cwd: copy, lastMessage }), copy, {
  input: prompt,
  env: { ...process.env, CARGO_TARGET_DIR: join(dir, "target") },
});
const seconds = (Date.now() - started) / 1000;
writeFileSync(join(dir, "events.jsonl"), result.stdout ?? "");
writeFileSync(join(dir, "stderr.txt"), result.stderr ?? "");

run("git", ["add", "-A"], copy);
const diff = run("git", ["diff", "--cached", "--binary"], copy).stdout;
if (args.mode === "build" && diff) writeFileSync(join(dir, "change.patch"), diff);

run("git", ["worktree", "remove", "--force", copy], process.cwd());
rmSync(join(dir, "target"), { recursive: true, force: true });
const stats = summarize((result.stdout ?? "").split("\n"));
console.log(report({ args, sha, seconds, status: result.status ?? 1, stats, dir, hasPatch: Boolean(args.mode === "build" && diff) }));
process.exit(result.status ?? 1);
