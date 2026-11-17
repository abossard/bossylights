# Screen guide

For anyone adding or replacing one of the six workspaces (Console, Songs, Looks, Patch, DMX, In/Out) or the two explainers (How it works, Modules). The screen briefs in `docs/design/screens.md` are the source of truth for what a screen shows.

## File convention

- A screen is the folder `src/screens/<id>/`. Its entry is `index.tsx`. Everything else it needs (CSS, sub-components, screen-only fake data) lives in the same folder.
- `index.tsx` exports:

| Export | Type | Meaning |
|---|---|---|
| `id` | `string` | Same as the folder name. URL `#/<id>`, review key. |
| `title` | `string` | Tab title, plain words. |
| `order` | `number` | Tab position. The six workspaces are 1 to 6, also their number keys. |
| `issues` | `{ number, title }[]` | GitHub issues the screen stands for. |
| `Screen` | `Component` | The screen. |
| `nowStrip` | `boolean`, optional | `false` on screens that show the full timetable (Console, Songs). |

- The shell finds screens with `import.meta.glob`. Adding or replacing a screen means creating or overwriting files in its folder. Never edit `src/shell`, `src/kit`, `src/fake`, `src/review` or another screen's folder for your screen.
- Never import `src/shell/registry` values from a screen: the registry imports every screen, so that is a circular import (only `import type { Issue }` is safe).
- Import CSS from `index.tsx` (`import "./<id>.css"`) and prefix every class with the screen id (`.patch-...`) so screens never restyle each other.
- The screen fills `.screen-host`, which scrolls vertically. Fit 1280 x 800 without scrolling where the brief allows; at 1024 x 768 vertical scroll is fine, horizontal scroll never.
- The LIVE switch lives in `src/shell/LiveSwitch.tsx` and `live-switch.css` on its own, so it can be replaced in one place.
- Learn lives in `src/shell/learn.tsx`. It works on every `button`, `[role=slider]`, `[role=switch]`, `[role=radio]` and `[role=tab]`, and names a control by its `aria-label` or its text. Give every control a clear name and it can be learned; a screen does nothing else for it.

## Regions and review

- Wrap every part of the screen named in its brief in `<Region id title>`. The review layer outlines each Region in comment mode and keys notes by `id`, so keep ids stable once reviewed.
- `id` is unique within the screen (`"library"`, `"timeline"`). `title` is the plain-words heading.
- Props: `bare` hides the visible heading (title still shows in comment mode), `aside` puts content at the right of the heading, `class`, `style`.
- Do not nest Regions.

## Kit (`import { ... } from "../../kit"`)

| Component | Props |
|---|---|
| `Region` | `id, title, bare?, aside?, class?, style?` |
| `Button` | `variant?: default, primary (paper), danger, quiet, layer`, `layer?`, `pressed?` (toggle), `size?: md, lg`, `reason?` (shown under the label while disabled), plus button attributes |
| `Tabs` | `items: { id, label, ariaLabel? }[], value, onChange, label, variant?: workspace, page` |
| `Segmented` | `options: { value, label }[], value, onChange, label, layer?, size?` |
| `Fader` | `value 0..100, onChange, label, orientation?, layer?, valueText?, bare?` |
| `Pad` | `label, colors, state: idle, autopilot, show, held, flash, kept, onDown, onUp(heldMs), disabled?`. A running pad fills with the colour of who runs it and carries the NOW flag. `kept`: the pad is off but what it touched is still held, so it keeps a pink outline and a "held" tag. |
| `XYPad` | `x, y (0..100, y up), onChange(x, y), label, layer?` |
| `RigView` | `fixtures, lookOf(f) => { color, level }, zones?, onFixture?, onMove?, selected?`. No screen uses it since review 1 (no rig view, no map). |
| `Timetable` | `from, to, now, lanes, tickEvery, format, laneHeight?, labelWidth?, laneControls?(lane), plain?` |
| `Decks` | `decks, compact?`: the two VirtualDJ decks, numbered 1 and 2, with the state word Playing, Cued or Stopped. The deck that plays has the largest title. A deck with `song: null` shows "Nothing loaded". |
| `NowStrip` | `from, to, now, lanes, decks, onActivate` (the shell already shows it) |
| `List` | `items: { id, title, sub?, meta?, swatch?, layer?, actions? }[], onSelect?, selected?, empty?, label?` |
| `Table` | `columns: { key, title, align?, width?, render? }[], rows, rowKey, onRowClick?, selected?, dense?, caption?, empty?` |
| `Meter` | `value 0..1, label, layer?, tone?: paper, danger, orientation?` |
| `EnergyCurve` | `spans: { start, end, energy, title }[], from, to, now, layer?, label` |
| `stepWindow(now, lead, span, step)` | A time window that holds still so the NOW line walks across it. |

