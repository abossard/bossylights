# Screen briefs for the mockups

Status: brief for the interactive mockups in `mockups/`. The mockups are for alignment, navigation and structure. They fake all data and talk to no engine. An approved screen becomes acceptance criteria on its GitHub issues.

Visual world: **Running Order** (chosen 2026-11-17). The composition to match is `.impeccable/mocks/decision/model-pick.html`.

## The world in four contributions

- **Palette:** warm off-black ground `#1c1a17`, paper `#f4efe6` for text. Each layer owns one flat colour: Autopilot marigold `#f2b705`, Show cyan `#20b2c9`, Manual hot pink `#ef3e7b`. Blocks are flat fills with dark text. No gradients, no glow, no shadows. Look pads keep their own colour chips. Red is only for blackout and errors.
- **Type:** a condensed poster sans (Big Shoulders Display) for lane labels, block titles and big numerals. A workhorse sans (Archivo) for everything small. Times and values in tabular numerals.
- **Density:** bold and chunky in the default layer, square corners, large targets (44 px minimum for anything pressed live). Expert screens (DMX) are denser and ruled.
- **Signature move:** the **NOW line**. Wherever a screen shows time, a paper-white vertical line with a "NOW" flag marks the present, and things that run carry the same flag.

Controls are standard: buttons, tabs, sliders, lists, tables. The world never supplies a layout or a control.

## Shell (every screen)

- **Top bar:** product name and show name, the seven workspaces as tabs, status at the right.
- **Status, always visible, always in the same place:** the LIVE switch with its four indicators, tempo with four beat dots, master level, Blackout, Undo.
- **Now playing, always visible:** the two VirtualDJ decks sit in the header of the timetable and of the Now strip. Per deck: letter, playing or paused, **song title** and artist, tempo, position, and which deck is on air. The title of the song on air is the largest text on the screen after the tempo.
- **Now strip:** directly under the top bar on every screen except Console and Show. Three thin lanes (Manual, Show, Autopilot) with the NOW line, about 56 px high. Click it to go to Console. On Console and Show the full timetable replaces it.
- **Workspaces, in this order:** Console, Show, Songs, Looks, Patch, DMX, In/Out.
- **Layer colours** are used the same way everywhere: anything that belongs to a layer carries that layer's colour.
- Keyboard: 1 to 7 switch workspace, Space is blackout hold, Cmd/Ctrl+Z is undo.
- Works at 1280 x 800 (laptop) and at 1024 x 768 with touch. No hover-only actions.

### The LIVE switch

One switch, one meaning: **the night is running**. Lights follow the app at all times, whether LIVE is on or off. There is no preview mode. Blackout is the only way to go dark.

| LIVE off: preparing | LIVE on: performing |
|---|---|
| Edit looks, patch, try things | Follows VirtualDJ |
| Nothing is recorded | Records the set |
| Autopilot waits | Autopilot armed |
| Surface can be edited | Surface locked |

- Four small indicators sit beside the switch: DJ link, recording, autopilot, lock. Each can be turned off on its own.
- With no lights connected, a chip says "No lights connected" and opens In/Out.
- Names: the home screen is **Console**. The top layer, your hands, is **Manual**. "Live" means only this switch.

## Review layer (every screen)

The point of the mockups is your judgement, so the review tools are part of the shell.

- A **Review** button at the right edge of the top bar opens a side panel for the current screen.
- **Verdict:** Approve, Change, Rethink. **Rating:** 1 to 5. **Comment:** free text.
- **Region notes:** in comment mode every region of the screen gets an outline and a title. Click one to leave a note on that region.
- Each screen lists the GitHub issues it stands for.
- A last tab, **Review**, shows all screens with verdict, rating and note count, and has **Export** (a JSON file) and **Copy as Markdown**.
- Everything is kept in the browser (localStorage) until exported.

## The seven screens

Each brief: what the screen is for, its regions, the states worth showing, what can be clicked, and the issues it stands for.

### 1. Console (home)

