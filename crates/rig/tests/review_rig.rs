//! Review tests for aspect 8 (`crates/rig`): channel lowering, modifiers, and writers.

mod common;

use common::{at, light, lights, mode};
use engine_core::{ADDRESSES, Field, Follows, Light, Property, Refusal, Universes};
use fixture_library::{Mode, parse};
use rig::channels::{Change, Ties};
use rig::{Unfit, compile};

fn first_mode(name: &str, qxf: &[u8]) -> Mode {
    parse(name, qxf).unwrap().modes.into_iter().next().unwrap()
}

fn byte_written(mode: &Mode, light_value: &Light, channel_index: u32) -> u8 {
    let rigged = compile(mode, at(1), light(0), Universes::ONE, lights()).unwrap();
    let offered = rigged.fields[0]
        .offers(light_value)
        .find_map(|(field, word)| (field == Field::Byte(channel_index)).then_some(word))
        .expect("writer offered on channel");
    (offered >> 8) as u8
}

/// Claim: An inverted dimmer (`Preset="BigToSmall"`, `255 = dark`, `0 = full`) is lowered with `dark = 255` in `rig::channels`, yet `rig::compile` binds `Take::INTENSITY` with `Sense::Rising` (`0..=255`), inverting full bright and blackout.
/// Source: `crates/fixture-library/src/lib.rs` (`Direction::Down`: "At 255 the channel is at its start; at 0, its end") and `crates/rig/src/channels.rs` (`dark` and `Sense::Falling` for `Direction::Down` dimmers).
/// Severity: high.
/// Show impact: Setting an inverted-dimmer fixture to 100% intensity drives its DMX byte to `255` (its dark value, lights off), while setting intensity to 0% drives its DMX byte to `0` (blinding full output).
#[test]
fn finding_rig_01_inverted_dimmer_bright_and_dark_swapped() {
    let inverted_dimmer_qxf = br#"<?xml version="1.0" encoding="UTF-8"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Test</Manufacturer>
 <Model>Inverted Dimmer</Model>
 <Type>Dimmer</Type>
 <Channel Name="Dimmer" Default="255">
  <Group Byte="0">Intensity</Group>
  <Capability Min="0" Max="255" Preset="BigToSmall">Dimmer 100% to 0%</Capability>
 </Channel>
 <Mode Name="1ch">
  <Channel Number="0">Dimmer</Channel>
 </Mode>
</FixtureDefinition>"#;

    let mode = first_mode("inverted-dimmer.qxf", inverted_dimmer_qxf);
    let space = u32::try_from(ADDRESSES).unwrap();
    let lowered = rig::channels::lower(&mode, 0, space).unwrap();
    assert_eq!(
        lowered.channels[0].dark, 255,
        "dark of an inverted dimmer is 255"
    );

    let full = Light {
        intensity: Some(u16::MAX),
        ..Light::DARK
    };
    let zero = Light {
        intensity: Some(0),
        ..Light::DARK
    };

    assert_eq!(
        byte_written(&mode, &full, 0),
        0,
        "full intensity (u16::MAX) on a falling dimmer (dark = 255) must write DMX 0, not dark 255"
    );
    assert_eq!(
        byte_written(&mode, &zero, 0),
        255,
        "zero intensity (0) on a falling dimmer (dark = 255) must write dark DMX 255, not full-bright 0"
    );
}

