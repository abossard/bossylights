# Required features

Target: parity with QLC+ v5 and with what the abossard/qlcplus fork added, reshaped for people who want a result fast.
IDs match the boxes in the plan map (`docs/plan/map/index.html`) and the "Map id" of each GitHub issue. Priorities live on the issues.

Origin: **Q** = QLC+ v5 parity, **F** = built in the fork, **N** = new.

## Who and what for

- Audience: people with ADHD who want to do cool stuff. Easy and obvious first, deep on request.
- Use: live busking, recording what was busked, timecoded shows, autopilot, and any mix of them.
- Fully open source. macOS required, Windows and Linux desirable.
- Reference rig:
  - Novation Launchpad Mini, Launchpad Pro or Akai APC (to confirm)
  - Enttec-style USB DMX adapter (model to confirm)
  - Show laser from Kickstarter with ILDA port
  - Hercules DJ controller, VirtualDJ, Magic Music Visuals

## Product requirements

| ID | Requirement | Test |
|---|---|---|
| r-first60 | A press on a pad changes the real lights at once | The app opens on the example setup with nothing playing; a pad press shows on the DMX screen in the next frame and, with an output connected, on the lights. No timed first run (owner decision D11, `docs/design/screens-slice-1.md`) |
| r-visible | State is always visible | LIVE on or off, tempo, running items and master are on screen in every view |
| r-layers | Simple by default, expert on request | Default layer shows a handful of controls. Channels and priorities are one level down |
| r-words | Plain words | No "HTP", "LTP" or "function" in the default layer |
| r-nomemory | Never remember a number | No task needs a value read on another screen |
| r-rt | Output never glitches | DMX frame timing stays in spec under UI load, App Nap, sleep and wake. A frozen UI does not stop output |
| r-headless | One engine, one API, no UI inside | Every UI action is possible through the API with no window open |
| r-tests | Every feature has an end-to-end test | Feature driven through API or UI with DMX output checked |
| r-license | Fully open source | One license file, all dependencies compatible |
| r-crossplat | macOS first, others possible | Engine builds and passes tests on Linux and Windows |
| r-ci | Signed Mac build on every commit | Download opens without a security warning |
| r-laser-safe | Laser safety defaults | Disarmed at start, blanks on underrun and disconnect, zones, brightness cap, e-stop |

## Foundation

- **f-engine (Q)** DMX engine
  - Universes of 512 channels, any number, with passthrough.
  - Highest-wins for intensity, latest-wins for the rest, per channel type.
  - Grand master, sub masters, blackout.
  - Fades with fade in, hold, fade out. Blend modes normal, mask, additive, subtractive.
  - Channel modifiers (QLC+ ships 17 curve templates).
  - Engine tick at 50 Hz like QLC+, output at up to 44 Hz.
