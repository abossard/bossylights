import assert from "node:assert/strict";
import { test } from "node:test";
import {
  codexArgs,
  codexModelsOf,
  copilotArgs,
  copilotModelsOf,
  doctorReport,
  lastMessageOf,
  ledgerRow,
  masked,
  parseArgs,
  patchFileOf,
  possibleValues,
  promptOf,
  report,
  runName,
  settingsOf,
  stampOf,
  summarizeCodex,
  summarizeCopilot,
  tokens,
} from "./peer.mjs";

const both = ["codex", "copilot"];
const none = { model: null, effort: null, name: null, out: null };

// [argv, parsed or the error it throws]
const argCases = [
  [["codex", "review", "abc", "t.md"], { tools: ["codex"], mode: "review", commit: "abc", task: "t.md", ...none }],
  [["copilot", "research", "abc", "t.md"], { tools: ["copilot"], mode: "research", commit: "abc", task: "t.md", ...none }],
  [["both", "build", "abc", "t.md", "--model", "sol", "--effort", "high", "--name", "t7.x_1", "--out", "/o"], { tools: both, mode: "build", commit: "abc", task: "t.md", model: "sol", effort: "high", name: "t7.x_1", out: "/o" }],
  [["codex", "review", "abc", "t.md", "--effort", "ultra"], { tools: ["codex"], mode: "review", commit: "abc", task: "t.md", ...none, effort: "ultra" }],
  [["copilot", "review", "abc", "t.md", "--effort", "none"], { tools: ["copilot"], mode: "review", commit: "abc", task: "t.md", ...none, effort: "none" }],
  [["claude", "review", "abc", "t.md"], /usage/],
  [["codex", "fix", "abc", "t.md"], /usage/],
  [["codex", "review", "abc"], /usage/],
  [["codex", "review", "-x", "t.md"], /usage/],
  [["codex", "review", "abc", "--model"], /usage/],
  [["codex", "review", "abc", "t.md", "--model"], /usage/],
  [["codex", "review", "abc", "t.md", "--model", "--effort"], /usage/],
  [["codex", "review", "abc", "t.md", "--bogus", "1"], /usage/],
  [["codex", "review", "abc", "t.md", "model", "sol"], /usage/],
  [["codex", "review", "abc", "t.md", "--model", "sol", "--model", "astra"], /usage/],
  [["codex", "review", "abc", "t.md", "--model", "gpt-9"], /unknown model gpt-9/],
  [["copilot", "review", "abc", "t.md", "--effort", "ultra"], /copilot takes no effort ultra/],
  [["both", "review", "abc", "t.md", "--effort", "ultra"], /copilot takes no effort ultra/],
  [["both", "review", "abc", "t.md", "--effort", "none"], /codex takes no effort none/],
  [["codex", "review", "abc", "t.md", "--name", "a/b"], /a name is letters/],
];
for (const [argv, expected] of argCases) {
  test(`args ${JSON.stringify(argv)}`, () => {
    if (expected instanceof RegExp) assert.throws(() => parseArgs(argv), expected);
    else assert.deepEqual(parseArgs(argv), expected);
  });
}

test("a refusal carries the usage text", () => assert.throws(() => parseArgs(["both", "review", "a", "t", "--effort", "ultra"]), /usage: npm run peer/));

// [tool, mode, model, effort, expected model, expected effort]
const settingCases = [
  ["codex", "review", null, null, "sol", "ultra"],
  ["codex", "build", null, null, "astra", "ultra"],
  ["codex", "research", null, null, "astra", "ultra"],
  ["copilot", "review", null, null, "sol", "max"],
  ["copilot", "build", null, null, "sol", "max"],
  ["copilot", "research", null, null, "sol", "max"],
  ["codex", "review", "astra", "xhigh", "astra", "xhigh"],
  ["copilot", "build", "astra", null, "astra", "max"],
  ["codex", "build", null, "low", "astra", "low"],
];
for (const [tool, mode, model, effort, m, e] of settingCases) {
  test(`settings of ${tool} ${mode} ${model} ${effort}`, () => assert.deepEqual(settingsOf(tool, { mode, model, effort }), { tool, mode, model: m, effort: e }));
}

