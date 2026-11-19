# Review of <commit or range> for #<issue>

Run: `npm run peer -- both review <commit> <this file>`, after your own `npm run verify`, not beside it.

## What was asked

<the boxes of the issue, copied as they are>

## What the orchestrator already checked

| Check | Result |
|---|---|
| `npm run verify -- <commit>` | <all suites passed, or what failed> |
| breaks planted on purpose | <rule, break, the test that went red> |

## Look for

| Question | Proof wanted |
|---|---|
| A box that is not truly met | the input that shows it, and the command |
| A test that cannot fail | the line you changed in the product code, and the test still green |
| A rule of `docs/coding.md` or `AGENTS.md` broken | file:line and the rule |
| A state the change allows that cannot come out right | a failing test |
| My verdict above is wrong | argue against it |

## Out of scope

<what not to review>
