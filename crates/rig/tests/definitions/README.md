# Test data: QLC+ fixture definitions for the rig

Real `.qxf` files, copied unchanged from
[mcallegari/qlcplus](https://github.com/mcallegari/qlcplus), `resources/fixtures/`,
commit `b02dc4a2336964d3a6810ddd77fe4d883f143ba2`. The rig tests also read the fixture
library's definitions (`crates/fixture-library/tests/definitions`).

Licence: Apache-2.0. QLC+ publishes the whole repository, these files included, under
the Apache License 2.0 (`COPYING` in the repository root, copied here as
`LICENSE-QLCPLUS.txt`). No GPL or AGPL material is copied.

| File | Maker folder | Used as |
|---|---|---|
| `Martin-Rush-MH6-Wash.qxf` | Martin | a moving head with a 16-bit dimmer, RGBW and a shutter marked open |
| `Griven-Kolorado-2500.qxf` | Griven | a dimmer and cyan, magenta, yellow |
| `GLP-Impression-X4.qxf` | GLP | a hybrid: RGBW beside a colour wheel whose bypass is 0 to 7, fixed white at 112 to 119 |
| `Elation-Platinum-Spot-5R.qxf` | Elation | gobo wheels whose open range has no preset |
| `Martin-MAC-2000-Wash.qxf` | Martin | a zoom that travels 0 to 244, big to small, then "no changes" |
| `Stairville-BS-280-R10-BeamSpot.qxf` | Stairville | gobo wheel 1 named "Gobo 1" without presets, before a wheel with presets |
| `Clay-Paky-Alpha-Spot-575-HPE.qxf` | Clay_Paky | gobo wheels whose open range is named "No gobo" |
| `Martin-Rush-MH5-Profile.qxf` | Martin | a gobo wheel whose open range is named "Open - Gobo selection" |
| `American-DJ-VPar.qxf` | American_DJ | a hybrid whose colour macro at 0 is "Off" with a black preset: the macro is off |
| `Showtec-Spectral-850-RGBA-Tour.qxf` | Showtec | a hybrid whose colour macro at 0 is "Not functional" with a white preset |
| `Robe-Robin-Spikie.qxf` | Robe | a hybrid whose colour macro at 0 is "No function (0=default)" with a white preset |
| `EK-R3-Wash.qxf` | EK | a hybrid whose colour macro at 0 is "Macro Color OFF" |
| `Cameo-Q-Spot-15-RGBW.qxf` | Cameo | a hybrid whose colour macro at 0 is "Colour off" |
| `Chauvet-Ovation-CYC-1-FC.qxf` | Chauvet | a hybrid with two colour macros, each "None" at 0 |
| `Eliminator-Lighting-Stealth-Beam.qxf` | Eliminator_Lighting | a hybrid whose colour macro at 0 is "Nothing" |
| `Martin-MAC-301-Wash.qxf` | Martin | a hybrid whose colour macro at 0 is "Inactive" |
| `MARQ-Colormax-Par64.qxf` | MARQ | a hybrid whose colour macro at 0 is "No function (overrides Ch. 2–4)" |
| `Briteq-BT-Theatre-261FC.qxf` | Briteq | a hybrid whose colour macro at 0 is "No function - no filter, color channels used" |
| `Cameo-HydraBeam-4000.qxf` | Cameo | heads whose colour macro at 0 names the channels they mix with ("Channel 11-14") |
| `Stairville-CLB4-RGB-Compact-LED-Bar-4.qxf` | Stairville | a colour macro at 0 named "All LEDs Set via Channels 1-3" |
| `Stairville-MH-100-Beam-36x3-LED.qxf` | Stairville | colour macros at 0 named "All LEDs off" with a black preset: refused |
| `Stairville-AF-180-LED-Fogger-Co2-FX.qxf` | Stairville | a colour effect at 0 named "All LEDs off": refused |
| `Contest-SFX-HO150QC.qxf` | Contest | a colour macro at 0 named "LED off": refused |
| `American-DJ-WiFly-EXR-HEX5-IP.qxf` | American_DJ | a colour macro at 0 named "Off/Color change/fade 1/Slow to fast": refused |
| `American-DJ-QA5-IP.qxf` | American_DJ | a colour macro at 0 named "Color 1 (off)": refused |
| `Equinox-Party-Par-LED-PAR-56.qxf` | Equinox | a colour macro at 0 named "Blackout": refused |
| `American-DJ-Focus-Spot-4Z.qxf` | American_DJ | fine prism indexing beside each prism selection channel |
| `Generic-Generic-Smoke.qxf` | Generic | a pump with a master dimmer preset on a smoke machine |
| `Shehds-120W-4in1-RGBW-LED-Wash-Moving-Head.qxf` | Shehds | an auxiliary head with its own dimmer and RGB channels |
| `Botex-DSP-405.qxf` | Botex | four Intensity channels with only a group declaration |
| `Vari-Lite-VL2202-Spot.qxf` | Vari-Lite | a gobo index declared as a fine byte right after the gobo selector: two channels, no pair |
| `Cameo-CLMPAR3.qxf` | Cameo | external On/Off fine bytes after White and the master dimmer: no pair |
