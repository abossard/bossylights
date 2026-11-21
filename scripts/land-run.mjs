// `npm run land -- <wip/branch> <message-file>`: a reviewed WIP branch becomes one commit on main.
// Reads every fact first (land.mjs decides), then writes: squash onto origin/main without a working tree
// (merge-tree, commit-tree), `npm run verify` on that commit, a plain push (a moved main refuses it),
// then the branch and its worktree go. Nothing in any checkout is touched before the push.
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { cleanupSteps, parseArgs, pushFailure, refusal, worktreeOf } from "./land.mjs";

const run = (command, args) => spawnSync(command, args, { encoding: "utf8", maxBuffer: 1 << 26 });
const failure = (command, args, r) => `${command} ${args.join(" ")}: ${(r.error?.message ?? r.stderr).trim()}`;
const out = (command, ...args) => {
  const r = run(command, args);
  if (r.status !== 0) throw new Error(failure(command, args, r));
  return r.stdout.trim();
};
const git = (...args) => out("git", ...args);
const tipOf = (ref) => run("git", ["rev-parse", "--verify", "-q", `${ref}^{commit}`]).stdout.trim() || null;
const json = (...args) => JSON.parse(out("gh", ...args));

let args;
try {
  args = parseArgs(process.argv.slice(2));
} catch (error) {
  console.error(error.message);
  process.exit(2);
}
const { branch, messageFile } = args;
try {
  const message = readFileSync(messageFile, "utf8");
  git("fetch", "--no-tags", "--prune", "origin", "+refs/heads/main:refs/remotes/origin/main", `+refs/heads/wip/*:refs/remotes/origin/wip/*`);
  const base = git("rev-parse", "refs/remotes/origin/main");
  const remoteTip = tipOf(`refs/remotes/origin/${branch}`);
  const localTip = tipOf(`refs/heads/${branch}`);
  const worktree = worktreeOf(git("worktree", "list", "--porcelain"), branch);
  const merge = remoteTip && run("git", ["merge-tree", "--write-tree", base, remoteTip]);
  if (merge && merge.status > 1) throw new Error(`git merge-tree: ${merge.stderr.trim()}`);
  const facts = {
    branch,
    remoteTip,
    localTip,
    here: git("rev-parse", "--show-toplevel"),
    worktree,
    dirty: worktree !== null && git("-C", worktree, "status", "--porcelain") !== "",
    conflict: merge?.status === 1,
    empty: merge?.status === 0 && merge.stdout.split("\n")[0] === git("rev-parse", `${base}^{tree}`),
    base,
    message,
    ci: json("run", "list", "--workflow", "ci.yml", "--branch", "main", "--event", "push", "--commit", base,
      "--limit", "1", "--json", "headSha,status,conclusion")[0],
    issues: json("issue", "list", "--state", "open", "--search", '"CI red on main" in:title', "--limit", "100", "--json", "number,title"),
  };
  const why = refusal(facts);
  if (why) throw new Error(why);

  const sha = git("commit-tree", merge.stdout.split("\n")[0], "-p", base, "-F", messageFile);
  console.log(`land: ${sha.slice(0, 7)} is ${branch} (${remoteTip.slice(0, 7)}) squashed onto origin/main (${base.slice(0, 7)})`);
  const verify = spawnSync(process.execPath, [fileURLToPath(new URL("./verify-run.mjs", import.meta.url)), sha], { stdio: "inherit" });
  if (verify.status !== 0) throw new Error(`verify failed on ${sha.slice(0, 7)}: main and ${branch} are as they were`);
  const pushed = run("git", ["push", "origin", `${sha}:refs/heads/main`]);
  if (pushed.status !== 0) throw new Error(pushFailure(pushed.stderr));
  console.log(`land: ${sha.slice(0, 7)} is on origin/main`);

  const after = cleanupSteps({ branch, remoteTip, localTip: tipOf(`refs/heads/${branch}`), worktree });
  let kept = after.kept;
  for (const step of after.steps) {
    const r = run("git", step);
    if (r.status !== 0) kept = [...kept, `kept, ${failure("git", step, r)}`];
  }
  for (const line of kept) console.error(`land: ${line}`);
  if (kept.length === 0) console.log(`land: ${branch}${worktree ? ` and ${worktree}` : ""} removed`);
  else process.exitCode = 1;
} catch (error) {
  console.error(`land: ${error.message}`);
  process.exit(1);
}