// [tool, settings, expected arguments]
const execCases = [
  [
    "codex",
    { model: "astra", effort: "ultra" },
    ["exec", "--ignore-user-config", "-m", "gpt-6.2-astra", "-c", "model_reasoning_effort=ultra", "-c", "model_context_window=872000", "-s", "workspace-write", "-c", "sandbox_workspace_write.network_access=true", "-c", 'web_search="live"', "--add-dir", "/r/out", "-C", "/r/copy", "--json", "-o", "/r/last-message.md"],
  ],
  [
    "codex",
    { model: "terra", effort: "low" },
    ["exec", "--ignore-user-config", "-m", "gpt-6.1-terra", "-c", "model_reasoning_effort=low", "-c", "model_context_window=872000", "-s", "workspace-write", "-c", "sandbox_workspace_write.network_access=true", "-c", 'web_search="live"', "--add-dir", "/r/out", "-C", "/r/copy", "--json", "-o", "/r/last-message.md"],
  ],
  [
    "copilot",
    { model: "sol", effort: "max" },
    ["-p", "PROMPT", "--model", "gpt-6.2-sol", "--reasoning-effort", "max", "--context", "long_context", "--allow-all", "--no-ask-user", "--add-dir", "/r/out", "--share", "/r/transcript.md", "--log-dir", "/r/logs", "--output-format", "json", "--usage-output-file", "/r/usage.json"],
  ],
  [
    "copilot",
    { model: "astra", effort: "low" },
    ["-p", "PROMPT", "--model", "gpt-6.2-astra", "--reasoning-effort", "low", "--context", "long_context", "--allow-all", "--no-ask-user", "--add-dir", "/r/out", "--share", "/r/transcript.md", "--log-dir", "/r/logs", "--output-format", "json", "--usage-output-file", "/r/usage.json"],
  ],
];
for (const [tool, settings, expected] of execCases) {
  test(`${tool} arguments for ${settings.model} at ${settings.effort}`, () => {
    const args =
      tool === "codex"
        ? codexArgs(settings, { cwd: "/r/copy", lastMessage: "/r/last-message.md", outDir: "/r/out" })
        : copilotArgs(settings, { prompt: "PROMPT", outDir: "/r/out", runDir: "/r" });
    assert.deepEqual(args, expected);
  });
}

// [mode, a phrase its ending must carry]
const promptCases = [
  ["review", /Do not fix anything\. Prove each finding by running something/],
  ["review", /Leave the tests you wrote in the copy/],
  ["build", /Leave your change uncommitted in the copy\. End with the exact commands that show every box passing/],
  ["research", /Primary sources first/],
  ["research", /Link, do not copy/],
  ["research", /Every claim has a link/],
  ["research", /a secret, a token, a user name, a local path or a machine name/],
];
for (const [mode, phrase] of promptCases) {
  test(`the ${mode} prompt says ${phrase}`, () => assert.match(promptOf("B", "T", mode, "/o"), phrase));
}

for (const mode of ["review", "build", "research"]) {
  test(`the ${mode} prompt is the brief, the task, the out folder, then how it is judged`, () => {
    const p = promptOf("BRIEF\n", "\nTASK", mode, "/run/out");
    const at = ["BRIEF", "# Task", "TASK", "# Where kept files go", "/run/out", "# How this is judged"].map((s) => p.indexOf(s));
    assert.ok(at.every((i, n) => i >= 0 && (n === 0 || i > at[n - 1])), `${at}`);
  });
}

// [mode, patch file]
for (const [mode, file] of [["build", "change.patch"], ["review", "repro.patch"], ["research", "repro.patch"]]) {
  test(`a ${mode} keeps its diff as ${file}`, () => assert.equal(patchFileOf(mode), file));
}

const line = (event) => JSON.stringify(event);
const zero = { input: 0, cached: 0, output: 0, reasoning: 0 };

