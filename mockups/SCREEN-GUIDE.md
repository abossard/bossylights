# Screen guide

For anyone adding or replacing one of the seven screens (Console, Show, Songs, Looks, Patch, DMX, In/Out). The screen briefs in `docs/design/screens.md` are the source of truth for what a screen shows.

## File convention

- A screen is the folder `src/screens/<id>/`. Its entry is `index.tsx`. Everything else it needs (CSS, sub-components, screen-only fake data) lives in the same folder.
- `index.tsx` exports:

| Export | Type | Meaning |
|---|---|---|
| `id` | `string` | Same as the folder name. URL `#/<id>`, review key. |
| `title` | `string` | Tab title, plain words. |
| `order` | `number` | Tab position 1 to 7, also its number key. |
| `issues` | `{ number, title }[]` | GitHub issues the screen stands for. |
| `Screen` | `Component` | The screen. |
| `nowStrip` | `boolean`, optional | `false` on screens that show the full timetable (Console, Show). |

- The shell finds screens with `import.meta.glob`. Adding or replacing a screen means creating or overwriting files in its folder. Never edit `src/shell`, `src/kit`, `src/fake`, `src/review` or another screen's folder for your screen.
- Import CSS from `index.tsx` (`import "./<id>.css"`) and prefix every class with the screen id (`.patch-...`) so screens never restyle each other.
- The screen fills `.screen-host`, which scrolls vertically. Fit 1280 x 800 without scrolling where the brief allows; at 1024 x 768 vertical scroll is fine, horizontal scroll never.
- The stubs use `src/shell/Stub.tsx`. A built screen stops using it.
- The LIVE switch lives in `src/shell/LiveSwitch.tsx` and `live-switch.css` on its own, so it can be replaced in one place.

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
| `Pad` | `label, colors, state: idle, autopilot, show, held, flash, onDown, onUp(heldMs), disabled?`. A running pad fills with its layer's colour and carries the NOW flag. |
| `XYPad` | `x, y (0..100, y up), onChange(x, y), label, layer?` |
| `RigView` | `fixtures, lookOf(f) => { color, level }, zones?: { label, layer?, fixtureIds }[]` (no layer: neutral group outline), `onFixture?, onMove?(f, x, y)` (drag and arrow keys), `selected?` |
| `Timetable` | `from, to, now, lanes, tickEvery, format, laneHeight?, labelWidth?, laneControls?(lane), plain?` |
| `Decks` | `decks, compact?`: the two VirtualDJ decks, on-air title largest. A deck with `song: null` shows "Nothing loaded". |
| `NowStrip` | `from, to, now, lanes, decks, onActivate` (the shell already shows it) |
| `List` | `items: { id, title, sub?, meta?, swatch?, layer?, actions? }[], onSelect?, selected?, empty?, label?` |
| `Table` | `columns: { key, title, align?, width?, render? }[], rows, rowKey, onRowClick?, selected?, dense?, caption?, empty?` |
| `Meter` | `value 0..1, label, layer?, tone?: paper, danger, orientation?` |
| `EnergyCurve` | `spans: { start, end, energy, title }[], from, to, now, layer?, label` |
| `stepWindow(now, lead, span, step)` | A time window that holds still so the NOW line walks across it. |

Missing a control? Build it in your screen folder from these pieces. Do not add it to the kit.

## Fake store (`import { store, hhmm, mmss, groupNames } from "../../fake/store"`)

