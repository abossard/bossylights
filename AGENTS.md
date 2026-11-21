# AGENTS.md

Bossylights is an open-source lighting desk: easy first, deep on request.
Rust engine, web UI in a Tauri shell, Apache-2.0. Status: skeleton (engine counter in a window).

## Commands

| What | Command |
|---|---|
| Install | `npm install` |
| Run (desktop app, dev) | `npm run dev` |
| Test and check everything (lints, layers, generated types, tests) | `npm test` |
| Scorecard / mutation score | `npm run scorecard` / `npm run mutants` (needs `cargo-mutants`) |
| Build | `npm run build` |
| Engine alone / UI alone | `npm run engine` / `npm run ui` (browser at http://127.0.0.1:5174) |

| Folder | What |
|---|---|
| `crates/engine-core` | the real-time core: `update`, state, commands. No `std`, no `alloc` |
| `crates/engine` | the `Engine` and its tick thread around the core |
| `crates/rt-thread` | starts the tick thread with real-time priority. The only `unsafe` in the product. Four tests carry a counting allocator, which needs `unsafe` too |
| `crates/outputs` | every output kind (DDP, DMX USB Pro) and its bytes. Opened off the tick thread, `take` a frame without waiting |
| `crates/fixture-library` | fixture definitions as a model: QLC+ `.qxf` bytes in, maker, model, modes, channels out. Reads no files. `cargo run -p fixture-library --example read_folder -- <folder>` reads a folder and prints how many were read, why any failed, and the tally of the model (functions, "other", fine bytes, defaults, what ranges do, shutters, pixels) |
| `crates/rig` | the Rig: a mode at an address becomes lowered channels (rest, dark, pairs) and per-light fields that turn a light value into offers. Off the tick thread. `cargo run -q --release -p rig --example resolve_lateness` measures a frame of 10 000 lights |
| `crates/engine-server` | `bossylights-engine`, the headless process serving the API |
| `ui/` | the product web UI (SolidJS). `mockups/` is design reference only |
| `shell/` | Tauri window: starts the engine, hosts `ui/` |
| `e2e/` | Playwright tests |

## Plan

- Map of all work: the GitHub issue labelled `map`, and `docs/plan/map/index.html`.
- Requirements: `docs/plan/requirements.md`. Research: `docs/plan/capability-map.md`.
- A map item is not a ticket. It becomes buildable in two steps: a spec on the item, then tickets labelled `ready-for-agent` that name it as their parent.
- Gate for the real UI: no product UI ticket starts before its controls are approved in the storyboard catalog (issue #102): every element, in every situation.

## Rules

- How we write code: `docs/coding.md`. Read it before writing or reviewing code: data, calculations and actions apart; state flows one way (view, command, update, state); state machines written down; deep modules; errors as states.
- Allowed edges between crates and ui layers: `scripts/layers.mjs`. A new crate or ui file goes there first.
- One engine, one API, no UI inside the engine.
- The output thread never blocks, locks or allocates. It runs with the real-time policy of the system.
- The engine is its own process and outlives the window. A UI crash or a hidden window never touches the lights.
- A press is in the very next frame. Each output has its own rate (`docs/design/theory.md`, idea 1).
- First engine slice: `docs/design/engine-slice-1.md`.
- Every feature has an end-to-end test that checks DMX output.
- Apache-2.0 only: no GPL or AGPL code or dependencies. Ideas from BlinderKitten, Chataigne and Mizer are fine, their code is not.
- Plain words in the default view: looks, pads. Desk words: Playback, Patch, Show file, Looks, Tempo. A song has one show.
- All who-wins rules live in one place (one table in the Mixer, one panel in the app). No other code decides between two values for the same light. See `docs/design/who-wins.md`.
- The engine is a channel manager. It knows channels, their metadata and types, and compound types in the API (X/Y, RGB, CMYK, RGBW, …), never a "light". 1 byte of value per channel; coarse and fine are two channels that move as one 16-bit value. Fixtures are data: no fixture shape is assumed anywhere. Owner decisions: #239.
- Outputs are protocol adapters over one channel space: DMX USB, Art-Net and sACN as 512-slot universes, DDP as one stream to an IP address.
- Every control comes from fixture metadata: a colour picker where channels form a colour, an X/Y pad for pan and tilt, named ranges as buttons, faders otherwise; never a raw number only.
- Nothing mocked, demo or half-working is reachable in the app. It grows by complete pieces, each working end to end on the real engine.
- No Test output that drives every channel: Identify only. Haze, strobe, sound and macro channels never move unless a look or a hand sets them.
- Machine load is normal: never hold, delay or stop work because of it.

## How to write: visual first, few words

The owner has ADHD and is a visual learner. This applies to every output: docs, issues, pull requests, reviews, reports and chat answers.

- A picture or a table before prose. One idea per picture.
- Mermaid for structure and flow (flowchart, sequence, state). Use `<br/>` for line breaks and no light fill colours, so it works in dark mode.
- At most one or two short lines per idea. Cut every word that carries no information.
- Questions and choices as a table with the options side by side, the recommended one marked.
- Animated and interactive is welcome where it explains better than a still picture: a small live demo beats a paragraph.
- No walls of bullets. If a list has more than five items, it wants to be a table or a diagram.
- No em dashes.

## Workflow

- Trunk-based: commit and push directly to `main`. No long-lived branches. This overrides any general "always work on branches" rule.
- Never force-push or rewrite history on `main`.
- Keep `main` green: run the tests before every push.
- All work is on `main` in the one checkout, no worktrees (owner). Work apart by modules: a brief names the files a worker owns. Stage only your own paths; `git log origin/main..` shows only your commits before a push.
- Verify your change in a clean copy first (`npm run verify -- <commit>`), then commit and push in one step. A commit left unpushed on the shared `main` blocks every other worker's push.
- A worker's slice ends with: a hand-over comment on the issue, `SendMessage` to the orchestrator (a chat answer reaches nobody), `clear_session "self"`.
- No invalid project state: a command that would make one is refused, and so is a later command that would break it. Never allow and flag.

## Agent skills

### How the work is run

Roles, models, the queue loop, milestones and gates. See `docs/agents/orchestration.md`.
Peer agents (Codex CLI, Copilot CLI) for builds, reviews and research: `npm run peer`, see `docs/agents/peers.md`.

### Issue tracker

Issues and specs live in GitHub Issues of abossard/bossylights. See `docs/agents/issue-tracker.md`.

### Triage labels

The default label vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context. See `docs/agents/domain.md`.

### External peers

`auto`. See `docs/agents/cross-model.json`.
