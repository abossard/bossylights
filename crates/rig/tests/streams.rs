//! Every channel of a fixture an output streams is one it sends (#155). A DDP output sends
//! whole pixels only: RGB on universes 1 and 2 sends 1023 of their 1024 channels.

mod common;

use common::{lights, whole};
use engine_core::{
    ChannelNumbers, Color, Command, Levels, Light, OutputKind, OutputNumber, PatchCommand, Pixels,
    Project, ProjectEvent, Refusal, Rgb, RunNumbers, Text, Universes, resolve,
};
use rig::Patch;
use std::net::{Ipv4Addr, SocketAddrV4};

const fn ddp(color: Color) -> OutputKind {
    OutputKind::Ddp {
        address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
        color,
    }
}

/// How output 1 streams universes 1 and 2.
#[derive(Clone, Copy, Debug)]
enum Streams {
    /// Open, of this kind.
    Open(OutputKind),
    /// Open as the first, being opened as the second: both count.
    Opening(OutputKind, OutputKind),
    /// A device a test brought: every channel.
    Attached,
}

fn project(streams: Streams) -> Project {
    let two = Universes::new(2).unwrap();
    let output = OutputNumber::try_from(1).unwrap();
    let run = RunNumbers { first: 1, count: 2 };
    let set = |kind| {
        ProjectEvent::Client(Command::SetOutput {
            output,
            kind,
            universes: run,
            rate: None,
        })
    };
    let events = match streams {
        Streams::Open(kind) => vec![set(kind), ProjectEvent::Opened { output, kind }],
        Streams::Opening(kind, next) => {
            vec![set(kind), ProjectEvent::Opened { output, kind }, set(next)]
        }
        Streams::Attached => vec![ProjectEvent::Attached {
            output,
            universes: run.check(two).unwrap(),
        }],
    };
    events
        .into_iter()
        .fold(Project::new(two), |project, event| {
            project.update(event).unwrap().0
        })
}

const fn not_sent(universe: u16, address: u16) -> Option<(u16, Refusal)> {
    Some((
        1,
        Refusal::NotSent {
            output: 1,
            universe,
            address,
        },
    ))
}

#[test]
fn a_fixture_up_to_the_last_channel_its_output_sends_is_kept_one_over_is_refused() {
    let (rgb, rgbw) = (ddp(Color::Rgb), ddp(Color::Rgbw));
    // (name, streams, pixels, colour, at universe, address, channels sent, unstreamed)
    let cases = [
        (
            "341 RGB from 1:1 end at 2:511, the last sent",
            Streams::Open(rgb),
            341,
            Color::Rgb,
            1,
            1,
            1023,
            None,
        ),
        (
            "341 RGB from 1:2 end at 2:512, one over",
            Streams::Open(rgb),
            341,
            Color::Rgb,
            1,
            2,
            1023,
            not_sent(2, 512),
        ),
        (
            "171 RGB from 1:512, the review's case",
            Streams::Open(rgb),
            171,
            Color::Rgb,
            1,
            512,
            1023,
            not_sent(2, 512),
        ),
        (
            "170 RGB from 1:512 end at 2:511",
            Streams::Open(rgb),
            170,
            Color::Rgb,
            1,
            512,
            1023,
            None,
        ),
        (
            "1 RGB at 2:509, the last pixel sent",
            Streams::Open(rgb),
            1,
            Color::Rgb,
            2,
            509,
            1023,
            None,
        ),
        (
            "1 RGB at 2:510, in one universe",
            Streams::Open(rgb),
            1,
            Color::Rgb,
            2,
            510,
            1023,
            not_sent(2, 512),
        ),
        (
            "1 RGB at 2:510, an RGBW output sends all",
            Streams::Open(rgbw),
            1,
            Color::Rgb,
            2,
            510,
            1024,
            None,
        ),
        (
            "1 RGBW at 2:509 on RGBW",
            Streams::Open(rgbw),
            1,
            Color::Rgbw,
            2,
            509,
            1024,
            None,
        ),
        (
            "1 RGB at 2:510, RGBW opening as RGB",
            Streams::Opening(rgbw, rgb),
            1,
            Color::Rgb,
            2,
            510,
            1023,
            not_sent(2, 512),
        ),
        (
            "1 RGB at 2:510, RGB opening as RGBW",
            Streams::Opening(rgb, rgbw),
            1,
            Color::Rgb,
            2,
            510,
            1023,
            not_sent(2, 512),
        ),
        (
            "1 RGB at 2:510, a test's device sends all",
            Streams::Attached,
            1,
            Color::Rgb,
            2,
            510,
            1024,
            None,
        ),
    ];
    for (name, streams, pixels, color, universe, address, sent, expected) in cases {
        let command = PatchCommand::PatchStrip {
            name: Text::new("Strip").unwrap(),
            pixels: Pixels::try_from(pixels).unwrap(),
            color,
            at: ChannelNumbers { universe, address },
        };
        let two = Universes::new(2).unwrap();
        let patch = Patch::new(two, lights(), whole())
            .update(command, &[])
            .unwrap()
            .0
            .unwrap();
        assert_eq!(patch.unstreamed(&project(streams)), expected, "{name}");
        if expected.is_some() {
            continue;
        }
        let white = Light {
            intensity: Some(u16::MAX),
            colour: Some(Rgb {
                red: u16::MAX,
                green: u16::MAX,
                blue: u16::MAX,
            }),
        };
        let mut out = [Levels::ZERO; 2];
        resolve(&vec![white; usize::from(pixels)], patch.writers(), &mut out);
        let bytes: Vec<u8> = out.iter().flat_map(|u| u.0).collect();
        let lit = bytes.iter().rposition(|&b| b != 0).unwrap();
        assert!(lit < sent, "{name}: channel {} lit, {sent} sent", lit + 1);
    }
}
