// `npm run verify` as a worker runs it (#335): the real script in a throwaway repository, with fake gh, npm and cargo.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { chmodSync, closeSync, constants, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { open } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { recordPath } from "./verify.mjs";

const script = join(import.meta.dirname, "verify-run.mjs");
const fakes = {
  cargo: `console.log(JSON.stringify({ packages: [] }));`,
  // npm ci makes node_modules; a suite exits with what its gate (a fifo the test writes) or FAKE_SUITE says
  npm: `const fs = require("node:fs");
const [command] = process.argv.slice(2);
if (command === "ci") fs.mkdirSync("node_modules", { recursive: true });
else process.exit(Number(process.env.FAKE_GATE ? fs.readFileSync(process.env.FAKE_GATE, "utf8") : process.env.FAKE_SUITE));`,
  // the mutants job: every call logged, its result FAKE_MISSED missed mutants
  gh: `const fs = require("node:fs");
const args = process.argv.slice(2);
fs.appendFileSync(process.env.FAKE_LOG, JSON.stringify(args) + "\\n");
if (args[0] === "workflow") console.log("https://github.com/o/r/actions/runs/7");
if (args[1] === "download") {
  const out = args[args.indexOf("--dir") + 1];
  const missed = Number(process.env.FAKE_MISSED);
  const mutant = { file: "scripts/a.mjs", span: { start: { line: 1 } }, name: "scripts/a.mjs: replace a" };
  fs.mkdirSync(out + "/mutants.out", { recursive: true });
  fs.writeFileSync(out + "/exit", missed ? "2" : "0");
  fs.writeFileSync(out + "/mutants.out/outcomes.json", JSON.stringify({ caught: 1, missed, timeout: 0, unviable: 0,
    outcomes: Array.from({ length: missed }, () => ({ summary: "MissedMutant", scenario: { Mutant: mutant } })) }));
}`,
};

/** origin, its main checkout and the worktree wip-x on wip/x, two commits past main. */
function world() {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "verify-run-")));
  const bin = join(root, "bin");
  mkdirSync(bin);
  for (const [name, body] of Object.entries(fakes)) {
    writeFileSync(join(bin, name), `#!${process.execPath}\n${body}\n`);
    chmodSync(join(bin, name), 0o755);
  }
  const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith("GIT_")));
  Object.assign(env, { PATH: `${bin}:${process.env.PATH}`, GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_NOSYSTEM: "1", GIT_AUTHOR_NAME: "t", GIT_AUTHOR_EMAIL: "t@t", GIT_COMMITTER_NAME: "t", GIT_COMMITTER_EMAIL: "t@t", FAKE_LOG: join(root, "gh.log"), FAKE_SUITE: "0", FAKE_MISSED: "0", TMPDIR: root });
  const git = (cwd, ...args) => {
    const r = spawnSync("git", args, { cwd, env, encoding: "utf8" });
    assert.equal(r.status, 0, r.stderr);
    return r.stdout.trim();
  };
  const main = join(root, "main");
  const wip = join(root, "wip-x");
  git(root, "init", "-q", "--bare", "-b", "main", "origin.git");
  git(root, "clone", "-q", "origin.git", "main");
  writeFileSync(join(main, "package.json"), JSON.stringify({ scripts: { test: "npm run test:checks" } }));
  writeFileSync(join(main, "package-lock.json"), "{}");
  writeFileSync(join(main, ".gitignore"), "node_modules\n");
  git(main, "add", ".");
  git(main, "commit", "-q", "-m", "main");
  git(main, "push", "-q", "origin", "main");
  git(main, "worktree", "add", "-q", "-b", "wip/x", wip, "origin/main");
  for (const line of ["a", "b"]) {
    writeFileSync(join(wip, "a.mjs"), `${line}\n`, { flag: "a" });
    git(wip, "add", ".");
    git(wip, "commit", "-q", "-m", line);
  }
  const commit = git(wip, "rev-parse", "HEAD");
  const tree = git(wip, "rev-parse", "HEAD^{tree}");
  return {
    root, env, commit, fork: git(main, "rev-parse", "HEAD"),
    record: recordPath(join(main, ".git"), tree), // the git folder every worktree shares, where land reads it
    gh: () => readFileSync(env.FAKE_LOG, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l)),
    verify: (more = {}, flags = ["--mutants"]) => {
      const child = spawn(process.execPath, [script, commit, ...flags], { cwd: wip, env: { ...env, ...more }, stdio: "pipe" });
      let out = "";
      child.stdout.on("data", (d) => (out += d));
      child.stderr.on("data", (d) => (out += d));
      return new Promise((ended) => child.on("close", (status) => ended({ status, out })));
    },
  };
}

