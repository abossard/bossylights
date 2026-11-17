---
name: Bossylights
description: A lighting desk that reads like tonight's running order.
colors:
  ground: "#1c1a17"
  surface: "#26231f"
  surface-raised: "#36322c"
  line: "#46413a"
  paper: "#f4efe6"
  mute: "#c4bbad"
  ink: "#1c1a17"
  layer-manual: "#ef3e7b"
  layer-show: "#20b2c9"
  layer-autopilot: "#f2b705"
  danger: "#ff4a3d"
typography:
  tempo:
    fontFamily: "Big Shoulders Display, Archivo Narrow, sans-serif"
    fontSize: "38px"
    fontWeight: 900
    lineHeight: 1
  playing:
    fontFamily: "Big Shoulders Display, Archivo Narrow, sans-serif"
    fontSize: "32px"
    fontWeight: 800
    lineHeight: 1
  title:
    fontFamily: "Big Shoulders Display, Archivo Narrow, sans-serif"
    fontSize: "22px"
    fontWeight: 800
    lineHeight: 1
    letterSpacing: "0.02em"
  heading:
    fontFamily: "Big Shoulders Display, Archivo Narrow, sans-serif"
    fontSize: "18px"
    fontWeight: 800
    lineHeight: 1
    letterSpacing: "0.02em"
  body:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 500
    lineHeight: 1.3
  label:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 700
    lineHeight: 1.15
rounded:
  none: "0px"
spacing:
  "1": "4px"
  "2": "8px"
  "3": "12px"
  "4": "16px"
  "5": "24px"
  gutter: "16px"
  target: "44px"
components:
  button:
    backgroundColor: "{colors.ground}"
    textColor: "{colors.paper}"
    rounded: "{rounded.none}"
    padding: "0 14px"
    height: "44px"
  button-primary:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ink}"
    rounded: "{rounded.none}"
    padding: "0 14px"
    height: "44px"
  button-danger-on:
    backgroundColor: "{colors.danger}"
    textColor: "{colors.ink}"
    rounded: "{rounded.none}"
  pad:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.paper}"
    rounded: "{rounded.none}"
  pad-running-manual:
    backgroundColor: "{colors.layer-manual}"
    textColor: "{colors.ink}"
  pad-running-show:
    backgroundColor: "{colors.layer-show}"
    textColor: "{colors.ink}"
  pad-running-autopilot:
    backgroundColor: "{colors.layer-autopilot}"
    textColor: "{colors.ink}"
  block:
    backgroundColor: "{colors.layer-autopilot}"
    textColor: "{colors.ink}"
    rounded: "{rounded.none}"
    padding: "7px 6px"
  now-flag:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ink}"
    padding: "3px 6px"
  live-switch-on:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ink}"
    height: "44px"
---

# Design System: Bossylights

Built in `mockups/` (tokens in `mockups/src/styles/tokens.css`, components in `mockups/src/kit/`). This file describes what is built.

## Overview

**Creative North Star: "Running Order"**

Tonight is a timetable, like the set-times board at a festival. Three rows (Manual, Show, Autopilot), a NOW line, flat blocks of colour with dark text. Made for a laptop in a dark room next to DJ decks, and for a touch screen.

What the eye finds, in this order:

| Order | What | How it wins |
|---|---|---|
| 1 | The song that plays | 32 px title, the only text that size |
| 2 | The NOW line | A paper line through every lane, with its flag |
| 3 | What is running | Solid fills in the colour of its row |
| 4 | Everything else | Quiet: dark surfaces, 18 px headings, 13 to 14 px labels |

**Key Characteristics:**
- Flat colour. No gradients, glow or shadows. Square corners.
- One colour per row (manual, show, autopilot), used for nothing else.
- Paper means "now" and "do this".
- Standard controls: buttons, tabs, sliders, lists, tables.

## Colors

Warm off-black and paper, plus one flat colour for each of the three rows.

| Token | Value | Used for |
|---|---|---|
| `--bg` | `#1c1a17` | Ground |
| `--s1` | `#26231f` | Panels, idle pads, deck slabs |
| `--s2` | `#36322c` | Hover, fader fill, unlit lamps |
| `--line` | `#46413a` | Rules and quiet borders |
| `--paper` | `#f4efe6` | Text, NOW, LIVE on, selected, primary action |
| `--mute` | `#c4bbad` | Secondary text |
| `--ink` | `#1c1a17` | Text on any colour or on paper |
| `--layer-manual` | `#ef3e7b` | Manual: your hands |
| `--layer-show` | `#20b2c9` | Show |
| `--layer-autopilot` | `#f2b705` | Autopilot |
| `--danger` | `#ff4a3d` | Blackout, recording, errors |