- State, read-only: `store.state` with `showName, tempo, master, night (the LIVE switch and its indicators), lightsConnected, blackout, faders, xy, manual (blocks), clips (Show timeline clips), autopilot, events`.
- Clock: `store.now()` (show clock, seconds since midnight, starts 22:47:12), `store.beat()` 0..3, `store.beatPhase()` 0..1.
- Set: song 1 recorded, song 2 playing on deck A with its own show for the first drop only, song 3 auto (cued on deck B), song 4 not analysed. `LIBRARY_ONLY` (from `fake/data`) has one more song, "Needs review" with low confidence, for the Songs screen.
- Data: `store.rig` (15 fixtures with group, position, model, mode, universe, address, channels; Laser overlaps Bar R on purpose, the one address conflict), `store.looks`, `store.songs` (four sample songs with sections, energy, analysis state), `store.groups`.
- Commands: `start(lookId)`, `stop(lookId)`, `hold(lookId)` and `release(lookId)` for a flash, `releaseAll()` (the Manual layer), `surprise()`, `setMaster(v)`, `setBlackout(on)`, `setLive(on)`, `toggleIndicator("dj" | "record" | "autopilot" | "lock")`, `setFader(id, v)`, `setXY(pan, tilt)`, `setAutopilot(key, value)`, `toggleGroup(g)`, `skipLook(id)`, `moreLikeThis(id)`, `skipPick(section)`, `morePick(section)`, `keepPick(section)` (an autopilot pick becomes show clips), `moveClip(id, start, end)`, `undo()`.
- Lanes from `lanes(from, to)` carry `chip` (one state word: holding, playing, idle, waiting, off), `on` (the chip is filled) and `status` (one short line).
- Edge states to design for: `store.state.lightsConnected` false, a deck with nothing loaded (`setLightsConnected`, `setCuedDeckLoaded`; the review panel has switches for both), LIVE off.
- Songs: `sectionLooksOf(song, i)`, `setSectionLooks(song, i, ids)`, `fillFromStyle(song)`, `analysisOf(song)`, `markReviewed(song)`. The autopilot plays what a section holds.
- Patch: `store.rig` is reactive. `moveFixture(id, x, y)`, `setAddress(id, a)`, `addFixture(f)`, `setIdentify(id | null)` (the fixture flashes), `conflicts()`, `inConflict(id)`, `freeAddress(channels, universe?, except?)`.
- DMX: `attributes(f)` (channel names), `channelValues(universe, phase?)` (index 1..512; a phase shift shows what changes with the beat), `state.held`, `holdChannel(a, v)`, `releaseChannel(a)`, `releaseAllChannels()`.
- In/Out: `outputs()` (the USB adapter drops out and reconnects every 45 s, on load too; Art-Net has an error), `state.clockFeed`, `setClockFeed(feed)`, `setTempo(bpm)`.
- Undo for your own screen state: `store.record(label, undoFn, key?)`. Calls with the same key within about a second merge into one step (one fader drag, one undo).
- The LIVE switch: lights follow the app whether LIVE is on or off, there is no preview. LIVE on means the night is running: `followsDj()`, `isRecording()`, `autopilotArmed()` and `surfaceLocked()` are true unless that indicator was turned off. `inEffect(i)` is the general form. With `lightsConnected` false the shell shows "No lights connected".
- Derived: `lookState(id)`, `fixtureState(f)`, `runningManual()`, `overrides()`, `handsOff()`, `lanes(from, to)`, `currentSection()`, `nextSection()`, `currentSong()`, `currentSongSections()`, `autopilotLooks(section)`, `autopilotNow()`, `pickReason(section)`, `covered(section)` (the show owns it, the autopilot leaves it), `activeClips()`, `decks()`, `setList()`, `sectionsBetween(from, to)`, `isDark()`.
- `store.state.events` logs every press on the Manual layer with its time. Show shows them on the Recorded track while `isRecording()`.
- Screen-only fake data (DMX values, device states, fixture library) goes in your screen folder. Derive from the store where the brief says the screens share it (a pad pressed on the Console shows in DMX and Show).

## Tokens (`src/styles/tokens.css`)

- Ground `--bg`, panels `--s1`, `--s2`, rules `--line`, text `--paper`, secondary text `--mute`, text on colour `--ink`.
- Layers: `--layer-manual` (pink), `--layer-show` (cyan), `--layer-autopilot` (marigold). Add class `layer-manual` (or `-show`, `-autopilot`) to an element and use `var(--layer)` inside it.
- `--danger` red: only blackout and errors.
- Type: `--font-display` (Big Shoulders Display, class `display`) for headings, lane labels, block titles, big numerals. `--font-ui` (Archivo) for everything else. Sizes `--text-sm` (13 px, the floor) to `--text-lg`, `--display-xs` to `--display-2xl`. `font-stretch: var(--tight)` narrows Archivo for labels in tight places. Numbers are tabular everywhere.
- Size order: the tempo (`--display-2xl`) is the largest text, the song on air (`--display-xl`) comes second, nothing else goes above `--display-lg`. Region headings are `--display-sm`.
- Space `--sp-1` to `--sp-5`, `--gutter` for the left and right page edge (use it for your screen's side padding), `--target` 44 px, `--rule` 2 px.
- Paper is the colour of "now" and of the primary action. Marigold, cyan and pink belong to their layers only.
- The full system is in `DESIGN.md` at the repo root.
- `.mute` for secondary text, `.now-flag` for the NOW flag.

## Rules

- Plain words in labels: looks, layers, pads. No "HTP", "LTP", "function", "cue stack" in the default layer.
- Names: the home screen is the Console, the top layer (your hands) is Manual. The word "Live" appears only on the LIVE switch.
- Anything that belongs to a layer carries that layer's colour, and only that colour. Blocks are flat fills with ink text. No gradients, glow or shadows. Square corners.
- The autopilot has no screen: it is the Auto lane of the Show screen, with a compact header (on/off, style) in the Console timetable.
- The decks are always visible: on Console and Show through `Decks`, everywhere else through the Now strip.
- Wherever the screen shows time, show the NOW line with its flag (`Timetable` or `EnergyCurve` do it). Things that run carry the flag.
- Anything pressed during a show is at least 44 px. Every control works by keyboard and shows a focus ring. No hover-only actions, no modals.
- Nothing read during a show is smaller than 13 px. A disabled control says why (`Button reason`).
- Number keys 1 to 7, Space and Cmd/Ctrl+Z are taken by the shell. Space is blackout while held, everywhere except text fields.
- No new dependencies. No engine code. No copied text from the design direction.
- Before you hand over: `npm run build` passes, and you have looked at the screen at 1280 x 800 and 1024 x 768.
