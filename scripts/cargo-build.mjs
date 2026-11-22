// Builds with cargo and answers where the programs are, from cargo's own answer: a fixed
// `target/debug` misses `CARGO_TARGET_DIR` and a release build.
import { execFileSync } from "node:child_process";

/** The program of each binary target in cargo's `--message-format=json` lines: { name: path }. */
export function executables(lines) {
  return Object.fromEntries(
    lines
      .split("\n")
      .filter((line) => line.startsWith("{"))
      .map((line) => JSON.parse(line))
      .filter((message) => message.reason === "compiler-artifact" && typeof message.executable === "string")
      .filter((message) => message.target.kind.includes("bin"))
      .map((message) => [message.target.name, message.executable]),
  );
}

/** Runs `cargo build` with `args` and answers the programs it built: { name: path }. */
export function cargoBuild(args) {
  const lines = execFileSync("cargo", ["build", "--message-format=json-render-diagnostics", ...args], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    stdio: ["ignore", "pipe", "inherit"],
  });
  return executables(lines);
}
