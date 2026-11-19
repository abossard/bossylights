import { defineConfig } from "@playwright/test";

// The headless run of the catalog (`npm run test:catalog`). It needs no engine: the catalog has none.
// A free port per run, so it runs next to `npm run catalog` and next to another run.
import { execFileSync } from "node:child_process";

if (!process.env.CATALOG_PORT) {
  const free = `require("node:net").createServer().listen(0, "127.0.0.1", function () { console.log(this.address().port); this.close(); })`;
  process.env.CATALOG_PORT = execFileSync(process.execPath, ["-e", free], { encoding: "utf8" }).trim();
}
const URL = `http://127.0.0.1:${process.env.CATALOG_PORT}`;

export default defineConfig({
  testDir: ".",
  testMatch: "catalog.spec.ts",
  outputDir: "../../../test-results/catalog",
  use: { baseURL: URL, browserName: "chromium" },
  // The run rates controls: it never writes the owner's reviews into the repository.
  webServer: { command: `vite --port ${process.env.CATALOG_PORT} --strictPort`, url: URL, env: { CATALOG_REVIEWS: "off" } },
});
