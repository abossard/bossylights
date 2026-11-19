// Calculations for `npm run peer` and `npm run peer:doctor` (docs/agents/peers.md): no files, no processes.

export const TOOLS = ["codex", "copilot"];
export const MODES = ["review", "build", "research"];

/** Short names both CLIs accept (`codex debug models`, `copilot help config`). */
export const MODELS = {
  astra: "gpt-6.2-astra", // most capable: hard, long tasks with a check
  sol: "gpt-6.2-sol", // fast go-to: reviews, big reads
  terra: "gpt-6.1-terra", // visual understanding and computer use
};

/** The efforts each CLI takes. Copilot has no `ultra`, Codex no `none` or `minimal`. */
export const EFFORTS = {
  codex: ["low", "medium", "high", "xhigh", "max", "ultra"],
  copilot: ["none", "minimal", "low", "medium", "high", "xhigh", "max"],
};

/** Model and effort when none is named: [model, effort] per tool and mode. */
const DEFAULTS = {
  codex: { review: ["sol", "ultra"], build: ["astra", "ultra"], research: ["astra", "ultra"] },
  copilot: { review: ["sol", "max"], build: ["sol", "max"], research: ["sol", "max"] },
};

/** The largest Codex context window (`max_context_window`); the default is 500k. */
export const LONG_CONTEXT = 872000;
/** Copilot's long context tier (`copilot --help`, `--context`). */
export const COPILOT_CONTEXT = "long_context";

export const USAGE =
  "usage: npm run peer -- <codex|copilot|both> <review|build|research> <commit> <task-file> [--model astra|sol|terra] [--effort <level>] [--name label] [--out dir]";

/** The command line, or the usage text as an error. `--model` and `--effort` apply to every tool picked. */
export function parseArgs(argv) {
  const [tool, mode, commit, task, ...rest] = argv;
  const tools = tool === "both" ? TOOLS : TOOLS.filter((t) => t === tool);
  if (tools.length === 0 || !MODES.includes(mode) || !commit || !task || commit.startsWith("-") || task.startsWith("-")) throw new Error(USAGE);
  const options = { model: null, effort: null, name: null, out: null };
  for (let i = 0; i < rest.length; i += 2) {
    const key = rest[i]?.startsWith("--") ? rest[i].slice(2) : null;
    const value = rest[i + 1];
    if (!(key in options) || value === undefined || value.startsWith("-") || options[key] !== null) throw new Error(USAGE);
    options[key] = value;
  }
  if (options.model !== null && !(options.model in MODELS)) throw new Error(`peer: unknown model ${options.model}: ${Object.keys(MODELS).join(", ")}\n${USAGE}`);
  for (const t of tools) {
    if (options.effort !== null && !EFFORTS[t].includes(options.effort)) throw new Error(`peer: ${t} takes no effort ${options.effort}: ${EFFORTS[t].join(", ")}\n${USAGE}`);
  }
  if (options.name !== null && !/^[A-Za-z0-9._-]+$/.test(options.name)) throw new Error(`peer: a name is letters, digits, dot, dash, underscore: ${options.name}\n${USAGE}`);
  return { tools, mode, commit, task, ...options };
}

/** What one tool runs with: the named model and effort, else the default of its mode. */
export function settingsOf(tool, { mode, model, effort }) {
  const [defaultModel, defaultEffort] = DEFAULTS[tool][mode];
  return { tool, mode, model: model ?? defaultModel, effort: effort ?? defaultEffort };
}

/**
 * The `codex exec` arguments; the prompt goes on stdin. The user config is ignored (it starts
 * MCP servers on every run). Writes stay in the scratch copy and the out folder; the network is
 * open so the engine tests bind 127.0.0.1 and a research run can read the web.
 */
export function codexArgs({ model, effort }, { cwd, lastMessage, outDir }) {
  return [
    "exec",
    "--ignore-user-config",
    "-m", MODELS[model],
    "-c", `model_reasoning_effort=${effort}`,
    "-c", `model_context_window=${LONG_CONTEXT}`,
    "-s", "workspace-write",
    "-c", "sandbox_workspace_write.network_access=true",
    "-c", 'web_search="live"',
    "--add-dir", outDir,
    "-C", cwd,
    "--json",
    "-o", lastMessage,
  ];
}

/**
 * The `copilot` arguments; stdin is /dev/null. `--allow-all` gives every tool, path and URL
 * without a prompt (a prompt in a `-p` run is a denial). The working directory is the copy.
 */
export function copilotArgs({ model, effort }, { prompt, outDir, runDir }) {
  return [
    "-p", prompt,
    "--model", MODELS[model],
    "--reasoning-effort", effort,
    "--context", COPILOT_CONTEXT,
    "--allow-all",
    "--no-ask-user",
    "--add-dir", outDir,
    "--share", `${runDir}/transcript.md`,
    "--log-dir", `${runDir}/logs`,
    "--output-format", "json",
    "--usage-output-file", `${runDir}/usage.json`,
  ];
}

