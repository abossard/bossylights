# The engine, in detail

The reference behind the [README](../README.md): how a command reaches the lights, the API, each output's states, the checks.

## A command, a frame, a picture

```mermaid
flowchart LR
  C["client<br/>Command"] --> SE["Sender, the engine front<br/>Project::update checks it,<br/>SetOutput: outputs::open, then attach"]
  SE -- "command + device" --> Q["command queue (rtrb)<br/>never locks"]
  Q --> U["tick thread<br/>update, frame"]
  U -- "take, never waits" --> O["outputs"]
  U -- "old device" --> R["return queue (rtrb)"] --> FR["free thread<br/>frees it, closes its port"]
```

```mermaid
flowchart LR
  RD["server reader<br/>twice a beat"] -- "empty box (rtrb)" --> U["tick thread"]
  U -- "the box, filled<br/>at the end of a wake (rtrb)" --> RD
  RD -- "one snapshot per frame,<br/>the same for all" --> S["every Subscribe, Query"]
```

- Two boxes go round. The tick thread never waits, never fails a push, never frees a box (`crates/engine/src/snapshot.rs`).
- No box asked for: no picture sent.

The engine knows "an output": a run of universes, a rate, a state, counters, and a `Device` that takes a frame without waiting. Kinds, devices and bytes live only in `crates/outputs`; `engine-core` only lists the kinds a client may ask for, as data (`kinds.rs`).

How an output gets on and off the tick thread:

| Step | Where | What |
|---|---|---|
| 1 | `Sender::begin` | `Project::update` checks the client `Command` against the project (`crates/engine-core/src/project.rs`, a calculation) and says what follows: an `EngineCommand` for the tick thread, keep the device, or open one. Only this makes an `EngineCommand` |
| 1b | `Opening::open`, then `Sender::finish` | `outputs::open(kind)` makes a `Box<dyn Device>` (for DDP: a socket bound, connected, non-blocking). The server runs it on the blocking pool, without the `Sender`: other clients are answered meanwhile. One device opens at a time; until it is open, `SetOutput` and `RemoveOutput` answer `Opening`. Once open, the project holds the output's kind |
| 2 | `Sender::attach` | the one door for every device, also a test's own: sends `Attach` with the device |
| 3 | tick thread | puts it in the output's slot; the old device goes to the return queue |
| 4 | tick thread | when the output is due: `take(frame)` sends or drops, never waits |
| 5 | free thread | `RemoveOutput` or a replaced output: the old device is freed within a beat of its own (its port closed), and `SetOutput` waits for that before it opens. `SetOutput` for the kind the output already has keeps its device: only the run and the rate change |

## API

Version 2. `GET /health` (`ok` while the engine runs) and the WebSocket `GET /api`. Types: `ui/src/engine-api.ts`, generated from `crates/engine-core/src/api.rs`.

| Who may open `/api` | Why |
|---|---|
| a client that is no web page (sends no `Origin`) | the shell, tests, scripts |
| the app window (`tauri://localhost`, `http://tauri.localhost`) | the desk itself |
| the UI in a browser (`http://127.0.0.1:5174`, and `:5299` under the end-to-end tests) | development |
| any other web page | refused with 403: a page must not stop or steer the lights. `/health` and `/version` stay open, they change nothing |

The list is `ORIGINS` in `crates/engine-server/src/lib.rs`. No pairing or token yet: that comes with a remote on a tablet or phone.

