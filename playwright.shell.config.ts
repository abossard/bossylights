import { defineConfig } from "@playwright/test";
// The shell tests: each shell starts or adopts its own engine, never the page tests' one.
// One engine on the machine at a time (#319): `npm run test:e2e` runs them after the page
// tests, under the same engine slot. The page is served by vite alone.
import { UI, UI_PORT } from "./e2e/ports";

export default defineConfig({
  testDir: "e2e",
  testMatch: "shell.spec.ts",
  outputDir: "test-results/shell",
  use: { baseURL: UI, browserName: "chromium" },
  webServer: { command: `vite ui --port ${UI_PORT} --strictPort`, url: UI },
});