- **For:** running the lights by hand on a console you designed.
- **Regions:** the two decks with song names above the timetable. Timetable (three lanes, time ruler, NOW line) top left; the Auto lane header carries autopilot on/off and style. Rig view bottom left. Console right: page tabs (Main, Movers, Pixels, +), override notice, pad grid 8 x 4, six faders, XY pad.
- **States:** three pads running, one held by hand. Notice "Manual overrides Autopilot on PAR 1-8" with Release.
- **Clickable:** press a pad to start or stop it (a block appears in the Manual lane, the rig view changes colour). Hold a pad to flash. Drag faders and the XY pad. Release. Surprise me. **Edit surface** switches to edit mode: the grid appears, widgets can be dragged and resized, a widget drawer opens (button, slider, XY pad, cue list, speed dial, label, frame).
- **Issues:** Build your own control surface, Widget set, Surface generated from your rig, Pads mirror the screen, Layers: base, override, flash, Launch on the beat.

### 2. Show (timeline, recorder, runner, autopilot)

- **For:** the night as one timeline. A song plays its own show if it has one. Where nothing is programmed, the autopilot fills in.
- **Regions:** top: the two decks with song names, and the mode control (**Edit**, **Record**, **Run**). Left: tonight's set, songs in play order, each marked "show", "auto" or "recorded". Centre: the timeline with time and bar ruler, waveform, section ruler, tracks (Looks, Movers, Pixels, Laser, Recorded) and at the bottom the **Auto** lane. The NOW line is the playhead. Right, in Run mode: a large "Now / Next" list.
- **Autopilot lives here, it has no screen of its own.** Controls in the Auto lane header: on/off, style (Chill, Club, Peak, No strobe), "how wild", which groups it may use. Its picks appear as outlined blocks for now and for what comes next, each with a reason ("Drop, high energy"). On a block: Skip, More like this, **Keep** (the block becomes a normal show block that you own).
- **Edit:** select, move and trim clips. **Record:** a red state, every press on the Console arrives as a chip on the Recorded track. **Run:** the timeline shrinks, "Now / Next" takes over with Go and Back.
- **States:** a set of four songs. Song 1 recorded. Song 2 playing, with its own show for the drop only and auto blocks around it. Song 3 auto. Song 4 not analysed yet.
- **Clickable:** mode, play and record, autopilot on/off and style, Keep on an auto block, select and move a clip, follows (Internal clock, VirtualDJ deck A, Timecode).
- **Issues:** Show timeline with waveform, Record what you busk, Follow timecode, DJ set shows, Cue lists, Autopilot: the show follows the music, Never-dark fallback, Looks generated from your rig.

### 3. Songs (Song Director)

- **For:** giving a song its looks, section by section.
- **Regions:** left: song library with analysis state (Analysed, Queued, Needs review), tempo and confidence. Centre: the selected song: waveform, section ruler with the five labels (intro, build, drop, break, outro), one block per section holding looks, the energy curve. Right: looks to drag in, grouped (Wash, Movement, Strobe, Pixels).
- **States:** one song analysed and filled, one section empty, one song "Needs review" with low confidence.
- **Clickable:** pick a song, select a section, add or remove a look in a section, "Fill from style", play from a section.
- **Issues:** Song Director, Song analysis: grid, sections, energy, VirtualDJ link.

### 4. Looks

Model: `docs/design/looks.md`. A look is anything you can play: a **step**, a **sequence** or a **pattern**.

- **For:** making steps, sequences and patterns, and trying them on the real lights at once.
- **Regions:**
  - **Library**, left: search, a kind filter (Step, Sequence, Pattern), groups as chips (one thing can sit in many groups, there are no folders). Each row shows a symbol for its kind, its colour chips and its measure in beats.
  - **Editor**, centre. It changes with the kind:
    - Step: which fixtures, then one card per property: Colour (chips and picker), Position (an x/y grid), Gobo (pictures), Dimmer, Spin. A property you did not set is shown as "not set" and is not part of the step.
    - Sequence: a beat grid. Each row is one item, a step or another sequence. One beat per item by default, drag an edge to change the width. A selected item shows its use settings: speed (÷2, ×1, ×2), loop or once, spread across fixtures, transition. Opening an inner sequence drills in, with a breadcrumb back.
    - Pattern: the kind (plasma, stripes, wave, sparkle), its measure, where the colours come from (a step), and which fixture group or pixel matrix it runs on.
  - **About this look**, right: "Used in" (every sequence that uses it, so a change here changes them all), its groups, **Try on lights** (press to toggle, hold to flash), "Put on Console".
