//! A broken file gives an error that names the file and the place. Never a panic.

use fixture_library::parse;

const HEAD: &str = r#"<?xml version="1.0"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
"#;

fn ok_channel() -> &'static str {
    r#" <Channel Name="Red" Preset="IntensityRed"/>"#
}

// name, the file, the line it must name (None: no line), a word of what is wrong
#[test]
fn a_broken_file_names_the_file_and_the_place() {
    let cases: Vec<(&str, String, Option<u32>, &str)> = vec![
        ("not xml", "this is not xml".into(), Some(1), "xml"),
        ("empty", String::new(), Some(1), "xml"),
        (
            "unclosed",
            format!("{HEAD} <Manufacturer>Acme"),
            Some(1),
            "xml",
        ),
        (
            "other root",
            "<Fixture/>".into(),
            Some(1),
            "FixtureDefinition",
        ),
        (
            "no maker",
            format!("{HEAD} <Model>M</Model><Type>Dimmer</Type>\n</FixtureDefinition>"),
            Some(2),
            "Manufacturer",
        ),
        (
            "no model",
            format!(
                "{HEAD} <Manufacturer>A</Manufacturer><Type>Dimmer</Type>\n</FixtureDefinition>"
            ),
            Some(2),
            "Model",
        ),
        (
            "mode names a channel that is not defined",
            format!(
                "{HEAD} <Manufacturer>A</Manufacturer><Model>M</Model><Type>Dimmer</Type>\n{}\n <Mode Name=\"X\">\n  <Channel Number=\"0\">Green</Channel>\n </Mode>\n</FixtureDefinition>",
                ok_channel()
            ),
            Some(6),
            "Green",
        ),
        (
            "two channels on one number",
            format!(
                "{HEAD} <Manufacturer>A</Manufacturer><Model>M</Model><Type>Dimmer</Type>\n{}\n <Mode Name=\"X\">\n  <Channel Number=\"0\">Red</Channel>\n  <Channel Number=\"0\">Red</Channel>\n </Mode>\n</FixtureDefinition>",
                ok_channel()
            ),
            Some(7),
            "0",
        ),
        (
            "a gap in the channel numbers",
            format!(
                "{HEAD} <Manufacturer>A</Manufacturer><Model>M</Model><Type>Dimmer</Type>\n{}\n <Mode Name=\"X\">\n  <Channel Number=\"0\">Red</Channel>\n  <Channel Number=\"2\">Red</Channel>\n </Mode>\n</FixtureDefinition>",
                ok_channel()
            ),
            Some(7),
            "2",
        ),
        (
            "a range backwards",
            format!(
                "{HEAD} <Manufacturer>A</Manufacturer><Model>M</Model><Type>Dimmer</Type>\n <Channel Name=\"C\">\n  <Group Byte=\"0\">Effect</Group>\n  <Capability Min=\"9\" Max=\"3\">x</Capability>\n </Channel>\n</FixtureDefinition>"
            ),
            Some(6),
            "range",
        ),
        (
            "a range past 255",
            format!(
                "{HEAD} <Manufacturer>A</Manufacturer><Model>M</Model><Type>Dimmer</Type>\n <Channel Name=\"C\">\n  <Capability Min=\"0\" Max=\"300\">x</Capability>\n </Channel>\n</FixtureDefinition>"
            ),
            Some(5),
            "300",
        ),
        (
            "a channel number that is not a number",
            format!(
                "{HEAD} <Manufacturer>A</Manufacturer><Model>M</Model><Type>Dimmer</Type>\n{}\n <Mode Name=\"X\">\n  <Channel Number=\"two\">Red</Channel>\n </Mode>\n</FixtureDefinition>",
                ok_channel()
            ),
            Some(6),
            "two",
        ),
    ];
    for (name, source, line, what) in cases {
        let error = parse("bad.qxf", source.as_bytes()).expect_err(name);
        let message = error.to_string();
        assert!(message.starts_with("bad.qxf"), "{name}: {message}");
        assert!(message.contains(what), "{name}: {message}");
        assert_eq!(error.file, "bad.qxf", "{name}");
        assert_eq!(error.line, line, "{name}: {message}");
        if let Some(line) = line {
            assert!(
                message.contains(&format!("line {line}")),
                "{name}: {message}"
            );
        }
    }
}

// bytes that are not even text
#[test]
fn bytes_that_are_not_text_are_an_error_not_a_panic() {
    let cases: [&[u8]; 4] = [&[0xff, 0xfe, 0x00], &[0x80], b"\xef\xbb\xbf<a>", &[]];
    for bytes in cases {
        let error = parse("junk.qxf", bytes).expect_err("not a definition");
        assert!(error.to_string().starts_with("junk.qxf"), "{error}");
    }
}

const BARE: &str = "<?xml version=\"1.0\"?>\n<!DOCTYPE FixtureDefinition>\n";
const BODY: &str = "<FixtureDefinition xmlns=\"http://www.qlcplus.org/FixtureDefinition\"><Manufacturer>A</Manufacturer><Model>M</Model><Type>Dimmer</Type></FixtureDefinition>";

// name, the file, the line it must name, a word of what is wrong. A definition may not declare entities or be huge: nothing is expanded.
#[test]
fn a_file_that_could_grow_when_read_is_an_error_before_it_is_read() {
    let expansions = "&b;".repeat(100);
    let cases: Vec<(&str, String, Option<u32>, &str)> = vec![
        (
            "entities that expand into each other",
            format!(
                "<?xml version=\"1.0\"?>\n<!DOCTYPE FixtureDefinition [\n<!ENTITY a \"aaaaaaaaaa\">\n<!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;\">\n]>\n<FixtureDefinition><Manufacturer>{expansions}</Manufacturer></FixtureDefinition>"
            ),
            Some(2),
            "declarations",
        ),
        (
            "an empty set of declarations",
            format!("<?xml version=\"1.0\"?>\n\n<!DOCTYPE FixtureDefinition []>\n{BODY}"),
            Some(3),
            "declarations",
        ),
        (
            "bigger than any real definition",
            format!("{BARE}{BODY}<!-- {} -->", "x".repeat(1024 * 1024)),
            None,
            "too large",
        ),
    ];
    for (name, source, line, what) in cases {
        let error = parse("bad.qxf", source.as_bytes()).expect_err(name);
        let message = error.to_string();
        assert!(message.starts_with("bad.qxf"), "{name}: {message}");
        assert!(message.contains(what), "{name}: {message}");
        assert_eq!(error.line, line, "{name}: {message}");
    }
}

// name, a file with a DOCTYPE that is fine
#[test]
fn a_doctype_without_declarations_is_read() {
    let cases = [
        ("bare", format!("{BARE}{BODY}")),
        (
            "with a public id",
            format!("<!DOCTYPE FixtureDefinition PUBLIC \"-//x[y]\" \"x.dtd\">\n{BODY}"),
        ),
        ("no doctype", BODY.to_string()),
    ];
    for (name, source) in cases {
        parse("ok.qxf", source.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}
