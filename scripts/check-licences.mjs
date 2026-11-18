// Fails when any Rust or npm dependency has a licence that is not compatible with Apache-2.0.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { allowed } from "./licences.mjs";

const lockfiles = ["package-lock.json", "mockups/package-lock.json"];

const crates = JSON.parse(
  execFileSync("cargo", ["metadata", "-q", "--format-version", "1", "--locked"], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  }),
).packages.map((p) => ({ name: `crate ${p.name} ${p.version}`, licence: p.license ?? "" }));

const npm = lockfiles.flatMap((file) =>
  Object.entries(JSON.parse(readFileSync(file, "utf8")).packages)
    .filter(([path]) => path !== "")
    .map(([path, p]) => ({ name: `npm ${path} ${p.version}`, licence: p.license ?? "" })),
);

const refused = [...crates, ...npm].filter((d) => !allowed(d.licence));
for (const d of refused) console.error(`refused: ${d.name}: "${d.licence || "no licence"}"`);
console.log(`${crates.length} crates, ${npm.length} npm packages, ${refused.length} refused`);
process.exit(refused.length ? 1 : 0);