/// Claim: On `<Type>Laser</Type>` fixtures that carry an intensity dimmer channel (such as `JB-Systems-Lounge-Laser-DMX.qxf`, ch 6 `"Dimmer LED"`), `rig::compile` drives the channel from `Light.intensity` while `rig::channels` forces `Follows::NONE`, `Ties::Latest`, and `Change::Jump`, so neither Blackout nor Grand Master = 0 brings the channel to dark.
/// Source: `docs/design/channel-model.md`, "What follows Master and Blackout": intensity/brightness channels follow Master and Blackout, fade, and break ties by Highest.
/// Severity: high.
/// Show impact: Hitting Blackout or pulling Grand Master to zero leaves the LED wash dimmer of a hybrid Laser/LED fixture at full output (`255`) on stage.
#[test]
fn finding_rig_02_laser_dimmer_ignores_blackout_and_master() {
    let lounge = mode("JB-Systems-Lounge-Laser-DMX.qxf", "7-Channel");
    assert_eq!(lounge.channels[5].name, "Dimmer LED");

    let space = u32::try_from(ADDRESSES).unwrap();
    let lowered = rig::channels::lower(&lounge, 0, space).unwrap();
    let rigged = compile(&lounge, at(1), light(0), Universes::ONE, lights()).unwrap();

    let full = Light {
        intensity: Some(u16::MAX),
        ..Light::DARK
    };
    let driven = rigged.fields[0]
        .offers(&full)
        .find_map(|(field, word)| (field == Field::Byte(5)).then_some((word >> 8) as u8));
    assert_eq!(
        driven,
        Some(255),
        "rig::compile drives ch 6 from Light.intensity"
    );

    assert_eq!(
        lowered.channels[5].follows,
        Follows::MASTER.with(Follows::BLACKOUT),
        "Dimmer LED is driven by Light.intensity yet follows neither Master nor Blackout"
    );
    assert_eq!(lowered.channels[5].ties, Ties::Highest);
    assert_eq!(lowered.channels[5].change, Change::Fade);
}

/// Claim: On a 16-bit RGB fixture whose colour-macro bypass range names all six RGB channels (`"All LEDs Set via Channels 1-6"`), `names_the_mix` checks `1..=6` against coarse-only `mixers` (`[0, 2, 4]`), fails on fine channels `1, 3, 5`, and refuses `Property::Colour` (`Unfit::Refused(Refusal::Unsupported { property: Property::Colour, .. })`).
/// Source: `docs/design/channel-model.md` and `AGENTS.md`: "coarse and fine are two channels that move as one 16-bit value. Every control comes from fixture metadata: a colour picker where channels form a colour."
/// Severity: medium.
/// Show impact: 16-bit RGB fixtures whose macro channel bypasses `"Channels 1-6"` cannot even be patched (`compile` fails with `Refusal::Unsupported { property: Property::Colour }`).
#[test]
fn finding_rig_03_sixteen_bit_rgb_macro_refuses_colour() {
    let qxf = br##"<?xml version="1.0" encoding="UTF-8"?>
<FixtureDefinition xmlns="http://www.qlcplus.org/FixtureDefinition">
 <Manufacturer>Test</Manufacturer>
 <Model>16-bit RGB With Macro</Model>
 <Type>Color Changer</Type>
 <Channel Name="Red" Preset="IntensityRed"/>
 <Channel Name="Red Fine" Preset="IntensityRedFine"/>
 <Channel Name="Green" Preset="IntensityGreen"/>
 <Channel Name="Green Fine" Preset="IntensityGreenFine"/>
 <Channel Name="Blue" Preset="IntensityBlue"/>
 <Channel Name="Blue Fine" Preset="IntensityBlueFine"/>
 <Channel Name="Color Macro">
  <Group Byte="0">Colour</Group>
  <Capability Min="0" Max="15">All LEDs Set via Channels 1-6</Capability>
  <Capability Min="16" Max="255" Preset="ColorMacro" Res1="#ff0000">Red Macro</Capability>
 </Channel>
 <Mode Name="7-Channel">
  <Channel Number="0">Red</Channel>
  <Channel Number="1">Red Fine</Channel>
  <Channel Number="2">Green</Channel>
  <Channel Number="3">Green Fine</Channel>
  <Channel Number="4">Blue</Channel>
  <Channel Number="5">Blue Fine</Channel>
  <Channel Number="6">Color Macro</Channel>
 </Mode>
</FixtureDefinition>"##;

    let mode = first_mode("16bit-rgb-macro.qxf", qxf);
    let rigged = compile(&mode, at(1), light(0), Universes::ONE, lights());
    assert!(
        !matches!(
            rigged,
            Err(Unfit::Refused(Refusal::Unsupported {
                property: Property::Colour,
                ..
            }))
        ),
        "16-bit RGB with 'All LEDs Set via Channels 1-6' bypass must compile, got {rigged:?}"
    );
}
