# Capability map

Researched 2026-11-17. Items marked (unverified) could not be confirmed from a primary source.

## The rumor

- No evidence that QLC+ will close its source, change license or restrict v5. No announcement, commit, issue or forum post.
- Code is Apache-2.0. Published code cannot be relicensed away from you. There is no CLA, so a relicense would need every contributor's consent.
- v5 binaries are free: 5.3.1 from 2026-11-05, direct download. https://www.qlcplus.org/download
- Likely origin of the rumor: paid side offers (Raspberry Pi image, themes, Premium Requests from 2026-03, Bounty Board from 2026-10). Both news posts speak against paywalled features. https://www.qlcplus.org/news/introducing-premium-requests
- The real risk is the bus factor. One person wrote about 78% of the commits of the last 12 months.
- A fork may keep and ship everything, but may not use the name QLC+ (Apache-2.0 §6).

## Candidates as a base

| Tool | License | Stack | Health | What it is | Verdict |
|---|---|---|---|---|---|
| QLC+ v5 + the abossard/qlcplus fork | Apache-2.0 | C++, Qt 6, QML | Upstream active, one main author. Fork has 690 own commits | Full desk with Virtual Console, timeline, 2D/3D, 1,782 fixtures, plus the fork's MCP, DJ, analysis and page planner | Working rig and parity reference. Stability and macOS pointer issues stay |
| BlinderKitten | GPL-3.0 | C++, forked JUCE 7, OrganicUI, Projucer | Active, one author (1,618 of about 1,650 commits), still "beta" | grandMA-style console: programmer, command line, tracking cue lists, layers, effects, carousels, mappers, trackers | Take the engine ideas, not the code |
| Chataigne | GPL-3.0 | Same as BlinderKitten | Active, one author. Got an MCP server in 2026 | Show-control hub: protocols, state machine, timeline, mappings, dashboard. No fixtures | Companion over OSCQuery, and inspiration |
| Mizer | AGPL-3.0 | Rust core, Flutter UI | Active, one maintainer, 157 open issues | Node graph plus programmer, laser, MIDI profiles, imports QLC+/OFL/GDTF fixtures | Closest in spirit. No USB DMX, no audio timeline, AGPL |
| xLights | GPL-3.0 | C++, wxWidgets | Very active | Offline pixel sequencer with audio timeline and 3D preview | Not a live desk |
| SwiftVJ | same author | Swift 6.2, SwiftUI, Metal | 157 commits, no CI | VJ hub: shaders, Launchpad, OS2L, OSC, Syphon | Donor of Launchpad, OS2L and OSC code. No lighting domain |

Parts, not bases:
- Open Fixture Library (MIT): fixture data with QLC+ export.
- GDTF: open fixture format, parsers in Python (MIT) and Rust (inside Mizer).
- OLA (LGPL/GPL): USB and network DMX I/O daemon, last release 2023.
- LedFx (GPL-3.0): audio-reactive pixel effects, a common companion.
- ETCLabs sACN (Apache-2.0), Rust `sacn` crate (MIT/Apache).
- laser-dac-rs (MIT): Helios, Ether Dream, IDN, LaserCube.

## Capability matrix

● yes, ◐ partial, ○ no, · unknown. The full, weighted version is the Tool fit tab of the plan map.

