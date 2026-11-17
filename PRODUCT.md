# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

SolidJS + Vite + TypeScript for the mockups (chosen by the owner, 2026-11-17). The app runs in a Tauri shell later. The framework for the real UI is confirmed by the UI spike (issue #24).

## Users

People who want to do cool things with lights and want a result fast, many of them with ADHD. They run house parties, small clubs and DJ sets. They are not lighting professionals, but the same app must let an experienced operator run a real show.

## Product Purpose

An open-source lighting desk that is easy first and deep on request. Version 1 succeeds when it carries one full night on a real rig: live busking over an autopilot, recorded, with no glitch.

## Positioning

The show follows the music by itself, and a hand on any control takes over. Three ways to run a show combine in layers: the autopilot, the show runner (a timeline), and the console, played by hand. Hands always win over what runs underneath.

## Operating Context

- A laptop in a dark room next to DJ decks, and a touch screen used as the live console.
- Pad controllers (Launchpad, APC) that mirror the screen.
- VirtualDJ for the music, a USB DMX adapter and network DMX for the lights, a show laser.
- Shows are prepared at home and run live. The same person often plays the music and runs the lights.

## Capabilities and Constraints

- Screens: Console (the customizable console, home), Songs (every song has one show: sections, layers, recording, with the autopilot as its Auto lane), Looks (steps, sequences, patterns), Patch (universe grid, no map), DMX, In/Out.
- Every button and value in the app can be mapped to a controller with one global Learn.
- Layers: Autopilot, Show and Manual to start with. The user can add layers.
- The two VirtualDJ decks (1 and 2) and the title of the song that plays are always visible.
- The live console is designed by the user: widgets on pages and frames.
- Words in the default layer are plain: looks, layers, pads. No "HTP", "LTP" or "function" there.
- No screen asks the user to remember a value from another screen.
- State is always visible: LIVE on or off, tempo, what runs, master level.
- Lights follow the app whenever it runs. There is no preview mode. One LIVE switch means the night is running: it follows the DJ software, records the set, arms the autopilot and locks the surface.
- Names: the home screen is the Console. The three layers are Autopilot, Show and Manual (your hands). "Live" means only the switch.
- First light changes within 60 seconds of launch.
- Undecided: the engine API, the project file format, how the autopilot picks looks.

## Product Principles

1. A result before any setup.
2. Simple by default, expert on request.
3. Hands win: a hand on a control overrides whatever runs.
4. Nothing to hold in your head.
5. Every action can be undone.

## Accessibility & Inclusion

Designed for people with ADHD: immediate feedback, low working-memory load, no modal dead ends, no timeouts, no interruptions during live output. Follows the W3C cognitive accessibility guidance (COGA). Readable in a dark room and usable by touch.
