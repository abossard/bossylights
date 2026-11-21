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
| 2026-11-20 | codex | review-194 | review | sol, ultra | 479 | 50 (10) | 4.92M / 4.80M / 12.1k | n/a | a disconnected Lock named Ready to screen readers, width growth on disconnect, three tests that could not fail; fixed in 7d80319 |
| 2026-11-20 | copilot | review-194 | review | sol, max | 631 | 19 (2) | 2.38M / 2.26M / 21.7k | n/a | the same name bug and width growth, a transparent-outline and a situation-state mutant; fixed in 7d80319 |
| 2026-11-20 | codex | build-196c | build | astra, ultra | 795 | 99 (6) | 5.72M / 5.62M / 16.5k | yes (as 1016b7f on a newer head) | Opus 12 breaks, 10 red; attach no longer a press, ordering and generation tests |
| 2026-11-20 | codex | review-197 | review | sol, ultra | 646 | 62 (3) | 6.92M / 6.78M / 21.2k | n/a | status judged at the read not the grid endpoint (false RunningLate), event order, a loss leaving Healthy, a test client taking Health as a reply, two weak tests |
| 2026-11-20 | copilot | review-197 | review | sol, max | 823 | 21 (4) | 3.25M / 3.11M / 28.5k | n/a | the same endpoint-status bug, loss leaving Healthy, test client and pacing gap; size probe 16,026 B in budget |
| 2026-11-20 | codex | build-197b | build | astra, ultra | 773 | 79 (4) | 9.45M / 9.31M / 17.9k | yes (87939d9) | Opus re-planted all, red; the next second look found a repeat-loss false warning |
| 2026-11-20 | codex | review-197c | review | sol, ultra | 455 | 37 (0) | 5.42M / 5.30M / 14.4k | n/a | repeat-loss false RunningLate, stale-snapshot pacing, two weak tests |
| 2026-11-20 | copilot | review-197c | review | sol, max | 696 | 30 (4) | 2.76M / 2.63M / 28.3k | n/a | the same repeat-loss bug, an overflow test that could not fail |
| 2026-11-20 | codex | build-197d | build | astra, ultra | 897 | 93 (9) | 8.22M / 8.09M / 28.2k | yes (d9364cd) | Opus: one equivalent gap; the next look found the loss-and-return variant |
| 2026-11-20 | codex | review-197d | review | sol, ultra | 552 | 45 (4) | 4.89M / 4.69M / 11.9k | n/a | loss and return between reads keeps a bad vote, first Health after a stale Picture early |
| 2026-11-20 | copilot | review-197d | review | sol, max | 537 | 17 (2) | 2.01M / 1.88M / 24.6k | n/a | the same two |
| 2026-11-20 | codex | build-197e | build | astra, ultra | 683 | 82 (11) | 7.17M / 7.05M / 16.9k | yes (5c46032) | Opus found one more variant (first read on the endpoint); a Claude worker fixed the class with one rule (6d0a0ae) |
| 2026-11-20 | codex | review-197f | review | sol, ultra | 196 | 24 (1) | 1.77M / 1.67M / 5.5k | n/a | no new defects |
| 2026-11-20 | copilot | review-197f | review | sol, max | 294 | 13 (0) | 1.17M / 1.09M / 9.8k | n/a | no new defects |
| 2026-11-20 | codex | spec-216 | spec | astra, ultra | 1186 | 65 (0) | 6.23M / 6.04M / 43.1k | n/a (spec) | dense and over-built; Claude replaced it with one nearest-beat rule; the Opus checker found 18 gaps, all fixed |
| 2026-11-20 | codex | review-247-249 | review | sol, ultra | 412 | 58 (7) | 6.07M / 5.93M / 13.4k | n/a | 13 findings with red repros: v11 hazer bytes changed, source table missing, fine pairing by function, wheel slots lost, fan as dimmer, reverse dimmer dark; Opus missed most |
| 2026-11-20 | copilot | review-247-249 | review | sol, max | 607 | 44 (4) | 4.95M / 4.78M / 32.0k | n/a | 10 findings, 7 red repros; overlaps Codex on 6, new: dimmer in some heads taken as shared, Byte metadata on word halves |
| 2026-11-20 | codex | build-240 | build | astra, ultra | 68 | 18 (5) | 463.9k / 419.2k / 2.1k | n/a | stopped at once: the red test (Claude's) did not compile (`80_000_u16`); right call, no patch |
| 2026-11-20 | codex | build-240b | build | astra, ultra | 492 | 58 (11) | 3.36M / 3.29M / 17.1k | yes | 11 of 11 table tests unchanged; its 5 boundary tests catch 4 planted breaks the table missed (5%, 200 ms, 100 ms, 2 × median). Tests sat in `src/`, an empty block could anchor: round 2 |
| 2026-11-21 | codex | build-240-fix | build | astra, ultra | 158 | 44 (9) | 971.9k / 924.0k / 4.0k | yes | clean; added a one-frame case that the asked 7 s case masked |
| 2026-11-21 | codex | review-240 | review | sol, ultra | 323 | 28 (2) | 2.24M / 2.16M / 10.0k | n/a | 7 findings with red tests (timeout reacquire, backward stamp, 5% gap window, half/double guard, median, stamps) and 3 surviving mutants; worker's own 14 breaks missed all of them |
| 2026-11-21 | copilot | review-240 | review | sol, max | 416 | 11 (2) | 1.03M / 954.3k / 22.3k | n/a | 6 red tests; overlaps Codex on 4, new: first stamp anchors the stream, NaN gives a full meter |
| 2026-11-20 | codex | build-248 | build | astra, ultra | 1562 | 98 (13) | 19.65M / 19.45M / 42.9k | no: `check-timing` flagged a 10 s guard without its `// guard` mark; green after that one-line fix (e982b41 verify) | Opus 5 breaks, all red (two-span gather, span shift, fresh-open fallback, source reset, whole-space Blackout). Opus added: a restart whose device does not come back opens a fresh one (Codex removed the output and left a fixture unstreamed), and the two channel-core tests from the T1 check |
| 2026-11-21 | codex | review-248 | review | sol, ultra | 926 | 66 (6) | 11.96M / 11.78M / 22.0k | n/a | 7 findings, 6 red repros: a refused resize removes the only output, route shifts drop a patched byte, a same-length reroute sends the old route's frame, 319-byte RGBW accepted raw, oversized v11 lane box unnoticed, modules.md diagram |
| 2026-11-21 | copilot | review-248 | review | sol, max | 733 | 9 (0) | 1.12M / 978.8k / 15.2k | n/a | no answer: timed out after 600 s waiting for its own cargo build (harness fixed: COPILOT_TASK_WAIT_TIMEOUT_SECONDS=3600); its repro tests overlap Codex's P1s |
| 2026-11-21 | codex | build-249-fix | build | astra, ultra | 1086 | 120 (9) | 14.03M / 13.75M / 25.2k | yes | 17 rows green; 5 planted breaks red (pairing order, paired slot, fan word, colour tag, shared dimmer); `channels.rs` mutants 0 missed. Did not run mutants (brief forbids). Jump travel now also on shutters (MH6 runs 181 to 140), harmless |
| 2026-11-21 | codex | review-fixes-1 | review | sol, ultra | 731 | 54 (2) | 7.04M / 6.90M / 18.9k | n/a | second look at three fix rounds: #247 clean; #249 pairing on a two-wheel mode, roles by name, v6 save format changed; #240 timeout at a block boundary |
| 2026-11-21 | copilot | review-fixes-1 | review | sol, max | 706 | 31 (2) | 4.66M / 4.47M / 43.6k | n/a | same pairing, v6 format and timeout findings; new: wheel-type guard untested |
| 2026-11-21 | codex | build-240-fix3 | build | astra, ultra | 127 | 15 (1) | 998.2k / 944.3k / 2.2k | n/a: stopped | stopped before editing: two old tracker rows contradict per-block stamps (row 3); a fair stop, the task had forbidden touching them |
| 2026-11-21 | codex | build-240-fix3b | build | astra, ultra | 756 | 107 (13) | 6.95M / 6.84M / 18.3k | no: tracker.rs red (the two allowed number edits were not enough; whole event lists shift) | 9 rows fixed, 14 boundary rows red then green; Claude rewrote the two tracker rows (441-frame blocks, clicks follow a shifted stamp). Landed as 25f969c |
| 2026-11-21 | codex | build-248-fix | build | astra, ultra | 160 | 22 (3) | 972.8k / 917.9k / 2.1k | n/a: stopped | stopped: the task said "Reconnecting with the new run", the red tests say refused, old run kept. The tests won (rule 6) |
| 2026-11-21 | codex | build-248-fixb | build | astra, ultra | 1896 | 101 (6) | 25.12M / 24.86M / 60.5k | yes (c69c713) | Opus LAND: every planted break red; found docs/engine.md:139 reverted (Claude copied whole files built on an older base) and modules.md:64; no_alloc missed Resume. Landed as fe2f05c |
| 2026-11-21 | codex | build-248-mutants | build | astra, ultra | 1009 | 90 (12) | 9.44M / 9.28M / 29.5k | yes (b334e0b) | tests only; cargo mutants on the 5 functions: 20 caught, 0 missed (was 12 missed) |
| 2026-11-21 | codex | build-241 | build | astra, ultra | 1026 | 47 (8) | 6.69M / 6.57M / 34.5k | no: the snapshot size guard in `crates/engine/src/snapshot.rs` (outside its file) needed +152 bytes for the clock; green after that one-line bump | 33 rows unchanged; Opus 7 planted breaks, 6 red, the 7th (Tap taking a stopped grid at the same tempo, elapsed jumps) needed a row; `cargo mutants` on clock.rs: 6 new-code misses, 5 killed by 5 more rows (last-change times), 1 equivalent. Module doc had lost the tap rules: restored. Did not run mutants (brief forbids) |
| 2026-11-21 | codex | review-241 | review | sol, ultra | 304 | 25 (1) | 2.31M / 2.23M / 11.7k | n/a | 4 red tests: one-tap lapse keeps the old tempo, stale beat shifts the grid, a missed click corrects past half a beat, pace at the first unheard crossing |
| 2026-11-21 | copilot | review-241 | review | sol, max | 583 | 14 (2) | 1.78M / 1.68M / 27.8k | n/a | same three plus: adding Tap after Audio skips Audio's lock; the one-tap guard untested |
| 2026-11-21 | codex | review-240-r2 | review | sol, ultra | 181 | 21 (0) | 1.28M / 1.22M / 5.8k | n/a | row fixed, red without it, nothing new |
| 2026-11-21 | copilot | review-240-r2 | review | sol, max | 282 | 14 (5) | 1.02M / 942.4k / 14.1k | n/a | row fixed; new: a published Lost jumps straight to Locked across an overlapping block |
| 2026-11-21 | codex | build-240-r2 | build | astra, ultra | 454 | 45 (8) | 2.58M / 2.50M / 15.2k | no: Claude's own copy step left the old tracker.rs in the first commit; green after adding the patched file (d7aab1c) | both timeout rows red on 25f969c, green on the fix |
| 2026-11-21 | codex | build-248-r3 | build | astra, ultra | 169 | 54 (5) | 2.43M / 2.34M / 3.7k | n/a: stopped | stopped: a reviewer test compared failure counts of two snapshots taken at different times (3 vs 4); a fair stop |
| 2026-11-21 | codex | build-248-r3b | build | astra, ultra | 113 | 22 (3) | 1.23M / 1.09M / 2.6k | n/a: stopped | stopped: two round-2 tests asserted the pause the new rule removes; a fair stop |
| 2026-11-21 | codex | build-248-r3c | build | astra, ultra | 1398 | 110 (13) | 18.32M / 18.08M / 44.4k | yes (c20ca27) | Opus LAND: 11 of 13 planted breaks red; no test pins Handoff::wait off the publisher lock or the tick's handoff() in swap |
| 2026-11-21 | codex | build-249-r3 | build | astra, ultra | 1303 | 101 (14) | 13.49M / 13.16M / 32.6k | yes | Claude LAND: roles and pairs from preset, group, `<Colour>` and Smoke/Hazer only; library test over 1782 QLC+ files green; cargo mutants 4 missed: 1 killed by a group-only fine row, 1 equivalent early return removed, 2 equivalent `from_preset` arms (PositionXAxis, ColorRGBMixer fall through to None); did not run mutants (brief) |
| 2026-11-21 | codex | review-249-r3 | review | sol, ultra | 407 | 55 (4) | 5.92M / 5.76M / 13.5k | n/a | class rule fails: library test green while gobo index joins wheel, mixed fog fixture loses its LED dimmer, strips lose masks, v6 reader too lenient |
| 2026-11-21 | copilot | review-249-r3 | review | sol, max | 665 | 35 (1) | 6.71M / 6.51M / 43.3k | n/a | same, plus: the library oracle repeats the matcher and passes on a one-file corpus; red fine inherits a foreign dimmer; pan fine joins the wrong coarse |
