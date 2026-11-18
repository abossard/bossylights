//! Bytes of a QLC+ `.qxf` file to a `Fixture`. All of the XML walking lives here.

use crate::error::{Error, Problem};
use crate::function::function_of;
use crate::model::{Byte, Channel, Does, Fixture, Kind, Layout, Mode, Range, Rgb};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use roxmltree::{Document, Node, ParsingOptions};

/// The largest real definition is 133 kB: this is about seven times that.
const MOST_BYTES: usize = 1024 * 1024;

/// The line of the `[` that opens declarations in a DOCTYPE, when there is one. Quoted text is skipped.
fn declarations_at(text: &str) -> Option<u32> {
    let start = text.find("<!DOCTYPE")?;
    let mut quote = None;
    for (at, c) in text[start..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '[') => return Some(1 + text[..start + at].matches('\n').count() as u32),
            (None, '>') => return None,
            _ => {}
        }
    }
    None
}

/// Reads one definition. `file` is only a name for the error: nothing is opened.
pub fn parse(file: &str, bytes: &[u8]) -> Result<Fixture, Error> {
    let failed = |line: Option<u32>, problem| Error {
        file: file.to_string(),
        line,
        problem,
    };
    let text = core::str::from_utf8(bytes).map_err(|_| failed(None, Problem::NotText))?;
    if bytes.len() > MOST_BYTES {
        return Err(failed(
            None,
            Problem::TooLarge {
                bytes: bytes.len(),
                most: MOST_BYTES,
            },
        ));
    }
    if let Some(line) = declarations_at(text) {
        return Err(failed(Some(line), Problem::Declarations));
    }
    // QLC+ files carry a bare `<!DOCTYPE FixtureDefinition>`: allowed, the check above left it no entities.
    let options = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    let document = Document::parse_with_options(text, options)
        .map_err(|e| failed(Some(e.pos().row), Problem::NotXml(e.to_string())))?;
    let at = |node: Node| document.text_pos_at(node.range().start).row;
    read(&document).map_err(|(node, problem)| failed(Some(at(node)), problem))
}

type Failure<'a> = (Node<'a, 'a>, Problem);

fn read<'a>(document: &'a Document<'a>) -> Result<Fixture, Failure<'a>> {
    let root = document.root_element();
    let name = root.tag_name().name();
    if name != "FixtureDefinition" {
        return Err((root, Problem::WrongRoot(name.to_string())));
    }
    let maker = text_of(root, "Manufacturer")?;
    let model = text_of(root, "Model")?;
    let kind = kind_of(&text_of(root, "Type")?);
    let channels = root
        .children()
        .filter(|n| n.has_tag_name("Channel"))
        .map(channel)
        .collect::<Result<Vec<_>, _>>()?;
    let modes = root
        .children()
        .filter(|n| n.has_tag_name("Mode"))
        .map(|node| mode(node, root, &channels))
        .collect::<Result<_, _>>()?;
    Ok(Fixture {
        maker,
        model,
        kind,
        modes,
    })
}

fn text_of<'a>(parent: Node<'a, 'a>, tag: &'static str) -> Result<String, Failure<'a>> {
    parent
        .children()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string())
        .ok_or((parent, Problem::Missing(tag)))
}

fn kind_of(name: &str) -> Kind {
    match name {
        "Color Changer" => Kind::ColorChanger,
        "Dimmer" => Kind::Dimmer,
        "Effect" => Kind::Effect,
        "Fan" => Kind::Fan,
        "Flower" => Kind::Flower,
        "Hazer" => Kind::Hazer,
        "Laser" => Kind::Laser,
        "LED Bar (Beams)" => Kind::LedBarBeams,
        "LED Bar (Pixels)" => Kind::LedBarPixels,
        "Moving Head" => Kind::MovingHead,
        "Other" => Kind::Other,
        "Scanner" => Kind::Scanner,
        "Smoke" => Kind::Smoke,
        "Strobe" => Kind::Strobe,
        other => Kind::Unlisted(other.to_string()),
    }
}

/// A number attribute: `what` names it in the error.
fn number<'a, T: core::str::FromStr>(
    node: Node<'a, 'a>,
    what: &'static str,
) -> Result<T, Failure<'a>> {
    let text = node.attribute(what).unwrap_or_default();
    text.trim().parse().map_err(|_| {
        (
            node,
            Problem::BadNumber {
                what,
                text: text.to_string(),
            },
        )
    })
}

fn channel<'a>(node: Node<'a, 'a>) -> Result<Channel, Failure<'a>> {
    let name = node.attribute("Name").unwrap_or_default().to_string();
    let group = match node.children().find(|n| n.has_tag_name("Group")) {
        Some(group) => {
            let byte = match group.attribute("Byte").map(str::trim) {
                None | Some("0") => Byte::Coarse,
                Some("1") => Byte::Fine,
                Some(other) => {
                    return Err((
                        group,
                        Problem::BadNumber {
                            what: "Byte",
                            text: other.to_string(),
                        },
                    ));
                }
            };
            Some((group.text().unwrap_or_default().trim(), byte))
        }
        None => None,
    };
    let (function, byte) = function_of(node.attribute("Preset"), group, &name);
    let default = match node.attribute("Default") {
        Some(_) => Some(number(node, "Default")?),
        None => None,
    };
    let ranges = node
        .children()
        .filter(|n| n.has_tag_name("Capability"))
        .map(range)
        .collect::<Result<_, _>>()?;
    Ok(Channel {
        name,
        function,
        byte,
        default,
        ranges,
    })
}

