// Measures every test on its own (#300, docs/testing.md). Writes to target/reach/, prints only a summary.
//   node scripts/test-reach-run.mjs rust                   one coverage profile per Rust test (needs `rustup component add llvm-tools`)
//   node scripts/test-reach-run.mjs node                   one V8 coverage per `node --test` test (ui/src, scripts)
//   node scripts/test-reach-run.mjs kills <pkg> [<pkg>…]   which test kills which mutant of the first package; the tests of all named packages run
//   node scripts/test-reach-run.mjs report                 inventory, double and triple coverage, the tests that add nothing: target/reach/report.md
import { execFile, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { availableParallelism } from "node:os";
import { basename, dirname, join, relative } from "node:path";
import { promisify } from "node:util";
import {
  KEEP,
  failedTests,
  markTests,
  multiplicity,
  mutantKills,
  rangeKeys,
  removable,
  rustReach,
  stableTests,
  testCode,
  testCodeAt,
  uniques,
  unusableRun,
  v8Ranges,
} from "./test-reach.mjs";

const ROOT = process.cwd();
const OUT = join(ROOT, "target/reach");
const run = promisify(execFile);
const BIG = { maxBuffer: 1 << 30 };

/** Runs jobs with at most `limit` at a time; prints progress every 50. */
async function pool(items, limit, job) {
  const results = new Array(items.length);
  let next = 0;
  let done = 0;
  const worker = async () => {
    while (next < items.length) {
      const i = next++;
      results[i] = await job(items[i], i);
      if (++done % 50 === 0) console.log(`  ${done}/${items.length}`);
    }
  };
  await Promise.all(Array.from({ length: limit }, worker));
  return results;
}

function llvmTool(name) {
  const sysroot = spawnSync("rustc", ["--print", "sysroot"], { encoding: "utf8" }).stdout.trim();
  const host = spawnSync("rustc", ["-vV"], { encoding: "utf8" }).stdout.match(/^host: (.*)$/m)[1];
  const path = join(sysroot, "lib/rustlib", host, "bin", name);
  if (!existsSync(path)) throw new Error(`${name} missing: rustup component add llvm-tools`);
  return path;
}

/** The test binaries and the programs of an instrumented build: { tests: [...], bins: [paths] }. */
function buildRust(target) {
  console.log("rust: building the tests with coverage (target/reach)");
  const built = spawnSync("cargo", ["test", "--workspace", "--no-run", "--message-format=json"], {
    encoding: "utf8",
    maxBuffer: 1 << 30,
    stdio: ["ignore", "pipe", "inherit"],
    // Build scripts run instrumented too: their profiles go to target/reach, not the repo root.
    env: { ...process.env, RUSTFLAGS: "-C instrument-coverage", CARGO_TARGET_DIR: target, LLVM_PROFILE_FILE: join(target, "build-%m.profraw") },
  });
  if (built.status !== 0) throw new Error(`cargo test --no-run failed (exit ${built.status})`);
  const artifacts = built.stdout
    .split("\n")
    .filter((l) => l.startsWith("{"))
    .map((l) => JSON.parse(l))
    .filter((m) => m.reason === "compiler-artifact" && m.executable);
  const binaries = artifacts
    .filter((m) => m.profile.test)
    .map((m) => {
      const dir = dirname(m.manifest_path);
      return { exe: m.executable, dir, package: basename(dir), src: relative(dir, m.target.src_path), stem: m.target.name.replaceAll("-", "_") };
    });
  const bins = artifacts.filter((m) => !m.profile.test && m.target.kind.includes("bin")).map((m) => m.executable);
  return { binaries, bins };
}

const listed = (exe, dir, extra = []) =>
  spawnSync(exe, ["--list", "--format", "terse", ...extra], {
    cwd: dir,
    encoding: "utf8",
    env: { ...process.env, LLVM_PROFILE_FILE: join(OUT, "prof", "list-%p.profraw") }, // not into the package folder
  })
    .stdout.split("\n")
    .filter((l) => l.endsWith(": test"))
    .map((l) => l.slice(0, -": test".length));

/** Where the test code of every product source is: (file, line) → true inside it. */
function rustTestCode() {
  const sources = ["crates", "shell"]
    .flatMap((dir) => readdirSync(join(ROOT, dir), { recursive: true }).map((f) => `${dir}/${f.split("\\").join("/")}`))
    .filter((f) => /^(crates\/[^/]+|shell)\/src\/.*\.rs$/.test(f));
  return testCodeAt(Object.fromEntries(sources.map((f) => [f, testCode(f, readFileSync(join(ROOT, f), "utf8"))])));
}

async function measureRust() {
  const target = OUT;
  const { binaries, bins } = buildRust(target);
  const tests = binaries.flatMap((b) => {
    const ignored = new Set(listed(b.exe, b.dir, ["--ignored"]));
    return listed(b.exe, b.dir)
      .filter((name) => !ignored.has(name))
      .map((name) => ({ ...b, name, id: `${b.package}/${b.src}::${name}` }));
  });
  console.log(`rust: ${tests.length} tests in ${binaries.length} binaries`);
  const profdata = llvmTool("llvm-profdata");
  const cov = llvmTool("llvm-cov");
  const isTest = rustTestCode();
  const prof = join(OUT, "prof");
  rmSync(prof, { recursive: true, force: true });
  mkdirSync(prof, { recursive: true });
  const keys = new Map();
  const intern = (k) => keys.get(k) ?? (keys.set(k, keys.size), keys.size - 1);
  const results = await pool(tests, availableParallelism(), async (t, i) => {
    const dir = join(prof, String(i));
    mkdirSync(dir);
    const started = performance.now();
    let ok = true;
    try {
      await run(t.exe, ["--exact", t.name, "--test-threads=1"], {
        cwd: t.dir,
        ...BIG,
        env: { ...process.env, LLVM_PROFILE_FILE: join(dir, "%p-%m.profraw"), CARGO_MANIFEST_DIR: t.dir },
      });
    } catch {
      ok = false;
    }
    const ms = Math.round(performance.now() - started);
    const raws = readdirSync(dir).map((f) => join(dir, f));
    if (raws.length === 0) return { id: t.id, package: t.package, src: t.src, stem: t.stem, name: t.name, ms, ok, reach: [] };
    await run(profdata, ["merge", "-sparse", ...raws, "-o", join(dir, "t.profdata")], BIG);
    const objects = [t.exe, ...bins].flatMap((o, n) => (n === 0 ? [o] : ["-object", o]));
    const { stdout } = await run(cov, ["export", "-format=text", "-skip-expansions", `-instr-profile=${join(dir, "t.profdata")}`, ...objects], BIG);
    rmSync(dir, { recursive: true, force: true });
    const reach = [...rustReach(JSON.parse(stdout), ROOT, isTest)].map(intern);
    return { id: t.id, package: t.package, src: t.src, stem: t.stem, name: t.name, ms, ok, reach };
  });
  writeFileSync(join(OUT, "rust.json"), JSON.stringify({ sources: sources(), keys: [...keys.keys()], tests: results }));
  const failed = results.filter((r) => !r.ok);
  console.log(`rust: ${results.length} measured, ${keys.size} regions reached, ${failed.length} failed alone: ${failed.map((r) => r.id).join(", ")}`);
}

const NODE_TESTS = ["ui/src", "scripts"];

function nodeTestNames(file) {
  const tap = spawnSync("node", ["--test", "--test-reporter=tap", file], { encoding: "utf8", maxBuffer: 1 << 28 }).stdout;
  return [...tap.matchAll(/^(?:not )?ok \d+ - (.*?)(?: # .*)?$\n\s+---\n\s+duration_ms: ([\d.]+)/gm)].map((m) => ({ name: m[1], ms: Math.round(Number(m[2])) }));
}

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

async function measureNode() {
  const files = NODE_TESTS.flatMap((d) => readdirSync(d).filter((f) => f.endsWith(".test.mjs")).map((f) => `${d}/${f}`));
  const tests = files.flatMap((file) => nodeTestNames(file).map((t) => ({ ...t, file, id: `${file}::${t.name}` })));
  console.log(`node: ${tests.length} tests in ${files.length} files`);
  const cov = join(OUT, "v8");
  rmSync(cov, { recursive: true, force: true });
  const keep = (f) => NODE_TESTS.some((d) => f.startsWith(`${d}/`)) && !f.endsWith(".test.mjs");
  const ranges = await pool(tests, availableParallelism(), async (t, i) => {
    const dir = join(cov, String(i));
    mkdirSync(dir, { recursive: true });
    let ok = true;
    try {
      await run("node", ["--test", `--test-name-pattern=^${escape(t.name)}$`, t.file], { ...BIG, env: { ...process.env, NODE_V8_COVERAGE: dir } });
    } catch {
      ok = false;
    }
    const merged = {};
    for (const f of readdirSync(dir)) {
      for (const [file, list] of Object.entries(v8Ranges(JSON.parse(readFileSync(join(dir, f), "utf8")), ROOT, keep))) {
        merged[file] = (merged[file] ?? []).concat(list);
      }
    }
    rmSync(dir, { recursive: true, force: true });
    return { ok, merged };
  });
  const keysOf = rangeKeys(Object.fromEntries(tests.map((t, i) => [t.id, ranges[i].merged])));
  const keys = new Map();
  const intern = (k) => keys.get(k) ?? (keys.set(k, keys.size), keys.size - 1);
  const results = tests.map((t, i) => ({ id: t.id, package: t.file, src: t.file, name: t.name, ms: t.ms, ok: ranges[i].ok, reach: [...keysOf[t.id]].map(intern) }));
  writeFileSync(join(OUT, "node.json"), JSON.stringify({ keys: [...keys.keys()], tests: results }));
  console.log(`node: ${results.length} measured, ${keys.size} pieces reached, ${results.filter((r) => !r.ok).length} failed alone`);
}

function measureKills(packages) {
  const [mutated] = packages;
  const out = join(OUT, `mutants-${mutated}`);
  // A fixed, long timeout: the auto one comes from a quick baseline and cuts a busy machine's test runs short, losing kills.
  const args = ["mutants", "--no-shuffle", "--package", mutated, ...packages.flatMap((p) => ["--test-package", p]), "--cargo-test-arg=--no-fail-fast", "--jobs", "4", "--timeout", "240", "-o", out];
  console.log(`kills: cargo ${args.join(" ")}`);
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, "packages.json"), JSON.stringify({ packages, sources: sources() }));
  const done = spawnSync("cargo", args, { stdio: ["ignore", "ignore", "inherit"] });
  if (![0, 2, 3].includes(done.status)) throw new Error(`cargo mutants failed (exit ${done.status})`);
  killMatrix(mutated, JSON.parse(readFileSync(join(OUT, "rust.json"), "utf8")).sources);
}

/** Reads a cargo-mutants run: which tests failed on each mutant. Writes kills-<crate>.json. Returns null, with the reason, while it cannot be used. */
function killMatrix(mutated, reachSources) {
  const out = join(OUT, `mutants-${mutated}`);
  const { packages, sources: runSources } = JSON.parse(readFileSync(join(out, "packages.json"), "utf8"));
  const results = JSON.parse(readFileSync(join(out, "mutants.out/outcomes.json"), "utf8"));
  const why = unusableRun(results, JSON.parse(readFileSync(join(out, "mutants.out/mutants.json"), "utf8")), runSources, reachSources);
  if (why) {
    console.log(`kills: ${mutated} left out, ${why}`);
    return null;
  }
  const tests = JSON.parse(readFileSync(join(OUT, "rust.json"), "utf8")).tests.filter((t) => packages.includes(t.package));
  const mutants = results.outcomes
    .filter((o) => o.scenario.Mutant)
    .map((o) => mutantKills(o, failedTests(readFileSync(join(out, "mutants.out", o.log_path), "utf8")), tests));
  writeFileSync(join(OUT, `kills-${mutated}.json`), JSON.stringify({ packages, mutants }, null, 1));
  const caught = mutants.filter((m) => m.outcome === "CaughtMutant");
  console.log(`kills: ${mutated}: ${mutants.length} mutants, ${caught.length} caught, ${mutants.filter((m) => m.killers.length === 1).length} by exactly one test`);
  return { crate: mutated, packages, mutants };
}

/** The Rust sources measured: git's tree hashes of crates/ and shell/, with "+dirty" when they have changes. */
function sources() {
  const trees = spawnSync("git", ["rev-parse", "HEAD:crates", "HEAD:shell"], { encoding: "utf8" }).stdout.trim().split("\n").join("+");
  const dirty = spawnSync("git", ["status", "--porcelain", "--", "crates", "shell"], { encoding: "utf8" }).stdout.trim();
  return dirty ? `${trees}+dirty` : trees;
}

function report() {
  const load = (f) => (existsSync(join(OUT, f)) ? JSON.parse(readFileSync(join(OUT, f), "utf8")) : { keys: [], tests: [] });
  const sets = (data) => data.tests.map((t) => ({ ...t, reach: new Set(t.reach.map((k) => data.keys[k])), kills: new Set() }));
  const node = sets(load("node.json"));
  const dirs = readdirSync(OUT).filter((d) => d.startsWith("mutants-") && existsSync(join(OUT, d, "mutants.out/outcomes.json")));
  const runs = dirs.map((d) => killMatrix(d.slice("mutants-".length), load("rust.json").sources)).filter(Boolean);
  const rust = markTests(sets(load("rust.json")), runs);
  const killed = new Set(runs.flatMap((r) => r.mutants.filter((m) => m.killers.length).map((m) => m.name)));
  const all = [...rust, ...node];
  const lines = [];
  const table = (head, rows) => lines.push(`| ${head.join(" | ")} |`, `|${head.map(() => "---").join("|")}|`, ...rows.map((r) => `| ${r.join(" | ")} |`), "");

  lines.push("## Inventory", "");
  const groups = Object.groupBy(all, (t) => t.package);
  table(
    ["Package or file", "Tests", "Time alone (ms, sum)", "Slowest"],
    Object.entries(groups)
      .sort((a, b) => b[1].length - a[1].length)
      .map(([p, ts]) => [p, ts.length, ts.reduce((s, t) => s + t.ms, 0), ts.toSorted((a, b) => b.ms - a.ms)[0].name]),
  );

  lines.push("## How many tests reach each piece of code", "");
  const mult = multiplicity(all);
  table(
    ["Crate or folder", "Pieces reached", "by 1 test", "by 2", "by 3", "by 4 to 9", "by 10 or more"],
    Object.entries(mult)
      .sort((a, b) => b[1].pieces - a[1].pieces)
      .map(([c, r]) => [c, r.pieces, r.once, r.twice, r.thrice, r.upTo9, r.tenOrMore]),
  );

  const uniq = uniques(all);
  const alone = all.filter((t) => uniq[t.id].reach === 0 && uniq[t.id].kills === 0);
  lines.push(`## Overlap`, "", `${alone.length} of ${all.length} tests reach no piece of code and kill no measured mutant that another test does not.`, "");

  const reason = (t) => KEEP.find(([rule]) => rule(t))?.[1] ?? null;
  const stable = stableTests(rust);
  const gone = removable(stable, reason, stable.toSorted((a, b) => b.ms - a.ms));
  lines.push("## Removal candidates (Rust; tests of packages being rewritten neither go nor cover)", "");
  table(
    ["Test", "Pieces", "Mutants it kills", "Still covered by"],
    gone.map((g) => [`\`${g.id}\``, g.reach, g.kills, g.coveredBy.map((c) => `\`${c}\``).join("<br/>")]),
  );
  const kept = Object.groupBy(rust.filter((t) => reason(t)), reason);
  table(["Kept because", "Tests"], Object.entries(kept).map(([r, ts]) => [r, ts.length]));
  lines.push(`Mutants killed by a measured test: ${killed.size}.`, "");
  writeFileSync(join(OUT, "report.md"), lines.join("\n"));
  writeFileSync(join(OUT, "removal.json"), JSON.stringify(gone, null, 1));
  console.log(`report: ${all.length} tests, ${alone.length} add nothing alone, ${gone.length} can go together. target/reach/report.md`);
}

if (import.meta.main) {
  const [what, ...rest] = process.argv.slice(2);
  mkdirSync(OUT, { recursive: true });
  if (what === "rust") await measureRust();
  else if (what === "node") await measureNode();
  else if (what === "kills" && rest.length) measureKills(rest);
  else if (what === "report") report();
  else {
    console.error("usage: node scripts/test-reach-run.mjs rust | node | kills <package> [<test package>…] | report");
    process.exit(1);
  }
}
