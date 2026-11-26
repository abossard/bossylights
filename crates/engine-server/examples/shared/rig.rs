//! The benchmark rig: each workload's strips, groups and looks, as `send_to_frame` and
//! `delay_agreement` set them up.

use engine::{
    ChannelNumbers, Color, Command, Light, Member, MemberList, Members, Palette, Pattern, Rank,
    Refusal, Rgb, Sender, Sequence, SequenceItem, Swatches, Target, Text, Use, WaveTarget, When,
};
use engine_core::{Pixels, Step};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Workload {
    Show,
    Biggest,
    Burst,
    /// The show's rig patched and its outputs sending, nothing playing.
    Idle,
}

impl Workload {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Biggest => "biggest",
            Self::Burst => "burst",
            Self::Idle => "idle",
        }
    }

    /// Whether its looks play. The idle rig plays nothing and gets no group edits.
    pub(crate) fn plays(self) -> bool {
        !matches!(self, Self::Idle)
    }

    /// The normal show runs on 10,000 pixels. In the biggest show all 10,922 whole
    /// RGB pixels fit, and the three target sizes jointly fill the visit budget.
    pub(crate) fn strips(self) -> (u16, u16, u16) {
        match self {
            Self::Show | Self::Idle => (1024, 256, 10_000 - 1024 - 256),
            Self::Biggest | Self::Burst => (77, 198, 10_922 - 77 - 198),
        }
    }

    pub(crate) fn waves(self) -> usize {
        match self {
            Self::Show => 1,
            Self::Biggest | Self::Burst => engine_core::PLAYS - 2,
            Self::Idle => 0,
        }
    }
}

const BACKGROUND: Rgb = Rgb {
    red: 12000,
    green: 16000,
    blue: 32000,
};
pub(crate) const RED: Rgb = Rgb {
    red: u16::MAX,
    ..Rgb::BLACK
};
const BLUE: Rgb = Rgb {
    blue: u16::MAX,
    ..Rgb::BLACK
};
const ALL: &str = "All strips";
const WAVE: &str = "Wave strips";

fn text(value: &str) -> Text {
    Text::new(value).expect("short benchmark names")
}

fn group(name: &str, members: &[Member]) -> Command {
    Command::SetGroup {
        name: text(name),
        members: MemberList::Fits(Members::new(members).expect("small benchmark group")),
    }
}

pub(crate) fn wave_group(reverse: bool) -> Command {
    let order = if reverse { [2, 1] } else { [1, 2] };
    group(WAVE, &order.map(|fixture| Member::Fixture { fixture }))
}

pub(crate) fn colour(target: Target, rgb: Rgb) -> Command {
    Command::SetColour {
        target,
        red: rgb.red,
        green: rgb.green,
        blue: rgb.blue,
    }
}

pub(crate) fn send(sender: &mut Sender, command: Command) -> Result<(), String> {
    sender
        .send(command)
        .map_err(|r| format!("{command:?}: {r:?}"))
}

fn step(look: u16, name: &str, target: Target, rgb: Rgb, intensity: Option<u16>) -> Command {
    Command::PutStep {
        look,
        name: text(name),
        step: Step {
            target,
            values: Light {
                intensity,
                colour: Some(rgb),
                ..Light::DARK
            },
        }
        .into(),
    }
}

/// Attach the streaming outputs before calling this, so all spanning strips can be admitted.
pub(crate) fn prepare(sender: &mut Sender, workload: Workload) -> Result<(), String> {
    prepare_strips(sender, workload, workload.strips())
}

