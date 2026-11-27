//! A laser's LED dimmer, driven by the light's intensity, keeps a dimmer's rules
//! (#358, finding RIG-02 of #349).

mod common;

use common::{at, light, lights, mode};
use engine_core::{ADDRESSES, Field, Follows, Light, Universes};
use rig::channels::{Change, Ties};
use rig::compile;

/// JB Systems Lounge Laser, channel 6 "Dimmer LED": group Intensity, no preset.
#[test]
fn a_laser_dimmer_driven_by_intensity_follows_master_and_blackout() {
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