- **f-project (N)** Project file with autosave and resume. Readable, versioned, friendly to git.
- **f-undo (Q)** Undo for everything. QLC+ v5 has it in parts (Tardis), with gaps in the Virtual Console editor.
- **f-qxw (N)** Import QLC+ workspaces: patch, functions, Virtual Console. Later (map #88); the first show is hand-written in the Bossylights format.

## Fixtures and patch

- **f-fixlib (Q)** Fixture library
  - Reads QLC+ `.qxf`: 1,782 fixtures from 143 manufacturers. Channels with groups and presets, capability ranges, aliases, modes, heads, physical data.
  - Reads Open Fixture Library and GDTF.
  - Colour filters and gobo pictures.
- **f-patch (Q)** Patch: fixture, mode, universe, address. Multi-head and pixel fixtures.
- **f-autopatch (N)** Guided patch: free address proposal, DIP switch or display value, flash to confirm. QLC+ has a Stage Wizard as a starting point.
- **f-groups (Q)** Groups and the rig map. Fixtures and heads on a grid, used by effects and selection.
- **f-remap (Q)** Move a show to another rig.
- **f-fixeditor (Q)** Fixture editor.
- **f-rdm (N)** RDM discovery. QLC+ has the plumbing in Art-Net and DMX USB but no v5 screen.

## Outputs and inputs

- **f-usbdmx (Q)** USB DMX: Enttec Open, Pro and Pro Mk2, DMXKing UltraPro, Eurolite Pro, DMX4ALL, Vince, NanoDMX, generic FTDI. No driver install.
- **f-artnet (Q)** Art-Net and sACN, in and out.
- **f-ddp (F)** DDP pixel output with offsets.
- QLC+ plugins not planned for the start: Enttec Wing, Peperoni, uDMX, Velleman, SPI, GPIO, UART, OLA, Loopback.

## Looks and effects

- **f-programmer (N)** Select on the rig map, change color, position or beam, see it at once.
- **f-palettes (Q)** Palettes: dimmer, color, pan, tilt, position, shutter, gobo, zoom. Linked, not copied.
- **f-looks (Q)** Looks (QLC+ scenes) with fade times.
- **f-fx (Q)** Effects across a group
  - Movement shapes: circle, eight, line, diamond, square, leaf, lissajous.
  - Dimmer and color waves with phase spread.
  - Speed in beats or seconds. Run order loop, single shot, ping pong, random. Forward and backward.
- **f-matrix (Q)** Pixel matrix effects. QLC+ ships 61 scripts.
- **f-huematrix (F)** Audio-reactive color scripts: the 72 HUE Matrix scripts ported from LedFx.
- **f-genlooks (F)** Looks generated per fixture type. Grows out of the fixture page engine.
- **f-tracking (N)** Tracking cue lists with per-fixture timing.

## Playback and busking

- **f-tempo (Q, F)** One tempo: tap, audio beat, MIDI clock, OS2L, Art-Net, sACN, OSC beat hooks, DJ software.
- **f-chaser (Q)** Chasers and sequences. Step timing default, common or per step.
- **f-cuelist (Q)** Cue lists: go, back, crossfade.
- **f-collection (Q)** Collections.
- **f-playbacks (F)** Layers: base, override, flash. From the LTP mix layers built for DJ sets.
- **f-quantize (N)** Launch on the beat or bar.
- **f-speed (Q)** Live speed and fade control with presets and multipliers.

## Control surface

- **f-vc (Q)** Build your own surface: pages, frames, solo frames, collapse, enable.
- **f-vc-widgets (Q)** Widgets
  - Button: toggle, flash, blackout, stop all, plus freeze and freeze-hold from the fork.
  - Slider: level, adjust, submaster, grand master, click-and-go colors and presets.
  - XY pad with presets.
  - Cue list, speed dial, matrix control, audio triggers by spectrum band, clock, label.
- **f-vc-edit (F)** Comfortable editing: outline, grid layout, arrange, snap, multi-select, smooth trackpad.
- **f-autosurface (F)** Surface generated from the rig. The page planner.
- **f-kiosk (Q)** Show mode that locks the surface.
- **f-flow (F)** Flow Console.
- **f-midi (Q)** MIDI in with learn, auto-connect.
- **f-feedback (Q)** Pads mirror the screen: color, flash, pulse, beat LED.
- **f-ctrl-pages (Q, F)** Controller pages: a controller switches between pages, each with its own mapping and its own pad colours (owner, 2026-11-19).
  - Every pad shows its state in colour: off, on, running-but-not-mine, flashing, pulsing, brightness. Colours come from the controller's profile (palette, modes per MIDI channel), any pad can override them.
  - A page switch redraws every pad at once: pads with nothing on the new page go dark. No stale LED from the old page.
  - A page switch never leaves a light stuck: a held flash releases when its pad leaves.
  - Global pads (page buttons, blackout, tap) stay on every page: a page can inherit pads it does not map (the QLC+ fork's Inherit mode).
  - Page next, previous and direct buttons come from the profile and work with no setup; the current page is lit.
  - Pages have names and an order, saved in the show. Several keys may select one page.
  - A colour picked on screen becomes the nearest colour the pad can show.
  - Learnt from the QLC+ fork: one feedback path for every widget, profile colour tables, nearest-colour mapping, one-step Launchpad setup. Avoid its gaps: no redraw on a top-level page switch, stale LEDs, stuck flashes, one key per page, OMNI breaking feedback above page 0.
- **f-profiles (Q)** Controller profiles. QLC+ ships 41, including Launchpad, Launchpad MK2, Mini MK3, Pro, APC20, APC40, APC mini and mk2.
- **f-osc (Q)** OSC in and out with feedback, published over OSCQuery.
- **f-web (Q)** Tablet and phone remote.
- **f-hid (Q)** Keyboard, HID, gamepads.

## Music and autopilot

- **f-audioin (Q, F)** Audio in: BPM, beat phase, band levels. Audio profiles from the fork. Levels over OSC from Synesthesia or Magic.
- **f-analysis (F)** Song analysis: beat grid, the five section labels, drops, energy, confidence. Must not exhaust memory (the EDMFormer run crashed the Mac).
- **f-vdj (F)** VirtualDJ link: OS2L with Bonjour, VDJ Bridge telemetry, OSC. Looks follow the play position literally.
- **f-timeline (Q)** Show timeline with waveform, tracks, audio.
- **f-record (F)** Record what you busk into command tracks.
- **f-timecode (N)** Follow MIDI timecode, LTC, Art-Net timecode, DJ deck position.
- **f-songdir (F)** Song Director: one block per analysed section.
- **f-djset (F)** DJ set shows: a recorded set with one nested show per song.
- **f-djperform (F)** DJ Perform: one show per song, decks mixed by weight.
- **f-autopilot (F, N)** Autopilot that yields when you grab a control.
- **f-neverdark (N)** Beat-synced fallback when nothing is programmed.
- **f-link (N)** Ableton Link.
- **f-av (Q)** Audio and video playback.

## See it

- **f-2d (Q)** 2D rig view with live output.
- **f-blind (N)** Preview-only mode.
- **f-monitor (Q)** DMX monitor and channel tester (Simple Desk).
- **f-3d (Q)** 3D preview with beams.

## First five minutes

- **f-demo (N)** Opens on an example setup of the owner's rig: real fixtures, a real show file in the Bossylights format, real looks, nothing playing. Nothing is a demo or a mock.
- **f-golive (N)** Connect your lights, then LIVE. Lights follow the app at once. The LIVE switch starts the night: DJ link, recording, autopilot, lock. There is no preview mode.
- **f-panic (N)** Blackout, panic and "surprise me".

Order of the first screens (`docs/design/screens-slice-1.md`): shell with DMX, then USB and DDP output setup saved with the show, then offline Patch, Looks and the Console, then Art-Net, sACN, controllers and richer editors. There is no timed script: every reachable control works end to end.

## Laser

- **t-laser-id** Identify the laser and its inputs.
- **f-laser-dmx (N)** Laser as a DMX fixture.
- **f-laser-dac (N)** DAC output: Helios, Ether Dream, IDN, LaserCube.
- **f-laser-gen (N)** Shapes and ILD frames as looks, one zone with keystone per projector.

## AI and automation

- **f-mcp (F)** MCP server. The fork registers 105 tools and 3 prompts: query and patch, functions, shows, I/O, channels, stage, live control, Virtual Console, workspace, palettes, input profiles, timing diagnostics.
- **f-cli (F)** Checks and repairs as tested commands.
- **f-script (Q)** Scripting and plugins.

## Not carried over

- The v4 Qt Widgets UI.
- Tardis network sync.
- 10 UI translations (English first).
- Theme presets.

## Open questions

- Which laser model, and does it have a DMX mode?
- Is the USB adapter an Enttec Open DMX or a DMX USB Pro? It changes the timing risk.
- Which Hercules controller, and should its jogs and faders drive lights or only VirtualDJ?
- Does Magic Music Visuals stay the visual engine, with the new app only sending it OSC?
