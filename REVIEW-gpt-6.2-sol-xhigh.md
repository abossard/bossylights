# #349 review: real-time safety and lifecycle

Reviewed `b6a4827b1853cd78a45eada320c888bc718f2dd7`. Each command below reaches the named assertion and fails on that code.

| ID | Aspect | Severity | Claim | Test | Failing command | Expected vs actual |
|---|---|---|---|---|---|---|
| 1-01 | 1: Real-time safety | Medium | When the sender drops after queuing `ReleaseAll`, [the tick frees the grab's heap storage](crates/engine/src/tick.rs#L959) while applying the release. | `crates/engine/tests/review_1.rs::finding_1_01_tick_frees_grab_after_sender_drops` | `cargo test -p engine --features test-support --test review_1 -- finding_1_01_tick_frees_grab_after_sender_drops` | Expected 0 tick-thread frees. Actual: 3. |
| 2-01 | 2: Lifecycle | High | [Disconnect cleanup releases Blackout but not the live grab](crates/engine/src/front.rs#L899), which keeps overriding the Show look. | `crates/engine-server/tests/review_2.rs::finding_2_01_live_grab_survives_socket_loss` | `cargo test -p engine-server --test review_2 -- finding_2_01_live_grab_survives_socket_loss` | Expected the first visible DDP frame after disconnect to show blue `[0, 0, 255]`. Actual: red `[255, 0, 0]`. |
| 2-02 | 2: Concurrency | Medium | At the second reroute, [the tick has room to count only three results](crates/engine/src/tick.rs#L885), so it loses the result of the newest frame that the old lane sent. | `crates/engine/tests/review_2.rs::finding_2_02_cutover_loses_fourth_frame_outcome` | `cargo test -p engine --features test-support --test review_2 -- finding_2_02_cutover_loses_fourth_frame_outcome` | Expected `[Accepted, Dropped, Dropped, Accepted]`. Actual: `[Accepted, Dropped, Dropped]`. |

## Not proven

- The server may finish shutdown before a WebSocket can answer an edit that is waiting for a worker.
- The shell may adopt an engine that answers `/version` just before it stops.

## Looked at, fine

- **1:** The production core builds without `std` or `alloc`. The adapters use fixed buffers in `take`, with serial I/O and socket opening on other threads.
- **2:** A Blackout hold reserves room in the queue for its release. Removing and re-adding an output did not make its next reroute visible early.