- **No animated preview.** Looks are told apart by symbols and colour chips. Testing happens on the lights.
- **States:** the sequence "Build-up end A" open, with "Mover run" used at ×2 and "Flicker" looping. A step that sets only colours. The pattern "Plasma" on the pixel bars taking colours from "Jungle colours". "Mover run" used in three sequences.
- **Clickable:** pick anything in the library, filter by kind and group, edit a step property, change an item's width, change a use setting, drill into an inner sequence and back, add to or remove from a group, Try on lights.
- **Issues:** Programmer: touch it, see it, Looks: store what you see, Palettes that stay linked, Effects across a group, Chasers and sequences, Pixel matrix effects, Audio-reactive colour scripts, Collections.

### 5. Patch

- **For:** telling the app which lights exist and where they are.
- **Regions:** left: fixture list by name plate (model and mode second, address last and small). Centre: the rig map, fixtures can be dragged, groups shown as outlines. Right: details of the selected fixture. Bottom: a strip of the 512 addresses of the universe with used ranges.
- **Guided add:** "Add lights" opens a flow in place (not a modal dead end): search with pictures, pick a mode, the app proposes a free address and shows the DIP switches, the fixture flashes, confirm "that is this one".
- **States:** 15 fixtures, one address conflict flagged in the strip and in the list.
- **Issues:** Patch: fixtures, modes, addresses, Guided patch, Fixture library, Groups and the rig map.

### 6. DMX

- **For:** the expert layer: see and set raw channels, find faults.
- **Regions:** universe tabs. A dense grid of 512 channels with value bars, grouped and labelled by fixture. Right: "Held by hand" list with Release all. Bottom: output state per universe (device, frames per second, errors).
- **States:** three channels held by hand (they show the Manual colour), one universe with a device error.
- **Clickable:** click a channel to get a fader for it, filter "patched only" and "changing".
- **Issues:** DMX monitor and channel tester, DMX engine: universes, merge, masters.

### 7. In/Out

- **For:** everything that connects: controllers, audio, DJ software, the clock, outputs.
- **Regions:** three columns. **In:** controllers with profile and state (Launchpad Mini connected, APC mini not found), audio input with a level meter, VirtualDJ link with deck states, OSC. **Clock:** which feed drives the beat (Tap, Audio, MIDI clock, VirtualDJ, Timecode), each with its tempo and confidence, one selected. **Out:** universes and their devices (USB DMX, Art-Net, sACN, DDP) with state and rate.
- **States:** USB adapter "reconnecting", VirtualDJ connected on deck A, clock following VirtualDJ.
- **Clickable:** pick the clock feed, tap tempo, MIDI learn (click a control, then it waits for a press), test an output.
- **Issues:** USB DMX adapters, Art-Net and sACN, DDP pixel output, MIDI in with learn, Ready-made controller profiles, OSC in and out, Audio in: beat and levels, One tempo for everything.

## Two explainer screens (after the seven)

The theory and the module map as live, animated pages inside the same app, with the same review tools. Sources: `docs/design/theory.md` and `docs/design/modules.md`.

### 8. How it works (the theory, animated)

- **For:** understanding the nine ideas by playing with them.
- **Regions:** the one-picture flow (writers, three layers, frame, rig, output) with commands travelling along it as dots. A small pad row and one fader to press. A "who wins" panel per fixture group that updates as layers speak. A step-through of the three walks (press a pad, record and replay, autopilot yields).
- **Clickable:** press a pad and watch the command travel and the Manual layer cover the others. Release and watch the layer below return. Toggle record and see commands written down, then replayed on Show.
- **Driven by** the same fake store as the other screens.

### 9. Modules (the boundary map, interactive)

- **For:** seeing which modules exist and how narrow each one is.
- **Regions:** the boundary map as a diagram. Click a module to open its card: whole interface, what it hides, which screens use it. A depth bar per module. A "trace" button that animates one press through Surface, Door, Mixer, Rig, Output.
- **Questions for review** from both documents appear here as A/B choices that are saved with the review export.

## Fake data, shared by all screens

- Show: "Friday at the Garage". Tempo 126. Master 85%.
- Rig: Mover L1, L2, R1, R2. PAR 1 to 8. Bar L, Bar R. Laser.
- Layers: Autopilot playing Build-up. Show off. Manual holding Strobe on PAR 1-8. LIVE is on.
- Songs in the set: four tracks with made-up titles and artists, marked as sample data.
- Decks: A playing song 2 and on air, B has song 3 cued.
- One fake state store drives all screens, so a pad pressed on the Console shows up in the Now strip, in DMX and in Show (when recording).
