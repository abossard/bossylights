// The one action that starts a peer CLI (docs/agents/peers.md), for `peer-run.mjs` and `peer-doctor.mjs`.
// It leaves in `dir`: prompt.md, events.jsonl, stderr.txt, last-message.md (and Copilot's usage.json,
// transcript.md, logs/). Events go straight to a file, so a long run holds nothing in memory.
import { spawn } from "node:child_process";
import { closeSync, existsSync, openSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { codexArgs, copilotArgs, lastMessageOf, summarizeCodex, summarizeCopilot } from "./peer.mjs";

/** Runs one CLI to its end. Resolves { status, seconds, stats }; a CLI that cannot start is status 127. */
export function runCli(settings, { cwd, dir, outDir, prompt, env = process.env }) {
  const events = join(dir, "events.jsonl");
  const stderr = join(dir, "stderr.txt");
  const lastMessage = join(dir, "last-message.md");
  writeFileSync(join(dir, "prompt.md"), prompt);
  const codex = settings.tool === "codex";
  const args = codex ? codexArgs(settings, { cwd, lastMessage, outDir }) : copilotArgs(settings, { prompt, outDir, runDir: dir });
  const out = openSync(events, "w");
  const err = openSync(stderr, "w");
  const started = Date.now();
  return new Promise((resolve) => {
    const child = spawn(settings.tool, args, { cwd, env, stdio: [codex ? "pipe" : "ignore", out, err] });
    if (codex) child.stdin.end(prompt);
    let failure = null;
    child.on("error", (error) => {
      failure = error;
    });
    child.on("close", (code) => {
      closeSync(out);
      closeSync(err);
      if (failure) writeFileSync(stderr, `${failure.message}\n`, { flag: "a" });
      const lines = readFileSync(events, "utf8").split("\n");
      const usageFile = join(dir, "usage.json");
      const usage = existsSync(usageFile) ? JSON.parse(readFileSync(usageFile, "utf8")) : null;
      if (!codex) writeFileSync(lastMessage, lastMessageOf(lines));
      else if (!existsSync(lastMessage)) writeFileSync(lastMessage, "");
      resolve({
        status: failure ? 127 : (code ?? 1),
        seconds: (Date.now() - started) / 1000,
        stats: codex ? summarizeCodex(lines) : summarizeCopilot(lines, usage),
        lastMessage: readFileSync(lastMessage, "utf8"),
      });
    });
  });
}
