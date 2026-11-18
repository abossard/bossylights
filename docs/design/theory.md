# The theory of Bossylights

Draft for review. Nothing is built. Questions are at the end.

> A program is the theory in its builders' heads (Peter Naur). This page is that theory, as pictures.

## The whole theory in one picture

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

  subgraph L["Three ranks: manual beats show beats autopilot"]
    direction TB
    LIVE["MANUAL<br/>your hands"]
    SHOW["SHOW"]
    AUTO["AUTOPILOT"]
  end

  FRAME(["Frame<br/>what every light does<br/>at a moment in time"])
  RIG["Rig<br/>lights to DMX"]
  OUT["Output"]

  YOU --> LIVE
  PAD --> LIVE
  AGENT --> LIVE
  SHOWP --> SHOW
  AUTOP --> AUTO
  LIVE --> FRAME
  SHOW --> FRAME
  AUTO --> FRAME
  FRAME --> RIG --> OUT
```

**One sentence:** every frame is computed fresh from what plays, with manual over show over autopilot, and everything that happens is a command.

## Nine ideas

### 1. A frame is computed, never stored

```mermaid
flowchart LR
  subgraph Q["QLC+: many writers, one shared universe"]
    direction LR
    S1["Scene"] --> U[("Universe")]
    S2["Chaser"] --> U
    S3["Slider"] --> U
  end
  subgraph B["Bossylights: inputs in, frame out"]
    direction LR
    P1["What plays"] --> C{{"Compute"}}
    P2["Who-wins rules"] --> C
    P3["Time"] --> C
    C --> F(["Frame"])
  end
```

**Each output asks at its own rate.** A frame is computed for a moment in time, so nothing in it knows a rate.

```mermaid
flowchart LR
  C["Commands"] --> U["State<br/>changes on the engine tick"]
  U --> F["frame(time)<br/>a calculation"]
  F -- "asked 44 times a second" --> D["DMX output"]
  F -- "asked 60 times a second" --> W["DDP output"]
```

| Output | Default rate | Can be set |
|---|---|---|
| USB DMX | 44 a second | lower, for fixtures that need it |
| Art-Net, sACN | 44 a second | yes |
| DDP (WLED and other pixel controllers) | 60 a second | 30, 44, 60 |

Decided on 2026-11-18: the rate belongs to the output, with a default per protocol. There is no global frame rate.

No shared state to corrupt. Stop means "stop contributing".

### 2. Lights, not channels

```mermaid
flowchart LR
  A["Mover L1<br/>colour: red"] --> R["Rig"] --> D["Universe 1<br/>ch 37 38 39 = 255 0 0"]
```

Addresses exist in one place only. You never remember a number.

### 3. A step is a value

| Fixture | Intensity | Colour |
|---|---|---|
| PAR 1 to 8 | 80% | amber |
| Mover L1 | not mentioned | not mentioned |

Plain data: store it, re-use it, hand it to an agent. On screen this is a **step**.

### 4. Everything that moves is a source

```mermaid
flowchart LR
  T1["Look"] --> S
  T2["Chase"] --> S
  T3["Movement"] --> S
  T4["Pixel matrix"] --> S
  T5["Audio script"] --> S
  T6["Laser shape"] --> S
  S{{"source(time) gives a look"}}
```

Ten QLC+ function types become one shape. On screen there are two kinds: a **sequence** (steps and other sequences on the beat grid) and a **pattern** (generated motion). Details: [looks.md](looks.md).

### 5. Who wins: one small set of rules

| Who | PARs | Movers |
|---|---|---|
| **MANUAL** (top) | strobe | silent |
| **SHOW** | silent | silent |
| **AUTOPILOT** (bottom) | amber | sweep |
| **What you see** | **strobe** (Manual speaks) | **sweep** (only Autopilot speaks) |

```mermaid
flowchart LR
  A["Hold Strobe"] --> B(["PARs: strobe"]) --> C["Release"] --> D(["PARs: amber again"])
```

What you touch on the Console stays under your control until you press Release. Then the PARs go back to amber.

There are no layers to manage. The rules sit in one place and can be changed there: [who-wins.md](who-wins.md).

### 6. One clock

```mermaid
flowchart LR
  TAP["Tap"] -.-> CL
  AUD["Audio"] -.-> CL
  MIDI["MIDI clock"] -.-> CL
  DJ["DJ software"] ==> CL
  TC["Timecode"] -.-> CL
  CL(("Clock<br/>beat, bar, position")) --> SRC["Every source"]
