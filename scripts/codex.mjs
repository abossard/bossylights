// Calculations for `npm run codex` (docs/agents/codex.md): no files, no processes.

/** What each model is for, as the Codex model list says (`~/.codex/models_cache.json`). */
export const MODELS = {
  astra: "gpt-6.2-astra", // most capable, 272k context: the very hard tasks
  sol: "gpt-6.2-sol", // fast go-to, 500k context: reviews, big reads
  terra: "gpt-6.1-terra", // visual understanding and computer use
};

/** The largest context window the Codex model list gives (`max_context_window`). */
export const LONG_CONTEXT = 872000;

export const EFFORTS = ["low", "medium", "high", "xhigh", "max", "ultra"];

/** `<mode> <commit> <task-file> [--model astra|sol|terra] [--effort ultra] [--name label]`. Throws the usage text otherwise. */
export function parseArgs(argv) {
  const usage = "usage: npm run codex -- <review|build> <commit> <task-file> [--model astra|sol|terra] [--effort ultra] [--name label]";
  const [mode, commit, task, ...rest] = argv;
  if (!["review", "build"].includes(mode) || !commit || !task || commit.startsWith("-") || task.startsWith("-")) throw new Error(usage);
  const options = { model: "astra", effort: "ultra", name: null };
  for (let i = 0; i < rest.length; i += 2) {
    const key = rest[i]?.replace(/^--/, "");
    const value = rest[i + 1];
    if (!(key in options) || value === undefined) throw new Error(usage);
    options[key] = value;
  }
  if (!(options.model in MODELS)) throw new Error(`codex: unknown model ${options.model}: ${Object.keys(MODELS).join(", ")}`);
  if (!EFFORTS.includes(options.effort)) throw new Error(`codex: unknown effort ${options.effort}: ${EFFORTS.join(", ")}`);
  return { mode, commit, task, ...options };
}

/**
 * Both modes run in a scratch copy that is thrown away, so both may write there and run the
 * tests, the engine on a local port included. A review is told to prove a finding with a
 * test and not to fix it; only a build's diff is kept. The shared checkout is never touched.
 */
export function sandboxOf() {
  return "workspace-write";
}

/**
 * The `codex exec` arguments. The user's config is ignored: it starts six MCP servers on
 * every run and carries deprecated flags. The model and effort are named here instead.
 */
export function execArgs({ mode, model, effort }, { cwd, lastMessage }) {
  return [
    "exec",
    "--ignore-user-config",
    "-m", MODELS[model],
    "-c", `model_reasoning_effort=${effort}`,
    "-c", `model_context_window=${LONG_CONTEXT}`, // the models' own maximum; the default for astra and sol is 500k
    "-s", sandboxOf(mode),
    "-c", "sandbox_workspace_write.network_access=true", // the engine tests bind 127.0.0.1
    "-C", cwd,
    "--json",
    "-o", lastMessage,
  ];
}

/** The prompt: the shared brief, then the task, then how the result is judged. */
export function promptOf(brief, task, mode) {
  const ends =
    mode === "build"
      ? "You work in a scratch copy. Leave your change uncommitted in it. End with the exact commands that show every box passing, and what you could not check."
      : "You work in a scratch copy that is thrown away. Do not fix anything. Prove each finding by running something: write a failing test or run a command, and give file:line, the concrete failing scenario and the command. A finding you could not run says why. End with what you could not check.";
  return [brief.trim(), "# Task", task.trim(), "# How this is judged", ends].join("\n\n");
}

/** One line per fact from the `--json` event lines: turns, tool calls, tokens, the failed commands. */
export function summarize(lines) {
  let commands = 0;
  let failed = 0;
  let messages = 0;
  const usage = { input: 0, cached: 0, output: 0, reasoning: 0 };
  for (const line of lines) {
    let event;
    try {
      event = JSON.parse(line);
    } catch {
      continue;
    }
    if (event.type === "item.completed" && event.item?.type === "command_execution") {
      commands += 1;
      if (event.item.exit_code !== 0) failed += 1;
    }
    if (event.type === "item.completed" && event.item?.type === "agent_message") messages += 1;
    if (event.type === "turn.completed" && event.usage) {
      usage.input += event.usage.input_tokens ?? 0;
      usage.cached += event.usage.cached_input_tokens ?? 0;
      usage.output += event.usage.output_tokens ?? 0;
      usage.reasoning += event.usage.reasoning_output_tokens ?? 0;
    }
  }
  return { commands, failed, messages, usage };
}

/** The report the orchestrator reads: facts first, then where the files are. */
export function report({ args, sha, seconds, status, stats, dir, hasPatch }) {
  const { commands, failed, messages, usage } = stats;
  return [
    `codex ${args.mode} ${sha.slice(0, 7)}: ${status === 0 ? "finished" : `FAILED (exit ${status})`} in ${Math.round(seconds)} s`,
    `model ${MODELS[args.model]} at ${args.effort}; ${commands} commands (${failed} failed), ${messages} messages`,
    `tokens: ${usage.input} in (${usage.cached} cached), ${usage.output} out, ${usage.reasoning} reasoning`,
    `last message: ${dir}/last-message.md`,
    hasPatch ? `patch: ${dir}/change.patch (verify it with npm run verify on a copy, then apply)` : "patch: none",
  ].join("\n");
}
