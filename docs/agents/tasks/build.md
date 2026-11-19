# Build #<issue>: <title>

Run: `npm run peer -- codex build <current head> <this file> --name <issue-slug>`. Build on the current head and verify the patch at once: a patch goes stale when `main` moves.

## Parent

<the map item or spec, with its path in `docs/design/`>

## Boxes

| Box | Command that shows it |
|---|---|
| <what must be true> | <`cargo test -p <crate> --offline <test>`> |

## Files you may touch

<crates, files or folders. A new file beats a wide edit of a shared one>

## Out of scope

<what not to change>
