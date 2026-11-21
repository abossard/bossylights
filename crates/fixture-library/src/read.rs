//! Bytes of a QLC+ `.qxf` file to a `Fixture`. All of the XML walking lives here.

use crate::capability::{self, Said};
use crate::error::{Error, LeftOut, Problem, Warning};
use crate::function::{colour_of, direction_of, function_of};
use crate::model::{Byte, Channel, Declared, Degrees, Dimming, Fixture, Kind, Layout, Mode, Range};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use roxmltree::{Document, Node, ParsingOptions};

/// The largest real definition is 133 kB: this is about seven times that.
pub const MOST_BYTES: usize = 1024 * 1024;

/// The line of the `[` that opens declarations in the DOCTYPE of the prolog, when there is one.
/// Only the prolog is walked (declaration, comments, processing instructions, white space), so text
/// inside a comment or an instruction is never taken for a DOCTYPE. Quoted text in the DOCTYPE is skipped.
fn declarations_at(text: &str) -> Option<u32> {
    let mut at = 0;
    loop {
        at += text[at..].len() - text[at..].trim_start().len();
        let rest = &text[at..];
        let skip = if rest.starts_with("<?") {
            "?>"
        } else if rest.starts_with("<!--") {
            "-->"
        } else {
            break;
        };
        at += rest.find(skip)? + skip.len();
    }
    if !text[at..].starts_with("<!DOCTYPE") {
        return None;
    }
    let mut quote = None;
    for (i, c) in text[at..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '[') => return Some(1 + text[..at + i].matches('\n').count() as u32),
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
    read(&document).map_err(|(node, problem)| failed(Some(line_of(node)), problem))
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
    let (mut modes, mut warnings) = (Vec::new(), Vec::new());
    for node in root.children().filter(|n| n.has_tag_name("Mode")) {
        modes.push(mode(node, root, &channels, &kind, &mut warnings)?);
    }
    Ok(Fixture {
        maker,
        model,
        kind,
        modes,
        warnings,
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
    let preset = node.attribute("Preset");
    let (function, byte) = function_of(preset, group, &name);
    let colour = node
        .children()
        .find(|n| n.has_tag_name("Colour"))
        .and_then(|n| n.text())
        .and_then(|text| colour_of(text.trim()));
    let direction = preset.and_then(direction_of);
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
        declared: Declared {
            preset: preset.map(String::from),
            group: group.map(|(name, _)| String::from(name)),
            colour,
            ..Declared::default()
        },
        byte,
        default,
        direction,
        ranges,
    })
}

fn range<'a>(node: Node<'a, 'a>) -> Result<Range, Failure<'a>> {
    let (min, max): (u8, u8) = (number(node, "Min")?, number(node, "Max")?);
    if min > max {
        return Err((node, Problem::BackwardsRange { min, max }));
    }
    let label = node.text().unwrap_or_default().trim().to_string();
    let does = capability::does(&Said {
        preset: node.attribute("Preset"),
        first: node.attribute("Res1"),
        second: node.attribute("Res2"),
        picture: node.attribute("Res"),
    });
    Ok(Range {
        min,
        max,
        label,
        does,
    })
}