// [codex event lines, summary]
const codexCases = [
  [[], { commands: 0, failed: 0, messages: 0, usage: zero }],
  [
    [
      line({ type: "item.completed", item: { type: "command_execution", exit_code: 0 } }),
      line({ type: "item.completed", item: { type: "command_execution", exit_code: 1 } }),
      line({ type: "item.completed", item: { type: "agent_message", text: "ok" } }),
      line({ type: "turn.completed", usage: { input_tokens: 10, cached_input_tokens: 4, output_tokens: 2, reasoning_output_tokens: 1 } }),
      line({ type: "turn.completed", usage: { input_tokens: 5, output_tokens: 1 } }),
      "not json",
      "null",
      "",
    ],
    { commands: 2, failed: 1, messages: 1, usage: { input: 15, cached: 4, output: 3, reasoning: 1 } },
  ],
];
for (const [lines, expected] of codexCases) {
  test(`codex summary of ${lines.length} lines`, () => assert.deepEqual(summarizeCodex(lines), expected));
}

const start = (id, toolName, extra = {}) => line({ type: "tool.execution_start", data: { toolCallId: id, toolName, ...extra } });
const done = (id, data) => line({ type: "tool.execution_complete", data: { toolCallId: id, ...data } });
const message = (content, phase) => line({ type: "assistant.message", data: { content, ...(phase ? { phase } : {}) } });
const usageFile = {
  modelMetrics: {
    "gpt-6.2-sol": { usage: { inputTokens: 100, outputTokens: 7, cacheReadTokens: 60, cacheWriteTokens: 30, reasoningTokens: 3 } },
    "gpt-6-luna": { usage: { inputTokens: 10, outputTokens: 1, cacheReadTokens: 5, reasoningTokens: 0 } },
  },
};

// [copilot event lines, usage file, summary]
const copilotCases = [
  [[], null, { commands: 0, failed: 0, messages: 0, usage: zero }],
  [
    [
      start("a", "bash"),
      done("a", { success: true, shellExecution: { exitCode: 0 } }),
      start("b", "bash"),
      done("b", { success: true, shellExecution: { exitCode: 2 } }),
      start("c", "run", { shellToolInfo: {} }),
      done("c", { success: false }),
      start("d", "sql"),
      done("d", { success: false }),
      message(""),
      message("looking"),
      message("done", "final_answer"),
      "garbage",
    ],
    usageFile,
    { commands: 3, failed: 2, messages: 2, usage: { input: 110, cached: 65, output: 8, reasoning: 3 } },
  ],
];
for (const [lines, usage, expected] of copilotCases) {
  test(`copilot summary of ${lines.length} lines`, () => assert.deepEqual(summarizeCopilot(lines, usage), expected));
}

// [copilot event lines, last message]
const lastCases = [
  [[], ""],
  [[message("first"), message("second")], "second"],
  [[message("answer", "final_answer"), message("after")], "answer"],
  [[message("one"), message("")], "one"],
];
for (const [lines, expected] of lastCases) {
  test(`last message ${JSON.stringify(expected)}`, () => assert.equal(lastMessageOf(lines), expected));
}

// [count, as written]
const tokenCases = [[0, "0"], [950, "950"], [5900, "5.9k"], [14700, "14.7k"], [2480000, "2.48M"], [6900000, "6.90M"]];
for (const [n, text] of tokenCases) {
  test(`tokens ${n}`, () => assert.equal(tokens(n), text));
}

// [date, stamp]
const stampCases = [
  ["2026-11-19T22:59:03.633Z", "20261119-225903"],
  ["2026-01-02T03:04:05.000Z", "20260102-030405"],
];
for (const [iso, stamp] of stampCases) {
  test(`stamp of ${iso}`, () => assert.equal(stampOf(new Date(iso)), stamp));
}