/** A run held in its suite until `release` gives the suite's exit code. inSuite: false when it ended before its suite. */
async function heldRun(w, name) {
  const gate = join(w.root, `${name}.gate`);
  assert.equal(spawnSync("mkfifo", [gate]).status, 0);
  const ended = w.verify({ FAKE_GATE: gate });
  const opening = open(gate, "w"); // opens once the suite reads it: the run is inside its suite
  const writer = await Promise.race([opening, ended.then(() => null)]);
  if (!writer) {
    const reader = openSync(gate, constants.O_RDONLY | constants.O_NONBLOCK); // lets the open above end
    await (await opening).close();
    closeSync(reader);
    return { inSuite: false, release: () => ended };
  }
  return { inSuite: true, release: async (code) => { await writer.write(String(code)); await writer.close(); return ended; } };
}

const stale = (w) => {
  mkdirSync(join(w.record, ".."), { recursive: true });
  writeFileSync(w.record, JSON.stringify({ tree: "stale" }));
};

// [run, its env, its flags, exit status, recorded]
const runCases = [
  ["every suite passed, no mutant missed", {}, ["--mutants"], 0, true],
  ["a suite failed", { FAKE_SUITE: "1" }, ["--mutants"], 1, false],
  ["a mutant missed", { FAKE_MISSED: "1" }, ["--mutants"], 1, false],
  ["every suite passed, without --mutants", {}, [], 0, false],
  ["a suite failed, without --mutants", { FAKE_SUITE: "1" }, [], 1, false],
];
for (const [name, more, flags, status, recorded] of runCases) {
  test(`verify run: ${name}: exit ${status}, ${recorded ? "records its tree" : "no record, an older one removed"}`, async () => {
    const w = world();
    try {
      stale(w);
      const ran = await w.verify(more, flags);
      assert.equal(ran.status, status, ran.out);
      assert.equal(existsSync(w.record), recorded, ran.out);
      if (recorded) assert.equal(JSON.parse(readFileSync(w.record, "utf8")).commit, w.commit);
    } finally {
      rmSync(w.root, { recursive: true, force: true });
    }
  });
}

test("verify run: the mutants of the diff from where the branch left main run on GitHub", async () => {
  const w = world();
  try {
    const ran = await w.verify();
    assert.equal(ran.status, 0, ran.out);
    const started = w.gh().find((args) => args[0] === "workflow");
    assert.deepEqual(started, ["workflow", "run", "mutants-diff.yml", "--ref", "main", "-f", `commit=${w.commit}`, "-f", `base=${w.fork}`]);
    assert.equal(JSON.parse(readFileSync(w.record, "utf8")).base, w.fork);
  } finally {
    rmSync(w.root, { recursive: true, force: true });
  }
});

// two runs of one tree at once: [older's suite, newer's suite, which ends first, recorded]
const overlapCases = [
  [0, 1, "newer", false],
  [1, 0, "newer", true],
  [0, 0, "newer", true],
  [1, 1, "newer", false],
  [0, 1, "older", false],
  [1, 0, "older", true],
];
for (const [older, newer, ends, recorded] of overlapCases) {
  test(`verify run: an older run ${older ? "red" : "green"} and a newer ${newer ? "red" : "green"} one, the ${ends} ends first: the newer decides, ${recorded ? "recorded" : "no record"}`, async () => {
    const w = world();
    try {
      stale(w);
      const first = await heldRun(w, "older");
      const removed = !existsSync(w.record);
      const second = await heldRun(w, "newer");
      const [olderEnd, newerEnd] = ends === "newer"
        ? await second.release(newer).then(async (n) => [await first.release(older), n])
        : await first.release(older).then(async (o) => [o, await second.release(newer)]);
      const out = `${olderEnd.out}\n${newerEnd.out}`;
      assert.ok(first.inSuite && second.inSuite, out);
      assert.ok(removed, "a run removes the record of its tree before its suites");
      assert.equal(newerEnd.status, newer, out);
      assert.equal(olderEnd.status, older, out);
      assert.equal(existsSync(w.record), recorded, out);
    } finally {
      rmSync(w.root, { recursive: true, force: true });
    }
  });
}

test("verify run: a record written while the newest run runs goes when that run ends red", async () => {
  const w = world();
  try {
    const held = await heldRun(w, "newest");
    stale(w); // what an older run writes in the instant after the newest started
    const ended = await held.release(1);
    assert.ok(held.inSuite, ended.out);
    assert.equal(ended.status, 1, ended.out);
    assert.equal(existsSync(w.record), false, ended.out);
  } finally {
    rmSync(w.root, { recursive: true, force: true });
  }
});
