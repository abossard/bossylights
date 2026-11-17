---
version: 1
slug: "mockups-src-app-tsx"
primary_target: "mockups/src/App.tsx"
related_targets: []
---

# Mockup app: shell and eight screens

Scope: the interactive mockups under `mockups/`. Visitor mode: Operate. Audience: people running lights on a laptop in a dark room, and on a touch screen. Job: judge alignment, navigation and structure of every screen, rate and comment. Screen briefs: `docs/design/screens.md`. Unresolved: the real UI framework (issue #24), real data.

## Direction contract

THESIS: Tonight is a timetable. Three lanes (Live, Show, Autopilot) and a NOW line are present wherever time exists. It refuses the dark grey panel console with one accent colour that this category ships.

OWN-WORLD: warm off-black ground #1c1a17, paper #f4efe6 text. Flat saturated blocks with dark text, one colour per layer: Autopilot marigold #f2b705, Show cyan #20b2c9, Live hot pink #ef3e7b. Square corners, no gradients, glow or shadows. Big Shoulders Display for lane labels, block titles and big numerals, Archivo for everything small, tabular numerals for time.

STORY: the operator sees what runs on which layer now and next, presses a pad, sees a block appear in the Live lane above the others, releases it and the lane below shows through again.

FIRST VIEWPORT: top bar with product name, eight workspace tabs and the status (LIVE, tempo with four beat dots, master) at the right. Left half: the timetable, three lanes with a time ruler and the NOW line with its flag, the rig view below it. Right half: console page tabs, the override notice, the 8 x 4 pad grid, six faders and the XY pad. The primary action is a pad press.

FORM: Running Order (festival set-times board), first on the ordered list of seven, chosen by the owner on the decision page over the rolled direction. seed:4919ed49790bc300. Signature interaction: the NOW line, on every screen that shows time, and its flag on everything that runs. Motion: blocks grow along the lane at clock speed, nothing eases in decoratively.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
