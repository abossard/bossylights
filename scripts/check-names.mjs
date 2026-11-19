// Fails when `engine-core` or `engine` names a protocol or a device outside the list
// of kinds: the engine knows "an output", the kinds live in `crates/outputs`.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { CHECKED, KINDS, kindsOf, protocolNames } from "./names.mjs";

const kinds = kindsOf(readFileSync(KINDS, "utf8"));
if (kinds.length === 0) {
  console.error(`names: no OutputKind variants found in ${KINDS}`);
  process.exit(1);
}
const hits = CHECKED.flatMap((dir) =>
  readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => join(entry.parentPath, entry.name))
    .flatMap((path) => protocolNames(path, readFileSync(path, "utf8"), kinds)),
);
for (const hit of hits) console.error(`names a protocol or device: ${hit}`);
process.exit(hits.length === 0 ? 0 : 1);