// [name, mode, tool, folder]
const nameCases = [
  [null, "review", "codex", "review-abcdef0-codex-20261119-225903"],
  [null, "research", "copilot", "research-abcdef0-copilot-20261119-225903"],
  ["t7-sequences", "build", "codex", "t7-sequences-codex-20261119-225903"],
];
for (const [name, mode, tool, folder] of nameCases) {
  test(`run folder ${folder}`, () => assert.equal(runName({ name, mode, sha: "abcdef0123", tool, stamp: "20261119-225903" }), folder));
}

const stats = { commands: 75, failed: 8, messages: 4, usage: { input: 6900000, cached: 6760000, output: 14700, reasoning: 900 } };
const date = new Date("2026-11-19T10:00:00Z");

// [settings, name, ledger row]
const rowCases = [
  [{ tool: "codex", mode: "review", model: "sol", effort: "ultra" }, null, "| 2026-11-19 | codex | review of abcdef0 | review | sol, ultra | 463 | 75 (8) | 6.90M / 6.76M / 14.7k | n/a | ? |"],
  [{ tool: "copilot", mode: "build", model: "sol", effort: "max" }, "t7", "| 2026-11-19 | copilot | t7 | build | sol, max | 463 | 75 (8) | 6.90M / 6.76M / 14.7k | ? | ? |"],
];
for (const [settings, name, row] of rowCases) {
  test(`ledger row of ${settings.tool} ${settings.mode}`, () => assert.equal(ledgerRow({ settings, sha: "abcdef0123", seconds: 462.6, stats, date, name }), row));
}

const reportBase = { sha: "abcdef0123", seconds: 61.4, status: 0, stats, dir: "/d", hasPatch: false, outFiles: 0, date, name: null };
// [settings and changes, phrase the report must carry]
const reportCases = [
  [{ settings: { tool: "codex", mode: "review", model: "sol", effort: "ultra" } }, /^codex review abcdef0: finished in 61 s$/m],
  [{ settings: { tool: "codex", mode: "review", model: "sol", effort: "ultra" } }, /^model gpt-6\.2-sol at ultra, context 872000; 75 commands \(8 failed\), 4 messages$/m],
  [{ settings: { tool: "copilot", mode: "review", model: "sol", effort: "max" } }, /context long_context/],
  [{ settings: { tool: "codex", mode: "build", model: "astra", effort: "ultra" }, status: 3 }, /FAILED \(exit 3\)/],
  [{ settings: { tool: "codex", mode: "build", model: "astra", effort: "ultra" }, hasPatch: true }, /^patch: \/d\/change\.patch/m],
  [{ settings: { tool: "codex", mode: "review", model: "sol", effort: "ultra" }, hasPatch: true }, /^repro: \/d\/repro\.patch/m],
  [{ settings: { tool: "copilot", mode: "research", model: "sol", effort: "max" }, hasPatch: true }, /^repro: \/d\/repro\.patch/m],
  [{ settings: { tool: "codex", mode: "build", model: "astra", effort: "ultra" } }, /^patch: none$/m],
  [{ settings: { tool: "codex", mode: "review", model: "sol", effort: "ultra" }, outFiles: 3 }, /^out: 3 files in \/d\/out$/m],
  [{ settings: { tool: "codex", mode: "review", model: "sol", effort: "ultra" }, outFiles: 1 }, /^out: 1 file in \/d\/out$/m],
  [{ settings: { tool: "codex", mode: "review", model: "sol", effort: "ultra" } }, /^out: none$/m],
  [{ settings: { tool: "codex", mode: "review", model: "sol", effort: "ultra" } }, /^last message: \/d\/last-message\.md$/m],
  [{ settings: { tool: "codex", mode: "review", model: "sol", effort: "ultra" } }, /\n\| 2026-11-19 \| codex \| review of abcdef0 \|.*\|$/],
];
for (const [changes, phrase] of reportCases) {
  test(`report ${JSON.stringify(changes)} says ${phrase}`, () => assert.match(report({ ...reportBase, ...changes }), phrase));
}

