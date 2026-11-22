// The allowed edges between crates and between the layers of ui/ (docs/coding.md, rules 1, 4, 7).
// An edge that is not written here fails `npm test`. Add one on purpose, never to make a test pass.

/**
 * Every workspace crate, its kind and the dependencies it may have.
 *
 * | kind   | std | alloc | deps listed      |
 * |--------|-----|-------|------------------|
 * | core   | no  | no    | all              |
 * | calc   | no  | yes   | all              |
 * | tick   | yes | yes   | all              |
 * | action | yes | yes   | workspace crates |
 *
 * core runs per tick. calc holds calculations that need the heap. tick is an action
 * whose code runs on the tick thread: an outside dependency there could lock, allocate
 * or log (docs/coding.md, rule 7), so each is listed.
 */
export const CRATES = {
  "engine-core": { kind: "core", deps: ["serde"] },
  "fixture-library": { kind: "calc", deps: ["roxmltree", "serde"] }, // QLC+ fixture definitions in, a model out. Reads no files
  rig: { kind: "calc", deps: ["engine-core", "fixture-library"] }, // a mode at an address becomes writers: only it knows an address belongs to a fixture
  looks: { kind: "calc", deps: ["engine-core", "rig"] }, // saved looks: checked against the patch, compiled into plans for the core
  "show-file": { kind: "calc", deps: ["engine-core", "fixture-library", "rig", "looks", "serde", "serde_json"] }, // the patch as a show file's text and back, checked as the commands check. Reads no files
  "rt-thread": { kind: "tick", deps: ["engine-core", "libc"] }, // the only unsafe: asks for real-time priority
  outputs: { kind: "tick", deps: ["engine-core", "nix", "rtrb", "serialport"] }, // opens devices, takes frames: only it knows bytes
  engine: { kind: "tick", deps: ["engine-core", "rt-thread", "outputs", "rig", "looks", "fixture-library", "rtrb"] }, // the front owns the patch: it checks a patch against the library and compiles writers
  "audio-in": { kind: "calc", deps: [] }, // samples and the caller's clock in, levels and beats out. Capture is its own action crate
  "engine-server": { kind: "action", deps: ["engine", "engine-core", "fixture-library", "looks", "show-file"] }, // reads the fixture folder, answers the library, opens and saves the show file
  shell: { kind: "action", deps: ["engine-core"] }, // knows the API version it speaks, nothing more
};

/** Every source file in ui/src and its layer. A new file is added here. A name that ends in "/" is a folder: all of it. */
export const UI_FILES = {
  "engine-api.ts": "data", // generated from crates/engine-core
  "connection.ts": "calc",
  "picture.ts": "calc",
  "engine.ts": "engine", // the only module that talks to the engine
  "main.tsx": "view",
  "controls/": "controls", // the behaviour table of every control, its commands and state machines (#102)
  "kit/": "kit", // the pure components the controls are drawn with: props in, a gesture out (#102)
  "catalog/": "catalog", // the storyboard catalog, its own app: `npm run catalog` (#102)
  "catalog/reviews-file.mjs": "catalog", // its dev server keeps the owner's reviews in docs/design/catalog-reviews.json
  "catalog/run-port.mjs": "catalog", // a free port for each run of the catalog tests
};

/** Which layer may import which. Views never import engine internals past engine.ts. */
export const UI_LAYERS = {
  data: [],
  calc: ["data"],
  engine: ["calc", "data"],
  view: ["engine", "calc", "data"],
  controls: ["controls"], // data and calculations about controls. No engine, no view
  kit: ["kit", "controls"], // views with no engine: the only thing they know of a control is its machine
  catalog: ["catalog", "kit", "controls"], // draws the table with the kit. No engine
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
  if ((kind === "core" || kind === "calc") && !NO_STD.test(source)) violations.push(`${name} has std: it needs #![no_std]`);
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

/** The layer of a ui file: its own entry, or the entry of the nearest folder around it. */
export function layerOf(file, files) {
  if (file in files) return files[file];
  const folder = Object.keys(files)
    .filter((name) => name.endsWith("/") && file.startsWith(name))
    .sort((a, b) => b.length - a.length)[0];
  return folder && files[folder];
}

/** What is wrong with the imports between ui files. `sources`: { file: text }, paths relative to ui/src. */
export function uiViolations(sources, files, layers) {
  const known = (path) => path in files || (path in sources && layerOf(path, files) !== undefined);
  return Object.entries(sources).flatMap(([file, source]) => {
    const layer = layerOf(file, files);
    if (!layer) return [`ui file ${file} is not on the list`];
    return importsOf(source)
      .filter((spec) => !/\.(css|svg|png|json)$/.test(spec))
      .flatMap((spec) => {
        const target = resolve(file, spec, known);
        if (!target) return [`${file} imports ${spec}, which is not on the list`];
        const to = layerOf(target, files);
        return layers[layer].includes(to) ? [] : [`${file} (${layer}) -> ${target} (${to}) is not an allowed edge`];
      });
  });
}

/** "./engine" from "main.tsx" is "engine.ts" or "engine.tsx", whichever is known. */
function resolve(from, spec, known) {
  const base = [...from.split("/").slice(0, -1), ...spec.split("/")].reduce((parts, part) => {
    if (part === "..") return parts.slice(0, -1);
    if (part === ".") return parts;
    return [...parts, part];
  }, []).join("/");
  return [base, `${base}.ts`, `${base}.tsx`].find(known);
}
