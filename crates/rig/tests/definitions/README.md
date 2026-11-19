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
