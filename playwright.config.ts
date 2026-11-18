import { defineConfig } from "@playwright/test";

// Ports differ from the dev ones (7744, 5174), so tests run next to `npm run dev`.
export const ENGINE = "127.0.0.1:7799";
const UI = "http://127.0.0.1:5299";

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
      command: "vite ui --port 5299 --strictPort",
      url: UI,
    },
  ],
});
