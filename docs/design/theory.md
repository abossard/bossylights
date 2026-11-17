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

  subgraph L["Three layers, top wins"]
    direction TB
    LIVE["LIVE"]
    SHOW["SHOW"]
    AUTO["AUTOPILOT"]
  end

  FRAME(["Frame<br/>what every light does now<br/>44 times a second"])
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

**One sentence:** every frame is computed fresh from what plays on three layers, and everything that happens is a command.

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
    P2["Layers"] --> C
    P3["Time"] --> C
    C --> F(["Frame"])
  end
```

No shared state to corrupt. Stop means "stop contributing".

### 2. Lights, not channels

```mermaid
flowchart LR
  A["Mover L1<br/>colour: red"] --> R["Rig"] --> D["Universe 1<br/>ch 37 38 39 = 255 0 0"]
```

Addresses exist in one place only. You never remember a number.

### 3. A look is a value

| Fixture | Intensity | Colour |
|---|---|---|
| PAR 1 to 8 | 80% | amber |
| Mover L1 | not mentioned | not mentioned |

Plain data: store it, blend it, preview it, hand it to an agent.

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

Ten QLC+ function types become one shape.

### 5. Layers: the top one that speaks wins

```mermaid
flowchart TB
  LIVE["LIVE<br/>says: PARs strobe"]
  SHOW["SHOW<br/>says nothing"]
  AUTO["AUTOPILOT<br/>says: PARs amber, Movers sweep"]
  LIVE --> R1(["PARs: strobe"])
  AUTO --> R2(["Movers: sweep"])
  LIVE ~~~ SHOW ~~~ AUTO
```

Release Live and the PARs go back to amber. This replaces HTP and LTP.

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
  P["You press a pad"] --> CMD["Command<br/>start Sunset on Live"]
  CMD --> ENG["Engine"]
  CMD -. "written down" .-> REC[("Recording")]
  REC -. "played back" .-> CMD2["Same command<br/>on Show"]
  CMD2 --> ENG
  MUS["Music"] --> AP["Autopilot"] --> CMD3["Same kind of command<br/>on Autopilot"] --> ENG
```

Recording, show and autopilot are three writers of one stream. That is why they combine.

### 8. Two kinds of state

```mermaid
flowchart LR
  subgraph PR["Project: what you made"]
    direction TB
    A1["Rig"] ~~~ A2["Looks and sources"] ~~~ A3["Surface"] ~~~ A4["Shows"]
  end
  subgraph PE["Performance: what happens now"]
    direction TB
    B1["What runs"] ~~~ B2["Levels"] ~~~ B3["Clock"]
  end
  PR -- "is played by" --> PE
```

| | Project | Performance |
|---|---|---|
| Saved | always, by itself | never by accident |
| Undo | yes | no |
| Recordable | no | yes |

### 9. The real-time core is small and dumb

```mermaid
flowchart LR
  subgraph OUTSIDE["Outside: clever, may be slow, may crash"]
    direction TB
    UI["UI"] ~~~ AN["Song analysis"] ~~~ AP["Autopilot choices"] ~~~ FI["Files"] ~~~ AG["Agents"]
  end
  subgraph CORE["Core: 44 times a second, never waits"]
    direction LR
    E["Evaluate"] --> M["Merge"] --> P["Patch"] --> S["Send"]
  end
  OUTSIDE -- "commands" --> CORE
  CORE -. "snapshots" .-> OUTSIDE
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
  Surface->>Engine: start Sunset on Live
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
  Show->>Engine: replays them on the Show layer
  You->>Engine: grab a fader (Live)
  Note over Engine: Live is on top, you win
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
  You->>Engine: hold Strobe on PARs (Live)
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
| Second UI or an agent | easy | another client of the same door |
| Laser shapes | medium | a source plus one output adapter |
| "Just set channel 37" | awkward on purpose | goes through a raw-channel attribute |
| More than three layers | hard | changes idea 5 |
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

## Questions for you

| # | Question | Option A (my lean) | Option B |
|---|---|---|---|
| 1 | Who wins inside a layer? | Top layer wins for everything | Intensity adds up, so two washes are brighter |
| 2 | How many layers? | Three, fixed | The user can add layers |
| 3 | What does recording store? | Commands: editable, follows rig changes | DMX values: exact, but frozen |
| 4 | Where do raw channels live? | Only on the DMX screen | Everywhere |
| 5 | DJ software and audio disagree on the beat | The feed you picked wins | The most confident feed wins |
| 6 | What is Song Director? | An editor for a show whose blocks follow song sections | Its own thing |
| 7 | Anything you rely on in QLC+ that these nine ideas cannot say? | | |

Answer with the number and A or B, here or in [issue #100](https://github.com/abossard/bossylights/issues/100).
