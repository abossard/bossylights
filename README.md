# Bossylights

An open-source lighting desk: easy first, deep on request.

![The Console: what runs on Manual, Show and Autopilot over time, pads to play looks, faders. Design, not built yet](docs/images/design-console.png)

<sub>The Console. **Design, not built yet** (`mockups/`).</sub>

| Live busking | Record what you busk | Timecoded shows | Autopilot to the music |
|---|---|---|---|
| **Your own surface, mirrored on pads** | **Reads the QLC+ fixture library** | **One engine API, MCP for AI agents** | **Rust engine, web UI, Apache-2.0** |

## What it looks like

| | |
|---|---|
| ![Looks: a sequence on the beat grid, what it is made of and where it is used](docs/images/design-looks.png) | ![Patch: lights placed on the universe grid, a clash shown in red](docs/images/design-patch.png) |
| **Looks**: steps, sequences, patterns. *Design, not built yet* | **Patch**: lights, not channels. *Design, not built yet* |
| ![DMX: every channel by light, held by hand, outputs, the who-wins panel](docs/images/design-dmx.png) | ![Storyboard catalog: the Blackout control alone and in every situation](docs/images/catalog-blackout.png) |
| **DMX**: channels, outputs, who wins. *Design, not built yet* | **Storyboard catalog**: every control, every situation. *Runs today* (`npm run catalog`) |

<img src="docs/images/runs-today-debug-ui.png" width="420" alt="Debug UI: connected, engine wake counter, eight channel faders, one DDP output Running with its counters">

**Runs today**: the debug UI against the real engine. Wakes, the real-time thread, faders that set channels, an output and its counters.