const ENDS = {
  review:
    "Do not fix anything. Prove each finding by running something: write a failing test or run a command, and give file:line, the concrete failing scenario and the command. Leave the tests you wrote in the copy: they are kept, so a worker starts from your red test. A finding you could not run says why. End with what you could not check.",
  build:
    "Leave your change uncommitted in the copy. End with the exact commands that show every box passing, and what you could not check.",
  research:
    "Primary sources first: the project's own docs, specs, source, release notes and issue tracker. Link, do not copy: summarise in your own words. Every claim has a link. Nothing meant for a public issue carries a secret, a token, a user name, a local path or a machine name. A claim you checked by running something names the command. End with what you could not check.",
};

/** The prompt: the shared brief, the task, where kept files go, then how the result is judged. */
export function promptOf(brief, task, mode, outDir) {
  const keep = `Your working directory is a scratch copy of the repository, thrown away after the run. Its diff is kept as a patch. Write any other evidence (logs, measurements, screenshots, notes) into ${outDir}: that folder is kept for the orchestrator.`;
  return [brief.trim(), "# Task", task.trim(), "# Where kept files go", keep, "# How this is judged", ENDS[mode]].join("\n\n");
}

/** The patch the copy's diff is kept as: a build's change, or a review's or research's repro. */
export const patchFileOf = (mode) => (mode === "build" ? "change.patch" : "repro.patch");

const parsed = (lines) =>
  lines.flatMap((line) => {
    try {
      const event = JSON.parse(line);
      return event && typeof event === "object" ? [event] : [];
    } catch {
      return [];
    }
  });

/** Codex `--json` event lines: commands, failed ones, messages, tokens. */
export function summarizeCodex(lines) {
  const stats = { commands: 0, failed: 0, messages: 0, usage: { input: 0, cached: 0, output: 0, reasoning: 0 } };
  for (const event of parsed(lines)) {
    if (event.type === "item.completed" && event.item?.type === "command_execution") {
      stats.commands += 1;
      if (event.item.exit_code !== 0) stats.failed += 1;
    }
    if (event.type === "item.completed" && event.item?.type === "agent_message") stats.messages += 1;
    if (event.type === "turn.completed" && event.usage) {
      stats.usage.input += event.usage.input_tokens ?? 0;
      stats.usage.cached += event.usage.cached_input_tokens ?? 0;
      stats.usage.output += event.usage.output_tokens ?? 0;
      stats.usage.reasoning += event.usage.reasoning_output_tokens ?? 0;
    }
  }
  return stats;
}

const SHELLS = ["bash", "shell", "powershell"];

/** Copilot `--output-format json` event lines and its `--usage-output-file` (or null): the same facts as Codex. */
export function summarizeCopilot(lines, usageFile) {
  const stats = { commands: 0, failed: 0, messages: 0, usage: { input: 0, cached: 0, output: 0, reasoning: 0 } };
  const shellCalls = new Set();
  for (const event of parsed(lines)) {
    const data = event.data ?? {};
    if (event.type === "tool.execution_start" && (SHELLS.includes(data.toolName) || data.shellToolInfo)) {
      shellCalls.add(data.toolCallId);
      stats.commands += 1;
    }
    if (event.type === "tool.execution_complete" && shellCalls.has(data.toolCallId)) {
      const exit = data.shellExecution?.exitCode;
      if (data.success === false || (exit !== undefined && exit !== 0)) stats.failed += 1;
    }
    if (event.type === "assistant.message" && data.content) stats.messages += 1;
  }
  for (const metrics of Object.values(usageFile?.modelMetrics ?? {})) {
    stats.usage.input += metrics.usage?.inputTokens ?? 0;
    stats.usage.cached += metrics.usage?.cacheReadTokens ?? 0;
    stats.usage.output += metrics.usage?.outputTokens ?? 0;
    stats.usage.reasoning += metrics.usage?.reasoningTokens ?? 0;
  }
  return stats;
}

/** Copilot's answer from its events: the final answer, else the last message with text. */
export function lastMessageOf(lines) {
  const messages = parsed(lines).filter((e) => e.type === "assistant.message" && e.data?.content);
  return (messages.findLast((e) => e.data.phase === "final_answer") ?? messages.at(-1))?.data.content ?? "";
}

/** A count as the ledger writes it: 950, 5.9k, 2.48M. */
export function tokens(n) {
  if (n >= 1e6) return `${(n / 1e6).toFixed(2)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}k`;
  return String(n);
}

/** The UTC time of a run as a folder stamp: 20261119-225903. */
export const stampOf = (date) => date.toISOString().slice(0, 19).replace(/[-:]/g, "").replace("T", "-");

/** The run folder's name: <name or mode-sha>-<tool>-<stamp>. */
export const runName = ({ name, mode, sha, tool, stamp }) => `${name ?? `${mode}-${sha.slice(0, 7)}`}-${tool}-${stamp}`;

