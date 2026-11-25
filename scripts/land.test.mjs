import assert from "node:assert/strict";
import { test } from "node:test";
import { ciRefusal, cleanupSteps, namesIncident, parseArgs, pushFailure, recordRefusal, refusal, worktreeOf } from "./land.mjs";

// [argv, parsed or null]
const argCases = [
  [["wip/ci-one-command", "msg.txt"], { branch: "wip/ci-one-command", messageFile: "msg.txt" }],
  [["wip/250-big-cut", "/tmp/m"], { branch: "wip/250-big-cut", messageFile: "/tmp/m" }],
  [["main", "msg.txt"], null],
  [["feature/x", "msg.txt"], null],
  [["wip/", "msg.txt"], null],
  [["wip/a b", "msg.txt"], null],
  [["wip/x"], null],
  [["wip/x", "-m"], null],
  [["wip/x", "msg.txt", "extra"], null],
];
for (const [argv, parsed] of argCases) {
  test(`land args ${JSON.stringify(argv)}`, () => {
    if (parsed) assert.deepEqual(parseArgs(argv), parsed);
    else assert.throws(() => parseArgs(argv), /usage/);
  });
}

const porcelain = [
  "worktree /p/bossylights\nHEAD 1111\nbranch refs/heads/main",
  "worktree /p/wip-ci-one-command\nHEAD 2222\nbranch refs/heads/wip/ci-one-command",
  "worktree /tmp/copy\nHEAD 3333\ndetached",
].join("\n\n");
// [branch, its worktree or null]
const worktreeCases = [
  ["wip/ci-one-command", "/p/wip-ci-one-command"],
  ["wip/ci-one", null],
  ["wip/theory-channels", null],
];
for (const [branch, folder] of worktreeCases) {
  test(`worktree of ${branch}`, () => assert.equal(worktreeOf(porcelain, branch), folder));
}

const incident = { number: 27, title: "CI red on main: Linux shell" };
// [message, open issues, names an incident]
const incidentCases = [
  ["fix (#27)", [incident], true],
  ["repair\n\nRefs #27.", [incident], true],
  ["#271", [incident], false],
  ["some/other#27", [incident], false],
  ["ordinary", [incident], false],
  ["#27", [], false],
  ["#27", [{ number: 27, title: "Watch CI red on main" }], false],
];
for (const [message, issues, named] of incidentCases) {
  test(`incident in ${JSON.stringify(message)} with ${issues.length} issue(s)`, () => assert.equal(namesIncident(message, issues), named));
}

const base = "a".repeat(40);
const green = { headSha: base, status: "completed", conclusion: "success" };
const red = { ...green, conclusion: "failure" };
const ciOk = { base, ci: green, issues: [incident], message: "ordinary" };
// [name, CI facts, refusal pattern or null]
const ciCases = [
  ["green", {}, null],
  ["no run", { ci: undefined }, /no finished run/],
  ["run of another commit", { ci: { ...green, headSha: "b".repeat(40) } }, /no finished run/],
  ["running", { ci: { ...green, status: "in_progress", conclusion: "" } }, /no finished run/],
  ["red, no issue named", { ci: red }, /is red/],
  ["red, open issue named", { ci: red, message: "fix (#27)" }, null],
  ["timed out, open issue named", { ci: { ...red, conclusion: "timed_out" }, message: "#27" }, null],
  ["could not start, open issue named", { ci: { ...red, conclusion: "startup_failure" }, message: "#27" }, null],
  ["could not start, no issue named", { ci: { ...red, conclusion: "startup_failure" } }, /is red/],
  ["red, issue closed", { ci: red, issues: [], message: "#27" }, /is red/],
  ["cancelled, issue named", { ci: { ...red, conclusion: "cancelled" }, message: "#27" }, /ended cancelled/],
];
for (const [name, facts, refused] of ciCases) {
  test(`land CI: ${name}`, () => {
    const why = ciRefusal({ ...ciOk, ...facts });
    if (refused) assert.match(why, refused);
    else assert.equal(why, null);
  });
}

