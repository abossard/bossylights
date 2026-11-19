// `npm run peer:doctor`: are Codex and Copilot installed and signed in, which models, efforts and
// context tiers they take, and one tiny low-effort call each through the same runner as `npm run peer`.
// Prints status lines only, never a token: the auth line is masked.
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runCli } from "./peer-cli.mjs";
import { codexModelsOf, copilotModelsOf, doctorReport, possibleValues } from "./peer.mjs";

const read = (command, args) => {
  const r = spawnSync(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  return r.error ? null : { status: r.status, text: `${r.stdout}${r.stderr}` };
};
const firstLine = (text) => text.split("\n").find((l) => l.trim())?.trim() ?? "";

async function tinyCall(tool) {
  const dir = mkdtempSync(join(tmpdir(), `bossylights-peer-doctor-${tool}-`));
  const cwd = join(dir, "work");
  mkdirSync(cwd);
  spawnSync("git", ["init", "-q", cwd]);
  const settings = { tool, mode: "review", model: "sol", effort: "low" };
  const run = await runCli(settings, { cwd, dir, outDir: dir, prompt: "Reply with the single word ok. Run nothing." });
  rmSync(dir, { recursive: true, force: true });
  return { model: settings.model, effort: settings.effort, ok: run.status === 0 && /\bok\b/i.test(run.lastMessage), status: run.status, seconds: run.seconds, stats: run.stats };
}

async function codex() {
  const version = read("codex", ["--version"]);
  if (!version) return { tool: "codex", version: null };
  const login = read("codex", ["login", "status"]);
  const models = read("codex", ["debug", "models"]);
  return {
    tool: "codex",
    version: firstLine(version.text),
    auth: login.status === 0 ? firstLine(login.text) : `NOT SIGNED IN (codex login): ${firstLine(login.text)}`,
    models: models.status === 0 ? codexModelsOf(JSON.parse(models.text.slice(models.text.indexOf("{")))) : [],
    efforts: null,
    call: await tinyCall("codex"),
  };
}

async function copilot() {
  const version = read("copilot", ["--version"]);
  if (!version) return { tool: "copilot", version: null };
  const help = read("copilot", ["--help"]).text;
  const call = await tinyCall("copilot");
  return {
    tool: "copilot",
    version: firstLine(version.text),
    auth: call.ok ? "signed in (the tiny call answered)" : "NOT SIGNED IN or no access (copilot login)",
    models: copilotModelsOf(read("copilot", ["help", "config"]).text),
    efforts: possibleValues(help, "--reasoning-effort"),
    contexts: possibleValues(help, "--context"),
    call,
  };
}

const entries = await Promise.all([codex(), copilot()]);
console.log(doctorReport(entries));
process.exit(entries.every((e) => e.version && e.call.ok) ? 0 : 1);
