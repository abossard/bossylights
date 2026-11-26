//! Review tests for aspect 7 (`crates/fixture-library`): QLC+ `.qxf` parsing and invariants.

use fixture_library::{Layout, Problem, parse};

/// Claim: `fixture_library::read` silently ignores non-numeric `<Layout>` dimensions (with zero warnings and silent fallback to a root `<Layout>`) and accepts `Width="0" Height="0"` (`Layout { width: 0, height: 0 }`).
/// Source: `crates/fixture-library/src/warnings.rs` ("Nothing is dropped silently") and `crates/fixture-library/src/read.rs` (`Problem::BadNumber` on unparsable numeric fields; positive pixel grid dimensions).
/// Severity: medium.
/// Show impact: A broken or typoed `<Layout>` in a `.qxf` file either loses its matrix layout with no warning or yields a 0x0 matrix layout, or silently adopts an unrelated root layout instead of reporting the malformed mode layout.
#[test]
fn finding_fixture_library_01_malformed_and_zero_layout() {
    let malformed = br#"<?xml version="1.0" encoding="UTF-8"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Test</Manufacturer>
 <Model>Broken Layout</Model>
 <Type>LED Bar (Pixels)</Type>
 <Channel Name="Red 1" Preset="IntensityRed"/>
 <Channel Name="Red 2" Preset="IntensityRed"/>
 <Mode Name="Matrix">
  <Channel Number="0">Red 1</Channel>
  <Channel Number="1">Red 2</Channel>
  <Head><Channel>0</Channel></Head>
  <Head><Channel>1</Channel></Head>
  <Layout Width="wide" Height="1"/>
 </Mode>
</FixtureDefinition>"#;

    let zero_sized = br#"<?xml version="1.0" encoding="UTF-8"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Test</Manufacturer>
 <Model>Zero Layout</Model>
 <Type>LED Bar (Pixels)</Type>
 <Channel Name="Red 1" Preset="IntensityRed"/>
 <Channel Name="Red 2" Preset="IntensityRed"/>
 <Mode Name="Matrix">
  <Channel Number="0">Red 1</Channel>
  <Channel Number="1">Red 2</Channel>
  <Head><Channel>0</Channel></Head>
  <Head><Channel>1</Channel></Head>
  <Layout Width="0" Height="0"/>
 </Mode>
</FixtureDefinition>"#;

    let malformed_result = parse("broken-layout.qxf", malformed);
    let flagged_bad_width = match &malformed_result {
        Err(error) => matches!(error.problem, Problem::BadNumber { what: "Width", .. }),
        Ok(fixture) => !fixture.warnings.is_empty(),
    };
    assert!(
        flagged_bad_width,
        "Width=\"wide\" was dropped with neither an error nor a warning: {malformed_result:?}"
    );

    if let Ok(fixture_zero) = parse("zero-layout.qxf", zero_sized) {
        assert_ne!(
            fixture_zero.modes[0].layout,
            Some(Layout {
                width: 0,
                height: 0,
            }),
            "a 0x0 matrix layout is nonsensical and must not be accepted as Some(Layout {{ width: 0, height: 0 }})"
        );
    }
}

/// Claim: `fixture_library::parse` accepts a `<Mode>` with zero `<Channel>` elements (`channels: []`), violating the engine invariant that every fixture mode occupies at least one DMX channel.
/// Source: `docs/engine.md`, "Reply::Picture / Patched": "`channels: 1..`" and "Patching": "Every fixture mode has at least one channel."
/// Severity: medium.
/// Show impact: Patching a zero-channel mode places a `channels: 0` fixture in the show that bypasses DMX address overlap checks (multiple fixtures can share the exact same universe and address) and publishes `Patched { channels: 0 }`.
#[test]
fn finding_fixture_library_02_empty_mode_accepted() {
    let qxf = br#"<?xml version="1.0" encoding="UTF-8"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Test</Manufacturer>
 <Model>Channel-less Mode</Model>
 <Type>Other</Type>
 <Channel Name="Dimmer" Preset="IntensityMasterDimmer"/>
 <Mode Name="Empty"/>
</FixtureDefinition>"#;

    let parsed = parse("empty-mode.qxf", qxf);
    if let Ok(fixture) = &parsed {
        assert!(
            fixture.modes.iter().all(|mode| !mode.channels.is_empty()),
            "mode with 0 channels was accepted: {parsed:?}"
        );
    }
}
