// Fails when `engine-core` or `engine` names a protocol or a device outside the list
// of kinds: the engine knows "an output", the kinds live in `crates/outputs`.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { CHECKED, protocolNames } from "./names.mjs";

const hits = CHECKED.flatMap((dir) =>
  readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => join(entry.parentPath, entry.name))
    .flatMap((path) => protocolNames(path, readFileSync(path, "utf8"))),
);
for (const hit of hits) console.error(`names a protocol or device: ${hit}`);
process.exit(hits.length === 0 ? 0 : 1);
