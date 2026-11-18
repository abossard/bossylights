// The allowed edges between crates and between the layers of ui/ (docs/coding.md, rules 1, 4, 7).
// An edge that is not written here fails `npm test`. Add one on purpose, never to make a test pass.

/**
 * Every workspace crate, its kind and the dependencies it may have.
 *
 * | kind   | std | alloc | deps listed      |
 * |--------|-----|-------|------------------|
 * | core   | no  | no    | all              |
 * | calc   | no  | yes   | all              |
 * | action | yes | yes   | workspace crates |
 *
 * core runs per tick. calc holds calculations that need the heap (none yet).
 */
export const CRATES = {
  "engine-core": { kind: "core", deps: ["serde"] },
  "rt-thread": { kind: "action", deps: ["engine-core"] }, // the only unsafe: asks for real-time priority
  engine: { kind: "action", deps: ["engine-core", "rt-thread"] },
  "engine-server": { kind: "action", deps: ["engine"] },
  shell: { kind: "action", deps: [] },
};

/** Every source file in ui/src and its layer. A new file is added here. */
export const UI_FILES = {
  "engine-api.ts": "data", // generated from crates/engine-core
  "connection.ts": "calc",
  "engine.ts": "engine", // the only module that talks to the engine
  "main.tsx": "view",
};

/** Which layer may import which. Views never import engine internals past engine.ts. */
export const UI_LAYERS = {
  data: [],
  calc: ["data"],
  engine: ["calc", "data"],
  view: ["engine", "calc", "data"],
};

const NO_STD = /^#!\[(?:no_std|cfg_attr\(not\(test\),\s*no_std\))\]/m;
const ALLOC = /\bextern\s+crate\s+alloc\b/;

/** What is wrong with a crate's dependencies. `deps`: [{ name, workspace }]. */
export function crateViolations(name, deps, crates) {
  const rule = crates[name];
  if (!rule) return [`crate ${name} is not on the list`];
  const checked = rule.kind === "action" ? deps.filter((d) => d.workspace) : deps;
  return checked
    .filter((d) => !rule.deps.includes(d.name))
    .map((d) => `${name} -> ${d.name} is not an allowed edge`);
}

/** What is wrong with a crate's source for its kind: core and calc have no std, core no alloc. */
export function sourceViolations(name, kind, source) {
  const violations = [];
  if (kind !== "action" && !NO_STD.test(source)) violations.push(`${name} has std: it needs #![no_std]`);
  if (kind === "core" && ALLOC.test(source)) {
    violations.push(`${name} has alloc: no \`extern crate alloc\` in a core crate`);
  }
  return violations;
}

/** The relative module specifiers a TypeScript source imports. */
export function importsOf(source) {
  const found = source.matchAll(/(?:\bfrom|\bimport)\s*\(?\s*["'](\.{1,2}\/[^"']+)["']/g);
  return [...found].map((m) => m[1]);
}

/** What is wrong with the imports between ui files. `sources`: { file: text }, paths relative to ui/src. */
export function uiViolations(sources, files, layers) {
  return Object.entries(sources).flatMap(([file, source]) => {
    const layer = files[file];
    if (!layer) return [`ui file ${file} is not on the list`];
    return importsOf(source)
      .filter((spec) => !/\.(css|svg|png|json)$/.test(spec))
      .flatMap((spec) => {
        const target = resolve(file, spec, files);
        if (!target) return [`${file} imports ${spec}, which is not on the list`];
        const to = files[target];
        return layers[layer].includes(to) ? [] : [`${file} (${layer}) -> ${target} (${to}) is not an allowed edge`];
      });
  });
}

/** "./engine" from "main.tsx" is "engine.ts" or "engine.tsx", whichever is listed. */
function resolve(from, spec, files) {
  const base = [...from.split("/").slice(0, -1), ...spec.split("/")].reduce((parts, part) => {
    if (part === "..") return parts.slice(0, -1);
    if (part === ".") return parts;
    return [...parts, part];
  }, []).join("/");
  return [base, `${base}.ts`, `${base}.tsx`].find((candidate) => candidate in files);
}
