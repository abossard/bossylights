import { defineConfig } from "@playwright/test";
// Free ports for every run (`e2e/ports.ts`): next to `npm run dev` and next to another run.
import { ENGINE, UI, UI_PORT } from "./e2e/ports";

export { ENGINE };

export default defineConfig({
  testDir: "e2e",
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