/// A mode: its channels by number, its pixels (`Head`s) and its layout. What does not fit
/// is left out and said in `warnings`.
fn mode<'a>(
    node: Node<'a, 'a>,
    root: Node<'a, 'a>,
    defined: &[Channel],
    kind: &Kind,
    warnings: &mut Vec<Warning>,
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
    let mut channels: Vec<_> = by_number.into_values().cloned().collect();
    let warn = |at: Node, left_out| Warning {
        mode: name.clone(),
        line: line_of(at),
        left_out,
    };
    let mut pixels: Vec<Vec<u16>> = Vec::new();
    for (head, at) in node
        .children()
        .filter(|n| n.has_tag_name("Head"))
        .enumerate()
    {
        let numbers = at
            .children()
            .filter(|n| n.has_tag_name("Channel"))
            .map(|n| head_channel(n).map(|number| (n, number)))
            .collect::<Result<Vec<_>, _>>()?;
        match numbers
            .iter()
            .find(|(_, number)| usize::from(*number) >= channels.len())
        {
            Some((n, channel)) => warnings.push(warn(
                *n,
                LeftOut::Head {
                    head,
                    channel: *channel,
                    channels: channels.len(),
                },
            )),
            None => pixels.push(numbers.into_iter().map(|(_, number)| number).collect()),
        }
    }
    set_dimming(kind, &mut channels, &pixels);
    let found = layout_in(node).or_else(|| {
        root.children()
            .find(|n| n.has_tag_name("Physical"))
            .and_then(layout_in)
    });
    // A layout must have a cell for every head. 244 modes of the QLC+ library give one
    // that does not (most share the fixture's grid with a mode of fewer heads).
    let layout = match found {
        Some((layout, at))
            if !pixels.is_empty()
                && usize::from(layout.width) * usize::from(layout.height) != pixels.len() =>
        {
            warnings.push(warn(
                at,
                LeftOut::Layout {
                    layout,
                    heads: pixels.len(),
                },
            ));
            None
        }
        Some((layout, _)) => Some(layout),
        None => None,
    };
    Ok(Mode {
        name,
        channels,
        pixels,
        layout,
        degrees: degrees_of(node, root)?,
    })
}

/// On fog fixtures a dimmer controls light when its head declares an emitter.
/// Without heads, the mode supplies the emitter declaration.
fn set_dimming(kind: &Kind, channels: &mut [Channel], heads: &[Vec<u16>]) {
    if *kind == Kind::Dimmer {
        for channel in channels {
            channel.declared.dimming = Dimming::DimmerPack;
        }
        return;
    }
    if !matches!(kind, Kind::Smoke | Kind::Hazer) {
        return;
    }
    let emits = |channel: &Channel| channel.declared.emitter_colour().is_some();
    if heads.is_empty() && channels.iter().any(emits) {
        return;
    }
    for channel in channels.iter_mut() {
        channel.declared.dimming = Dimming::Fog;
    }
    for head in heads {
        if head
            .iter()
            .filter_map(|&n| channels.get(usize::from(n)))
            .any(emits)
        {
            for &n in head {
                if let Some(channel) = channels.get_mut(usize::from(n)) {
                    channel.declared.dimming = Dimming::Preset;
                }
            }
        }
    }
}

/// A channel number of a head.
fn head_channel<'a>(node: Node<'a, 'a>) -> Result<u16, Failure<'a>> {
    let text = node.text().unwrap_or_default();
    text.trim().parse().map_err(|_| {
        (
            node,
            Problem::BadNumber {
                what: "Head channel",
                text: text.to_string(),
            },
        )
    })
}

/// The line of a node in its file, counted from 1.
fn line_of(node: Node) -> u32 {
    node.document().text_pos_at(node.range().start).row
}

/// How far the head pans and tilts: the `Focus` under the mode, else under the fixture.
fn degrees_of<'a>(mode: Node<'a, 'a>, root: Node<'a, 'a>) -> Result<Option<Degrees>, Failure<'a>> {
    let focus = |node: Node<'a, 'a>| node.descendants().find(|n| n.has_tag_name("Focus"));
    let Some(at) = focus(mode).or_else(|| {
        root.children()
            .find(|n| n.has_tag_name("Physical"))
            .and_then(focus)
    }) else {
        return Ok(None);
    };
    let axis = |what| match at.attribute(what) {
        Some(_) => number(at, what),
        None => Ok(0),
    };
    let degrees = Degrees {
        pan: axis("PanMax")?,
        tilt: axis("TiltMax")?,
    };
    Ok((degrees.pan, degrees.tilt).ne(&(0, 0)).then_some(degrees))
}

/// The `Layout` under a mode or under the fixture's own `Physical`, and its node.
fn layout_in<'a>(node: Node<'a, 'a>) -> Option<(Layout, Node<'a, 'a>)> {
    let at = node.descendants().find(|n| n.has_tag_name("Layout"))?;
    let layout = Layout {
        width: at.attribute("Width")?.trim().parse().ok()?,
        height: at.attribute("Height")?.trim().parse().ok()?,
    };
    Some((layout, at))
}
