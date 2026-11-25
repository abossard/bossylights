//! Pixels: each head of a bar, a panel or a strip is a light.

mod common;

use common::{fixture, lights, said};
use engine_core::{
    ChannelNumbers, Color, LightNumber, Lights, PatchCommand, Pixels, Refusal, Text, Universes,
};
use engine_core::{Command, OutputKind, OutputNumber, Project, ProjectEvent, RunNumbers};
use rig::writer::Writer;
use rig::{Patch, PatchEffect};
use std::net::{Ipv4Addr, SocketAddrV4};

fn text(text: &str) -> Text {
    Text::new(text).unwrap()
}

const fn at(universe: u16, address: u16) -> ChannelNumbers {
    ChannelNumbers { universe, address }
}

fn bar(mode: &str) -> PatchCommand {
    PatchCommand::Patch {
        name: text("Bar"),
        maker: text("XStatic"),
        model: text("X-240Bar RGB"),
        mode: text(mode),
        at: at(1, 1),
    }
}

fn strip(pixels: u16, color: Color, universe: u16, address: u16) -> PatchCommand {
    PatchCommand::PatchStrip {
        name: text("Strip"),
        pixels: Pixels::try_from(pixels).unwrap(),
        color,
        at: at(universe, address),
    }
}

/// The light of each writer in order, `-` for a wheel's slot: `0 1 1 1 -`.
fn owners(writers: &[Writer]) -> String {
    writers
        .iter()
        .map(|w| match w {
            Writer::Takes { light, .. } | Writer::Picks { light, .. } => light.index().to_string(),
            Writer::Fixed { .. } | Writer::ColourSlot { .. } | Writer::GoboSlot { .. } => {
                "-".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_fixture_with_heads_is_one_light_per_head_in_the_order_of_the_definition() {
    let library = ["XStatic-X-240Bar-RGB.qxf", "Generic-Generic-RGB.qxf"].map(fixture);
    let par = PatchCommand::Patch {
        name: text("PAR"),
        maker: text("Generic"),
        model: text("Generic RGB"),
        mode: text("RGB"),
        at: at(1, 1),
    };
    // (command, model and mode, lights, layout, light of each writer)
    let cases = [
        (
            // a master dimmer and a channel in no head: the bar is light 0
            bar("26 Channel"),
            "X-240Bar RGB, 26 Channel",
            9,
            Some((8, 1)),
            "0 1 1 1 2 2 2 3 3 3 4 4 4 5 5 5 6 6 6 7 7 7 8 8 8",
        ),
        (
            // the third head lists its channels 8 9 7, the fourth 12 10 11: each head is
            // a light by its place in the definition, not by its channels. The fixture's
            // grid is 8 by 1: no cell for each of 4 heads, so no layout
            bar("14 Channel"),
            "X-240Bar RGB, 14 Channel",
            5,
            None,
            "0 1 1 1 2 2 2 3 3 3 4 4 4",
        ),
        (par, "Generic RGB, RGB", 1, None, "0 0 0"),
        (
            // every channel in a head: no light for the strip itself
            strip(3, Color::Rgb, 1, 1),
            "RGB strip, 3 pixels",
            3,
            Some((3, 1)),
            "0 0 0 1 1 1 2 2 2",
        ),
        (
            strip(2, Color::Rgbw, 1, 1),
            "RGBW strip, 2 pixels",
            2,
            Some((2, 1)),
            "0 0 0 0 1 1 1 1",
        ),
    ];
    for (command, named, count, layout, expected) in cases {
        let empty = Patch::new(Universes::ONE, lights(), common::whole());
        let (patch, _) = empty.update(command, &library).unwrap();
        let patch = patch.unwrap();
        let row = patch.fixtures()[0];
        assert_eq!(format!("{}, {}", row.model, row.mode), named);
        assert_eq!(row.lights, count, "{named}");
        assert_eq!(row.layout.map(|l| (l.width, l.height)), layout, "{named}");
        let rigged = rig::compile(
            patch.mode(1).unwrap(),
            at(row.universe, row.address),
            LightNumber::new(row.first - 1, lights()).unwrap(),
            Universes::ONE,
            lights(),
        )
        .unwrap();
        assert_eq!(owners(&rigged.writers), expected, "{named}");
    }
}

#[test]
fn a_strip_is_written_like_a_par_without_a_dimmer() {
    // (colour, writers in words)
    let cases: [(Color, &[&str]); 2] = [
        (
            Color::Rgb,
            &[
                "37 light 0: red × intensity",
                "38 light 0: green × intensity",
                "39 light 0: blue × intensity",
                "40 light 1: red × intensity",
                "41 light 1: green × intensity",
                "42 light 1: blue × intensity",
            ],
        ),
        (
            Color::Rgbw,
            &[
                "37 light 0: red - shared × intensity",
                "38 light 0: green - shared × intensity",
                "39 light 0: blue - shared × intensity",
                "40 light 0: shared × intensity",
                "41 light 1: red - shared × intensity",
                "42 light 1: green - shared × intensity",
                "43 light 1: blue - shared × intensity",
                "44 light 1: shared × intensity",
            ],
        ),
    ];
    for (color, expected) in cases {
        let empty = Patch::new(Universes::ONE, lights(), common::whole());
        let (patch, effects) = empty.update(strip(2, color, 1, 37), &[]).unwrap();
        assert_eq!(effects[0], PatchEffect::Patched { fixture: 1 });
        let patch = patch.unwrap();
        let row = patch.fixtures()[0];
        let rigged = rig::compile(
            patch.mode(1).unwrap(),
            at(row.universe, row.address),
            LightNumber::new(row.first - 1, lights()).unwrap(),
            Universes::ONE,
            lights(),
        )
        .unwrap();
        let said: Vec<String> = said(&rigged);
        assert_eq!(said, expected, "{color:?}");
    }
}

#[test]
fn a_strip_that_does_not_fit_is_refused() {
    let few = Lights::new(300).unwrap();
    // (pixels, colour, universe, address, why not)
    let cases = [
        (301, Color::Rgbw, 1, 1, Refusal::NoLightsLeft),
        (60_000, Color::Rgb, 1, 1, Refusal::NoLightsLeft),
        (300, Color::Rgb, 1, 0, Refusal::NoSuchAddress),
        (
            1,
            Color::Rgb,
            2,
            1,
            Refusal::NoSuchUniverse { universes: 1 },
        ),
    ];
    for (pixels, color, universe, address, expected) in cases {
        let empty = Patch::new(Universes::ONE, few, common::whole());
        let refused = empty.update(strip(pixels, color, universe, address), &[]);
        assert_eq!(refused.err(), Some(expected), "{pixels} {color:?}");
    }
}

#[test]
fn a_fixture_that_runs_on_into_the_next_universe_overlaps_there() {
    let two = Universes::new(2).unwrap();
    // (patched first, then, refused or not)
    let cases = [
        // channels 1.511 1.512 2.1 meet 2.1 2.2 2.3
        ((1, 511), (2, 1), Err(Refusal::Overlaps { fixture: 1 })),
        ((1, 510), (2, 1), Ok(())),
        ((2, 1), (1, 511), Err(Refusal::Overlaps { fixture: 1 })),
    ];
    for (first, then, expected) in cases {
        let patch = Patch::new(two, lights(), common::whole());
        let (patch, _) = patch
            .update(strip(1, Color::Rgb, first.0, first.1), &[])
            .unwrap();
        let updated = patch
            .unwrap()
            .update(strip(1, Color::Rgb, then.0, then.1), &[])
            .map(|_| ());
        assert_eq!(updated, expected, "{first:?} then {then:?}");
    }
}

#[test]
fn a_fixture_over_more_than_one_universe_needs_one_output_that_streams_them_all() {
    let four = Universes::new(4).unwrap();
    let ddp = OutputKind::Ddp {
        address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4048),
        color: Color::Rgb,
    };
    let streaming = |runs: &[(u8, u16, u16)]| {
        runs.iter()
            .fold(Project::new(four), |project, &(n, first, count)| {
                let output = OutputNumber::try_from(n).unwrap();
                let set = ProjectEvent::Client(Command::SetOutput {
                    output,
                    kind: ddp,
                    universes: RunNumbers { first, count },
                    rate: None,
                });
                let project = project.update(set).unwrap().0;
                let opened = ProjectEvent::Opened { output, kind: ddp };
                project.update(opened).unwrap().0
            })
    };
    // strips: pixels, universe, address. Outputs: number, first universe, count
    type Strips = &'static [(u16, u16, u16)];
    type Outputs = &'static [(u8, u16, u16)];
    type Unstreamed = Option<(u16, Refusal)>;
    let not_streamed = |fixture, first, last| Some((fixture, Refusal::NotStreamed { first, last }));
    // (strips, outputs, unstreamed)
    let cases: [(Strips, Outputs, Unstreamed); 7] = [
        // 170 RGB pixels fill universe 1 to 510: one universe needs no output
        (&[(170, 1, 1)], &[], None),
        (&[(171, 1, 1)], &[], not_streamed(1, 1, 2)),
        (&[(171, 1, 1)], &[(1, 1, 2)], None),
        (
            &[(171, 1, 1)],
            &[(1, 1, 1), (2, 2, 1)],
            not_streamed(1, 1, 2),
        ),
        // output 1 sends universe 1 to 510: pixel 171 starts at 511
        (
            &[(171, 1, 1)],
            &[(1, 1, 1), (2, 1, 4)],
            Some((
                1,
                Refusal::NotSent {
                    output: 1,
                    universe: 1,
                    address: 511,
                },
            )),
        ),
        (
            &[(10, 1, 1), (400, 2, 1)],
            &[(1, 1, 2)],
            not_streamed(2, 2, 4),
        ),
        (&[(10, 1, 1), (400, 2, 1)], &[(1, 2, 3)], None),
    ];
    for (strips, outputs, expected) in cases {
        let patch = strips.iter().fold(
            Patch::new(four, lights(), common::whole()),
            |patch, &(pixels, universe, address)| {
                let command = strip(pixels, Color::Rgb, universe, address);
                patch.update(command, &[]).unwrap().0.unwrap()
            },
        );
        assert_eq!(
            patch.unstreamed(&streaming(outputs)),
            expected,
            "{strips:?} with {outputs:?}"
        );
    }
}
