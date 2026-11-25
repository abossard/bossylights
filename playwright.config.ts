import { defineConfig } from "@playwright/test";
// Free ports for every run (`e2e/ports.ts`): next to `npm run dev` and next to another run.
import { ENGINE, UI, UI_PORT } from "./e2e/ports";

export { ENGINE };

export default defineConfig({
  testDir: "e2e",
  // One engine on the machine at a time (#319): the shell tests start their own engine, so
  // they run after these, with their own config (`playwright.shell.config.ts`).
  testIgnore: "shell.spec.ts",
  use: { baseURL: UI, browserName: "chromium" },
  webServer: [
    {
      command: `cargo run -q -p engine-server -- --listen ${ENGINE}`,
      url: `http://${ENGINE}/health`,
      timeout: 600_000, // first run compiles the engine
    },
    {
      command: `vite ui --port ${UI_PORT} --strictPort`,
      url: UI,
    },
  ],
});
