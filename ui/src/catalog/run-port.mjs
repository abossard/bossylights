// A free port for one run of the catalog: the first load (Playwright) picks it and keeps it in the
// environment, the workers read it. So two runs in one checkout never meet.
import { execFileSync } from "node:child_process";

const FREE = `require("node:net").createServer().listen(0, "127.0.0.1", function () { console.log(this.address().port); this.close(); })`;

export function runPort() {
  const kept = process.env.CATALOG_PORT;
  if (kept) return kept;
  const port = execFileSync(process.execPath, ["-e", FREE], { encoding: "utf8" }).trim();
  process.env.CATALOG_PORT = port;
  return port;
}