```

One feed drives it at a time. Launch-on-the-beat and timecode are the same thing.

### 7. Everything is a command

```mermaid
flowchart LR
  P["You press a pad"] --> CMD["Command<br/>start Sunset on Manual"]
  CMD --> ENG["Engine"]
  CMD -. "written down" .-> REC[("Recording")]
  REC -. "played back" .-> CMD2["Same command<br/>on Show"]
  CMD2 --> ENG
  MUS["Music"] --> AP["Autopilot"] --> CMD3["Same kind of command<br/>on Autopilot"] --> ENG
```

Recording, show and autopilot are three writers of one stream. That is why they combine. A fader move is a command too, so a recording keeps it.

### 8. Two kinds of state

```mermaid
flowchart LR
  PR[("Project: what you made<br/>rig, looks, sources, surface, shows")]
  PE(["Performance: what happens now<br/>what runs, levels, clock"])
  PR -- "is played by" --> PE
```

| | Project | Performance |
|---|---|---|
| Saved | always, by itself | never by accident |
| Undo | yes | no |
| Recordable | no | yes |

**The LIVE switch** is part of the performance. On means the night is running: it follows the DJ software, records the set, arms the autopilot and locks the surface. Lights follow the app either way. There is no preview mode.

### 9. The real-time core is small and dumb

```mermaid
flowchart LR
  OUTSIDE["Outside: clever, may be slow, may crash<br/>UI, song analysis, autopilot choices, files, agents"]
  subgraph CORE["Core: on a steady beat, never waits"]
    direction LR
    E["Evaluate"] --> M["Merge"] --> P["Patch"] --> S["Send"]
  end
  OUTSIDE -- "commands" --> E
  S -. "snapshots" .-> OUTSIDE
```

A frozen UI cannot stop the lights.

## Three walks through the system

### You press a pad

```mermaid
sequenceDiagram
  actor You
  participant Surface
  participant Engine
  participant Rig as Lights
  participant Pad as Launchpad
  You->>Surface: press "Sunset"
  Surface->>Engine: start Sunset on Manual
  Engine->>Rig: next frame includes Sunset
  Engine-->>Surface: state changed
  Engine-->>Pad: state changed
  Note over Surface,Pad: both light up, nobody told the Launchpad
```

### You record a busk and play it back

```mermaid
sequenceDiagram
  actor You
  participant Engine
  participant Show
  You->>Engine: commands (recording on)
  Engine-->>Show: same commands, with time
  Note over Show: this list is now a show
  Show->>Engine: replays them as the show
  You->>Engine: grab a fader (Manual)
  Note over Engine: Manual is on top, you win
  You->>Engine: let go
  Note over Engine: the show is back
```

### The autopilot yields to your hand

```mermaid
sequenceDiagram
  participant Auto as Autopilot
  participant Engine
  actor You
  Auto->>Engine: rising look, all fixtures
  You->>Engine: hold Strobe on PARs (Manual)
  Note over Engine: PARs follow you, Movers follow Autopilot
  You->>Engine: release
  Note over Engine: PARs return to Autopilot, no cut
```

## Is the theory any good? What gets easy, what gets hard

| Change | Effort | Why |
|---|---|---|
| New output protocol | easy | one adapter behind Output |
| New kind of effect | easy | one new source |
| New controller | easy | one profile, mirrors the screen for free |
| Second UI or an agent | easy | another client of the Engine |
| Laser shapes | medium | a source plus one output adapter |
| "Just set channel 37" | awkward on purpose | goes through a raw-channel attribute |
| Change who wins | easy | one rule table in one place |
| A source reading another source | not possible | compose them into one |

## The theory says no

```mermaid
flowchart LR
  X1["Writing DMX<br/>outside Output"]:::no
  X2["A private way to<br/>start, stop or fade"]:::no
  X3["Truth held<br/>in UI code"]:::no
  X4["Smart code in<br/>the frame loop"]:::no
  classDef no stroke:#ef3e7b,stroke-width:2px,stroke-dasharray: 5 3
```

## Answers from review 1

| # | Question | Decided |
|---|---|---|
| 1 | Who wins? | manual beats show beats autopilot. Among equals the latest wins. Brightness can be set to "highest wins" |
| 2 | How many layers? | none to manage. The layer idea was dropped: only steps, sequences and patterns play |
| 3 | What does recording store? | commands, fader moves included |
| 4 | Where do raw channels live? | only on the DMX screen |
| 5 | DJ software and audio disagree on the beat | the feed you picked wins |
| 6 | What is Song Director? | the editor of a song's show. Song = show, one screen |
| 7 | Missing from QLC+? | nothing named |

Details: [review-1.md](review-1.md).
