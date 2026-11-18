// Writes docs/design/kit.md: the primitives of the kit and what every control is made of (issue #102).
// Run it after the table or `ui/src/catalog/compose.ts` changes: `node scripts/write-kit-doc.mjs`. `npm test` fails when the file is stale.
import { writeFileSync } from "node:fs";
import { kitDoc } from "./kit-doc.mjs";

export const DOC = "docs/design/kit.md";

export const docText = () => kitDoc();

if (import.meta.main) {
  writeFileSync(DOC, docText());
  console.log(`${DOC} written, ${docText().split("\n").length} lines`);
}
