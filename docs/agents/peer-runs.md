# Peer runs: the ledger

One row per run of Codex or Copilot, pasted by the orchestrator after verifying (step 4c of `orchestration.md`): `npm run peer` prints the row, the orchestrator fills in the last two columns. The orchestrator reads it every 10 runs and when a run disappoints, and improves the harness (`peers.md`, `peer-brief.md`, `tasks/`, `scripts/peer*.mjs`). A change to the harness is planned with an Opus model, never a smaller one.

| Date | Tool | Task | Mode | Model, effort | Seconds | Commands (failed) | Tokens in / cached / out | Verify first time | What our checks found |
|---|---|---|---|---|---|---|---|---|---|
| 2026-11-19 | codex | #181 `SetMixRules` over the file cap, pilot 1 (built on 5ae0048, old sandbox) | build | astra, high | 274 | 44 (6) | 2.48M / 2.42M / 5.9k | no: `main` had moved, `Command::StartLook` gained `when`, the new test no longer compiled | stale base. 8 of 10 `show` tests could not start (port bind denied) |
| 2026-11-19 | codex | #181 same task, pilot 2 (built on 9c6ea02, new sandbox) | build | astra, high | 501 | 46 (3) | 2.95M / 2.87M / 9.5k | yes, all 5 suites (applied on f2f3f46) | break (`fits()` removed) fails the new test; all 10 `show` tests ran in the sandbox. Landed as 0d3a641 |
| 2026-11-19 | codex | review of #169 at dc16e07 (first run through `npm run codex -- review`) | review | astra, ultra | 463 | 75 (8) | 6.90M / 6.76M / 14.7k | n/a | 7 findings, 6 proved red in its copy (core, engine and DMX tests): a tempo change on a crossing skips a bar, a due DMX frame keeps the old colour after a feed edit, a cancelled wait reopens an adapter, a kept play starts in the future, generation zero removes a sounding look, the m3_tempo flake remains; plus a press compiling under the sender lock. The repro tests were thrown away with the copy (see #184; a review now keeps them as `repro.patch`) |
| 2026-11-20 | copilot | Helios and OpenIDN research and read-only connectivity tests (#185 to #187) | research | sol, max, long context | 2016 | n/a | 27.8M / 26.7M / 305k | n/a | facts with links; measured 20 of 20 scan replies, 6.9 ms median, service map withheld on purpose (the server refreshes the USB DAC on that query); first launch lacked long context and was restarted |
| 2026-11-20 | copilot | MIDI controllers (QLC+, Launchpad, APC mini, MIDImix) | research | sol, max, long context | 2606 | n/a | 76.2M / 74.8M / 605k | n/a | five reports with paste-ready sections, one safe LED write on a real APC mini mk2, four owner questions |
| 2026-11-20 | codex | UX review of the catalog (#102) | review | sol, ultra | 689 | 56 (11) | 7.32M / 7.13M / 28k | n/a | 33 findings, 31 confirmed by three Opus verifiers, 2 by design |
| 2026-11-20 | codex | T6 sequences (#170) | build | astra, ultra | 1350 | 155 (17) | 22.8M / 22.5M / 45k | yes (5 suites) | Opus: 32 breaks, 7 not caught, a saved sequence that could never start, an untested v4 show file, a leaked plan |
| 2026-11-20 | codex | T6 fixes | build | astra, ultra | 504 | 111 (7) | 4.89M / 4.78M / 16k | yes | all four fixes red then green; Opus round 2 clean |
| 2026-11-20 | codex and copilot | T6 second look | review | sol ultra / sol max | 500 / 206 | 50 / 5 | 8.5M / 0.5M | n/a | Codex: 7 defects with red tests; Copilot refused the prompt as a cybersecurity risk (the words 'client can cause' and 'unbounded growth'): phrase reviews as correctness and robustness |
| 2026-11-20 | codex | T6 fixes round 2 | build | astra, ultra | 1094 | 100 (3) | 12.9M / 12.7M / 35k | yes | six fixes; Opus round 3 caught every break; two new edge cases left in #190 |
| 2026-11-20 | codex | T7 Wave (#171) | build | astra, ultra | 1810 | 73 (7) | 11.6M / 11.5M / 27k | yes | Opus 34 breaks, 32 caught; the second look found valid v5 shows that no longer open |
| 2026-11-20 | codex and copilot | T7 second look | review | sol ultra / sol max | 410 / 915 | 53 / 58 | 6.8M / 7.4M | n/a | both found the v5 regression and the clock rounding; Copilot found a left-limit bug, Codex the work-admission cases |
| 2026-11-20 | codex | T7 fixes | build | astra, ultra | 1218 | 153 (11) | 11.9M / 11.7M / 28k | yes | all red then green; Opus round 2 LAND |
| 2026-11-20 | codex | T8 night, capacity and scorecard (#172) | build | astra, ultra | 1146 | 79 (9) | 10.3M / 10.1M / 33k | yes | Opus: 26 breaks, holes only in the scorecard checks, example tests not run by npm test |
| 2026-11-20 | codex | T8 fixes | build | astra, ultra | 951 | 108 (8) | 10.4M / 10.1M / 28k | yes | Opus round 2 LAND; burst workload added; the real 60 s run waits for a quiet machine |

## Reading the ledger

| Question | Where it shows |
|---|---|
| Did it finish what the boxes asked | "Verify first time", and the boxes ticked from `last-message.md` |
| Was the time and token cost worth it, against a Claude worker on the same task | seconds and tokens, against the worker's turns on a comparable ticket |
| Which tasks suit which tool | rows grouped by task kind: bug with a repro, feature with an end-to-end test, review, design |
| What does the harness lose it time on | failed commands: sandbox denials (`rm`, binding a port), missing files, wrong crate names. Each becomes a line in `peer-brief.md` |
| Where does it overclaim | "What our checks found": a break that passes, a test that cannot fail, a finding that did not reproduce |

## Known limits of the sandbox (fix in the harness, not by hand each time)

| Limit seen | Effect | Handling now |
|---|---|---|
| The first sandbox (workspace-write, network off) denied binding `127.0.0.1` | 8 of 10 `show` tests failed at startup in the pilot | fixed 2026-11-19: `sandbox_workspace_write.network_access=true`, probe: bind ok, `show` 9 of 9 |
| `rm -rf` is blocked by Codex's automatic review | a test wrapper that removes a temp dir is rejected (`unlink` and `rmdir` work) | the brief says so |
| Input tokens are mostly cached (97 percent in the pilot) | a run's cost is low per token, high in count | read cached and uncached apart |

## Lessons so far

| Lesson | Rule |
|---|---|
| A patch built on an old commit can stop compiling when `main` moves | build on the current head, verify at once; if `main` moved, rebuild or ask Codex to rebase. Prefer short tasks over one that lives for hours |
| Pilot 2 took 501 s against 274 s because it could run all 10 `show` tests | more checking is slower and worth it: the patch passed all suites first time on a newer head |
| Copilot declined a review prompt as a possible cybersecurity risk | phrase reviews as correctness and robustness, not as what a client can cause | recorded 2026-11-20 |
| A small task (12 changed lines) cost 2.9M input tokens, 97 percent cached | peers are for the hard tasks; do not send them one-file fixes |
| 2026-11-20 | codex | spec-193 | spec (research mode) | sol, ultra | 821 | 33 (0) | 3.76M / 3.62M / 32.2k | n/a | reviewed by Opus (POST AFTER FIXES), v2 posted on #193, tickets #195 to #197 |
| 2026-11-20 | codex | review-195 | review | sol, ultra | 329 | 15 (2) | 1.71M / 1.63M / 11.0k | n/a | 5 surviving mutants and a u64 rank overflow, each with a red test; fixed in 1c86c12 |
| 2026-11-20 | copilot | review-195 | review | sol, max | 409 | 13 (3) | 1.30M / 1.21M / 19.1k | n/a | a same-time read kept the older sample (real bug), a status mutant, two overflows; fixed in 1c86c12 except a press count past 2^64 |
| 2026-11-20 | codex | review-196 | review | sol, ultra | 426 | 39 (3) | 3.98M / 3.88M / 11.7k | n/a | a reconfigure regression (status, counters, lateness lost), a 12 KB tick stack frame, two surviving mutants; each with a red test; fix round briefed |
| 2026-11-20 | copilot | review-196 | review | sol, max | 845 | 48 (3) | 3.82M / 3.69M / 35.1k | n/a | the same reconfigure regression and stack growth, a frames_handed mutant; Opus had graded the regression as intended |
| 2026-11-20 | codex | review-196b | review | sol, ultra | 527 | 70 (2) | 6.96M / 6.81M / 15.7k | n/a | the fix round itself added a bug (attaching one output counts a press on another), plus an ordering and a generation test gap; each with a red test or mutant |
| 2026-11-20 | copilot | review-196b | review | sol, max | 622 | 36 (1) | 3.40M / 3.28M / 28.4k | n/a | the same attach bug and ordering gap; the outage guard pinned only by a private-state test |
