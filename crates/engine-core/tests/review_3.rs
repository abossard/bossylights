use engine_core::{
    Bpm, Channels, Configured, EngineCommand, Field, Flags, Follows, Grabs, PlayId, PlayState,
    Priority, Rank, State, When, frame, update,
};
use std::time::Duration;

// Two looks launched on the same beat must give the first request the channel.
// Source: docs/design/who-wins.md, rule 3, "Among equals".
// Medium: two pads queued for the same beat can show the wrong scene.
#[test]
fn finding_3_01_first_request_wins_equal_start() {
    let mut grabs = Grabs::NONE;
    let mut state = State::new(Priority::RealTime);
    let mut apply = |command| state = update(state, &mut [], &mut [], &mut grabs, command);
    apply(EngineCommand::Feed {
        bpm: Bpm::new(120).unwrap(),
    });
    for (slot, ms) in [(0, 1100), (1, 1200)] {
        let at = Duration::from_millis(ms);
        apply(EngineCommand::Tick { now: at });
        apply(EngineCommand::Settle { at });
        apply(EngineCommand::Start {
            play: PlayId::new(slot, 1).unwrap(),
            look: slot as u16 + 1,
            rank: Rank::Show,
            when: When::Beat,
        });
    }
    let at = Duration::from_millis(1500);
    apply(EngineCommand::Tick { now: at });
    apply(EngineCommand::Settle { at });
    let first = state.play_at(0).unwrap();
    let second = state.play_at(1).unwrap();
    assert_eq!(
        (first.state, second.state),
        (PlayState::Playing, PlayState::Playing)
    );
    assert_eq!(first.start, second.start);

    let patched = [Configured {
        at: 0,
        rest: 0,
        dark: 0,
        flags: Flags::NONE,
        follows: Follows::NONE,
    }];
    let mut channels = Box::new(Channels::NEW);
    channels.configure(&[], &patched);
    frame(
        &state,
        &grabs,
        (&[], &[]),
        (&patched, &[]),
        &mut channels,
        |offers| {
            offers.play(0, Field::Byte(0), 0x2500);
            offers.play(1, Field::Byte(0), 0x9100);
        },
    );
    assert_eq!(
        channels.values()[0].0[0],
        37,
        "slot 1 must show the first request when the two starts coincide"
    );
}
