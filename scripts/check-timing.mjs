// Fails when a test compares an elapsed time with a bound under one second. See timing.mjs.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { CHECKED, isTestFile, timingBounds } from "./timing.mjs";

const hits = CHECKED.flatMap((dir) =>
  readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile() && !entry.parentPath.includes("node_modules") && !entry.parentPath.includes("target"))
    .map((entry) => join(entry.parentPath, entry.name))
    .filter(isTestFile)
    .flatMap((path) => timingBounds(path, readFileSync(path, "utf8"))),
);
for (const hit of hits) console.error(`times a test against a bound under one second: ${hit}`);
process.exit(hits.length === 0 ? 0 : 1);