### Named Rules

**The Row Colour Rule.** Pink, cyan and marigold mean Manual, Show and Autopilot: who started it. There are no layers to manage. (The tokens keep their names, `--layer-manual` and so on.) They never decorate, highlight or mark a primary action.

**The Paper Rule.** Paper fills mean "now" (NOW flag, the deck that plays, LIVE on) or "this one" (selected tab, primary button). Text on a fill is always ink.

**The Red Rule.** Red is for blackout, recording and errors only. Recording is red while it is on: the indicator, the Record button and the take being recorded. Look pads keep their own colours as a field on the pad.

## Typography

**Display Font:** Big Shoulders Display (800, 900), sentence case, tracking 0.02em
**Body Font:** Archivo (variable: weight 400 to 700, width 62 to 125)

**The Case Rule.** Sentence case everywhere. Capitals only for LIVE and NOW.

**Character:** a condensed poster face for names and numerals, a workhorse sans for everything read at a glance.

### Hierarchy

| Token | Size | Face | Used for |
|---|---|---|---|
| `--display-2xl` | 38 px | Display 900 | Tempo. The largest text on any screen. |
| `--display-xl` | 32 px | Display 800 | Title of the song that plays |
| `--display-lg` | 26 px | Display 900 | LIVE, page title |
| `--display-md` | 22 px | Display 800 | Fader values, product name, panel titles |
| 20 px | 20 px | Display 800 | Lane names, block titles, workspace tabs |
| `--display-sm` | 18 px | Display 800 | Region headings, cued deck title |
| `--text-lg` | 16 px | Archivo | Rarely: emphasis in prose |
| `--text-md` | 14 px | Archivo 500 to 700 | Body, buttons, pad names |
| `--text-sm` | 13 px | Archivo 600 to 700 | Labels, status lines, chips |

### Named Rules

**The 13 Rule.** Nothing read during a show is smaller than 13 px.

**The Tempo Rule.** The tempo is the largest text. The song that plays is second. Nothing else exceeds 26 px.

**The Narrow Rule.** Where a label is tight, narrow Archivo with `font-stretch: var(--tight)` (82%) before shrinking it. Numbers are tabular everywhere.

## Layout

- Shell: two rows. Row 1 (56 px): name, LIVE switch with four indicators, tempo, master, Blackout, Undo. Row 2 (44 px): workspace tabs, then "All reviews" and "Review this screen" at the right.
- One gutter (`--gutter`: 16 px, 12 px at 1180 px and below) for the top bar, the Now strip and every screen. Left and right edges line up.
- Spacing scale: 4, 8, 12, 16, 24 (`--sp-1` to `--sp-5`).
- Touch target: 44 px (`--target`) for anything pressed during a show.
- Sizes checked: 1280 x 800 and 1024 x 768. One breakpoint at 1180 px. No horizontal scroll.
- The Now strip (56 px) sits under the top bar on every screen without its own timetable.

## Elevation & Depth

None. No shadows, no glow, no blur. Depth is three tones: ground, `--s1` panels, `--s2` hover. Overlays (review panel, widget drawer) are opaque panels with a 2 px border.

## Shapes

Square corners everywhere (radius 0). The only round shapes are things that are round in the world: the four beat dots. Rules are 2 px (`--rule`) or 1 px for quiet dividers. Dashed means "not now": planned, waiting, off, or locked.

## Components

### Pad (the primary control)

```
+----------------+   +----------[NOW]+   +----------[NOW]+
|  colour field  |   |  colour field  |   |  colour field  |
+----------------+   +================+   +================+
| Name           |   | Name           |   | Name           |
+----------------+   +----------------+   +----------------+
  idle                 running             flash (finger down)
  dark body            body = layer        + 4 px paper frame
```

| State | Body | Extras |
|---|---|---|
| Idle | `--s1`, name in paper | Look colour as a field across the top third |
| Hover | `--s2` | |
| Pressed | | 4 px paper frame at once |
| Running: Autopilot | Marigold, ink text | NOW flag |
| Running: Show | Cyan, ink text | NOW flag |
| Running: Manual | Pink, ink text | NOW flag |
| Flash (held down) | Pink, ink text | NOW flag and the paper frame |

Name: 14 px bold, narrow, never breaks at a hyphen. Pads 84 px and taller also say who runs them.

### LIVE switch

| State | Look |
|---|---|
| On | Paper slab, ink "LIVE on", knob right |
| Off | Outline, mute "LIVE off", knob left |

Four indicators beside it (DJ link, Recording, Autopilot, Lock), each a lamp and a word:

