import assert from "node:assert/strict";
import { test } from "node:test";
import { execArgs, parseArgs, promptOf, report, sandboxOf, summarize } from "./codex.mjs";

// [argv, parsed or null]
const argCases = [
  [["review", "abc", "t.md"], { mode: "review", commit: "abc", task: "t.md", model: "astra", effort: "ultra", name: null }],
  [["build", "abc", "t.md", "--model", "sol", "--effort", "max", "--name", "x"], { mode: "build", commit: "abc", task: "t.md", model: "sol", effort: "max", name: "x" }],
  [["fix", "abc", "t.md"], null],
  [["review", "abc"], null],
  [["review", "abc", "t.md", "--model"], null],
  [["review", "abc", "t.md", "--model", "gpt-9"], null],
  [["review", "abc", "t.md", "--effort", "extreme"], null],
  [["review", "abc", "t.md", "--bogus", "1"], null],
];
for (const [argv, parsed] of argCases) {
  test(`args ${JSON.stringify(argv)}`, () => {
    if (parsed) assert.deepEqual(parseArgs(argv), parsed);
    else assert.throws(() => parseArgs(argv), /usage|unknown/);
  });
}

test("both modes write in a scratch copy and may run the tests", () => {
  assert.equal(sandboxOf("review"), "workspace-write");
  assert.equal(sandboxOf("build"), "workspace-write");
});

test("exec arguments ignore the user config and name the model, effort and sandbox", () => {
  const args = execArgs({ mode: "build", model: "astra", effort: "ultra" }, { cwd: "/c", lastMessage: "/m" });
  assert.deepEqual(args, ["exec", "--ignore-user-config", "-m", "gpt-6.2-astra", "-c", "model_reasoning_effort=ultra", "-s", "workspace-write", "-c", "sandbox_workspace_write.network_access=true", "-C", "/c", "--json", "-o", "/m"]);
});

test("the prompt is the brief, the task, then how it is judged", () => {
  const review = promptOf("BRIEF", "TASK", "review");
  assert.ok(review.indexOf("BRIEF") < review.indexOf("TASK") && review.indexOf("TASK") < review.indexOf("You work in a scratch copy"));
  assert.match(promptOf("B", "T", "build"), /scratch copy/);
  assert.match(review, /Prove each finding by running something/);
});

test("the summary counts commands, failed ones, messages and tokens", () => {
  const lines = [
    JSON.stringify({ type: "item.completed", item: { type: "command_execution", exit_code: 0 } }),
    JSON.stringify({ type: "item.completed", item: { type: "command_execution", exit_code: 1 } }),
    JSON.stringify({ type: "item.completed", item: { type: "agent_message", text: "ok" } }),
    JSON.stringify({ type: "turn.completed", usage: { input_tokens: 10, cached_input_tokens: 4, output_tokens: 2, reasoning_output_tokens: 1 } }),
    "not json",
    "",
  ];
  assert.deepEqual(summarize(lines), { commands: 2, failed: 1, messages: 1, usage: { input: 10, cached: 4, output: 2, reasoning: 1 } });
});

test("the report names the facts and the patch only when there is one", () => {
  const base = { args: { mode: "build", model: "astra", effort: "ultra" }, sha: "abcdef0123", seconds: 61.4, status: 0, stats: summarize([]), dir: "/d" };
  assert.match(report({ ...base, hasPatch: true }), /patch: \/d\/change\.patch/);
  assert.match(report({ ...base, hasPatch: false }), /patch: none/);
  assert.match(report({ ...base, status: 3, hasPatch: false }), /FAILED \(exit 3\)/);
});