/** One row of `docs/agents/peer-runs.md`. The orchestrator fills in the last two columns after verifying. */
export function ledgerRow({ settings, sha, seconds, stats, date, name }) {
  const { tool, mode, model, effort } = settings;
  const { usage } = stats;
  const task = name ?? `${mode} of ${sha.slice(0, 7)}`;
  const verify = mode === "build" ? "?" : "n/a";
  return `| ${date.toISOString().slice(0, 10)} | ${tool} | ${task} | ${mode} | ${model}, ${effort} | ${Math.round(seconds)} | ${stats.commands} (${stats.failed}) | ${tokens(usage.input)} / ${tokens(usage.cached)} / ${tokens(usage.output)} | ${verify} | ? |`;
}

/** The report the orchestrator reads: facts first, then where the files are, then the ledger row. */
export function report({ settings, sha, seconds, status, stats, dir, hasPatch, outFiles, date, name }) {
  const { tool, mode, model, effort } = settings;
  const { commands, failed, messages, usage } = stats;
  const context = tool === "codex" ? `context ${LONG_CONTEXT}` : `context ${COPILOT_CONTEXT}`;
  const patch = !hasPatch
    ? "patch: none"
    : mode === "build"
      ? `patch: ${dir}/change.patch (verify it with npm run verify on a copy, then apply)`
      : `repro: ${dir}/repro.patch (the tests and scripts it wrote: start from its red test)`;
  return [
    `${tool} ${mode} ${sha.slice(0, 7)}: ${status === 0 ? "finished" : `FAILED (exit ${status})`} in ${Math.round(seconds)} s`,
    `model ${MODELS[model]} at ${effort}, ${context}; ${commands} commands (${failed} failed), ${messages} messages`,
    `tokens: ${usage.input} in (${usage.cached} cached), ${usage.output} out, ${usage.reasoning} reasoning`,
    `last message: ${dir}/last-message.md`,
    patch,
    outFiles > 0 ? `out: ${outFiles} ${outFiles === 1 ? "file" : "files"} in ${dir}/out` : "out: none",
    "ledger row (docs/agents/peer-runs.md):",
    ledgerRow({ settings, sha, seconds, stats, date, name }),
  ].join("\n");
}

// --- npm run peer:doctor

/** The visible models of `codex debug models`: slug, context, largest context, efforts. */
export function codexModelsOf(json) {
  return (json.models ?? [])
    .filter((m) => m.visibility !== "hide")
    .map((m) => ({ slug: m.slug, context: m.context_window, max: m.max_context_window, efforts: (m.supported_reasoning_levels ?? []).map((l) => l.effort) }));
}

/** The model names listed under `model` in `copilot help config`. */
export function copilotModelsOf(configHelp) {
  const start = configHelp.search(/`model`:/);
  if (start === -1) return [];
  const names = [];
  for (const line of configHelp.slice(start).split("\n").slice(1)) {
    const m = line.match(/^\s*- "([^"]+)"\s*$/);
    if (!m) break;
    names.push(m[1]);
  }
  return names;
}

/** The `[possible values: ...]` of one flag in a `--help` text, e.g. `--context`. */
export function possibleValues(help, flag) {
  const block = help.split(/\n(?=\s*-{1,2}[A-Za-z])/).find((b) => b.trimStart().startsWith(`${flag} `));
  const m = block?.match(/\[possible values: ([^\]]+)\]/);
  return m ? m[1].split(",").map((v) => v.trim()) : [];
}

/** A status line with anything that looks like a key or token masked. */
export const masked = (text) => text.replace(/\b(sk|ghp|gho|ghu|ghs|github_pat)[-_][A-Za-z0-9_*.-]+/g, "<masked>");

const k = (n) => `${Math.round(n / 1000)}k`;

/** One block per tool for the doctor. `call` is null when the CLI is missing. */
export function doctorReport(entries) {
  return entries
    .map(({ tool, version, auth, models, efforts, contexts, call }) => {
      if (!version) return `${tool}: NOT INSTALLED (no \`${tool}\` on PATH)`;
      const list = models.map((m) => (typeof m === "string" ? m : `${m.slug} (${k(m.context)}, up to ${k(m.max)}; ${m.efforts.join(" ")})`));
      const lines = [`${tool}: ${version}; auth: ${masked(auth)}`, `  models: ${list.join(", ")}`];
      if (efforts) lines.push(`  efforts: ${efforts.join(", ")}; context tiers: ${contexts.join(", ")}`);
      const { usage } = call.stats;
      lines.push(
        `  tiny call (${call.model} at ${call.effort}): ${call.ok ? "ok" : `FAILED (exit ${call.status})`} in ${Math.round(call.seconds)} s, tokens ${tokens(usage.input)} in / ${tokens(usage.cached)} cached / ${tokens(usage.output)} out`,
      );
      return lines.join("\n");
    })
    .join("\n");
}
