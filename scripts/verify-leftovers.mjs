// `npm run verify`: what a step left running from the worktree is stopped (#317). An action.
import { spawnSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { leftovers } from "./verify.mjs";

/** The pids running now: what runs before a step is never stopped after it. */
export function running() {
  const ps = spawnSync("ps", ["-ww", "-eo", "pid="], { encoding: "utf8", maxBuffer: 1 << 26 }).stdout;
  return new Set(ps.split("\n").map((l) => Number(l.trim())).filter(Boolean));
}

/** Stops every program the step started from the worktree at `dir` that still runs (a test window, its engine):
 * a step that failed or timed out leaves nothing behind. `before`: running() before the step. A line per program stopped. */
export function stopLeftovers(dir, before) {
  const copy = realpathSync(dir); // cargo names programs by the real path (/private/var/...), the temp folder is /var/...
  const ps = spawnSync("ps", ["-ww", "-eo", "pid=,args="], { encoding: "utf8", maxBuffer: 1 << 26 }).stdout;
  return leftovers(ps, copy, before).map(({ pid, args }) => {
    try {
      process.kill(pid, "SIGKILL");
    } catch {
      // it ended by itself in between
    }
    return `verify: stopped what the suite left running: ${pid} ${args}`;
  });
}
