import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { reviewsFile } from "./reviews-file.mjs";

// The catalog is its own app (`npm run catalog`). Nothing of it is built into the product.
// It reads `../kit` and `../controls`, so the server may serve the repository, and it keeps its own cache
// so it can run beside `npm run ui`.
export default defineConfig({
  plugins: [solid(), reviewsFile()],
  cacheDir: "../../../node_modules/.vite-catalog",
  server: { host: "127.0.0.1", port: 5175, strictPort: true, fs: { allow: ["../../.."] } },
});
