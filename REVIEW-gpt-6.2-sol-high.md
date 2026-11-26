# #349 specification review

Each test failed on `b6a4827` with the default profile for `cargo test`. The tests do not start an engine.

| ID | Aspect | Severity | Claim | Test | Reproduce | Expected vs actual |
|---|---|---|---|---|---|---|
| 3.01 | Mixer | Medium | When two Show looks start on the same beat, the first request must win their shared channel. [Who wins, rule 3](docs/design/who-wins.md#the-rules). | `crates/engine-core/tests/review_3.rs::finding_3_01_first_request_wins_equal_start` | `cargo test -p engine-core --test review_3 -- finding_3_01_first_request_wins_equal_start` | Expected DMX slot 1 to be `37`; got `145` from the second request. Both plays had the same start. |
| 4.01 | No invalid state | Medium | Opening a show must refuse a pinned mode whose head points to a channel that the mode does not have. [Theory, decision Q18 and idea 8](docs/design/theory.md). | `crates/show-file/tests/review_4.rs::finding_4_01_show_refuses_an_out_of_range_head_channel` | `cargo test -p show-file --test review_4 -- finding_4_01_show_refuses_an_out_of_range_head_channel` | Expected refusal: head index `3` is outside a three-channel mode. Got `Ok("opened")`. |
| 5.01 | Time, health and delay | High with overflow checks | A fixture wholly before an output's run must not crash the slot-range calculation that feeds its health row. [Engine API, “After the handoff”](docs/engine.md#api). | `crates/rig/tests/review_5.rs::finding_5_01_health_range_ignores_fixtures_before_the_run` | `cargo test -p rig --test review_5 -- finding_5_01_health_range_ignores_fixtures_before_the_run` | Expected `Ok(None)` for an empty universe 2. Got `Err("panicked")` after `attempt to subtract with overflow` in `patch.rs:675`. |

## Not proven

- **3:** The existing same-beat test expects the later request to win. Finding 3.01 proves that the code and the written rule disagree, not which should change.
- **4:** I did not find a later command that bypasses the checks for output coverage or saved look targets.
- **5:** The panic was reproduced with overflow checks on. I did not reproduce a failure in a release build or find a separate error in the reported timing bounds.

## Looked at, fine

- **3:** Rank, highest by distance from dark, follower-only Master and Blackout, and rounding on the assembled 16-bit word.
- **4:** Patch overlap checks, refusal to drop output coverage, and recompilation of saved looks before patch edits are admitted.
- **5:** The press cutoff and per-output due times, four-second health windows, and conversion of timing bounds to wire values.