pub(crate) fn prepare_strips(
    sender: &mut Sender,
    workload: Workload,
    (wave, sequence, background): (u16, u16, u16),
) -> Result<(), String> {
    let mut channels: u16 = 0;
    for count in [wave / 2, wave - wave / 2, sequence, background] {
        send(
            sender,
            Command::PatchStrip {
                name: text("RGB strip"),
                pixels: Pixels::try_from(count)?,
                color: Color::Rgb,
                at: ChannelNumbers {
                    universe: channels / 512 + 1,
                    address: channels % 512 + 1,
                },
            },
        )?;
        channels += count * 3;
    }
    let all = Target::Group { group: text(ALL) };
    send(
        sender,
        group(
            ALL,
            &[1, 2, 3, 4].map(|fixture| Member::Fixture { fixture }),
        ),
    )?;
    send(sender, wave_group(false))?;
    let probe = "two lights";
    if matches!(workload, Workload::Biggest | Workload::Burst) {
        send(
            sender,
            group(
                probe,
                &[Member::Light { light: 1 }, Member::Light { light: 2 }],
            ),
        )?;
    }
    let target = Target::Fixture { fixture: 3 };
    send(
        sender,
        step(1, "Background", all, BACKGROUND, Some(u16::MAX)),
    )?;
    send(
        sender,
        step(
            2,
            "Green",
            target,
            Rgb {
                green: u16::MAX,
                ..Rgb::BLACK
            },
            None,
        ),
    )?;
    send(
        sender,
        step(
            3,
            "Amber",
            target,
            Rgb {
                red: u16::MAX,
                green: 32768,
                blue: 0,
            },
            None,
        ),
    )?;
    let items = [2, 3].map(|look| SequenceItem {
        look,
        transition: engine_core::Percent::MOST,
        ..SequenceItem::default()
    });
    send(
        sender,
        Command::PutSequence {
            look: 4,
            name: text("Crossfade"),
            sequence: Sequence::new(&items).ok_or("sequence length")?,
        },
    )?;
    let palette = Palette::Inline(Swatches::new(&[RED, BLUE]).ok_or("palette length")?);
    let waves = workload.waves() as u16;
    for n in 0..waves {
        send(
            sender,
            Command::PutPattern {
                look: 5 + n,
                name: text("Wave"),
                pattern: Pattern::Wave {
                    target: WaveTarget::Group { group: text(WAVE) },
                    period: engine_core::Period::new(if n == 0 { 4 } else { n as u8 + 1 })
                        .ok_or("a period of 1 to 32 beats")?,
                    palette,
                },
            },
        )?;
    }
    // Measure what the compiler actually reserved, including an event sample.
    // Start the background before the other Autopilot roots, so their colour wins.
    let mut visits = 0;
    let roots = [(1, Rank::Autopilot, 0), (4, Rank::Show, 0)]
        .into_iter()
        .chain((5..5 + waves).map(|look| (look, Rank::Autopilot, 100)))
        .filter(|_| workload.plays());
    for (look, rank, spread) in roots {
        visits += 2 * sender
            .looks()
            .plan(look)
            .map_err(|r| format!("{r:?}"))?
            .work();
        send(
            sender,
            Command::StartLook {
                look,
                when: When::Now,
                rank,
                usage: Use {
                    spread: engine_core::Percent::new(spread).ok_or("a spread of 0 to 100")?,
                    ..Use::default()
                },
            },
        )?;
    }
    if matches!(workload, Workload::Biggest | Workload::Burst) {
        // With all variable visits booked, the desk guarantees a one-light grab.
        // Prove this is the admitted edge before measuring it.
        let refused = sender.send(colour(Target::Group { group: text(probe) }, RED));
        check_worst(visits, refused)?;
    }
    eprintln!(
        "{} RGB pixels, {} roots ({waves} Waves on {wave} pixels, sequence on {sequence}, background), {visits} reserved visits",
        channels / 3,
        if workload.plays() { waves + 2 } else { 0 }
    );
    Ok(())
}

pub(crate) fn check_worst(visits: usize, probe: Result<(), engine::Unsent>) -> Result<(), String> {
    if visits != 65_536 || probe != Err(engine::Unsent::Refused(Refusal::WorkLimit)) {
        return Err(format!(
            "expected a full work budget, booked {visits} and two-light grab gave {probe:?}"
        ));
    }
    Ok(())
}