test("codex models are the visible ones with context and efforts", () => {
  const json = {
    models: [
      { slug: "gpt-6.2-sol", visibility: "list", context_window: 500000, max_context_window: 872000, supported_reasoning_levels: [{ effort: "low" }, { effort: "ultra" }] },
      { slug: "codex-auto-review", visibility: "hide", context_window: 272000, max_context_window: 872000, supported_reasoning_levels: [] },
    ],
  };
  assert.deepEqual(codexModelsOf(json), [{ slug: "gpt-6.2-sol", context: 500000, max: 872000, efforts: ["low", "ultra"] }]);
  assert.deepEqual(codexModelsOf({}), []);
});

const configHelp = `  \`mouse\`: x\n\n  \`model\`: AI model to use; see \`copilot help providers\`.\n    - "claude-opus-5.5"\n    - "gpt-6.2-sol"\n    - "gpt-6.2-astra"\n\n  \`contextTier\`: tier\n    - "default"\n`;
// [config help, models]
const copilotModelCases = [
  [configHelp, ["claude-opus-5.5", "gpt-6.2-sol", "gpt-6.2-astra"]],
  ["no models here", []],
];
for (const [help, models] of copilotModelCases) {
  test(`copilot models of ${JSON.stringify(help.slice(0, 20))}`, () => assert.deepEqual(copilotModelsOf(help), models));
}

const help = `Options:
      --context <tier>
          Set the context window tier [possible values: default, long_context]
      --continue
          Resume the most recent session
  -i, --interactive <prompt>
          Start interactive mode
      --reasoning-effort <level>
          Set the reasoning effort level; supported levels depend on the model [possible values: none, minimal, low, medium, high, xhigh, max]
      --reasoning-other <x>
          No list here
`;
// [flag, values]
const valueCases = [
  ["--context", ["default", "long_context"]],
  ["--reasoning-effort", ["none", "minimal", "low", "medium", "high", "xhigh", "max"]],
  ["--continue", []],
  ["--reasoning-other", []],
  ["--missing", []],
];
for (const [flag, values] of valueCases) {
  test(`possible values of ${flag}`, () => assert.deepEqual(possibleValues(help, flag), values));
}

// [status line, masked]
const maskCases = [
  ["Logged in using ChatGPT", "Logged in using ChatGPT"],
  ["Logged in using an API key - sk-proj-***ABCD", "Logged in using an API key - <masked>"],
  ["token ghp_abc123XYZ and github_pat_11AA_bb", "token <masked> and <masked>"],
];
for (const [text, expected] of maskCases) {
  test(`masked ${JSON.stringify(text)}`, () => assert.equal(masked(text), expected));
}

const call = { model: "sol", effort: "low", ok: true, status: 0, seconds: 9.4, stats: { ...stats, usage: { input: 7100, cached: 3200, output: 5, reasoning: 0 } } };
// [entries, phrase the doctor must print]
const doctorCases = [
  [[{ tool: "codex", version: null }], /^codex: NOT INSTALLED/],
  [[{ tool: "codex", version: "codex-cli 0.169.0", auth: "key sk-abc", models: [{ slug: "gpt-6.2-sol", context: 500000, max: 872000, efforts: ["low", "ultra"] }], efforts: null, call }], /^codex: codex-cli 0\.169\.0; auth: key <masked>\n  models: gpt-6\.2-sol \(500k, up to 872k; low ultra\)\n  tiny call \(sol at low\): ok in 9 s, tokens 7\.1k in \/ 3\.2k cached \/ 5 out$/],
  [[{ tool: "copilot", version: "GitHub Copilot CLI 1.0.115-1.", auth: "signed in", models: ["gpt-6.2-sol"], efforts: ["low", "max"], contexts: ["default", "long_context"], call: { ...call, ok: false, status: 1 } }], /models: gpt-6\.2-sol\n  efforts: low, max; context tiers: default, long_context\n  tiny call \(sol at low\): FAILED \(exit 1\)/],
];
for (const [entries, phrase] of doctorCases) {
  test(`doctor ${entries[0].tool} ${entries[0].version}`, () => assert.match(doctorReport(entries), phrase));
}
