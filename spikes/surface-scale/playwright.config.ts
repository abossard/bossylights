// Spike #24, throwaway: widgets can be dragged, resized and pressed by mouse and by touch.
// npx playwright test -c spikes/surface-scale
import { defineConfig } from "@playwright/test";

export const ENGINE = "127.0.0.1:7746";
const PAGE = "http://127.0.0.1:5181";

export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.ts",
  use: { baseURL: PAGE, browserName: "chromium", hasTouch: true, viewport: { width: 1440, height: 900 } },
  webServer: [
    {
      command: `cargo run -q --release --manifest-path engine/Cargo.toml -- --listen ${ENGINE}`,
      url: `http://${ENGINE}/health`,
      timeout: 600_000,
    },
    { command: "npx vite web --host 127.0.0.1 --port 5181 --strictPort", url: PAGE },
  ],
});
