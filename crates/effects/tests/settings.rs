//! The setting metadata the engine publishes and the values it refuses. Expected rows from
//! `docs/design/effects.md`, sections 2 and 3.

use effects::{Effect, Kind, MEASURE, Refused, Rgb, Setting, Unit};

const RED: Rgb = Rgb {
    red: 65535,
    green: 0,
    blue: 0,
};
const DIRECTIONS: &[&str] = &["Right", "Left", "Down", "Up"];

#[test]
fn every_kind_publishes_its_settings() {
    let setting = |id, name, unit, min, max, default, choices| Setting {
        id,
        name,
        unit,
        min,
        max,
        default,
        choices,
    };
    let cases: [(Kind, &[Setting]); 4] = [
        (
            Kind::Wave,
            &[
                setting("spread", "Spread", Unit::Percent, 0, 100, 100, &[]),
                setting("direction", "Direction", Unit::Choice, 0, 3, 0, DIRECTIONS),
            ],
        ),
        (
            Kind::Stripes,
            &[
                setting("size", "Stripe width", Unit::Cells, 1, 64, 4, &[]),
                setting("direction", "Direction", Unit::Choice, 0, 3, 0, DIRECTIONS),
            ],
        ),
        (
            Kind::Plasma,
            &[setting("scale", "Scale", Unit::Times, 1, 8, 1, &[])],
        ),
        (
            Kind::Sparkle,
            &[
                setting("density", "Density", Unit::Percent, 1, 100, 10, &[]),
                setting("rate", "Rate", Unit::PerBeat, 1, 8, 4, &[]),
                setting("fade", "Fade", Unit::Choice, 0, 1, 1, &["Off", "On"]),
                setting("seed", "Seed", Unit::Seed, 0, 65535, 1, &[]),
                setting(
                    "background",
                    "Background",
                    Unit::Choice,
                    0,
                    1,
                    0,
                    &["Black", "Through"],
                ),
            ],
        ),
    ];
    assert_eq!(Kind::ALL.map(|kind| kind.settings()), cases.map(|c| c.1));
    assert_eq!(
        MEASURE,
        setting("measure", "Measure", Unit::Beats, 1, 32, 4, &[])
    );
}

#[test]
fn a_missing_value_takes_its_default() {
    let cases: [(Kind, &[u16]); 4] = [
        (Kind::Wave, &[100, 0]),
        (Kind::Stripes, &[4, 0]),
        (Kind::Plasma, &[1]),
        (Kind::Sparkle, &[10, 4, 1, 1, 0]),
    ];
    for (kind, defaults) in cases {
        for given in 0..defaults.len() {
            assert_eq!(
                Effect::new(kind, 4, &[RED], &defaults[..given]),
                Effect::new(kind, 4, &[RED], defaults),
                "{kind:?} with {given} values"
            );
        }
    }
}

#[test]
fn a_wrong_value_is_refused() {
    let out = |setting, min, max| Err(Refused::OutOfRange { setting, min, max });
    let five = &[RED; 5];
    // (kind, measure, colours, values, answer)
    type Case<'a> = (Kind, u16, &'a [Rgb], &'a [u16], Result<(), Refused>);
    let cases: [Case; 30] = [
        (Kind::Wave, 0, &[RED], &[], out("measure", 1, 32)),
        (Kind::Wave, 1, &[RED], &[], Ok(())),
        (Kind::Wave, 32, &[RED], &[], Ok(())),
        (Kind::Wave, 33, &[RED], &[], out("measure", 1, 32)),
        (Kind::Wave, 4, &[], &[], Err(Refused::EmptyPalette)),
        (Kind::Wave, 4, &[RED; 4], &[], Ok(())),
        (Kind::Wave, 4, five, &[], out("colours", 1, 4)),
        (Kind::Wave, 4, &[RED], &[100, 3], Ok(())),
        (Kind::Wave, 4, &[RED], &[101], out("spread", 0, 100)),
        (Kind::Wave, 4, &[RED], &[0, 4], out("direction", 0, 3)),
        (
            Kind::Wave,
            4,
            &[RED],
            &[0, 0, 0],
            Err(Refused::NoSuchSetting),
        ),
        (Kind::Stripes, 4, &[RED], &[1, 3], Ok(())),
        (Kind::Stripes, 4, &[RED], &[64], Ok(())),
        (Kind::Stripes, 4, &[RED], &[0], out("size", 1, 64)),
        (Kind::Stripes, 4, &[RED], &[65], out("size", 1, 64)),
        (Kind::Stripes, 4, &[RED], &[4, 4], out("direction", 0, 3)),
        (Kind::Plasma, 4, &[RED], &[1], Ok(())),
        (Kind::Plasma, 4, &[RED], &[8], Ok(())),
        (Kind::Plasma, 4, &[RED], &[0], out("scale", 1, 8)),
        (Kind::Plasma, 4, &[RED], &[9], out("scale", 1, 8)),
        (
            Kind::Plasma,
            4,
            &[RED],
            &[1, 0],
            Err(Refused::NoSuchSetting),
        ),
        (Kind::Sparkle, 4, &[RED], &[1, 1, 0, 0, 0], Ok(())),
        (Kind::Sparkle, 4, &[RED], &[100, 8, 1, 65535, 1], Ok(())),
        (Kind::Sparkle, 4, &[RED], &[0], out("density", 1, 100)),
        (Kind::Sparkle, 4, &[RED], &[101], out("density", 1, 100)),
        (Kind::Sparkle, 4, &[RED], &[10, 0], out("rate", 1, 8)),
        (Kind::Sparkle, 4, &[RED], &[10, 9], out("rate", 1, 8)),
        (Kind::Sparkle, 4, &[RED], &[10, 4, 2], out("fade", 0, 1)),
        (
            Kind::Sparkle,
            4,
            &[RED],
            &[10, 4, 1, 1, 2],
            out("background", 0, 1),
        ),
        (
            Kind::Sparkle,
            4,
            &[RED],
            &[10, 4, 1, 1, 0, 0],
            Err(Refused::NoSuchSetting),
        ),
    ];
    for (kind, measure, colours, values, answer) in cases {
        assert_eq!(
            Effect::new(kind, measure, colours, values).map(|_| ()),
            answer,
            "{kind:?}, measure {measure}, {} colours, {values:?}",
            colours.len()
        );
    }
}
