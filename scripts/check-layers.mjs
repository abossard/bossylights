// Fails when a crate or a ui file has an edge that scripts/layers.mjs does not allow,
// or when a core crate could reach std or alloc.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { CRATES, UI_FILES, UI_LAYERS, crateViolations, sourceViolations, uiViolations } from "./layers.mjs";

const UI_SRC = "ui/src";

const metadata = JSON.parse(
  execFileSync("cargo", ["metadata", "-q", "--format-version", "1", "--no-deps", "--locked"], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  }),
);

const filesUnder = (dir) =>
  readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => join(entry.parentPath, entry.name));

const crates = metadata.packages.flatMap((p) => {
  const deps = p.dependencies
    .filter((d) => d.kind !== "dev")
    .map((d) => ({ name: d.name, workspace: d.path !== undefined }));
  const source = filesUnder(join(dirname(p.manifest_path), "src"))
    .filter((file) => file.endsWith(".rs"))
    .map((file) => readFileSync(file, "utf8"))
    .join("\n");
  const kind = CRATES[p.name]?.kind ?? "action";
  return [...crateViolations(p.name, deps, CRATES), ...sourceViolations(p.name, kind, source)];
});

const uiSources = Object.fromEntries(
  filesUnder(UI_SRC)
    .filter((file) => /\.tsx?$/.test(file))
    .map((file) => [relative(UI_SRC, file), readFileSync(file, "utf8")]),
);
const ui = uiViolations(uiSources, UI_FILES, UI_LAYERS);

const violations = [...crates, ...ui];
for (const v of violations) console.error(`layers: ${v}`);
console.log(`${metadata.packages.length} crates, ${Object.keys(uiSources).length} ui files, ${violations.length} violations`);
process.exit(violations.length ? 1 : 0);