Missing a control? Build it in your screen folder from these pieces. Do not add it to the kit.

## Fake store (`import { store, hhmm, mmss, groupNames } from "../../fake/store"`)

- State, read-only: `store.state` with `showName, tempo, master, night (the LIVE switch and its indicators), lightsConnected, blackout, faders, xy, manual (blocks, each with `end`: pad off, and `released`: manual control ended), xyHeld, rules (the who-wins settings), songSections, takes, showMuted, sectionLooks, mappings, panels, autopilot, events`.
- Clock: `store.now()` (show clock, seconds since midnight, starts 22:47:12), `store.beat()` 0..3, `store.beatPhase()` 0..1.
- Songs: song 1 recorded, song 2 playing on deck 1 with its own show, song 3 auto only (cued on deck 2), song 4 not analysed. There is no set list: a song is its show. `LIBRARY_ONLY` (from `fake/data`) has one more song, "Needs review" with low confidence, for the Songs screen.
- Data: `store.rig` (15 fixtures with group, position, model, mode, universe, address, channels; Laser overlaps Bar R on purpose, the one address conflict), `store.looks`, `store.songs` (four sample songs with sections, energy, analysis state), `store.groups`.
- Commands: `start(lookId)`, `stop(lookId)`, `hold(lookId)` and `release(lookId)` for a flash, `releaseAll()` (the Release button: everything you hold), `surprise()`, `setMaster(v)`, `setBlackout(on)`, `setLive(on)`, `toggleIndicator("dj" | "record" | "autopilot" | "lock")`, `setFader(id, v)`, `setXY(pan, tilt)`, `setAutopilot(key, value)`, `toggleGroup(g)`, `skipLook(id)`, `moreLikeThis(id)`, `skipPick(section)`, `morePick(section)`, `keepPick(section)` (an autopilot pick becomes part of the song's show), `tap()` (tap tempo), `undo()`.
- Three fixed rows: Manual, Show, Autopilot. There are no layers to add, name or order.
- Who wins (`docs/design/who-wins.md`) is decided in one place in the store. Ask it, never decide in a screen: `winner(group, prop)` and `contenders(group, prop)` (winner first, then who is under it; each `{ rank, look, padOff?, left? }`), with `prop` one of `PROPS` (`colour`, `brightness`, `position`). `propsOf(look)` says what a look sets. The settings: `state.rules` (`brightness: latest | highest`, `manualEnds: release | pad`, `lookEnds: fallback | stay`), changed with `setRule(key, value)`. The "Who wins" panel on the DMX screen is the only place that calls `setRule`.
- Manual holds until Release: `stop(lookId)` switches a pad off but what it touched stays held (`lookState` gives `"kept"`). `holds()` lists what the Console holds (`name, live, props, groups, beaten`), `holdText(hold)` says it in words, `heldManual()` gives the blocks. A flash holds nothing after the finger lifts. `stop(lookId, true)` also gives back (a try in an editor). The Movers position pad holds the position (`state.xyHeld`). The group faders are masters and hold nothing.
- Lanes from `lanes(from, to)` (always three) carry `chip` (one state word: holding, playing, idle, waiting, off), `on` (the chip is filled) and `status` (one short line).
- Edge states to design for: `store.state.lightsConnected` false, a deck with nothing loaded (`setLightsConnected`, `setCuedDeckLoaded`; the review panel has switches for both), LIVE off.
- Songs, the song's show: `sectionsOf(song)` (sections with ids), `sectionLooksOf(song, i)`, `setSectionLooks(song, i, ids)` (the show), `fillFromStyle(song)`, `analysisOf(song)`, `markReviewed(song)`. Section edits: `moveSectionEdge(song, i, delta)`, `splitSection(song, i)`, `mergeSections(song, i)`, `renameSection(song, i, name)`. Takes (recording passes): `takesOf(song)` (each `{ id, name, muted, recording, started?, events }`), `setTakeMuted(song, takeId | "show", muted)`, `mergeTake(song, takeId)` (into the show), `deleteTake(song, takeId)`.
- Recording: while `isRecording()` and a song plays, `start`, `stop`, `release` (a flash with its length), `setFader` and `setMaster` write events into a new take of that song ("Recording", `recording: true`, drawn red; afterwards "Recorded today 21:58").
- Patch: `store.rig` is reactive. `setAddress(id, a)`, `addFixture(f)`, `reorderFixture(id, delta)` (the order inside its group), `setIdentify(id | null)` (the fixture flashes), `conflicts()`, `inConflict(id)`, `freeAddress(channels, universe?, except?)`. Universes: `universes()` (`universe, fixtures, used, panel, conflict`), `firstFreeUniverse()`. Pixel panels: `state.panels`, `addPanel({ name, cols, rows, wiring, startUniverse })`, `panelRuns(panel)` and `allPanelRuns()` (170 pixels per universe). Sample data: 15 fixtures on universe 1, a 24 x 16 panel on universes 2 to 4.
- DMX: `attributes(f)` (channel names), `channelValues(universe, phase?)` (index 1..512; a phase shift shows what changes with the beat), `state.held`, `holdChannel(a, v)`, `releaseChannel(a)`, `releaseAllChannels()`.
- In/Out: `outputs()` (the USB adapter drops out and reconnects every 45 s, on load too; Art-Net has an error), `state.clockFeed`, `setClockFeed(feed)`, `setTempo(bpm)`. Mappings made with Learn: `state.mappings` (`control, screen, device, message, feedback`), `addMapping`, `updateMapping(id, patch)`, `removeMapping(id)`.
- How it works: `state.replay` and `setReplay(ids)`: looks the show replays from a recording.
- Review questions: `src/review/questions.ts`, answers via `review.answerOf(id)`, `setChoice(id, "A" | "B" | null)`, `setAnswerText(id, text)`. They go into Export and Copy as Markdown.
- Looks library (`src/fake/library.ts`, model in `docs/design/looks.md`): `state.library` holds steps, sequences and patterns by id; every older look is in it too. `item(id)`, `items()`, `colorsOf(id)`, `fixturesOf(id)`, `measureOf(id)` (beats per loop, null for a step), `usedIn(id)`, `allGroups()`, `setStep(id, prop, value | undefined)`, `setEntry(seqId, entryId, patch)`, `setPattern(id, patch)`, `addToGroup(id, g)`, `removeFromGroup(id, g)`, `putOnConsole(id)`. Any library id can be passed to `start`, `stop`, `hold` and `release`: it plays like a look.
- Console pads: `state.pads` (by page). The last row of Main plays the new library items.
- Undo for your own screen state: `store.record(label, undoFn, key?)`. Calls with the same key within about a second merge into one step (one fader drag, one undo).
- The LIVE switch: lights follow the app whether LIVE is on or off, there is no preview. LIVE on means the night is running: `followsDj()`, `isRecording()`, `autopilotArmed()` and `surfaceLocked()` are true unless that indicator was turned off. `inEffect(i)` is the general form. With `lightsConnected` false the shell shows "No lights connected".
- Derived: `lookState(id)`, `fixtureState(f)` (colour from the colour winner, level from the brightness winner), `runningManual()` (pads that are on), `lanes(from, to)`, `currentSection()`, `nextSection()`, `currentSong()`, `currentSongSections()`, `autopilotLooks(section)`, `autopilotNow()`, `pickReason(section)`, `covered(section)` (the show owns it, the autopilot leaves it), `showLooksNow()`, `decks()`, `sectionsBetween(from, to)`, `isDark()`.
- `store.state.events` logs every press by hand with its time.
- Screen-only fake data (DMX values, device states, fixture library) goes in your screen folder. Derive from the store where the brief says the screens share it (a pad pressed on the Console shows in DMX and in the song's show on Songs).

## Tokens (`src/styles/tokens.css`)

- Ground `--bg`, panels `--s1`, `--s2`, rules `--line`, text `--paper`, secondary text `--mute`, text on colour `--ink`.
- Row colours: `--layer-manual` (pink), `--layer-show` (cyan), `--layer-autopilot` (marigold). The token and class names kept the word "layer"; nothing on screen says it. Add class `layer-manual` (or `-show`, `-autopilot`) to an element and use `var(--layer)` inside it.
- `--danger` red: only blackout, errors and recording while it is on.
- Type: `--font-display` (Big Shoulders Display, class `display`) for headings, lane labels, block titles, big numerals. `--font-ui` (Archivo) for everything else. Sizes `--text-sm` (13 px, the floor) to `--text-lg`, `--display-xs` to `--display-2xl`. `font-stretch: var(--tight)` narrows Archivo for labels in tight places. Numbers are tabular everywhere.
- Size order: the tempo (`--display-2xl`) is the largest text, the song that plays (`--display-xl`) comes second, nothing else goes above `--display-lg`. Region headings are `--display-sm`.
- Space `--sp-1` to `--sp-5`, `--gutter` for the left and right page edge (use it for your screen's side padding), `--target` 44 px, `--rule` 2 px.
- Paper is the colour of "now" and of the primary action. Marigold, cyan and pink belong to autopilot, show and manual only.
- The full system is in `DESIGN.md` at the repo root.
- `.mute` for secondary text, `.now-flag` for the NOW flag.

## Rules

- Plain words in labels: looks, pads. No "layer", "HTP", "LTP", "function", "cue stack" on screen.
- Names: the home screen is the Console, the top row (your hands) is Manual. The word "Live" appears only on the LIVE switch and the "Live edit" badge on Songs. Decks are 1 and 2; their states are Playing, Cued, Stopped. Never "on air".
- Sentence case everywhere. Capitals only for LIVE and NOW. Do not set `text-transform: uppercase`.
- Anything started by hand, by the show or by the autopilot carries the colour of its row, and only that colour. Blocks are flat fills with ink text. No gradients, glow or shadows. Square corners.
- A song is its show. There is no Show screen: Songs holds the show, its takes, the Auto lane and the section edits of the selected song. The autopilot has no screen either: it is the Auto lane on Songs, with a compact header (on/off, style) in the Console timetable.
- No rig view and no map. Patch shows universes and the universe grid.
- The decks are always visible: on the Console through `Decks`, on Songs through the "Follows" switch and the Playing badge, everywhere else through the Now strip.
- Wherever the screen shows time, show the NOW line with its flag (`Timetable` or `EnergyCurve` do it). Things that run carry the flag.
- Anything pressed during a show is at least 44 px. Every control works by keyboard and shows a focus ring. No hover-only actions, no modals.
- Nothing read during a show is smaller than 13 px. A disabled control says why (`Button reason`).
- Number keys 1 to 6, Space and Cmd/Ctrl+Z are taken by the shell. Space is blackout while held, everywhere except text fields.
- No new dependencies. No engine code. No copied text from the design direction.
- Before you hand over: `npm run build` passes, and you have looked at the screen at 1280 x 800 and 1024 x 768.
