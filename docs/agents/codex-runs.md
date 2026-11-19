# Codex runs: the ledger

One row per run, written by the orchestrator after verifying (step 4c of `orchestration.md`). The orchestrator reads it every 10 runs and when a run disappoints, and improves the harness (`codex.md`, `codex-brief.md`, `scripts/codex*.mjs`). A change to the harness is planned with an Opus model, never a smaller one.

| Date | Task | Mode | Model, effort | Seconds | Commands (failed) | Tokens in / cached / out | Verify first time | What our checks found |
|---|---|---|---|---|---|---|---|---|
| 2026-11-19 | #181 `SetMixRules` over the file cap, pilot 1 (built on 5ae0048, old sandbox) | build | astra, high | 274 | 44 (6) | 2.48M / 2.42M / 5.9k | no: `main` had moved, `Command::StartLook` gained `when`, the new test no longer compiled | stale base. 8 of 10 `show` tests could not start (port bind denied) |
| 2026-11-19 | #181 same task, pilot 2 (built on 9c6ea02, new sandbox) | build | astra, high | 501 | 46 (3) | 2.95M / 2.87M / 9.5k | yes, all 5 suites (applied on f2f3f46) | break (`fits()` removed) fails the new test; all 10 `show` tests ran in the sandbox. Landed as 0d3a641 |

## Reading the ledger

| Question | Where it shows |
|---|---|
| Did it finish what the boxes asked | "Verify first time", and the boxes ticked from `last-message.md` |
| Was the time and token cost worth it, against a Claude worker on the same task | seconds and tokens, against the worker's turns on a comparable ticket |
| Which tasks suit it | rows grouped by task kind: bug with a repro, feature with an end-to-end test, review, design |
| What does the harness lose it time on | failed commands: sandbox denials (`rm`, binding a port), missing files, wrong crate names. Each becomes a line in `codex-brief.md` |
| Where does it overclaim | "What our checks found": a break that passes, a test that cannot fail, a finding that did not reproduce |

## Known limits of the sandbox (fix in the harness, not by hand each time)

| Limit seen | Effect | Handling now |
|---|---|---|
| The first sandbox (workspace-write, network off) denied binding `127.0.0.1` | 8 of 10 `show` tests failed at startup in the pilot | fixed 2026-11-19: `sandbox_workspace_write.network_access=true`, probe: bind ok, `show` 9 of 9 |
| `rm -rf` is blocked by the automatic review | a test wrapper that removes a temp dir is rejected (`unlink` and `rmdir` work) | the brief says so |
| Input tokens are mostly cached (97 percent in the pilot) | a run's cost is low per token, high in count | read cached and uncached apart |

## Lessons so far

| Lesson | Rule |
|---|---|
| A patch built on an old commit can stop compiling when `main` moves | build on the current head, verify at once; if `main` moved, rebuild or ask Codex to rebase. Prefer short tasks over one that lives for hours |
| Pilot 2 took 501 s against 274 s because it could run all 10 `show` tests | more checking is slower and worth it: the patch passed all suites first time on a newer head |
| A small task (12 changed lines) cost 2.9M input tokens, 97 percent cached | Codex is for the hard tasks; do not send it one-file fixes |
