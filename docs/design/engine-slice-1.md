# Engine, first slice: from a command to light

The stack is confirmed (2026-11-18, issue #25). This is the first buildable slice of the engine. No product UI: that waits for the storyboard catalog (#102).

## What it proves

```mermaid
flowchart LR
  C["A command<br/>set channel 1 to 255"] --> E["Engine"] --> F["Frame"] --> O1["DMX USB Pro<br/>44 a second"]
  F --> O2["WLED over DDP<br/>60 a second"]
```

A command goes in through the API, and real light comes out of two different outputs at two different rates, with exact timing.

## Decisions it builds on

| Decision | From |
|---|---|
| Rust engine, web UI, Tauri shell | #25 |
| The engine is its own process and outlives the window | #25 |
| The output thread runs with the real-time policy | #23, #25 |
| A press is in the very next frame | #24, #25 |
| The rate belongs to each output | [theory.md](theory.md), idea 1 |
| Everything is a command. State flows one way | [../coding.md](../coding.md), rule 2 |

## The real-time core: one thread, many outputs

```mermaid
flowchart LR
  Q["Commands<br/>a queue that never locks"] --> T
  subgraph T["The output thread (real-time)"]
    direction LR
    W["wake when the next<br/>output is due"] --> A["apply waiting commands<br/>update(state, command)"] --> G["frame(time)<br/>for that output"] --> S["hand to the output<br/>never waits"]
  end
  S --> D1["Output A, 44 a second"]
  S --> D2["Output B, 60 a second"]
  T -. "state, handed over<br/>without locks" .-> API["API: subscribe, query"]
```

| Choice | Why |
|---|---|
| **One** real-time thread serves every output | no state is shared between threads, so nothing needs a lock |
| It wakes when the next output is due | each output keeps its own rate. No global tick |
| Commands are applied right before a frame | a press is in the very next frame of every output |
| Handing a frame to an output never waits | a slow or lost device cannot hold up the others. A frame that cannot be handed over is dropped and counted |
| Buffers belong to the caller | the core crate stays without `alloc` |

## Where the code goes

| Crate | Kind | Holds |
|---|---|---|
| `engine-core` | core: no `std`, no `alloc` | state, commands, `update`, `frame(time)`, the schedule of what is due |
| `engine` | action | the output thread, the command queue, the hand-over of state |
| `rt-thread` (new, tiny) | action | the one place that asks the system for real-time priority. The only `unsafe` |
| `outputs` (new) | action | DDP, DMX USB Pro, a mock. Each: hand over a frame, report its state |
| `fixture-library` (new) | calculation, may use the heap | reads fixture definitions |
| `engine-server` | action | the API: `send`, `subscribe`, `query` |

The allowed edges are added to `scripts/layers.mjs` with each crate.

## An output is a state machine

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

Every output counts frames accepted, dropped and failed. "On time on the host" is not enough: the first DMX USB Pro run of the spike was minutes behind on the wire while the host numbers looked perfect.

## The six tickets

```mermaid
flowchart LR
  E1["1. Real-time<br/>output thread"] --> E2["2. Commands<br/>change universes"] --> E3["3. Outputs with their<br/>own rate, DDP"] --> E4["4. DMX USB Pro"]
  E1 --> E5["5. Engine outlives<br/>the window"]
  E6["6. Fixture library:<br/>read definitions"]
```

| # | Ticket | Ends in | Parent |
|---|---|---|---|
| 1 | Real-time output thread | tick lateness under 1 ms at p99 on a Mac, shown in the scorecard | #27 |
| 2 | Commands change universes | a command sent over the API shows up in the very next frame | #26 |
| 3 | Outputs with their own rate, DDP | a WLED device at 60 and a second output at 44 from one engine | #44 |
| 4 | DMX USB Pro output | a real adapter, unplug and replug survive | #42 |
| 5 | Engine outlives the window | kill the window, the lights go on. A hidden window does not slow them | #26 |
| 6 | Fixture library: read definitions | QLC+ fixture files become a searchable model | #41 |

## Not in this slice

| Later | Why |
|---|---|
| Lights instead of channels (Rig, patch) | needs the fixture library first |
| Looks, the Mixer's who-wins table | needs the Rig |
| Any product screen | waits for the storyboard catalog (#102) |
| Art-Net, sACN, Open DMX | same shape as DDP and the Pro, once those two stand |
