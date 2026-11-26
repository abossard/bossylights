# Module boundary map

Read [theory.md](theory.md) first: the engine knows channels, the Rig and the screens know lights. Answers from the reviews are at the end.

| Module | State |
|---|---|
| Engine, Mixer (per channel, #250), Sources (steps, sequences, Wave), Clock, Rig, Fixture library, Output (DMX USB, DDP), Project | built |
| Rig rules from the file, Master | built (#251) |
| Typed grabs, rules in Patch, virtual dimmers | designed (#251) |
| Surface, Controllers, Show, Music, Autopilot | design |

## The rule: deep modules

```
   shallow module                deep module
┌────────────────────┐          ┌────────┐
│     interface      │          │interface│   small: what others must know
├────────────────────┤          ├────────┤
│   little inside    │          │        │
└────────────────────┘          │  a lot │   large: what others never see
                                │ inside │
   costs a lot,                 │        │
   hides little                 └────────┘
                                 costs little,
                                 hides a lot
```

John Ousterhout: the interface is the cost, what it hides is the benefit. So: few modules, tiny interfaces.

## The map

```mermaid
flowchart LR
  subgraph writers["Who issues commands"]
    direction TB
    UI["Web UI"]
    HW["Controllers"]
    AG["AI agents"]
  end

  SURF["Surface"]
  CTRL["Controllers<br/>new, not rated yet"]
  DOOR{{"Engine<br/>commands in, state out"}}
  SHOW["Show"]
  AUTO["Autopilot"]
  MUSIC["Music"]
  PROJ[("Project")]
  LIB[("Fixture library")]

  subgraph rt["Real-time core"]
    direction LR
    CLOCK(("Clock"))
    MIX["Mixer<br/>per channel"]
    OUT["Output<br/>adapters"]
  end

  RIG["Rig<br/>off the tick"]

  UI --> DOOR
  AG --> DOOR
  HW --> CTRL --> DOOR
  SURF --> DOOR
  SHOW --> DOOR
  AUTO --> DOOR
  MUSIC --> AUTO
  MUSIC --> CLOCK
  DOOR --> MIX
  DOOR --> PROJ
  LIB --> RIG
  RIG -- "channels and rules" --> PROJ
  PROJ -- "channel plans" --> MIX
  CLOCK --> MIX
  MIX -- "frame" --> OUT
  MIX -. "state" .-> DOOR
```

| Arrow | Meaning |
|---|---|
| solid | commands and data flow this way only |
| dotted | state snapshots coming back |

**Three rules**

```mermaid
flowchart LR
  R1["Every writer goes<br/>through the Engine"] ~~~ R2["The core never<br/>calls outward"] ~~~ R3["The core knows only channels<br/>Only adapters know wire formats<br/>Only Rig turns lights into channels"]
```

## The thirteen modules

The bar shows depth: interface size on the left, what it hides on the right.

| Module | Whole interface | Hides | Depth |
|---|---|---|---|
| **Engine** | `send` `subscribe` `query` | a listed set of commands, transports (UI, MCP, OSC), versions, permissions | ▮▮▮ → ▮▮▮▮▮▮ |
| **Mixer** | `apply` `frame` | the who-wins rules (one table, per channel), fades, launch on beat, flash, release, raw holds, modifiers (Master, Blackout, virtual dimmers) | ▮▮ → ▮▮▮▮▮▮▮▮ |
| **Sources** | `evaluate`: channel bytes and words at a time | chases, movement maths, pixel matrices, audio scripts, palettes, laser shapes (built off the tick into plans) | ▮ → ▮▮▮▮▮▮▮▮ |
| **Clock** | `now` `follow` | tap, audio beat, MIDI clock, DJ position, timecode, smoothing | ▮▮ → ▮▮▮▮▮▮ |
| **Rig** | `fixtures` `groups` `lower` | addresses, modes, pairs, compounds, colour mixing, pan/tilt limits, fixture swap, the default rules of each channel | ▮▮▮ → ▮▮▮▮▮▮▮▮▮ |
| **Fixture library** | `search` `model` | QLC+, OFL and GDTF file formats | ▮▮ → ▮▮▮▮▮ |
| **Output** | `take` bytes, a route, a rate | USB timing, the rate of each device (44 for DMX, 60 for DDP by default), reconnects, switching lanes, Art-Net, sACN, DDP, laser DAC | ▮▮ → ▮▮▮▮▮▮▮▮ |
| **Project** | `read` `edit` `undo` `redo` | file format, autosave, migration, history | ▮▮▮▮ → ▮▮▮▮▮▮ |
| **Surface** | `layout` `press` | widgets, pages | ▮▮ → ▮▮▮▮▮ |
| **Controllers** (new, not rated yet) | `devices` `learn` `mappings` | MIDI, OSC, controller profiles, pad colours sent back | ▮▮▮ → ▮▮▮▮▮▮ |
| **Show** | `record` `play` `edit` | tracks, clips, nested song shows, quantize, section blocks | ▮▮▮ → ▮▮▮▮▮▮ |
| **Music** | `playing` `analyse` | VirtualDJ protocols, analysis pipeline, beat grids, confidence | ▮▮ → ▮▮▮▮▮▮ |
| **Autopilot** | `start` `stop` | choosing looks, not repeating, staying out of your hands' way | ▮▮ → ▮▮▮▮▮ |

### Inside the Engine

```mermaid
flowchart LR
  FRONT["Front<br/>admit commands<br/>install patches"] --> QUEUE["command queue"]
  QUEUE --> TICK["Tick<br/>mutate performance<br/>schedule outputs"]
  TICK --> LANE["Lane<br/>worker control<br/>returned frames"]
  TICK --> PUB["Publication<br/>copy one completed wake"]
  PUB --> SNAP["Snapshot<br/>immutable views<br/>and deltas"]
```

| Owner | Keeps |
|---|---|
| **Front** | project, patch, saved looks, command admission, prepared output openings, the grab book (what each grab holds, the held lights of the picture) |
| **Tick** | mutable performance state, the field table, grab boxes, kept plays' frozen moments, cutover stamps, scheduling, frame measurements |
| **Lane** | worker control, queued frames, returned-frame accounting |
| **Snapshot** | immutable performance, channel levels, health and revision reads |
| **`lib.rs`** | crate exports and composition only |

### Rig: a mode becomes channels (channel model, #239)

`rig::channels::lower` is a calculation off the tick. Since #250 L1 it gives every patched channel its rest, dark, fade and pair on the tick, and the writers join a fine only where it pairs. Since #251 also what each channel follows, and the follower index: the channels that follow a modifier, a pair as one word, so the modifier pass visits only those.

The field table (`rig::FieldPlan`, #250 L3) is the interim step from a light index to channels: each light's fields (a channel, or a pair as one word), which fade, the dimmer channels. Swapped whole per patch edit, so a Move keeps a playing look. #253 replaces it with fixture refs.

```mermaid
flowchart LR
  M["Mode + first channel"] --> L["lower"]
  L --> C["Per channel<br/>rest, dark, fade or jump,<br/>ties, follows, travel"]
  L --> P["Pairs<br/>coarse + fine, adjacent or not"]
  L --> K["Compounds<br/>Byte, Word, Xy, Rgb, Rgbw,<br/>Rgba, Cmy, Slot, Components"]
```

| Hides | Rule |
|---|---|
| which channels follow Master and Blackout | from the function: dimmers, emitters no dimmer carries, a shutter with a closed range (Blackout only). A shared dimmer beside head dimmers follows Blackout only. Fog and unknown channels stay independent |
| where a pair is | only when certain, from the file: what the fine acts on, then its preset, then the coarse right before it. Any other fine stays a single byte, joined by the user in Patch. The definition is the truth: no rule for one fixture |
| what a typed value is set on | one compound per head, every channel in one Byte or Word |
| the helpers | `word_split`, `word_join`, `to_native`, `rgb_to_rgbw`, `slot_byte`, `degrees` |

All who-wins rules live in the Mixer, in one table. No screen and no other module decides between two values for the same channel. See [who-wins.md](who-wins.md).

## How a press travels

Built (#250). A plan names lights by index; the field table gives their channels each frame.

```mermaid
sequenceDiagram
  participant S as Surface
  participant D as Engine
  participant M as Mixer
  participant R as Rig
  participant C as Channel space
  participant O as Output
  S->>D: send(start Sunset on Manual)
  Note over D,R: off the tick, before the press:<br/>Rig makes the field table,<br/>the look is compiled to words per light field
  D->>M: apply(command with its plan)
  loop each frame
    M->>C: offers through the field table,<br/>one winner per channel, modifiers
    loop each output due
      O->>C: gather this output's route
      C-->>O: bytes in route order
    end
  end
  M-->>D: state
  D-->>S: reflect(state)
```

## Which screen uses which module

Screens are thin. Each is a view of one or two modules through the Engine.

| Screen | Surface | Controllers | Mixer | Show | Music | Autopilot | Project | Rig | Library | Output | Clock |
|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| Console | ● | | ● | | | | | | | | |
| Songs | | | ● | ● | ● | | | | | | ● |
| Looks (with autopilot settings) | | | ● | | | ● | ● | | | | |
| Patch | | | | | | | | ● | ● | | |
| DMX | | | ● | | | | | ● | | ● | |
| In/Out | | ● | | | | | | | | ● | ● |

## What gets no module

```mermaid
flowchart LR
  N1["Function manager,<br/>virtual console, simple desk<br/>as separate engines"]:::no
  N2["A universe object<br/>that code writes into"]:::no
  N4["A light or fixture<br/>in the engine core"]:::no
  N3["A plugin interface<br/>(until two real cases exist)"]:::no
  classDef no stroke:#ef3e7b,stroke-width:2px,stroke-dasharray: 5 3
```

## Answers from reviews 1 and 2

| # | Question | Decided |
|---|---|---|
| 1 | Is a Door with three operations believable? | not under that name. It is now called **Engine**, and it accepts a listed set of commands, not "anything" |
| 2 | Can you play something that is not saved? | no. Everything playable lives in Project, and what you try is saved by itself |
| 3 | Where does controller mapping live? | review 2: in In/Out, like QLC+ profiles. So mapping leaves Surface and becomes the Controllers module. Learn still works on every control in the app |
| 4 | Music and Autopilot | two modules: knowing and deciding |
| 5 | Hardest module to keep narrow | the **Mixer**: every who-wins rule lives in it ([who-wins.md](who-wins.md)) |
| 6 | Words on the desk | Playback, Patch, Show file, Looks, Tempo |
| 7 | Songs and shows | a song has one show |

Details: [review-1.md](../history/review-1.md), [review-2.md](../history/review-2.md).
