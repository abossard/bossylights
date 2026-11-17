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

- **Top bar:** product name and show name, the six workspaces as tabs, status at the right.
- **Status, always visible, always in the same place:** the LIVE switch with its four indicators, tempo with four beat dots and a **Tap** button, master level, Blackout, Undo, **Learn**.
- **Now playing, always visible:** the two VirtualDJ decks sit in the header of the timetable and of the Now strip. Decks are numbered **1 and 2**. Per deck: number, state in a word (Playing, Cued, Stopped), **song title** and artist, tempo, position. The title of the song that plays is the largest text on the screen after the tempo. The words "on air" are not used.
- **Now strip:** directly under the top bar on every screen except Console and Songs. Three thin lanes (Manual, Show, Autopilot) with the NOW line, about 56 px high. Click it to go to Console. On Console and Songs the full timetable replaces it.
- **Workspaces, in this order:** Console, Songs, Looks, Patch, DMX, In/Out.
- **Layer colours** are used the same way everywhere: anything that belongs to a layer carries that layer's colour.
- **Layers:** three to start with (Autopilot, Show, Manual). The user can add layers. Every layer gets a lane and a colour.
- **Text case:** sentence case everywhere. Capitals only for LIVE and NOW.
- **Learn, across the whole app:** press Learn in the top bar, click any button, pad, fader or value on any screen, then move a hardware control. It is mapped. Everything that can be clicked can be mapped. The list of mappings is edited on In/Out.
- **Red** means blackout, an error, or recording.
- Keyboard: 1 to 6 switch workspace, Space is blackout hold, Cmd/Ctrl+Z is undo.
- Works at 1280 x 800 (laptop) and at 1024 x 768 with touch. No hover-only actions.

### The LIVE switch

One switch, one meaning: **the night is running**. Lights follow the app at all times, whether LIVE is on or off. There is no preview mode. Blackout is the only way to go dark.

| LIVE off: preparing | LIVE on: performing |
|---|---|
| Edit looks, patch, try things | Follows VirtualDJ |
| Nothing is recorded | Records the set |
| Autopilot waits | Autopilot armed |
| Surface can be edited | Surface locked |

- Four small indicators sit beside the switch: DJ link, recording, autopilot, lock. Each can be turned off on its own. **Recording is red while it is on.**
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

## The six screens

Each brief: what the screen is for, its regions, the states worth showing, what can be clicked, and the issues it stands for.

### 1. Console (home)

- **For:** running the lights by hand on a console you designed.
- **Regions:** the two decks above the timetable. Timetable (one lane per layer, time ruler, NOW line) on the left; lane headers carry on or off, and for Autopilot the style. Console on the right: page tabs (Main, Movers, Pixels, +), override notice, pad grid 8 x 4, six faders, XY pad. **There is no rig view.** The space it used goes to the timetable and the pads.
- **States:** three pads running, one held by hand. Notice "Manual overrides Autopilot on PAR 1-8" with Release.
- **Clickable:** press a pad to start or stop it (a block appears in the Manual lane). Hold a pad to flash. Drag faders and the XY pad. Release. Surprise me. Tap. **Edit surface** switches to edit mode: the grid appears, widgets can be dragged and resized, a widget drawer opens (button, slider, XY pad, cue list, speed dial, label, frame).
- **Issues:** Build your own control surface, Widget set, Surface generated from your rig, Pads mirror the screen, Layers: base, override, flash, Launch on the beat.

### 2. Songs (a song has one show)

Song = show. There is no separate Show screen and no "tonight's set": the app does not know the set in advance.

- **For:** finding songs, editing the show of a song, and editing it live while the song plays.
- **Regions:**
  - **Library**, left: search, filters (has a show, auto only, recorded today, needs review), analysis state, tempo and confidence per song. The song that plays is marked "Playing".
  - **The show of the selected song**, centre: waveform, the section ruler with the five labels, the show's **layers** as rows, blocks of looks per section, the **Auto** lane at the bottom, the energy curve. The NOW line is the playhead.
  - **Looks to add**, right: by group.
- **Sections can be changed:** drag an edge to move it, split one, merge two, rename one.
- **Layers of a show:** every recording pass adds a layer. Layers can be muted, merged and deleted.
- **Autopilot lives in the Auto lane:** on or off, style (Chill, Club, Peak, No strobe), "how wild", which groups it may use. Its picks are outlined blocks with a reason. On a block: Skip, More like this, **Keep** (it becomes a normal block that you own).
- **Live editing:** when the selected song is the one that plays and LIVE is on, a "Live edit" badge shows and every change is on the lights at once.
- **Recording:** with Recording on, what happens on the Console is written into the show of the song that plays, as a new layer, drawn in red while it records. Pad presses and fader moves are both kept.
- **States:** a song that plays with LIVE on. A layer recorded today beside the base layer. One section with no blocks, filled by Auto. A song that needs review. A song with no show yet (auto only).
- **Clickable:** search and filter, pick a song, select a section, move a section edge, rename a section, add or remove a look in a section, Fill from style, mute or merge a layer, Keep an auto block, play from a section, autopilot on or off and style, follows (deck 1, deck 2, timecode).
- **Issues:** Song Director, Song analysis: grid, sections, energy, VirtualDJ link, Show timeline with waveform, Record what you busk, Follow timecode, DJ set shows, Cue lists, Autopilot: the show follows the music, Never-dark fallback, Looks generated from your rig.

