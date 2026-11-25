// `npm run verify`: what a suite left running from the copy is stopped (#317). An action.
import { spawnSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { leftovers } from "./verify.mjs";

/** Stops every program still running from the copy at `dir` (a test window, its engine): a suite
 * that failed or timed out leaves nothing behind. Returns a line per program stopped. */
export function stopLeftovers(dir) {
  const copy = realpathSync(dir); // cargo names programs by the real path (/private/var/...), the temp folder is /var/...
  const ps = spawnSync("ps", ["-ww", "-eo", "pid=,args="], { encoding: "utf8", maxBuffer: 1 << 26 }).stdout;
  return leftovers(ps, copy).map(({ pid, args }) => {
    try {
      process.kill(pid, "SIGKILL");
    } catch {
      // it ended by itself in between
    }
    return `verify: stopped what the suite left running: ${pid} ${args}`;
  });
}