| Client sends | Engine answers |
|---|---|
| `{"Send": {"command": {"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": 255}}}}` | `"Accepted"`, or `{"Refused": {"reason": "NoSuchAddress"}}` and nothing changes |
| `"Query"` | `{"Picture": {"state": ..., "project": ..., "library": ..., "universes": [[...512 levels]]}}`. `project.outputs`: per output `null`, `{"Kind": {"kind": ...}}` (what it sends to) or `"Attached"` (a test's own device) |
| `"Subscribe"` | the `Picture`, then `{"Changed": {"state": ..., "project": ..., "library": ..., "channels": [...]}}` with every frame |

| Command | Does |
|---|---|
| `SetChannel` (universe, address, value) | holds that channel at the value |
| `ReleaseChannel` (universe, address) | lets it go back to zero |
| `ReleaseAll` | lets every held channel go |
| `Blackout` (on) | every channel goes out at zero while on; the held values stay |
| `SetOutput` (output 1 to 16, kind, universes, rate) | adds or replaces an output. Kind `{"Ddp": {"address": "ip:port", "color": "Rgb" or "Rgbw"}}` or `{"DmxUsbPro": {"adapter": 1}}` (the first ENTTEC DMX USB Pro by USB identity, in the order of their USB serial numbers, when the output first finds one; an adapter not there yet is no error, the output is NotFound until it is plugged in; from then on the output finds that same adapter by its serial, so a lost adapter never comes back as another). Adapters count from 1: 0 is `NotACommand`. Universes `{"first": 1, "count": 3}`, sent as one stream of channels. A DMX USB Pro takes one universe: more is refused with `{"TooManyUniverses": {"most": 1}}`. DDP RGB sends whole pixels: 510 of each universe's 512 channels. Rate `"30"`, `"44"`, `"60"` or `null`: 60 for DDP, 44 for the DMX USB Pro |
| `RemoveOutput` (output) | removes it |

```json
{"Send": {"command": {"SetOutput": {"output": 1, "kind": {"Ddp": {"address": "192.168.1.50:4048", "color": "Rgbw"}}, "universes": {"first": 1, "count": 3}, "rate": null}}}}
```

## Fixture library

| | |
|---|---|
| Start | `npm run engine -- --fixtures <folder>`: every `.qxf` under the folder, read on a thread of its own while the engine runs |
| Where definitions come from | [QLC+](https://github.com/mcallegari/qlcplus), `resources/fixtures/`, Apache-2.0. Not shipped with the app yet: point `--fixtures` at a QLC+ checkout |
| State | `library` in `Picture` and `Changed`: `NoFolder`, `Reading`, `Read { read, failed }`, `CannotOpen` |
| Requests | `Search { text }`: best matches, each mode with its channel count. `Fixture { maker, model }`: each mode's channels by name and function. `Failures`: each file that could not be read, its line and why. A broken file never stops the engine |

## Each output's states

```mermaid
stateDiagram-v2
  direction LR
  [*] --> NotFound: SetOutput
  NotFound --> Connecting: accepted, the device appeared
  Connecting --> Running: accepted
  Connecting --> Reconnecting: failed
  Running --> Reconnecting: failed, the device is lost
  Reconnecting --> Running: accepted
  Reconnecting --> NotFound: 100 failures in a row
```

"Accepted" is the device's word: a DMX USB Pro accepts only once its adapter is found. DDP gets no answer from the device, so for it accepted, and Running, mean the frames were sent. After a refusal (nothing listens) a DDP output shows Reconnecting until 30 frames went out without one. A broken socket (a new address) is opened again by itself, off the tick thread.

Counters: `accepted`, `dropped` (the socket or the link was busy: the frame is dropped, never waited for), `failed`, `off_beat` (intervals more than 2 ms from the rate). With no output the engine still wakes 44 times a second and publishes its state.

## The DMX USB Pro port thread

The output thread never calls the serial driver: a vanishing device holds the driver up for many milliseconds. Each DMX USB Pro has a port thread that does every call:

```mermaid
sequenceDiagram
  participant O as output thread
  participant P as port thread (one per adapter)
  Note over P: not there when the output is set: "lost" from the start, frames fail until found
  O->>P: the message (one-slot wait-free queue), wake
  P->>P: write it, then wait until the link is free
  Note over P: unplugged: write fails
  P->>P: close the port, set "lost"
  O->>O: next frames: Failed, at once
  loop every 500 ms while lost
    P->>P: list USB serial ports, match the identity, open at 500 000 baud
  end
  P->>P: clear "lost"
  O->>O: next frame: Accepted, the state goes back to Running
```

| Who looks | How often | Cost |
|---|---|---|
| the port thread of each DMX USB Pro output | writes each message; while the adapter is not there yet or lost, lists the USB serial ports every 500 ms | asleep between messages. One listing is an IOKit or sysfs query of a few ms, never on the output thread |

## Checks with hands or hardware

Each prints what to do and when, with a bell:

| What | Command |
|---|---|
| WLED pattern (4 dots on 320 RGBW pixels, 30 s) | `cargo run -p engine-server --example wled_pattern -- 192.168.1.50` (port 4048 by default, or `ip:port`; optional seconds) |
| DMX USB Pro, 10 minutes: no interval off, nothing dropped | `cargo run --release -p engine-server --example dmx_usb_pro -- <port> ten-minutes` |
| DMX USB Pro, unplug and plug back in | `cargo run --release -p engine-server --example dmx_usb_pro -- <port> unplug` |
| DMX USB Pro, sleep and wake (press Enter once awake) | `cargo run --release -p engine-server --example dmx_usb_pro -- <port> sleep` |

`<port>` is the adapter's port (`/dev/cu.usbserial-...` on macOS). It only says which adapter; the output finds it by its USB identity.

The DMX USB Pro checks print a timed line for every state change (with the system error that lost the port), every interval off by more than 2 ms, and every `take` slower than 2 ms.

## What `npm test` runs

| Check | Proves |
|---|---|
| licence check | every crate and npm package is compatible with Apache-2.0 |
| layers check | crates and ui files depend only along the edges in `scripts/layers.mjs`; `engine-core` has no `std` and no `alloc` |
| names check | `engine-core` and `engine` name no protocol or device (DDP, UDP, DMX USB Pro, serial) outside the list of kinds (`scripts/names.mjs`) |
| `cargo clippy` | the lints for state machines and errors (root `Cargo.toml`, core crates); nothing on the tick thread can lock, block, do I/O or log (`crates/engine/clippy.toml`, linked from `crates/rt-thread`); every `unsafe` block says why it is sound |
| `cargo test` | `update`, `frame`, which output is due next, each output's state machine, DDP packets (every header byte, split at 1440 bytes), DMX USB Pro messages and the thread's priority as tables, no allocation while serving two test devices, taking commands and handing a snapshot to a reader every frame (`crates/engine/tests/no_alloc.rs`) and in each kind's `take` (`crates/outputs/tests/no_alloc.rs`); a DMX USB Pro against a fake serial port (a pseudo terminal): every byte, and unplugged, it fails until its port thread opens a new port; a port stuck in the driver never holds up `take`, and one that takes no byte for half a second is lost and looked for again, `ui/src/engine-api.ts` matches the Rust types; over the WebSocket: an output added, then "set channel" changes the bytes on its DDP socket in the next frame, wrong requests are refused, every subscriber gets the same picture of a frame (`crates/engine-server/tests/api.rs`) |
| `tsc`, oxlint, `node --test` | the UI type-checks; exhaustive switches, no `any`, only `engine.ts` talks to the engine; its connection state machine, every state and event; how replies change its picture |
| Playwright | engine + UI in a browser: the counter advances, the thread's priority shows, a fader sets a channel, an output and its counters show |
| Playwright, catalog | the storyboard catalog headless: every page mounts, a gesture shows the command it sends (`ui/src/catalog/catalog.spec.ts`) |

`npm run scorecard` prints numbers to watch, measured on the release build: lateness per output with two outputs at 60 and 44 a second, the same at the limits the API allows (16 outputs of 64 universes), intervals off by more than 2 ms, and send to frame, counted per output. A measurement that fails makes it exit non-zero. CI writes the line counts, public items and escape hatches into each run's summary. The timing numbers read "not measured on CI": run `npm run scorecard` on a real machine. A nightly workflow runs `npm run mutants`.
