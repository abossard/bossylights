// Spike #24, throwaway. Starts the spike engine and the page, then opens it.
//
//   node spikes/surface-scale/run.mjs browser [--measure]   Chromium from Playwright
//   node spikes/surface-scale/run.mjs tauri [--measure]     the product Tauri shell, spike page
//
// --measure runs the timed phases (about 75 s), writes results/<chromium|tauri>.json and
// stops everything. Without it the page stays open until you close the window.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const HERE = dirname(fileURLToPath(import.meta.url));
const ENGINE = "127.0.0.1:7745";
const PAGE = "http://127.0.0.1:5180";
const WIDTH = 1440;
const HEIGHT = 900;
const TIMEOUT_MS = 115_000;

const [where, flag] = process.argv.slice(2);
const measure = flag === "--measure";
if (!["browser", "tauri"].includes(where)) {
  console.error("usage: node spikes/surface-scale/run.mjs <browser|tauri> [--measure]");
  process.exit(2);
}
const label = where === "browser" ? "chromium" : "tauri";
const results = join(HERE, "results", `${label}.json`);
const url = `${PAGE}/?label=${label}${measure ? "&run=auto" : ""}`;

const children = [];
function start(command, args, options = {}) {
  const child = spawn(command, args, { stdio: "inherit", detached: true, ...options });
  children.push(child);
  return child;
}
function stopAll() {
  for (const child of children) {
    try {
      process.kill(-child.pid, "SIGTERM"); // the whole group: tauri also starts the app and its engine
    } catch {
      // already gone
    }
  }
}
process.on("SIGINT", () => {
  stopAll();
  process.exit(130);
});

async function until(check, what, ms = 120_000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check().catch((error) => { if (error.message.startsWith("no results")) throw error; return false; })) return;
    await new Promise((done) => setTimeout(done, 250));
  }
  throw new Error(`timed out waiting for ${what}`);
}
const up = (address) => () => fetch(address).then((r) => r.ok);

mkdirSync(dirname(results), { recursive: true });
if (measure) rmSync(results, { force: true });

let shownAt = 0; // when the window opened or the page connected: the time limit counts from here
const engine = start("cargo", [
  "run", "-q", "--release", "--manifest-path", join(HERE, "engine", "Cargo.toml"), "--",
  "--listen", ENGINE, "--results", results,
], { stdio: ["ignore", "pipe", "inherit"] });
engine.stdout.on("data", (chunk) => {
  process.stdout.write(chunk);
  if (!shownAt && chunk.toString().includes(`page: ${label} connected`)) shownAt = Date.now();
});
/** The results, or a failure once the window has been up for TIMEOUT_MS. */
const finished = () => {
  if (existsSync(results)) return true;
  if (shownAt && Date.now() - shownAt > TIMEOUT_MS) throw new Error("no results within the time limit");
  return false;
};
start("npx", ["vite", join(HERE, "web"), "--host", "127.0.0.1", "--port", "5180", "--strictPort"]);

try {
  await until(up(`http://${ENGINE}/health`), "the spike engine", 600_000);
  await until(up(PAGE), "the page");
  console.log(`spike: ${url}`);

  if (where === "browser") {
    const browser = await chromium.launch({
      headless: false,
      args: ["--disable-backgrounding-occluded-windows", "--disable-renderer-backgrounding", "--disable-background-timer-throttling"],
    });
    const page = await browser.newPage({ viewport: { width: WIDTH, height: HEIGHT }, deviceScaleFactor: 2, hasTouch: true });
    console.log(`spike: Chromium ${browser.version()}`);
    await page.goto(url);
    if (measure) await until(async () => finished(), "the results", TIMEOUT_MS);
    else await new Promise((done) => browser.on("disconnected", done));
    await browser.close();
  } else {
    const config = {
      build: { beforeDevCommand: "", devUrl: url },
      app: {
        windows: [{ title: "Surface at scale (spike)", width: WIDTH, height: HEIGHT, alwaysOnTop: measure }],
        security: { csp: null },
      },
    };
    const tauri = start("npx", ["tauri", "dev", "--no-watch", "--config", JSON.stringify(config)], {
      stdio: ["ignore", "inherit", "pipe"],
    });
    tauri.stderr.on("data", (chunk) => {
      process.stderr.write(chunk);
      if (!shownAt && /Running.*bossylights/.test(chunk.toString())) shownAt = Date.now();
    });
    if (measure) await until(async () => finished(), "the results", 600_000); // the shell may compile first
    else await new Promise((done) => tauri.on("exit", done));
  }
  if (measure) console.log(`spike: results in ${results}`);
} catch (error) {
  console.error(`spike: ${error.message}`);
  process.exitCode = 1;
} finally {
  stopAll();
}
