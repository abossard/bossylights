// Fails when `engine-core` or `engine` names a protocol or a device outside the list
// of kinds: the engine knows "an output", the kinds live in `crates/outputs`. Also fails on
// a test file named by a milestone, ticket or review instead of what it proves.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { CHECKED, KINDS, TEST_FOLDERS, datedTestNames, kindsOf, protocolNames } from "./names.mjs";

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
const dated = datedTestNames(
  TEST_FOLDERS.flatMap((dir) =>
    readdirSync(dir, { recursive: true, withFileTypes: true })
      .filter((entry) => entry.isFile() && !entry.parentPath.includes("node_modules"))
      .map((entry) => join(entry.parentPath, entry.name)),
  ),
);
for (const path of dated) console.error(`test file named by a milestone, ticket or review, not by what it proves: ${path}`);
process.exit(hits.length === 0 && dated.length === 0 ? 0 : 1);
