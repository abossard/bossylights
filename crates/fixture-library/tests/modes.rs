//! Each mode is read on its own (owner decision Q18): a mode that cannot be used is
//! refused with why, and the file's other modes are read. A file with no mode left is an
//! error, never a fixture without modes.

use fixture_library::{Problem, Refused, parse};

/// Mode `Good`, then mode `Bad` holding `bad` on line 5.
fn beside_a_good_mode(bad: &str) -> String {
    format!(
        "<FixtureDefinition><Manufacturer>A</Manufacturer><Model>M</Model><Type>Other</Type>\n <Channel Name=\"C\" Preset=\"IntensityRed\"/>\n <Mode Name=\"Good\"><Channel Number=\"0\">C</Channel></Mode>\n <Mode Name=\"Bad\">\n  {bad}\n </Mode>\n</FixtureDefinition>"
    )
}

fn bad_number(what: &'static str, text: &str) -> Problem {
    Problem::BadNumber {
        what,
        text: text.to_string(),
    }
}

// what is in mode Bad, the problem that refuses it on line 5 (None: it is read)
#[test]
fn a_mode_that_cannot_be_used_is_refused_and_the_others_stay() {
    let one = r#"<Channel Number="0">C</Channel>"#;
    let cases: Vec<(String, Option<Problem>)> = vec![
        (format!("{one}<Head><Channel>0</Channel></Head>"), None),
        (r#"<Channel Number="0" ActsOn="0">C</Channel>"#.into(), None),
        (
            format!("{one}<Head><Channel>1</Channel></Head>"),
            Some(Problem::HeadPastEnd {
                head: 0,
                channel: 1,
                channels: 1,
            }),
        ),
        (
            format!("{one}<Head><Channel>not a channel number</Channel></Head>"),
            Some(bad_number("Head channel", "not a channel number")),
        ),
        (
            r#"<Channel Number="two">C</Channel>"#.into(),
            Some(bad_number("Number", "two")),
        ),
        (
            r#"<Channel Number="0">Green</Channel>"#.into(),
            Some(Problem::UnknownChannel("Green".into())),
        ),
        (format!("{one}{one}"), Some(Problem::DuplicateNumber(0))),
        (
            format!(r#"{one}<Channel Number="2">C</Channel>"#),
            Some(Problem::SkippedNumber(2)),
        ),
        (
            r#"<Channel Number="0" ActsOn="not a channel number">C</Channel>"#.into(),
            Some(bad_number("ActsOn", "not a channel number")),
        ),
        (
            r#"<Channel Number="0" ActsOn="1">C</Channel>"#.into(),
            Some(Problem::ActsOnPastEnd {
                channel: 1,
                channels: 1,
            }),
        ),
        (
            r#"<Channel Number="0" ActsOn="9">C</Channel>"#.into(),
            Some(Problem::ActsOnPastEnd {
                channel: 9,
                channels: 1,
            }),
        ),
        (
            format!(r#"{one}<Physical><Focus PanMax="wide" TiltMax="0"/></Physical>"#),
            Some(bad_number("PanMax", "wide")),
        ),
    ];
    for (bad, problem) in cases {
        let fixture = parse("t.qxf", beside_a_good_mode(&bad).as_bytes())
            .unwrap_or_else(|e| panic!("{bad}: {e}"));
        let modes: Vec<_> = fixture.modes.iter().map(|m| m.name.as_str()).collect();
        let (kept, refused) = match problem {
            None => (vec!["Good", "Bad"], vec![]),
            Some(problem) => (
                vec!["Good"],
                vec![Refused {
                    mode: "Bad".into(),
                    line: 5,
                    problem,
                }],
            ),
        };
        assert_eq!((modes, fixture.refused), (kept, refused), "{bad}");
    }
}

// the modes of the file, the line the error names, the problem
#[test]
fn a_file_with_no_mode_left_is_an_error() {
    let refused = |mode: &str, line| Refused {
        mode: mode.into(),
        line,
        problem: Problem::HeadPastEnd {
            head: 0,
            channel: 1,
            channels: 1,
        },
    };
    let broken = |name: &str| {
        format!(
            "\n <Mode Name=\"{name}\"><Channel Number=\"0\">C</Channel><Head><Channel>1</Channel></Head></Mode>"
        )
    };
    let cases = [
        ("", 1, Problem::NoMode),
        (
            &*format!("{}{}", broken("A"), broken("B")),
            3,
            Problem::NoModeKept(vec![refused("A", 3), refused("B", 4)]),
        ),
    ];
    for (modes, line, problem) in cases {
        let source = format!(
            "<FixtureDefinition><Manufacturer>A</Manufacturer><Model>M</Model><Type>Other</Type>\n <Channel Name=\"C\" Preset=\"IntensityRed\"/>{modes}\n</FixtureDefinition>"
        );
        let error = parse("t.qxf", source.as_bytes()).expect_err(modes);
        assert_eq!(
            (error.line, error.problem),
            (Some(line), problem),
            "{modes}"
        );
    }
}

// a refusal, what it says
#[test]
fn a_refused_mode_says_the_mode_the_line_and_why() {
    let cases = [
        (
            Refused {
                mode: "32 Channel".into(),
                line: 475,
                problem: Problem::HeadPastEnd {
                    head: 11,
                    channel: 32,
                    channels: 32,
                },
            },
            "mode \"32 Channel\": line 475: head 11 names channel 32, the mode has 32",
        ),
        (
            Refused {
                mode: "M".into(),
                line: 4,
                problem: Problem::ActsOnPastEnd {
                    channel: 9,
                    channels: 2,
                },
            },
            "mode \"M\": line 4: ActsOn names channel 9, the mode has 2",
        ),
    ];
    for (refused, said) in cases {
        assert_eq!(refused.to_string(), said);
    }
}