| Status | |
|---|---|
| Built | engine, API, outputs DDP (WLED) and ENTTEC DMX USB Pro, fixture library reader, debug UI |
| Not built | the product screens above: they wait for the storyboard catalog ([#102](https://github.com/abossard/bossylights/issues/102)) |

## The theory in one picture

```mermaid
flowchart LR
  subgraph W["Five writers, one kind of command"]
    direction TB
    YOU["You<br/>screen, touch"]
    PAD["Controller<br/>Launchpad, OSC"]
    AGENT["AI agent<br/>MCP"]
    SHOWP["Show<br/>replays commands"]
    AUTOP["Autopilot<br/>commands from music"]
  end
  subgraph L["Three ranks"]
    direction TB
    LIVE["MANUAL"]
    SHOW["SHOW"]
    AUTO["AUTOPILOT"]
  end
  FRAME(["Frame<br/>computed fresh,<br/>never stored"])
  YOU --> LIVE
  PAD --> LIVE
  AGENT --> LIVE
  SHOWP --> SHOW
  AUTOP --> AUTO
  LIVE --> FRAME
  SHOW --> FRAME
  AUTO --> FRAME
  FRAME --> RIG["Rig<br/>lights to DMX"] --> OUT["Outputs<br/>each at its own rate"]
```

**Every frame is computed fresh from what plays, manual over show over autopilot, and everything that happens is a command.**

| # | Idea | # | Idea |
|---|---|---|---|
| 1 | A frame is computed, never stored | 6 | One clock |
| 2 | Lights, not channels | 7 | Everything is a command |
| 3 | A step is a value | 8 | Two kinds of state: project, performance |
| 4 | Everything that moves is a source | 9 | The real-time core is small and dumb |
| 5 | Who wins: one small set of rules | | |

Each idea as a picture: [docs/design/theory.md](docs/design/theory.md).

## Architecture

### Two processes

```mermaid
flowchart LR
  subgraph WIN["Window process: may crash, may hide"]
    direction TB
    SH["shell/<br/>Tauri window"]
    UI["ui/<br/>SolidJS"]
    SH -- "hosts" --> UI
  end
  subgraph ENG["Engine process: outlives the window"]
    direction TB
    ES["engine-server<br/>bossylights-engine, the API"]
    EN["engine<br/>tick thread"]
    CO["engine-core<br/>update, frame<br/>no std, no alloc"]
    OU["outputs<br/>DDP, DMX USB Pro bytes"]
    RT["rt-thread<br/>real-time priority<br/>the only unsafe"]
    FL["fixture-library<br/>QLC+ .qxf in, a model out"]
    ES --> EN
    ES --> FL
    EN --> CO
    EN --> OU
    EN --> RT
  end
  SH -- "adopts or starts" --> ES
  UI -- "WebSocket /api" --> ES
  OU -- "UDP, USB serial" --> LIGHTS(["Lights"])
```

- A crashed or hidden window never touches the lights. The next window adopts the running engine.
- Allowed edges: [scripts/layers.mjs](scripts/layers.mjs), checked by `npm test`.

### A press, to the next frame

```mermaid
sequenceDiagram
  participant UI as UI
  participant S as engine-server
  participant T as tick thread
  participant O as output
  UI->>S: Send SetChannel (WebSocket)
  S->>T: command queue (rtrb, wait-free)
  S-->>UI: Accepted
  Note over T: wakes when the next output is due
  T->>T: apply commands, compute the frame
  T->>O: take(frame), never waits
  T->>S: the snapshot, in a box the server sent (rtrb)
  S-->>UI: Changed, the same picture for every subscriber
```

- The tick thread never locks, blocks or allocates. Two boxes go round for the snapshot: no shared memory, no atomics.
- Each output has its own rate: by default 60 a second for DDP, 44 for DMX.

### Who wins

```mermaid
flowchart LR
  A["Two looks want<br/>the same property<br/>of the same light"] --> B{"One under<br/>manual control?"}
  B -- "yes" --> M(["Manual wins,<br/>until Release"])
  B -- "no" --> C{"Show or<br/>autopilot?"}
  C -- "show" --> S(["Show wins"])
  C -- "same rank" --> D(["Started last wins"])
```

All rules live in one table: [docs/design/who-wins.md](docs/design/who-wins.md).

## Milestones

```mermaid
flowchart LR
  M1["M1 done<br/>Command to light"] --> M2["M2 in progress<br/>Lights, not channels"] --> M3["M3<br/>Looks and who wins"] --> M4["M4<br/>First real screens"] --> M5["M5<br/>One great night"]
  classDef done stroke:#3fb950,stroke-width:3px
  classDef now stroke:#d29922,stroke-width:3px,stroke-dasharray:6 3
  class M1 done
  class M2 now
```

A gate review sits between each two. How the work is run: [docs/agents/orchestration.md](docs/agents/orchestration.md).

## Build, run, test

Needs Rust, Node 24 and npm. On Linux also the [Tauri system libraries](https://v2.tauri.app/start/prerequisites/#linux).

| What | Command |
|---|---|
| Install | `npm install` |
| Desktop app for development | `npm run dev` |
| All tests | `npm test` |
| Build | `npm run build` |
| Engine only, no window | `npm run engine` (API on `127.0.0.1:7744`; `-- --universes 64` for more, 4 by default) |
| Engine with the fixture library | `npm run engine -- --fixtures <folder>` (a QLC+ checkout) |
| UI in a browser | `npm run engine` and `npm run ui`, then open http://127.0.0.1:5174 |
| Storyboard catalog | `npm run catalog` (opens http://127.0.0.1:5175, needs no engine) |
| Design mockups | `npm install` and `npm run dev` in `mockups/` (on http://localhost:5199) |
| Scorecard, mutation score | `npm run scorecard`, `npm run mutants` |

## More

| Read | For |
|---|---|
| [docs/engine.md](docs/engine.md) | the engine in detail: API, commands, output states, DMX port thread, hand checks, what `npm test` proves |
| [docs/coding.md](docs/coding.md) | how we write code |
| [The map issue](https://github.com/abossard/bossylights/issues?q=is%3Aissue+label%3Amap), [interactive map](docs/plan/map/index.html) | every area, feature, requirement and task |
| [docs/plan/requirements.md](docs/plan/requirements.md) | requirements |
| [docs/plan/capability-map.md](docs/plan/capability-map.md) | what QLC+, BlinderKitten, Chataigne, Mizer and xLights can do |

| Decided | |
|---|---|
| Engine | new, in Rust; web UI in a Tauri shell |
| Model | looks and pads on top of a programmer engine |
| Licence | Apache-2.0 |
| Version 1 | one great night on one rig |
