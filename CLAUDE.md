# CLAUDE.md

Bossylights is an open-source lighting desk: easy first, deep on request.
Rust engine, web UI in a Tauri shell, Apache-2.0. Status: planning, no code yet.

## Plan

- Map of all work: the GitHub issue labelled `map`, and `docs/plan/map/index.html`.
- Requirements: `docs/plan/requirements.md`. Research: `docs/plan/capability-map.md`.
- A map item is not a ticket. It becomes buildable in two steps: a spec on the item, then tickets labelled `ready-for-agent` that name it as their parent.

## Rules

- One engine, one API, no UI inside the engine.
- The output thread never blocks, locks or allocates.
- Every feature has an end-to-end test that checks DMX output.
- Apache-2.0 only: no GPL or AGPL code or dependencies. Ideas from BlinderKitten, Chataigne and Mizer are fine, their code is not.
- Plain words in the default UI layer: looks, layers, pads.

## How to write: visual first, few words

The owner has ADHD and is a visual learner. This applies to every output: docs, issues, pull requests, reviews, reports and chat answers.

- A picture or a table before prose. One idea per picture.
- Mermaid for structure and flow (flowchart, sequence, state). Use `<br/>` for line breaks and no light fill colours, so it works in dark mode.
- At most one or two short lines per idea. Cut every word that carries no information.
- Questions and choices as a table with the options side by side, the recommended one marked.
- No walls of bullets. If a list has more than five items, it wants to be a table or a diagram.
- No em dashes.

## Workflow

- Trunk-based: commit and push directly to `main`. No long-lived branches. This overrides any general "always work on branches" rule.
- Never force-push or rewrite history on `main`.
- Keep `main` green: run the tests before every push.

## Agent skills

### Issue tracker

Issues and specs live in GitHub Issues of abossard/bossylights. See `docs/agents/issue-tracker.md`.

### Triage labels

The default label vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context. See `docs/agents/domain.md`.

### External peers

`auto`. See `docs/agents/cross-model.json`.
