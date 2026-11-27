//! The rules every mode the library reads keeps (`Mode::check`): a mode from elsewhere, a
//! show file, is refused by the same rule with the same problem (#354).

use fixture_library::{Layout, Mode, Problem, Range, Refused, parse};

/// Two channels, a head on each, a layout of 2 by 1: every rule kept.
fn bar() -> Mode {
    let qxf = br#"<FixtureDefinition><Manufacturer>A</Manufacturer><Model>Bar</Model><Type>LED Bar (Pixels)</Type>
 <Channel Name="R1" Preset="IntensityRed"/><Channel Name="R2" Preset="IntensityRed"/>
 <Mode Name="2px"><Channel Number="0">R1</Channel><Channel Number="1">R2</Channel>
  <Head><Channel>0</Channel></Head><Head><Channel>1</Channel></Head><Layout Width="2" Height="1"/></Mode>
</FixtureDefinition>"#;
    parse("bar.qxf", qxf).unwrap().modes.remove(0)
}

fn range(min: u8, max: u8) -> Range {
    Range {
        min,
        max,
        label: String::new(),
        does: None,
    }
}

// case, the change to the whole bar, the answer
#[test]
fn a_mode_that_breaks_a_rule_of_the_model_is_refused_with_why() {
    type Change = fn(&mut Mode);
    let cases: [(&str, Change, Result<(), Problem>); 11] = [
        ("whole", |_| {}, Ok(())),
        (
            "no channel",
            |m| {
                m.channels.clear();
                m.pixels.clear();
            },
            Err(Problem::NoChannel),
        ),
        (
            "a head on the last channel",
            |m| m.pixels[0] = vec![1],
            Ok(()),
        ),
        (
            "a head past the end",
            |m| m.pixels[1] = vec![0, 2],
            Err(Problem::HeadPastEnd {
                head: 1,
                channel: 2,
                channels: 2,
            }),
        ),
        (
            "ActsOn the last channel",
            |m| m.channels[0].declared.acts_on = Some(1),
            Ok(()),
        ),
        (
            "ActsOn past the end",
            |m| m.channels[1].declared.acts_on = Some(2),
            Err(Problem::ActsOnPastEnd {
                channel: 2,
                channels: 2,
            }),
        ),
        (
            "a range of one value",
            |m| m.channels[1].ranges = vec![range(0, 9), range(10, 10)],
            Ok(()),
        ),
        (
            "a range that runs backwards",
            |m| m.channels[1].ranges = vec![range(0, 9), range(200, 10)],
            Err(Problem::BackwardsRange { min: 200, max: 10 }),
        ),
        (
            "a layout without one cell per head",
            |m| {
                m.layout = Some(Layout {
                    width: 3,
                    height: 1,
                })
            },
            Err(Problem::LayoutCells { cells: 3, heads: 2 }),
        ),
        (
            "any layout on a mode without heads",
            |m| {
                m.pixels.clear();
                m.layout = Some(Layout {
                    width: 3,
                    height: 1,
                });
            },
            Ok(()),
        ),
        ("no layout", |m| m.layout = None, Ok(())),
    ];
    for (case, change, answer) in cases {
        let mut mode = bar();
        change(&mut mode);
        assert_eq!(mode.check(), answer, "{case}");
    }
}

// Every mode of the real definitions in `tests/definitions/` keeps every rule.
#[test]
fn every_mode_the_library_reads_keeps_the_rules() {
    let folder = format!("{}/tests/definitions", env!("CARGO_MANIFEST_DIR"));
    let mut modes = 0;
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "qxf") {
            continue;
        }
        let fixture = parse("real.qxf", &std::fs::read(&path).unwrap()).unwrap();
        for mode in &fixture.modes {
            assert_eq!(mode.check(), Ok(()), "{}: {}", path.display(), mode.name);
            modes += 1;
        }
    }
    assert!(modes > 10, "{modes} modes read");
}

// A mode with no channel is refused, with why, as a mode with a broken head is (Q18).
// Source: owner, 27 Nov, on #354. Finding LIB-02 of #349.
#[test]
fn a_mode_without_a_channel_is_refused() {
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
    assert_eq!(
        parsed.map_err(|error| error.problem),
        Err(Problem::NoModeKept(vec![Refused {
            mode: "Empty".into(),
            line: 7,
            problem: Problem::NoChannel,
        }]))
    );
}
