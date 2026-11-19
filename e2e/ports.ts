// Free ports for one run, so two runs in one checkout never meet. The first load (the Playwright
// process) picks them and puts them in the environment; the workers load this file again and read them.
import { execFileSync } from "node:child_process";

const FREE = `
const net = require("node:net");
const servers = [0, 1, 2, 3].map(() => net.createServer().listen(0, "127.0.0.1"));
Promise.all(servers.map((s) => new Promise((ok) => s.on("listening", ok)))).then(() => {
  console.log(servers.map((s) => s.address().port).join(","));
  servers.forEach((s) => s.close());
});
`;

if (!process.env.E2E_PORTS) {
  process.env.E2E_PORTS = execFileSync(process.execPath, ["-e", FREE], { encoding: "utf8" }).trim();
}
const [engine, shell, ui, nowhere] = process.env.E2E_PORTS.split(",");

/** The engine of the page tests. */
export const ENGINE = `127.0.0.1:${engine}`;
/** The engine the shell tests start. */
export const SHELL_ENGINE = `127.0.0.1:${shell}`;
/** An address of no machine here (TEST-NET-1): nothing answers, the engine cannot listen. The port is the run's, for the shell's one window per address. */
export const NOWHERE = `192.0.2.1:${nowhere}`;
export const UI_PORT = ui;
export const UI = `http://127.0.0.1:${ui}`;

// The engine lets this run's page open its API, and no other (`engine-server`: BOSSYLIGHTS_ORIGINS).
process.env.BOSSYLIGHTS_ORIGINS = UI;
