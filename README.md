# Bossylights

An open-source lighting desk that is easy first and deep on request.

- Live busking, recording what you busk, timecoded shows, and an autopilot that follows the music.
- A control surface you design yourself, mirrored on pad controllers.
- Reads the QLC+ fixture library.
- Built for AI agents too: one engine API, with an MCP server on top.

Status: skeleton. A debug window shows the engine's frame counter, its outputs, and sets raw channels through the API. Outputs: DDP (WLED) and a mock.

## Build, run, test

Needs Rust, Node 24 and npm. On Linux also the [Tauri system libraries](https://v2.tauri.app/start/prerequisites/#linux).

| What | Command |
|---|---|
| Install | `npm install` |
| Desktop app for development | `npm run dev` |
| All tests | `npm test` |
| Build | `npm run build` |
| Engine only, no window | `npm run engine` (API on `127.0.0.1:7744`; `-- --universes 64` for more, 4 by default) |
| UI in a browser | `npm run engine` and `npm run ui`, then open http://127.0.0.1:5174 |

```mermaid
flowchart LR
  SH["shell/<br/>Tauri window"] -- "starts" --> EB["crates/engine-server<br/>bossylights-engine"]
  EB -- "runs" --> EN["crates/engine<br/>tick thread, wakes when<br/>the next output is due"]
  EN -- "calls" --> CO["crates/engine-core<br/>update, schedule,<br/>no std, no alloc"]
  EN -- "hands frames to" --> OU["crates/outputs<br/>DDP, mock"]
  EN -- "starts it with" --> RT["crates/rt-thread<br/>real-time priority,<br/>the only unsafe"]
  SH -- "hosts" --> UI["ui/<br/>SolidJS"]
  UI -- "WebSocket /api" --> EB
```

```mermaid
flowchart LR
  C["client<br/>Send"] --> SE["Sender<br/>SetOutput opens<br/>the socket here"] -- "command + device" --> Q["queue (rtrb)<br/>never locks"] --> U["tick thread<br/>update, frame"] -- "take, never waits" --> O["outputs"]
  U -- "old device" --> R["return queue (rtrb)"] -- "freed on the next send" --> SE
  U -- "seqlock,<br/>atomics only" --> S["Subscribe, Query"]
```

How an output gets on and off the tick thread:

| Step | Where | What |
|---|---|---|
| 1 | `Sender::send` | checks `SetOutput`, opens the socket (bound, connected, non-blocking) |
| 2 | command queue | the command carries the opened device |
| 3 | tick thread | puts it in the output's slot; the old device goes to the return queue |
| 4 | tick thread | when the output is due: `take(frame)` sends or drops, never waits |
| 5 | `Sender::send` | `RemoveOutput` or a replaced output: the next send frees the old device |

Engine API: `GET /health` (`ok` while the engine runs) and the WebSocket `GET /api`. Types: `ui/src/engine-api.ts`, generated from `crates/engine-core/src/api.rs`.

| Client sends | Engine answers |
|---|---|
| `{"Send": {"command": {"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": 255}}}}` | `"Accepted"`, or `{"Refused": {"reason": "NoSuchAddress"}}` and nothing changes |
| `"Query"` | `{"Picture": {"state": ..., "universes": [[...512 levels]]}}` |
| `"Subscribe"` | the `Picture`, then `{"Changed": {"state": ..., "channels": [...]}}` with every frame |

| Command | Does |
|---|---|
| `SetChannel` (universe, address, value) | holds that channel at the value |
| `ReleaseChannel` (universe, address) | lets it go back to zero |
| `ReleaseAll` | lets every held channel go |
| `Blackout` (on) | every channel goes out at zero while on; the held values stay |
| `SetOutput` (output 1 to 16, kind, universes, rate) | adds or replaces an output. Kind `{"Ddp": {"address": "ip:port", "color": "Rgb" or "Rgbw"}}` or `"Mock"`. Universes `{"first": 1, "count": 3}`, sent as one stream of channels. Rate `"30"`, `"44"`, `"60"` or `null`: 60 for DDP, 44 for DMX kinds |
| `RemoveOutput` (output) | removes it |

```json
{"Send": {"command": {"SetOutput": {"output": 1, "kind": {"Ddp": {"address": "192.168.1.50:4048", "color": "Rgbw"}}, "universes": {"first": 1, "count": 3}, "rate": null}}}}
```

Each output in the state:

```mermaid
stateDiagram-v2
  [*] --> Connecting: SetOutput
  Connecting --> Running: accepted
  Running --> Reconnecting: failed
  Connecting --> Reconnecting: failed
  Reconnecting --> Running: accepted
  Reconnecting --> NotFound: 100 failures in a row
  NotFound --> Connecting: accepted
```

Counters: `accepted`, `dropped` (the socket was full: the frame is dropped, never waited for), `failed`, `off_beat` (intervals more than 2 ms from the rate). With no output the engine still wakes 44 times a second and publishes its state.

A moving pattern on a WLED device or simulator (4 dots on 320 RGBW pixels, 30 s):

| What | Command |
|---|---|
| WLED pattern | `cargo run -p engine-server --example wled_pattern -- 192.168.1.50` (port 4048 by default, or `ip:port`; optional seconds) |

`npm test` runs:

| Check | Proves |
|---|---|
| licence check | every crate and npm package is compatible with Apache-2.0 |
| layers check | crates and ui files depend only along the edges in `scripts/layers.mjs`; `engine-core` has no `std` and no `alloc` |
| `cargo clippy` | the lints for state machines and errors (root `Cargo.toml`, core crates); nothing on the tick thread can lock, block, do I/O or log (`crates/engine/clippy.toml`, linked from `crates/rt-thread`); every `unsafe` block says why it is sound |
| `cargo test` | `update`, `frame`, which output is due next, each output's state machine, DDP packets (every header byte, split at 1440 bytes) and the thread's priority as tables, no allocation while serving a DDP and a mock output and taking commands (`crates/engine/tests/no_alloc.rs`), `ui/src/engine-api.ts` matches the Rust types; over the WebSocket: an output added, then "set channel" changes the bytes on its DDP socket in the next frame, wrong requests are refused (`crates/engine-server/tests/api.rs`) |
| `tsc`, oxlint, `node --test` | the UI type-checks; exhaustive switches, no `any`, only `engine.ts` talks to the engine; its connection state machine, every state and event; how replies change its picture |
| Playwright | engine + UI in a browser: the counter advances, the thread's priority shows, a fader sets a channel, an output and its counters show |

`npm run scorecard` prints numbers to watch (lateness per output with two outputs at 60 and 44 a second, intervals off by more than 2 ms, send to frame); CI writes them into each run's summary. A nightly workflow runs `npm run mutants`.

## Plan

- [The map issue](https://github.com/abossard/bossylights/issues?q=is%3Aissue+label%3Amap): every area, feature, requirement and task, with blockers.
- [Interactive map](docs/plan/map/index.html): open it in a browser.
- [Requirements](docs/plan/requirements.md)
- [Capability map](docs/plan/capability-map.md): what QLC+, BlinderKitten, Chataigne, Mizer and xLights can do.

## Decisions so far

- A new engine in Rust, a web UI in a Tauri shell.
- Looks, layers and pads on top of a programmer engine.
- Apache-2.0.
- Version 1 is one great night on one rig.