| Lamp | Meaning |
|---|---|
| Filled paper | On, in effect now |
| Filled mute | Ready, starts with LIVE |
| Empty, word struck through | Turned off |
| Red lamp and red word | Recording, while it is on |

With no lights connected a red-outlined chip says so and opens In/Out.

### Timetable and NOW line

| Part | Look |
|---|---|
| Lane label | Name in the row's colour (20 px), one state word, one short line |
| State word | holding, playing (filled) or idle, waiting, off (outlined) |
| Held, pad off | The Manual block goes on as a dashed tail "held, pad off" until Release |
| Take (Songs) | Red while it records ("Recording"), then the Show colour ("Recorded today 21:58") |
| Block, now or later | Flat fill, ink text |
| Block, past | 2 px outline in the row's colour |
| Block, row off or waiting | Dashed outline |
| Still running | Fill up to NOW, then an open dashed tail: "holding" |
| NOW | 6 px paper line with ink edges, flag "NOW 22:47" |

A narrow block wraps its title at 15 px. Narrower than a word, it keeps only its colour and a tooltip. Blocks that run at the same time stack in the lane.

### Decks

Decks are numbered 1 and 2. Their states are Playing, Cued and Stopped. The deck that plays gets the room: paper number block, 32 px title, a paper "Playing" chip, a 3 px progress line. The other deck is as wide as its text needs: 18 px title in mute. A deck with nothing loaded is a dashed outline that says "Nothing loaded".

### Buttons

| Variant | Look | Use |
|---|---|---|
| Default | 2 px `--line` outline | Most actions |
| Primary | Paper fill, ink text | The one main action of a view |
| Row colour | Outline or fill in pink, cyan or marigold | Acts on what you hold, on the show or on the autopilot (Release, Keep) |
| Danger | Red outline, red fill when on | Blackout, Record |
| Toggle on | Paper fill | Any pressed toggle |
| Disabled with reason | Dashed outline, the reason under the label | "Edit surface / Locked while LIVE is on" |

44 px high, square, hover brightens the border, pressed darkens the fill. Focus: 3 px paper outline, 2 px off the control.

### Tabs, Segmented, Fader, XY pad

- Tabs: display face, selected is a paper slab with ink text.
- Segmented: one outlined group, the chosen option filled.
- Fader: 44 px track, paper thumb with ink edge, value above in 22 px display, label below.
- XY pad: paper crosshair and square dot on a quiet grid.

### Held until Release

What you touch on the Console stays yours until Release. Three marks say so, all pink:

| Where | Mark |
|---|---|
| Notice above the pads | "You hold, until Release", one line per pad or control with what it touched ("brightness of PAR 1-8"), a dashed "pad off" chip, and the Release button |
| Pad that is off but still held | 3 px pink outline and a pink "held" tag in the corner. No NOW flag, no fill |
| Timetable, Manual row | The block continues as a dashed tail "held, pad off" |

### Who wins (DMX)

Five numbered rows: rule name, then the rule or its setting. Settings are two-option switches, 30 px high (an expert screen, not pressed during a show). The row names Manual, Show and Autopilot in their colours.

### Learn

"Learn" sits in the top bar. Step 1: click any button, pad, fader or value on any screen. Step 2: move a control on the hardware. A mapped control carries a 9 px paper square in its top left corner. The list of mappings is on In/Out.

### Universe grid (Patch)

512 cells, 32 per row. A fixture is a flat run of cells with its name plate, coloured by its group (never pink, cyan or marigold). A clash is a red bar over the shared cells. Drag a run to give it another address.

### Region

Every part of a screen is a Region: an 18 px display heading with an optional 13 px aside at the right. In comment mode each Region gets a dashed paper outline and a paper title chip.

### Edit mode and review layer

Neutral paper chrome, no layer colour: paper frames, paper name chips, 44 px handles for remove and resize, a dashed 12 x 12 grid. The review panel is an opaque `--s1` panel at the right.

## Do's and Don'ts

### Do:
- **Do** give a running thing the colour of its row as a solid fill, and the NOW flag.
- **Do** use paper for the single primary action and for "now".
- **Do** say why a control is disabled, on the control.
- **Do** narrow a label before shrinking it, and keep it at 13 px or more.
- **Do** use dashed lines for planned, waiting, off and locked.

### Don't:
- **Don't** use pink, cyan or marigold for anything but manual, show and autopilot.
- **Don't** add gradients, glow, shadows or rounded corners.
- **Don't** set any text larger than the tempo, or as large as the song that plays.
- **Don't** rely on hover: every action has a visible control.
- **Don't** open a modal. Flows happen in place.
