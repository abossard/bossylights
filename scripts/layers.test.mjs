import assert from "node:assert/strict";
import { test } from "node:test";
import { crateViolations, importsOf, sourceViolations, uiViolations } from "./layers.mjs";

const CRATES = {
  core: { kind: "core", deps: ["serde"] },
  calc: { kind: "calc", deps: ["core"] },
  app: { kind: "action", deps: ["core"] },
  tick: { kind: "tick", deps: ["core", "rtrb"] },
};
const dep = (name, workspace = true) => ({ name, workspace });

// [crate, its dependencies, violations]
const crateCases = [
  ["core", [dep("serde", false)], []],
  ["core", [dep("libc", false)], ["core -> libc is not an allowed edge"]],
  ["calc", [dep("core")], []],
  ["calc", [dep("app")], ["calc -> app is not an allowed edge"]],
  ["app", [dep("core"), dep("tokio", false)], []],
  ["app", [dep("calc")], ["app -> calc is not an allowed edge"]],
  ["tick", [dep("core"), dep("rtrb", false)], []],
  ["tick", [dep("core"), dep("log", false)], ["tick -> log is not an allowed edge"]], // an outside dependency counts too
  ["tick", [dep("calc")], ["tick -> calc is not an allowed edge"]],
  ["new", [], ["crate new is not on the list"]],
];
for (const [name, deps, expected] of crateCases) {
  test(`crate ${name} with ${deps.map((d) => d.name).join(", ") || "nothing"}`, () => {
    assert.deepEqual(crateViolations(name, deps, CRATES), expected);
  });
}

// [kind, source, violations]
const sourceCases = [
  ["core", "#![cfg_attr(not(test), no_std)]\npub fn f() {}", []],
  ["core", "#![no_std]\n", []],
  ["core", "pub fn f() {}", ["x has std: it needs #![no_std]"]],
  ["core", "#![no_std]\nextern crate alloc;", ["x has alloc: no `extern crate alloc` in a core crate"]],
  ["core", "#![no_std]\nfn f() { extern crate alloc; }", ["x has alloc: no `extern crate alloc` in a core crate"]],
  ["calc", "#![no_std]\nextern crate alloc;", []],
  ["calc", "use std::fs;", ["x has std: it needs #![no_std]"]],
  ["calc", "#![no_std]\nextern crate std;", ["x has std: no `extern crate std` outside a test"]],
  ["core", "#![no_std]\nfn f() { extern crate std; }", ["x has std: no `extern crate std` outside a test"]],
  ["calc", "#![no_std]\n#[cfg(test)]\nextern crate std;", []],
  // inside a test module, the extern needs its own `#[cfg(test)]` line too
  ["calc", "#![no_std]\n#[cfg(test)]\nmod tests {\n    extern crate std;\n}", ["x has std: no `extern crate std` outside a test"]],
  ["tick", "extern crate std;", []],
  ["calc", "#![no_std]\n// no extern crate std here", []],
  ["calc", "#![no_std]\n//! never `extern crate std` in this crate", []],
  ["calc", "#![no_std]\nextern crate std; // a comment after it", ["x has std: no `extern crate std` outside a test"]],
  ["action", "use std::fs;", []],
  ["tick", "use std::thread;", []],
];
for (const [kind, source, expected] of sourceCases) {
  test(`${kind} source ${JSON.stringify(source)}`, () => {
    assert.deepEqual(sourceViolations("x", kind, source), expected);
  });
}

// [source, relative imports]
const importCases = [
  [`import { a } from "./a";`, ["./a"]],
  [`import type { B } from "../b";\nimport "./style.css";`, ["../b", "./style.css"]],
  [`export { c } from './c';`, ["./c"]],
  [`const d = await import("./d");`, ["./d"]],
  [`import { render } from "solid-js/web";`, []],
  [`// from "./comment"`, ["./comment"]], // comments count: better one false alarm than a missed edge
];
for (const [source, expected] of importCases) {
  test(`imports of ${JSON.stringify(source)}`, () => {
    assert.deepEqual(importsOf(source), expected);
  });
}

const FILES = { "api.ts": "data", "machine.ts": "calc", "engine.ts": "engine", "main.tsx": "view", "kit/": "kit" };
const LAYERS = { data: [], calc: ["data"], engine: ["calc", "data"], view: ["engine", "calc", "data", "kit"], kit: ["kit", "data"] };

// [ui files and their sources, violations]
const uiCases = [
  [{ "main.tsx": `import "./engine"; import "./style.css";`, "engine.ts": `import "./machine";` }, []],
  [{ "machine.ts": `import { connect } from "./engine";` }, ["machine.ts (calc) -> engine.ts (engine) is not an allowed edge"]],
  [{ "engine.ts": `import "./main";` }, ["engine.ts (engine) -> main.tsx (view) is not an allowed edge"]],
  [{ "extra.ts": "" }, ["ui file extra.ts is not on the list"]],
  [{ "main.tsx": `import "./gone";` }, ["main.tsx imports ./gone, which is not on the list"]],
  // a folder on the list covers every file in it
  [{ "kit/a.ts": `import "./b.ts"; import "../api";`, "kit/b.ts": "" }, []],
  [{ "kit/deep/c.tsx": `import "../a";`, "kit/a.ts": "" }, []],
  [{ "main.tsx": `import "./kit/a";`, "kit/a.ts": "" }, []],
  [{ "kit/a.ts": `import "../engine";` }, ["kit/a.ts (kit) -> engine.ts (engine) is not an allowed edge"]],
  [{ "kit/a.ts": `import "./gone";` }, ["kit/a.ts imports ./gone, which is not on the list"]],
  [{ "other/a.ts": "" }, ["ui file other/a.ts is not on the list"]],
];
for (const [sources, expected] of uiCases) {
  test(`ui ${Object.keys(sources).join(", ")}`, () => {
    assert.deepEqual(uiViolations(sources, FILES, LAYERS), expected);
  });
}