| Capability | QLC+ fork | BlinderKitten | Chataigne | Mizer | xLights | SwiftVJ |
|---|---|---|---|---|---|---|
| Fixture library | ● 1,782 | ● 19,782 + GDTF | ○ | ● OFL, GDTF, QLC+ | ◐ | ○ |
| Patch and groups | ● | ● | ○ | ● | ● | ○ |
| Live programmer | ◐ | ● | ○ | ● | ○ | ○ |
| Palettes / presets | ● | ● | ○ | ● | ○ | ○ |
| Tracking cue lists | ○ | ● | ○ | ◐ | ○ | ○ |
| Effects across groups | ● | ● | ○ | ● | ● | ○ |
| Pixel matrix effects | ● | ◐ | ○ | ● | ● | ○ |
| Priority layers | ● | ● | ○ | ◐ | ○ | ○ |
| User-built control surface | ● | ◐ fixed grids | ◐ dashboard | ◐ | ○ | ○ |
| MIDI in | ● | ● | ● | ● | ○ | ◐ |
| MIDI LED feedback | ◐ | ● | ◐ | ● | ○ | ● |
| Controller profiles | ● 41 | ○ | ○ | ● | ○ | ◐ |
| OSC | ● | ● | ● | ● | ○ | ● |
| Web / tablet remote | ● | ○ | ● | ◐ | ○ | ○ |
| USB DMX | ● | ● | ● | ○ | ● | ○ |
| Art-Net, sACN | ● | ● | ● | ● | ● | ○ |
| DDP | ● | ○ | ○ | ○ | ● | ○ |
| Audio in, beat | ● | ○ | ◐ FFT only | ◐ | ○ | ◐ |
| Song analysis (sections) | ● | ○ | ○ | ○ | ◐ | ○ |
| VirtualDJ link | ● | ○ | ○ | ○ | ○ | ◐ |
| Timeline with audio | ● | ○ | ● | ◐ | ● | ◐ |
| Record live play | ● | ○ | ○ | ○ | ○ | ○ |
| Timecode | ◐ | ◐ MTC | ● LTC, MTC | ● | ◐ | ○ |
| Autopilot from music | ◐ | ○ | ○ | ○ | ○ | ○ |
| 2D live view | ● | ● | ○ | ● | ● | ○ |
| 3D preview | ● | ○ | ○ | ○ | ● | ○ |
| Laser DAC | ○ | ○ | ○ | ● | ○ | ○ |
| MCP / agent API | ● 105 tools | ◐ opt-in | ● | ◐ gRPC | ○ | ◐ |
| Undo | ◐ | ● | ● | · | ◐ | ○ |
| Headless engine | ○ | ○ | ○ | ◐ | ○ | ○ |
| Easy first five minutes | ○ | ○ | ○ | ○ | ○ | ○ |

## What each tool teaches

**QLC+ v5 and the fork**
- Keep: Virtual Console, function model, Show Manager, fixture files, input profiles, plain HTP/LTP engine.
- Fork additions worth carrying: MCP (105 tools), DDP, VDJ Bridge and OS2L discovery, HUE Matrix (72 audio scripts), audio beat source, show recorder and LTP mix layers, DJ Perform, DJSET, EDM analysis, Song Director, page planner, VC editing, Flow Console.
- Pain on file: use-after-free when deleting running functions, DMX read races, flaky tests, trackpad behaviour, undo gaps in the VC editor.

**BlinderKitten**
- Keep the ideas: programmer, integer layers with HTP or LTP per channel type, timing "thru" spread, wings and blocks, mappers, trackers, selection masters.
- Not usable as a library: engine objects derive from UI classes and the engine thread calls UI singletons. No headless mode.
- Missing: audio, timeline, scripting, free control surface, web UI, 3D.
- Hard for beginners: 15 glossary terms before first steps, inspector-driven editing, docs without pictures.
- Source: https://github.com/norbertrostaing/BlinderKitten , https://norbertrostaing.gitbook.io/blinderkitten/

**Chataigne**
- Keep the ideas: one addressable parameter tree that OSC, OSCQuery, MCP, undo and dashboards all share. Parrot records and replays parameter changes. State machine plus timeline.
- Dashboard is a free canvas with a web version, but has no fixture-aware widgets and no per-widget LED feedback.
- Best use: the new app publishes OSCQuery and Chataigne drives it.
- Source: https://github.com/benkuper/Chataigne

**Mizer**
- Proof that a Rust lighting engine with laser, MIDI profiles and fixture import works.
- Source: https://github.com/maxjoehnk/Mizer

**SwiftVJ**
- Reusable: CoreMIDI Launchpad code with LED SysEx (solid, flash, pulse) and learn mode, OS2L listener, VDJ monitor over OSC, OSC routing.
- Not reusable as a host: no fixture model, no DMX, no real-time thread.

## Taste benchmarks (closed, for ideas only)

