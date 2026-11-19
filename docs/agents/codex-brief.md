# Brief for Codex runs

You work on Bossylights, a lighting desk: Rust engine, SolidJS UI. This brief comes first in every run. The task follows it.

## Read first, nothing else yet

| File | Why |
|---|---|
| `AGENTS.md` | rules and the crate map |
| `docs/coding.md` | how code is written: data, calculations, actions apart; one-way state; deep modules; errors as states |
| the spec the task names (`docs/design/...`) | the boxes you must meet |

Do not read the whole tree. Search for the symbol the task names.

## Rules that decide a review or a change

| Rule | In practice |
|---|---|
| No invalid state | a command that would make an invalid project or patch is refused. Never allow and flag |
| One writer per channel, patches never overlap | every DMX value resolves at the look stage, in one table (`docs/design/who-wins.md`) |
| A press is in the very next frame | nothing a press waits behind: no compile, save or lock the tick or another client needs |
| The tick and output threads never block, lock, allocate or free | counted by the `no_alloc` tests |
| Every feature has an end-to-end test that checks DMX output | `engine-server/tests/m3_*.rs` decode real frames |
| Tests are tables, through the public interface | no test on internals, no assert on a time under a second |
| Apache-2.0 only | no GPL or AGPL code or dependencies |

## What you can run

| Allowed | Not allowed |
|---|---|
| `cargo test -p <crate> --offline -j 4`, one crate or one test at a time, engine-server tests included | `npm install`, anything that needs the internet |
| `cargo clippy -p <crate> --offline` | the whole `npm test`, the UI and e2e suites (the orchestrator runs them) |
| `node --test scripts/<name>.test.mjs` | `cargo mutants`, `git commit`, `git push`, `git stash`, branch switches |

You may bind ports on `127.0.0.1`, so the engine-server tests that start the engine run here too. The copy is yours to write and is thrown away. Network use beyond `127.0.0.1` is not needed: cargo runs with `--offline`. `rm -rf` is blocked by the automatic review: leave temp files to the system or use `unlink` and `rmdir`.

The machine is shared: at most four build jobs, and clean up what you create. `CARGO_TARGET_DIR` is already set outside your copy.

## How to work

1. For a bug or a box: write the failing test first, run it, show it red. Then the fix, show it green.
2. For a finding in a review: write the failing test or run the command that shows it, in your copy, and report it with that command. Do not fix it. A finding you could not run says why.
3. Change only the files the task names. A new file beats a wide edit of a shared one.
4. Leave generated files (`ui/src/engine-api.ts`) to the command that generates them, and say so.
5. Stay inside your copy. No commit. The orchestrator verifies your diff in a clean copy and applies it.

## What the orchestrator checks afterwards

| Check | So you know |
|---|---|
| `npm run verify` on your diff, all suites | a green `cargo test` of one crate is not enough |
| A break planted on purpose in your fix | each rule you added needs a test that fails without it |
| A second model reads your diff | say plainly what you are unsure of, it is cheaper than being found |

## Your last message

A table of what you did per box (box, change, the command that proves it, red then green), then a short list of what you could not check. No restating the task, no praise.
