# Spec #<issue>: <title>

Run: `npm run peer -- codex spec <current head> <this file> --name spec-<issue>`. The default is Codex astra at ultra; `--model sol` or `--model luna --effort <level>` for a lighter one.

You write the spec for one issue as ONE markdown file. The prompt names its path (`out/spec.md` in the run folder). Change no file in the copy: it is thrown away. An Opus model checks the spec against the code before it is posted on the issue and cut into build tickets, so it must be exact enough to build from.

## The issue

<the issue number and title, and its text if `gh issue view <n>` may not work in the sandbox>

<the owner's question in one or two lines>

## Facts to check first

<what we believe the code does today, each with the files it lives in. Verify each by reading it before you rely on it>

## Read

`AGENTS.md`, `docs/coding.md` (data, calculations and actions apart; one-way state; state machines written down; deep modules; errors as states; the real-time core never allocates, locks or waits), `docs/design/theory.md`, then:

<the design docs, source files, scripts and issue comments this spec depends on>

## What the spec contains, in this order

| # | Part | What it holds |
|---|---|---|
| 1 | Plain-words summary for the owner | five lines a performer would understand, one small mermaid diagram (`<br/>` for line breaks, no fill colours). No milestone codes, ticket numbers or internal terms: explain a term once in plain words, then use it |
| 2 | Today | a table: what exists, where (file:line), what is missing |
| 3 | The wire or API | exact shapes: Rust types with serde as the repo does it, and the generated TypeScript. Compare the options (new message or a field on an existing one) and pick one with the reason. What a late subscriber gets |
| 4 | The state machine | a mermaid `stateDiagram` (rule 3 of `docs/coding.md`), the exact thresholds and their source, entering and leaving rules, startup, attach and remove, and a table test of every state and event pair |
| 5 | How it is computed | where the numbers come from, how they are kept without breaking the real-time rules, who reads them, the cost per message. Name every place the real-time path is touched and the test that proves it stays free of allocation, lock and wait |
| 6 | Failure modes and invalid states | no data yet, wrong-time commands, a slow reader, clock steps, wrapping counters. Errors become states; a command that would make an invalid state is refused, never allowed and flagged |
| 7 | Test plan | the end-to-end test through the API with its exact fields and order, table tests, real-time checks, the fresh-engine case |
| 8 | Build tickets | ordered; each with a title, boxes as acceptance criteria with the command that proves each, the files it owns, what blocks it. Small enough for one worker. Shared API files in one fixed order |
| 9 | Decisions for the owner | a table with the options side by side, the recommended one marked, and the plain-words consequence of each |
| 10 | What could not be checked | and what a reviewer should challenge first |

## Method

| Rule | |
|---|---|
| Every claim about the code | carries file:line, checked by reading that line (`rg`, `sed -n`). Do not invent function or type names |
| Writing | no em dashes; short sentences; tables over prose; one idea per picture |
| Public text | no secrets, tokens, user names, local paths or machine names: the spec is posted on a public issue |
| End | your last message ends with a ten-line summary and the path of the spec |

## Out of scope

<what this spec does not decide or design>
