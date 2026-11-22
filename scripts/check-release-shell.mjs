// Fails when a release build of the shell holds a debug-only test hook (see release-shell.mjs).
// Runs after the Rust tests: the debug shell is built by then.
import { readFileSync } from "node:fs";
import { cargoBuild } from "./cargo-build.mjs";
import { DEBUG_ONLY, releaseShellViolations } from "./release-shell.mjs";

const program = (args) => {
  const path = cargoBuild(["-q", "-p", "shell", ...args]).bossylights;
  if (!path) throw new Error(`cargo built no bossylights program (${args.join(" ") || "debug"})`);
  return readFileSync(path, "latin1");
};

const violations = releaseShellViolations(program(["--release"]), program([]), DEBUG_ONLY);
for (const v of violations) console.error(`release shell: ${v}`);
console.log(`release shell: ${DEBUG_ONLY.length} debug-only names, ${violations.length} violations`);
process.exit(violations.length ? 1 : 0);