const tree = "f".repeat(40);
const passed = { tree, commit: "c".repeat(40), suites: ["test:checks"], mutants: { caught: 1, missed: 0, timeout: 0, unviable: 0 } };
const same = { branch: "wip/x", tree, tipTree: tree, record: passed };
// [name, record facts that differ from a verified branch on the newest main, refusal pattern or null]
const recordCases = [
  ["same tree, its record: no check again", {}, null],
  ["same tree, no record", { record: null }, /no passed "npm run verify -- <commit> --mutants" for the tree of wip\/x/],
  ["another tree: main moved since the branch left it", { tree: "9".repeat(40), record: null }, /merge origin\/main into it, push, verify again/],
  ["another tree with the record of the branch alone", { tree: "9".repeat(40) }, /merge origin\/main into it, push, verify again/],
  ["a record of another tree under this tree's name", { record: { ...passed, tree: "8".repeat(40) } }, /no passed/],
  ["a record with missed mutants (by hand)", { record: { ...passed, mutants: { ...passed.mutants, missed: 3 } } }, /no passed/],
  ["a record without mutants (by hand)", { record: { tree, commit: passed.commit, suites: passed.suites } }, /no passed/],
  ["a record with no suite (by hand)", { record: { ...passed, suites: [] } }, /no passed/],
];
for (const [name, facts, refused] of recordCases) {
  test(`land record: ${name}`, () => {
    const why = recordRefusal({ ...same, ...facts });
    if (refused) assert.match(why, refused);
    else assert.equal(why, null);
  });
}

const tip = "c".repeat(40);
const ready = {
  branch: "wip/x", remoteTip: tip, localTip: tip, empty: false, here: "/p/bossylights",
  worktree: "/p/wip-x", dirty: false, conflict: false, ...same, ...ciOk,
};
// [name, facts that differ from a branch ready to land, refusal pattern or null]
const branchCases = [
  ["ready", {}, null],
  ["ready, no local branch or worktree", { localTip: null, worktree: null }, null],
  ["not pushed", { remoteTip: null }, /origin has no wip\/x/],
  ["local commits not pushed", { localTip: "d".repeat(40) }, /differs from origin/],
  ["nothing to land: the merge leaves main's tree as it is (landed already, or no change)", { empty: true }, /changes nothing on origin\/main/],
  ["worktree folder named otherwise", { worktree: "/p/ci" }, /wip-x/],
  ["worktree folder keeps the slash's place", { worktree: "/p/wip/x" }, /wip-x/],
  ["run from its own worktree", { here: "/p/wip-x" }, /another checkout/],
  ["uncommitted changes", { dirty: true }, /uncommitted/],
  ["conflict with main", { conflict: true }, /rebase/],
  ["CI red", { ci: red }, /is red/],
  ["not verified", { record: null }, /no passed/],
];
for (const [name, facts, refused] of branchCases) {
  test(`land: ${name}`, () => {
    const why = refusal({ ...ready, ...facts });
    if (refused) assert.match(why, refused);
    else assert.equal(why, null);
  });
}

const late = "e".repeat(40);
const lease = ["push", `--force-with-lease=refs/heads/wip/x:${tip}`, "origin", "--delete", "wip/x"];
// [name, local branch and worktree after the push, git steps, what is kept]
const cleanupCases = [
  ["local at the landed tip", { localTip: tip, worktree: "/p/wip-x" }, [["worktree", "remove", "/p/wip-x"], ["branch", "-D", "wip/x"], lease], []],
  ["no local branch, no worktree", { localTip: null, worktree: null }, [lease], []],
  ["local branch, no worktree", { localTip: tip, worktree: null }, [["branch", "-D", "wip/x"], lease], []],
  ["a commit after the fetch", { localTip: late, worktree: "/p/wip-x" }, [lease], [/local wip\/x moved/]],
];
for (const [name, after, steps, kept] of cleanupCases) {
  test(`land cleanup: ${name}`, () => {
    const plan = cleanupSteps({ branch: "wip/x", remoteTip: tip, ...after });
    assert.deepEqual(plan.steps, steps);
    assert.equal(plan.kept.length, kept.length);
    kept.forEach((pattern, i) => assert.match(plan.kept[i], pattern));
  });
}

// [push stderr, message]
const pushCases = [
  [" ! [rejected]        abc -> main (fetch first)\nerror: failed to push some refs", /^main moved, run land again/],
  [" ! [rejected]        abc -> main (non-fast-forward)", /^main moved, run land again/],
  ["remote: Permission denied\nfatal: unable to access", /Permission denied\nfatal: unable to access/],
];
for (const [stderr, message] of pushCases) {
  test(`land push refused: ${JSON.stringify(stderr.slice(0, 30))}`, () => assert.match(pushFailure(stderr), message));
}
