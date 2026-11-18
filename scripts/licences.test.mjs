import assert from "node:assert/strict";
import { test } from "node:test";
import { allowed } from "./licences.mjs";

const cases = [
  ["MIT", true],
  ["Apache-2.0 OR MIT", true],
  ["MIT/Apache-2.0", true],
  ["(MIT OR Apache-2.0) AND Unicode-3.0", true],
  ["Apache-2.0 WITH LLVM-exception", true],
  ["MIT OR GPL-3.0-only", true],
  ["GPL-3.0-only", false],
  ["AGPL-3.0-or-later", false],
  ["LGPL-2.1", false],
  ["MIT AND GPL-2.0", false],
  ["Apache-2.0 WITH Classpath-exception-2.0", false],
  ["", false],
];

for (const [expression, expected] of cases) {
  test(`"${expression}" is ${expected ? "allowed" : "refused"}`, () => {
    assert.equal(allowed(expression), expected);
  });
}