| Product | Steal this |
|---|---|
| Ableton Live | Clip grid mirrored on the Launchpad with no setup. MIDI map mode: click, move, done. Launch quantize |
| Lightkey | Live preview always on. Drag fixtures in. 70+ effect templates |
| SoundSwitch | Autoloops: beat-synced fallback whenever nothing is scripted. One-click auto-scripting |
| MaestroDMX | Zero-programming show from audio, picked by mood and energy |
| Rekordbox Lighting | Phrase analysis drives the lights |
| Avolites Titan | Busking: playbacks, shapes, key frames |
| grandMA3 | Programmer, phasers, presets |
| GarageBand Live Loops | Starts from a template full of content |

## Stack options

| Option | Engine safety | Free canvas UI | macOS feel | Other platforms | License forced | Agent test loop |
|---|---|---|---|---|---|---|
| Rust engine + web UI in Tauri | Strongest | Very strong | Good | Same UI, also tablet | None | Best (Playwright) |
| Rust engine + SwiftUI/Metal | Strongest | Excellent | Best | Engine only | None | Clumsy UI tests |
| All Swift | Needs discipline | Excellent | Best | UI experimental | None | Clumsy UI tests |
| C++ JUCE + OrganicUI | Proven | CPU-drawn | Good | Good | GPL-3 (AGPL-3 on JUCE 8) | Mediocre |
| C++ Qt 6 / QML | Proven in QLC+ | Strong | Known glitches | Good | LGPL, GPL for 3D and graphs | Commercial tools |

- rmcp (Rust MCP SDK) 3.10, Swift MCP SDK 0.12 (pre-1.0), SwiftCrossUI still experimental, JUCE modules AGPLv3 or commercial. https://crates.io/crates/rmcp , https://github.com/modelcontextprotocol/swift-sdk/releases , https://github.com/juce-framework/JUCE/blob/master/LICENSE.md
- The language is not the latency risk. App Nap, timer coalescing, thread priority and blocking USB writes are (unverified on macOS 26).
- Open DMX timing is generated by the host. FTDI's VCP driver blocks D2XX on macOS, QLC+ talks to the device directly.

## Laser

- "ILDA" is an analog connector. A computer needs a DAC that streams points.
- Kickstarter candidate: Laserabc A1 (2025-12), "ILDA + DMX ready". LaserCube was Indiegogo and has a built-in DAC.
- Zero-code path: if the laser has a DMX mode, it is a plain fixture today.
- DAC path: Helios (about 114 USD, USB, open SDK) or Ether Dream (network, safety-stops on missing data). laser-dac-rs (MIT) covers Helios, Ether Dream, IDN and LaserCube.
- ofxLaser is non-commercial share-alike, so not usable. No Swift laser library exists.
- LaserCube does not blank on underrun. Prefer DACs that stop safely.
- Switzerland: shows with class 1M and above need a notification to the FOPH and a certified operator. Software must never claim audience-scanning safety. https://www.bag.admin.ch/bag/en/home/gesund-leben/umwelt-und-gesundheit/strahlung-radioaktivitaet-schall/elektromagnetische-felder-emf-uv-laser-licht/laser-und-lasershows/lasershows.html

## Gaps no open-source tool fills

- Console-grade busking together with a music-driven autopilot.
- Phrase-aware DJ sync (today only Rekordbox and SoundSwitch).
- Lightkey-level onboarding.
- Lights, pixels and laser on one audio timeline, with USB DMX and feedback surfaces.
- A desk with a first-class agent API. The QLC+ fork and Chataigne are the only ones found.

## Easy to use: design rules

Sources: W3C COGA "Making Content Usable" https://www.w3.org/TR/coga-usable/ , delay aversion in ADHD https://pmc.ncbi.nlm.nih.gov/articles/PMC2565443 , UIST '25 study on working memory https://arxiv.org/pdf/2507.18151v1.pdf

1. Every press gives visible output at once, on screen and on the pad.
2. Launch to first changed light in under 60 seconds, with no form.
3. No screen needs a value remembered from another screen.
4. State always visible: live or preview, BPM, what runs, master.
5. One global undo. Blackout can be undone too.
6. No modal dead ends, no timeouts.
7. Default layer shows a handful of controls. Expert layer is opt-in.
8. Every added fixture gets generated looks right away.
9. Reopens exactly where you left.
10. A "surprise me" button that is always safe.
11. No tips or update prompts during live output.
12. Plain words in the default layer.
