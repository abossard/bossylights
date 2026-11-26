// Calculations for `npm run land` (docs/agents/orchestration.md, worker step 7): no files, no processes.

export const CI_RED = "CI red on main:";

/** Arguments: <wip/branch> <message-file> and an optional --last (the ticket's last slice). Throws the usage text otherwise. */
export function parseArgs(argv) {
  const last = argv.includes("--last");
  const rest = argv.filter((a) => a !== "--last");
  if (rest.length !== 2 || !/^wip\/[A-Za-z0-9][\w.-]*$/.test(rest[0]) || rest[1].startsWith("-")) {
    throw new Error("usage: npm run land -- <wip/branch> <message-file> [--last]");
  }
  return { branch: rest[0], messageFile: rest[1], last };
}

/** The folder of the worktree that has `branch` checked out, from `git worktree list --porcelain`, or null. */
export function worktreeOf(porcelain, branch) {
  const entry = porcelain.split("\n\n").find((block) => block.split("\n").includes(`branch refs/heads/${branch}`));
  return entry?.match(/^worktree (.+)$/m)?.[1] ?? null;
}

/** True when the message names an open "CI red on main:" issue of this repository by #number. */
export function namesIncident(message, issues) {
  const cited = new Set(Array.from(message.matchAll(/(?<![\w./#-])#([1-9]\d*)\b/g), (m) => Number(m[1])));
  return issues.some(({ number, title }) => title.startsWith(CI_RED) && cited.has(number));
}

const RED = ["failure", "timed_out", "startup_failure"];

/** Why CI on origin/main blocks a landing, or null. ci: the newest push run of ci.yml on `base`, or undefined. */
export function ciRefusal({ ci, base, issues, message }) {
  if (ci?.headSha !== base || ci.status !== "completed") return "CI has no finished run on origin/main yet: wait for it";
  if (RED.includes(ci.conclusion)) {
    return namesIncident(message, issues) ? null : `CI on origin/main is red: name the open "${CI_RED}" issue by #number in the message`;
  }
  return ci.conclusion === "success" ? null : `CI on origin/main ended ${ci.conclusion}: a finished green run is needed`;
}

/**
 * Why the passed check does not cover what land would push, or null. tree: the tree of the squash onto origin/main,
 * tipTree: the tree of the branch alone, record: what `npm run verify` recorded for `tree`, or null.
 */
export function recordRefusal({ branch, tree, tipTree, record }) {
  if (record?.tree === tree && record.suites?.length > 0 && record.mutants?.missed === 0) return null;
  if (tree !== tipTree) return `${branch} squashed onto origin/main is another tree than ${branch}: merge origin/main into it, push, verify again`;
  return `no passed "npm run verify -- <commit> --mutants" for the tree of ${branch}: verify its tip, then land`;
}

/**
 * Why `branch` may not land, or null. Facts, all read before anything is written:
 * remoteTip, localTip (null when absent), here (this checkout's folder), worktree (the branch's folder or null),
 * dirty, conflict, empty (the merge leaves origin/main's tree as it is), what recordRefusal and ciRefusal read.
 */
export function refusal({ branch, remoteTip, localTip, here, worktree, dirty, conflict, empty, tree, tipTree, record, ...ci }) {
  const folder = branch.replaceAll("/", "-");
  if (remoteTip === null) return `origin has no ${branch}: push it, the review reads the pushed branch`;
  if (localTip !== null && localTip !== remoteTip) return `local ${branch} differs from origin: push it or drop the local commits`;
  if (worktree !== null && worktree.split("/").at(-1) !== folder) return `the worktree of ${branch} is ${worktree}: its folder must be named ${folder}`;
  if (worktree === here) return `this is the worktree of ${branch}: run land from another checkout`;
  if (dirty) return `the worktree of ${branch} has uncommitted changes`;
  if (conflict) return `${branch} does not merge cleanly onto origin/main: rebase it`;
  if (empty) return `${branch} changes nothing on origin/main: landed already, or no change`;
  return recordRefusal({ branch, tree, tipTree, record }) ?? ciRefusal(ci);
}

/**
 * The git steps after a landing, what is kept, and what was done. A slice keeps its branch and worktree:
 * both move onto the landed commit, the build folder stays. The last slice (`last`) removes them.
 * localTip and worktree are read after the push: a local branch that moved since the fetch keeps its commits
 * and worktree, and the remote branch goes only while it still points at the landed remoteTip.
 */
export function cleanupSteps({ branch, remoteTip, localTip, worktree, landed, last }) {
  const remote = ["push", `--force-with-lease=refs/heads/${branch}:${remoteTip}`, "origin", "--delete", branch];
  if (localTip !== null && localTip !== remoteTip) {
    return { steps: [remote], kept: [`local ${branch} moved after the fetch: it and its worktree are kept`], done: `origin's ${branch} removed` };
  }
  if (last) {
    const local = [...(worktree ? [["worktree", "remove", worktree]] : []), ...(localTip ? [["branch", "-D", branch]] : [])];
    return { steps: [...local, remote], kept: [], done: `${branch}${worktree ? ` and ${worktree}` : ""} removed` };
  }
  if (worktree) {
    return { steps: [["-C", worktree, "reset", "--keep", landed], remote], kept: [], done: `${worktree} is on ${landed.slice(0, 7)}, its build kept: the next slice starts there` };
  }
  const local = localTip ? [["branch", "-f", branch, landed]] : [];
  return { steps: [...local, remote], kept: [], done: localTip ? `${branch} is on ${landed.slice(0, 7)}` : `origin's ${branch} removed` };
}

/** What a refused push of main says: a moved main is the usual reason, anything else in full. */
export function pushFailure(stderr) {
  return /\[rejected\]|non-fast-forward|fetch first/.test(stderr) ? `main moved, run land again\n${stderr.trim()}` : stderr.trim();
}
