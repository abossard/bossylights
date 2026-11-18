// Writes docs/design/controls.md from the behaviour table in ui/src/controls (issue #102).
// Run it after the table changes: `node scripts/write-controls-doc.mjs`. `npm test` fails when the file is stale.
import { readFileSync, writeFileSync } from "node:fs";
import { controlsDoc } from "./controls-doc.mjs";
import { INVENTORY } from "./write-inventory.mjs";

export const DOC = "docs/design/controls.md";

/** The document as text, from the inventory on disk. */
export const docText = () => controlsDoc(JSON.parse(readFileSync(INVENTORY, "utf8")));

if (import.meta.main) {
  writeFileSync(DOC, docText());
  console.log(`${DOC} written, ${docText().split("\n").length} lines`);
}
