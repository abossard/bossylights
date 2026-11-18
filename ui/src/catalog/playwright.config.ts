import { defineConfig } from "@playwright/test";

// The headless run of the catalog (`npm run test:catalog`). It needs no engine: the catalog has none.
// Its port differs from the dev one (5175), so it runs next to `npm run catalog`.
const URL = "http://127.0.0.1:5298";

export default defineConfig({
  testDir: ".",
  testMatch: "catalog.spec.ts",
  outputDir: "../../../test-results/catalog",
  use: { baseURL: URL, browserName: "chromium" },
  // The run rates controls: it never writes the owner's reviews into the repository.
  webServer: { command: "vite --port 5298 --strictPort", url: URL, env: { CATALOG_REVIEWS: "off" } },
});
