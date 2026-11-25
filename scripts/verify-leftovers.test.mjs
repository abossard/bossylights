import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { setTimeout as after } from "node:timers/promises";
import { stopLeftovers } from "./verify-leftovers.mjs";

/** A program in `folder`, named by its real path as cargo names the programs it builds. */
async function startIn(folder) {
  mkdirSync(join(folder, "bin"));
  symlinkSync("/bin/sleep", join(folder, "bin", "sleep"));
  const child = spawn(join(realpathSync(folder), "bin", "sleep"), ["60"], { stdio: "ignore" });
  await new Promise((started) => child.once("spawn", started));
  return child;
}

// [where the program runs, stopped]
const cases = [
  ["in the copy", true],
  ["in another folder", false],
];
for (const [where, stopped] of cases) {
  test(`a program left running ${where}: ${stopped ? "stopped" : "left alone"}`, async () => {
    const copy = mkdtempSync(join(tmpdir(), "verify-leftovers-"));
    const other = mkdtempSync(join(tmpdir(), "verify-leftovers-other-"));
    const child = await startIn(stopped ? copy : other);
    const exited = new Promise((done) => child.once("exit", (code, signal) => done(signal)));
    try {
      const lines = stopLeftovers(copy);
      if (stopped) {
        assert.deepEqual(lines, [`verify: stopped what the suite left running: ${child.pid} ${realpathSync(copy)}/bin/sleep 60`]);
        assert.equal(await Promise.race([exited, after(5_000, "still running", { ref: false })]), "SIGKILL");
      } else {
        assert.deepEqual(lines, []);
        assert.equal(child.exitCode, null);
      }
    } finally {
      child.kill("SIGKILL");
      await exited;
      rmSync(copy, { recursive: true, force: true });
      rmSync(other, { recursive: true, force: true });
    }
  });
}
