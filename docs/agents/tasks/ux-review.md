# UX review of <screen, control or catalog page> at <commit>

Run: `npm run peer -- both review <commit> <this file> --name ux-<slug>`. A review runs `sol` at each CLI's highest effort by default, as the owner asked for UX reviews.

## What to look at

| Source | Path |
|---|---|
| the controls, every situation | `docs/design/controls.md` (generated from `ui/src/controls`) |
| the kit the controls are drawn with | `docs/design/kit.md`, `ui/src/kit/` |
| the screens and the visual world | `docs/design/screens.md` |
| the live catalog | `npm run catalog`, or its Playwright tests `ui/src/catalog/catalog.spec.ts` |
| the part under review | <files, rows or pages> |

## The person using it

A busy operator at a live show, in the dark, often with no time to read. Plain words in the default view (looks, pads), desk words where they belong (Playback, Patch, Show file, Looks, Tempo). Easy first, deep on request.

## Look for

| Question | Proof wanted |
|---|---|
| A situation with no clear look: what does the person see, and what can they do next | the control and situation, a screenshot in the out folder |
| A control that allows a state that cannot come out right | the steps, and the command or test that shows it |
| Two controls that do the same, or one control that does two things | the rows |
| A word the person would not know in the default view | the text and where it shows |
| Too small, too low in contrast or too hard to hit live (44 px for anything pressed live) | the measured size or contrast ratio |
| Something a keyboard or screen reader cannot reach | the element and the check that fails |

Screenshots and measurements go into the out folder. A finding without a picture or a number is a lead, not a finding.

## Out of scope

<what not to review>
