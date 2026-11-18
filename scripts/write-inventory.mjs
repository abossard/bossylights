// Reads the mockups and writes the inventory of their interactive elements (issue #102).
// Run it after the mockups change: `node scripts/write-inventory.mjs`. `npm test` fails when the file is stale.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";
import { inventory } from "./inventory.mjs";

export const MOCKUPS = "mockups/src";
export const INVENTORY = "ui/src/controls/inventory.json";

/** The mockups' sources: { path relative to mockups/src: text }. */
export function readMockups() {
  return Object.fromEntries(
    readdirSync(MOCKUPS, { recursive: true, withFileTypes: true })
      .filter((entry) => entry.isFile() && /\.tsx?$/.test(entry.name))
      .map((entry) => join(entry.parentPath, entry.name))
      .sort()
      .map((file) => [relative(MOCKUPS, file).split("\\").join("/"), readFileSync(file, "utf8")]),
  );
}

/** The inventory file as text. */
export const inventoryText = () => `${JSON.stringify(inventory(readMockups()), null, 1)}\n`;

if (import.meta.main) {
  writeFileSync(INVENTORY, inventoryText());
  console.log(`${JSON.parse(inventoryText()).length} interactive elements written to ${INVENTORY}`);
}
