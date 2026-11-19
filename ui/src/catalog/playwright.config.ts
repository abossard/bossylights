import { defineConfig } from "@playwright/test";

// The headless run of the catalog (`npm run test:catalog`). It needs no engine: the catalog has none.
// A free port per run, so it runs next to `npm run catalog` and next to another run.
import { runPort } from "./run-port.mjs";

const PORT = runPort();
const URL = `http://127.0.0.1:${PORT}`;

export default defineConfig({
  testDir: ".",
  testMatch: "catalog.spec.ts",
  outputDir: "../../../test-results/catalog",
  use: { baseURL: URL, browserName: "chromium" },
  // The run rates controls: it never writes the owner's reviews into the repository.
  webServer: { command: `vite --port ${PORT} --strictPort`, url: URL, env: { CATALOG_REVIEWS: "off" } },
});