### 3. Looks

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

### 4. Patch

- **For:** telling the app which lights exist and where they sit on the DMX line. There is no map.
- **Regions:**
  - **Universes**, left: a list that can be long (pixel screens take many). Each universe shows a fill bar, the number of fixtures and any conflict. Search, and collapse runs of universes that belong to one pixel panel.
  - **Universe grid**, centre: the 512 addresses of the selected universe as a grid. Fixtures are coloured runs with a name plate. Drag a run to give it another address. Conflicts are marked.
  - **Fixtures**, beside or below the grid: by name plate, then model and mode, the address last and small. Inside a group the fixtures have an **order** that can be dragged. The order is what "left to right" means.
  - **Details**, right: the selected fixture.
- **Guided add:** "Add lights" opens a flow in place (not a modal dead end): search with pictures, pick a mode, the app proposes a free address and shows the DIP switches, the fixture flashes, confirm "that is this one".
- **Add a pixel panel:** columns and rows, wiring (snake or zigzag), start universe. The app spreads it over as many universes as it needs and shows them.
- **States:** 15 fixtures on universe 1. A 24 x 16 pixel panel on universes 2 to 4. One address conflict, flagged in the list and in the grid.
- **Issues:** Patch: fixtures, modes, addresses, Guided patch, Fixture library, Groups and the rig map.

### 5. DMX

- **For:** the expert layer: see and set raw channels, find faults.
- **Regions:** universe tabs. A dense grid of 512 channels with value bars, grouped and labelled by fixture. Right: "Held by hand" list with Release all. Bottom: output state per universe (device, frames per second, errors).
- **States:** three channels held by hand (they show the Manual colour), one universe with a device error.
- **Clickable:** click a channel to get a fader for it, filter "patched only" and "changing".
- **Issues:** DMX monitor and channel tester, DMX engine: universes, merge, masters.

### 6. In/Out

- **For:** everything that connects: controllers, audio, DJ software, the clock, outputs.
- **Regions:** three columns. **In:** controllers with profile and state (Launchpad Mini connected, APC mini not found), audio input with a level meter, VirtualDJ link with deck states, OSC. **Clock:** which feed drives the beat (Tap, Audio, MIDI clock, VirtualDJ, Timecode), each with its tempo and confidence, one selected. **Out:** universes and their devices (USB DMX, Art-Net, sACN, DDP) with state and rate.
- **States:** USB adapter "reconnecting", VirtualDJ connected on deck A, clock following VirtualDJ.
- **Mappings:** a table of everything that was mapped with Learn, anywhere in the app: control, screen, device, note or controller number, feedback. Rows can be edited, tested and removed.
- **Clickable:** pick the clock feed, tap tempo, Learn, edit or remove a mapping, test an output.
- **Issues:** USB DMX adapters, Art-Net and sACN, DDP pixel output, MIDI in with learn, Ready-made controller profiles, OSC in and out, Audio in: beat and levels, One tempo for everything.

## Two explainer screens (after the six)

The theory and the module map as live, animated pages inside the same app, with the same review tools. Sources: `docs/design/theory.md` and `docs/design/modules.md`.

### 7. How it works (the theory, animated)

- **For:** understanding the nine ideas by playing with them.
- **Regions:** the one-picture flow (writers, three layers, frame, rig, output) with commands travelling along it as dots. A small pad row and one fader to press. A "who wins" panel per fixture group that updates as layers speak. A step-through of the three walks (press a pad, record and replay, autopilot yields).
- **Clickable:** press a pad and watch the command travel and the Manual layer cover the others. Release and watch the layer below return. Toggle record and see commands written down, then replayed on Show.
- **Driven by** the same fake store as the other screens.

### 8. Modules (the boundary map, interactive)

- **For:** seeing which modules exist and how narrow each one is.
- **Regions:** the boundary map as a diagram. Click a module to open its card: whole interface, what it hides, which screens use it. A depth bar per module. A "trace" button that animates one press through Surface, Door, Mixer, Rig, Output.
- **Questions for review** from both documents appear here as A/B choices that are saved with the review export.

## Fake data, shared by all screens

- Show: "Friday at the Garage". Tempo 126. Master 85%.
- Rig: Mover L1, L2, R1, R2. PAR 1 to 8. Bar L, Bar R. Laser.
- Layers: Autopilot playing Build-up. Show off. Manual holding Strobe on PAR 1-8. LIVE is on.
- Songs in the set: four tracks with made-up titles and artists, marked as sample data.
- Decks: deck 1 plays song 2, deck 2 has song 3 cued.
- One fake state store drives all screens, so a pad pressed on the Console shows up in the Now strip, in DMX and in Show (when recording).