fn range<'a>(node: Node<'a, 'a>) -> Result<Range, Failure<'a>> {
    let (min, max): (u8, u8) = (number(node, "Min")?, number(node, "Max")?);
    if min > max {
        return Err((node, Problem::BackwardsRange { min, max }));
    }
    let label = node.text().unwrap_or_default().trim().to_string();
    let does = node
        .attribute("Preset")
        .and_then(|preset| does(preset, node.attribute("Res1"), node.attribute("Res2")));
    Ok(Range {
        min,
        max,
        label,
        does,
    })
}

/// What a QLC+ capability preset does (`engine/src/qlccapability.h`), with its colours.
/// A colour preset without a colour it can read says nothing, as in QLC+.
fn does(preset: &str, first: Option<&str>, second: Option<&str>) -> Option<Does> {
    match preset {
        "ShutterOpen" => Some(Does::Open),
        "ShutterClose" => Some(Does::Closed),
        "LampOn" => Some(Does::LampOn),
        "ColorMacro" => Some(Does::Color(rgb(first?)?)),
        "ColorDoubleMacro" => Some(Does::Colors(rgb(first?)?, rgb(second?)?)),
        _ if ["Strobe", "Pulse", "Ramp"]
            .iter()
            .any(|start| preset.starts_with(start)) =>
        {
            Some(Does::Strobe)
        }
        _ if preset.starts_with("Reset") => Some(Does::Reset),
        _ => None,
    }
}

/// `#rrggbb`, as every colour in the QLC+ library is written.
fn rgb(text: &str) -> Option<Rgb> {
    let hex = text.trim().strip_prefix('#')?;
    let byte = |at: usize| {
        hex.get(at..at + 2)
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
    };
    (hex.len() == 6).then_some(())?;
    Some(Rgb {
        red: byte(0)?,
        green: byte(2)?,
        blue: byte(4)?,
    })
}

/// A mode: its channels by number, its pixels (`Head`s) and its layout.
fn mode<'a>(
    node: Node<'a, 'a>,
    root: Node<'a, 'a>,
    defined: &[Channel],
) -> Result<Mode, Failure<'a>> {
    let name = node.attribute("Name").unwrap_or_default().to_string();
    let mut by_number: BTreeMap<u16, &Channel> = BTreeMap::new();
    for entry in node.children().filter(|n| n.has_tag_name("Channel")) {
        let number: u16 = number(entry, "Number")?;
        // Names match as written, else ignoring spaces at the ends: real files have both.
        let wanted = entry.text().unwrap_or_default();
        let found = defined
            .iter()
            .find(|c| c.name == wanted)
            .or_else(|| defined.iter().find(|c| c.name.trim() == wanted.trim()))
            .ok_or_else(|| (entry, Problem::UnknownChannel(wanted.trim().to_string())))?;
        if by_number.insert(number, found).is_some() {
            return Err((entry, Problem::DuplicateNumber(number)));
        }
    }
    // The numbers must be 0, 1, 2, ... with none skipped.
    if let Some((number, _)) = by_number
        .iter()
        .zip(0u16..)
        .find(|((n, _), want)| *n != want)
        .map(|((n, c), _)| (*n, c))
    {
        let entry = node
            .children()
            .filter(|n| n.has_tag_name("Channel"))
            .find(|n| {
                n.attribute("Number")
                    .and_then(|t| t.trim().parse::<u16>().ok())
                    == Some(number)
            })
            .unwrap_or(node);
        return Err((entry, Problem::SkippedNumber(number)));
    }
    let channels: Vec<Channel> = by_number.values().map(|c| (*c).clone()).collect();
    let pixels: Vec<Vec<u16>> = node
        .children()
        .filter(|n| n.has_tag_name("Head"))
        .map(|head| {
            head.children()
                .filter(|n| n.has_tag_name("Channel"))
                .map(|n| head_channel(n, channels.len()))
                .collect::<Result<Vec<u16>, _>>()
        })
        .collect::<Result<_, _>>()?;
    // A layout must have a cell for every head. 244 modes of the QLC+ library give one
    // that does not (most share the fixture's grid with a mode of fewer heads): their
    // layout is left out, the heads stay.
    let layout = layout_in(node)
        .or_else(|| {
            root.children()
                .find(|n| n.has_tag_name("Physical"))
                .and_then(layout_in)
        })
        .filter(|l: &Layout| {
            pixels.is_empty() || usize::from(l.width) * usize::from(l.height) == pixels.len()
        });
    Ok(Mode {
        name,
        channels,
        pixels,
        layout,
    })
}

/// A channel of a head: a number of a channel the mode has.
fn head_channel<'a>(node: Node<'a, 'a>, count: usize) -> Result<u16, Failure<'a>> {
    let text = node.text().unwrap_or_default();
    let number: u16 = text.trim().parse().map_err(|_| {
        (
            node,
            Problem::BadNumber {
                what: "Head channel",
                text: text.to_string(),
            },
        )
    })?;
    if usize::from(number) >= count {
        return Err((node, Problem::UnknownHeadChannel(number)));
    }
    Ok(number)
}

/// The `Layout` under a mode or under the fixture's own `Physical`.
fn layout_in(node: Node) -> Option<Layout> {
    let layout = node.descendants().find(|n| n.has_tag_name("Layout"))?;
    Some(Layout {
        width: layout.attribute("Width")?.trim().parse().ok()?,
        height: layout.attribute("Height")?.trim().parse().ok()?,
    })
}
