//! What a definition gets wrong but the rest can be read without: left out, and said as
//! a warning on the fixture. Nothing is dropped silently.

use fixture_library::{Layout, LeftOut, Warning, parse};

/// A definition of one channel `C`, used `channels` times in mode `M`, with `heads` (each
/// a list of channel numbers) and a layout in the mode or the fixture.
fn definition(channels: u16, heads: &[&[u16]], in_mode: &str, in_fixture: &str) -> String {
    let numbers: String = (0..channels)
        .map(|n| format!("\n  <Channel Number=\"{n}\">C</Channel>"))
        .collect();
    let heads: String = heads
        .iter()
        .map(|head| {
            let inside: String = head
                .iter()
                .map(|n| format!("<Channel>{n}</Channel>"))
                .collect();
            format!("\n  <Head>{inside}</Head>")
        })
        .collect();
    format!(
        "<FixtureDefinition><Manufacturer>A</Manufacturer><Model>M</Model><Type>Other</Type>\n <Channel Name=\"C\" Preset=\"IntensityRed\"/>{in_fixture}\n <Mode Name=\"M\">{in_mode}{numbers}{heads}\n </Mode>\n</FixtureDefinition>"
    )
}

fn layout(width: u16, height: u16) -> String {
    format!("<Physical><Layout Width=\"{width}\" Height=\"{height}\"/></Physical>")
}

fn warning(line: u32, left_out: LeftOut) -> Warning {
    Warning {
        mode: "M".to_string(),
        line,
        left_out,
    }
}

/// Each head's channel numbers.
type Heads = Vec<Vec<u16>>;
/// A name, the definition, the heads kept, the layout kept, the warnings.
type Case = (&'static str, String, Heads, Option<Layout>, Vec<Warning>);

const fn grid(width: u16, height: u16) -> Layout {
    Layout { width, height }
}

// name, the definition, the heads kept, the layout kept, the warnings
#[test]
fn what_does_not_fit_is_left_out_with_a_warning() {
    let cases: Vec<Case> = vec![
        (
            "all fits",
            definition(2, &[&[0], &[1]], &layout(2, 1), ""),
            vec![vec![0], vec![1]],
            Some(grid(2, 1)),
            vec![],
        ),
        (
            "a head names a channel past the end: that head goes",
            definition(3, &[&[0, 1], &[1, 3]], "", ""),
            vec![vec![0, 1]],
            None,
            vec![warning(
                8,
                LeftOut::Head {
                    head: 1,
                    channel: 3,
                    channels: 3,
                },
            )],
        ),
        (
            "two heads past the end",
            definition(1, &[&[1], &[0], &[2]], "", ""),
            vec![vec![0]],
            None,
            vec![
                warning(
                    5,
                    LeftOut::Head {
                        head: 0,
                        channel: 1,
                        channels: 1,
                    },
                ),
                warning(
                    7,
                    LeftOut::Head {
                        head: 2,
                        channel: 2,
                        channels: 1,
                    },
                ),
            ],
        ),
        (
            "the mode's layout has a cell too many",
            definition(2, &[&[0], &[1]], &layout(3, 1), ""),
            vec![vec![0], vec![1]],
            None,
            vec![warning(
                3,
                LeftOut::Layout {
                    layout: grid(3, 1),
                    heads: 2,
                },
            )],
        ),
        (
            "the fixture's layout has too few cells",
            definition(4, &[&[0], &[1], &[2], &[3]], "", &layout(2, 1)),
            vec![vec![0], vec![1], vec![2], vec![3]],
            None,
            vec![warning(
                2,
                LeftOut::Layout {
                    layout: grid(2, 1),
                    heads: 4,
                },
            )],
        ),
        (
            "a head left out, then the layout counts the heads kept",
            definition(2, &[&[0], &[1], &[2]], &layout(2, 1), ""),
            vec![vec![0], vec![1]],
            Some(grid(2, 1)),
            vec![warning(
                8,
                LeftOut::Head {
                    head: 2,
                    channel: 2,
                    channels: 2,
                },
            )],
        ),
        (
            "no heads: the layout is not checked",
            definition(1, &[], "", &layout(1, 1)),
            vec![],
            Some(grid(1, 1)),
            vec![],
        ),
    ];
    for (name, source, heads, kept, warnings) in cases {
        let fixture = parse("t.qxf", source.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mode = fixture.modes.first().expect("one mode");
        assert_eq!(mode.pixels, heads, "{name}");
        assert_eq!(mode.layout, kept, "{name}");
        assert_eq!(fixture.warnings, warnings, "{name}\n{source}");
    }
}

// a warning, what it says
#[test]
fn a_warning_says_the_mode_the_line_and_why() {
    let cases = [
        (
            warning(
                279,
                LeftOut::Head {
                    head: 10,
                    channel: 32,
                    channels: 32,
                },
            ),
            "mode \"M\": line 279: head 10 names channel 32, the mode has 32: the head is left out",
        ),
        (
            warning(
                12,
                LeftOut::Layout {
                    layout: grid(10, 4),
                    heads: 10,
                },
            ),
            "mode \"M\": line 12: the layout is 10 by 4, the mode has 10 heads: the layout is left out",
        ),
    ];
    for (warning, said) in cases {
        assert_eq!(warning.to_string(), said);
    }
}
