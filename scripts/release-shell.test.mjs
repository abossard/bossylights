import assert from "node:assert/strict";
import { test } from "node:test";
import { SLOT, releaseEngineViolations, releaseShellViolations } from "./release-shell.mjs";

const NAMES = ["HOOK_A", "hook-b"];

// [release bytes, debug bytes, violations]
const cases = [
  ["\0plain\0", "\0HOOK_A\0hook-b\0", []],
  ["\0HOOK_A\0", "\0HOOK_A\0hook-b\0", ["the release shell holds the debug-only HOOK_A"]],
  ["xhook-bx", "HOOK_A hook-b", ["the release shell holds the debug-only hook-b"]],
  ["", "HOOK_A", ["the debug shell has no hook-b: update DEBUG_ONLY"]],
];
for (const [release, debug, expected] of cases) {
  test(`release ${JSON.stringify(release)}, debug ${JSON.stringify(debug)}`, () =>
    assert.deepEqual(releaseShellViolations(release, debug, NAMES), expected));
}

const SOURCE = `const NAME: &str = "${SLOT}";`;

// [release engine bytes, slot source, violations]
const engineCases = [
  ["\0plain\0", SOURCE, []],
  [`\0/tmp/${SLOT}\0`, SOURCE, [`the release engine holds the test-only ${SLOT}`]],
  ["\0plain\0", 'const NAME: &str = "other.slot";', [`crates/engine-slot names no ${SLOT}: update SLOT`]],
];
for (const [release, source, expected] of engineCases) {
  test(`release engine ${JSON.stringify(release)}, slot source ${JSON.stringify(source)}`, () =>
    assert.deepEqual(releaseEngineViolations(release, source), expected));
}
