// `npm run land` as a worker runs it (#335): the real script in a throwaway repository, with fake gh and npm.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { recordPath } from "./verify.mjs";

const script = join(import.meta.dirname, "land-run.mjs");
const fakes = {
  // CI is green for every commit asked about, no "CI red on main" issue is open
  gh: `const args = process.argv.slice(2);
const commit = args[args.indexOf("--commit") + 1];
console.log(JSON.stringify(args[0] === "issue" ? [] : [{ headSha: commit, status: "completed", conclusion: "success", url: "https://github.com/o/r/actions/runs/7" }]));`,
  // land runs no check: any npm call is logged
  npm: `require("node:fs").appendFileSync(process.env.FAKE_NPM, process.argv.slice(2).join(" ") + "\\n");
process.exit(1);`,
};

/** origin, its main checkout, wip-x on wip/x (one commit, pushed), main moved past it, and the worktree other. */
function world() {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "land-run-")));
  const bin = join(root, "bin");
  mkdirSync(bin);
  for (const [name, body] of Object.entries(fakes)) {
    writeFileSync(join(bin, name), `#!${process.execPath}\n${body}\n`);
    chmodSync(join(bin, name), 0o755);
  }
  const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith("GIT_")));
  Object.assign(env, { PATH: `${bin}:${process.env.PATH}`, GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_NOSYSTEM: "1", GIT_AUTHOR_NAME: "t", GIT_AUTHOR_EMAIL: "t@t", GIT_COMMITTER_NAME: "t", GIT_COMMITTER_EMAIL: "t@t", FAKE_NPM: join(root, "npm.log") });
  const git = (cwd, ...args) => {
    const r = spawnSync("git", args, { cwd, env, encoding: "utf8" });
    assert.equal(r.status, 0, r.stderr);
    return r.stdout.trim();
  };
  const commit = (cwd, file) => {
    writeFileSync(join(cwd, file), `${file}\n`);
    git(cwd, "add", ".");
    git(cwd, "commit", "-q", "-m", file);
  };
  const main = join(root, "main");
  const wip = join(root, "wip-x");
  const other = join(root, "other");
  git(root, "init", "-q", "--bare", "-b", "main", "origin.git");
  git(root, "clone", "-q", "origin.git", "main");
  commit(main, "first");
  git(main, "push", "-q", "origin", "main");
  git(main, "worktree", "add", "-q", "-b", "wip/x", wip, "origin/main");
  commit(wip, "slice");
  git(wip, "push", "-q", "origin", "wip/x");
  commit(main, "moved");
  git(main, "push", "-q", "origin", "main");
  git(main, "worktree", "add", "-q", "--detach", other, "main");
  writeFileSync(join(root, "message"), "#335 a slice\n");
  const base = git(main, "rev-parse", "main");
  const tip = git(wip, "rev-parse", "HEAD");
  return {
    root, env, git, base,
    squash: git(main, "merge-tree", "--write-tree", base, tip),
    tipTree: git(wip, "rev-parse", "HEAD^{tree}"),
    record: (tree) => {
      const path = recordPath(join(main, ".git"), tree); // the git folder every worktree shares, where verify writes it
      mkdirSync(join(path, ".."), { recursive: true });
      writeFileSync(path, JSON.stringify({ tree, commit: tip, base, suites: ["test:checks"], left: [], mutants: { caught: 1, missed: 0, timeout: 0, unviable: 0 } }));
    },
    // land from the worktree other: neither the branch's own nor the main checkout
    land: () => {
      const child = spawn(process.execPath, [script, "wip/x", join(root, "message")], { cwd: other, env, stdio: "pipe" });
      let out = "";
      child.stdout.on("data", (d) => (out += d));
      child.stderr.on("data", (d) => (out += d));
      return { child, ended: new Promise((ended) => child.on("close", (status) => ended({ status, out }))) };
    },
    originMain: () => git(main, "ls-remote", "origin", "refs/heads/main").split("\t")[0],
  };
}

// [the tree verify recorded, lands]
const landCases = [
  ["the squash onto the moved main", true],
  ["the branch alone", false],
];
for (const [recorded, lands] of landCases) {
  // asks CI right after the push: no 30 s wait before the first answer
  test(`land run: recorded is ${recorded}: ${lands ? "lands that tree onto main, no check run again" : "refused, nothing pushed"}`, { timeout: 20_000 }, async (t) => {
    const w = world();
    try {
      assert.notEqual(w.squash, w.tipTree);
      w.record(lands ? w.squash : w.tipTree);
      const landing = w.land();
      t.signal.addEventListener("abort", () => landing.child.kill());
      const ran = await landing.ended;
      const out = ran.out;
      assert.equal(existsSync(w.env.FAKE_NPM), false, "land ran npm");
      if (lands) {
        assert.equal(ran.status, 0, out);
        const landed = w.originMain();
        assert.equal(w.git(w.root, "--git-dir=origin.git", "rev-parse", `${landed}^{tree}`), w.squash);
        assert.equal(w.git(w.root, "--git-dir=origin.git", "rev-parse", `${landed}^`), w.base);
        assert.match(out, /CI on main is green/);
      } else {
        assert.equal(ran.status, 1, out);
        assert.match(out, /another tree than wip\/x/);
        assert.equal(w.originMain(), w.base);
      }
    } finally {
      rmSync(w.root, { recursive: true, force: true });
    }
  });
}
