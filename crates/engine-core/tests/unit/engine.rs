use super::*;

/// No grab: a fresh table for a command that changes none.
fn no_grabs() -> Grabs {
    Grabs::NONE
}

const CALM: State = State {
    frame: 7,
    ..State::new(Priority::RealTime)
};
const DARK: State = State {
    blackout: true,
    ..CALM
};

/// Two universes, with these (universe, address, level) set; counted from 1.
fn held(levels: &[(u16, u16, u8)]) -> [Levels; 2] {
    let mut universes = [Levels::ZERO; 2];
    for (universe, address, level) in levels {
        universes[usize::from(*universe - 1)].0[usize::from(*address - 1)] = *level;
    }
    universes
}

/// The raw channels of two universes, these (universe, address, level) held.
fn raw(levels: &[(u16, u16, u8)]) -> ([Levels; 2], [ChannelSet; 2]) {
    let mut holds = [ChannelSet::NONE; 2];
    for (universe, address, _) in levels {
        channel(*universe, *address).mark(&mut holds, true);
    }
    (held(levels), holds)
}

fn channel(universe: u16, address: u16) -> Channel {
    ChannelNumbers { universe, address }
        .check(Universes::new(2).unwrap())
        .unwrap()
}

fn tick(ms: u64) -> EngineCommand {
    EngineCommand::Tick {
        now: Duration::from_millis(ms),
    }
}

/// One wake at `now`: the commands sent by then, each settled at the time it was
/// sent, then the frame's time, as the tick applies them.
fn wake(state: State, now: u64, sent: &[(u64, EngineCommand)]) -> State {
    let apply = |state, command| update(state, &mut [], &mut [], &mut no_grabs(), command);
    let mut state = apply(state, tick(now));
    for (at, command) in sent {
        let at = Duration::from_millis(*at);
        state = apply(state, EngineCommand::Settle { at });
        state = apply(state, *command);
    }
    apply(
        state,
        EngineCommand::Settle {
            at: Duration::from_millis(now),
        },
    )
}

/// An engine with the feed at 120 beats a minute: beat n at n * 500 ms.
fn at_120() -> State {
    wake(
        CALM,
        0,
        &[(
            0,
            EngineCommand::Feed {
                bpm: Bpm::new(120).unwrap(),
            },
        )],
    )
}

/// A count of the clock is `beats`, to a millionth of a beat.
fn close(count: impl Into<BeatTime>, want: f64) -> bool {
    (count.into().beats() - want).abs() < 1e-6
}

fn play(slot: usize) -> PlayId {
    PlayId::new(slot, 1).unwrap()
}

#[test]
fn review_a_waiting_play_keeps_its_crossing_when_the_same_wake_passes_the_lapse() {
    let ms = Duration::from_millis;
    // At 512 ms per beat the tapped grid would cross at 2148 ms. The
    // release at 2100 ms + 1 ns moves that crossing to 2160 ms.
    for (tap_period, crossing, late_wake) in [(497, ms(2088), ms(2120)), (512, ms(2160), ms(2200))]
    {
        let clock = clock::feeds::waiting_play_lapse_clock(tap_period);
        let state = State {
            clock,
            ..State::new(Priority::RealTime)
        };
        let apply = |state, command| update(state, &mut [], &mut [], &mut no_grabs(), command);
        let waiting = apply(
            state,
            EngineCommand::Start {
                play: play(0),
                look: 7,
                rank: Rank::Show,
                when: When::Beat,
            },
        );
        assert_eq!(
            waiting.plays.at(0).unwrap().state,
            PlayState::Waiting { beat: 5 }
        );
        assert_eq!(clock.crossing(5), crossing);
        let expected_start = clock.beat_at(crossing);

        let before = apply(
            waiting,
            EngineCommand::Settle {
                at: crossing - Duration::from_nanos(1),
            },
        );
        assert_eq!(before.clock.crossing(5), crossing, "period {tap_period} ms");
        assert_eq!(
            before.plays.at(0).unwrap().state,
            PlayState::Waiting { beat: 5 }
        );

        for (name, settles) in [
            ("wake on the beat", &[crossing, late_wake][..]),
            ("wake after the lapse", &[late_wake][..]),
        ] {
            let state = settles.iter().fold(waiting, |state, &at| {
                apply(state, EngineCommand::Settle { at })
            });
            let play = state.plays.at(0).unwrap();
            assert_eq!(play.state, PlayState::Playing, "{name}");
            assert_eq!(play.start, expected_start, "{name}, period {tap_period} ms");
            assert_eq!(state.clock.crossing(5), crossing, "{name}");
        }
    }
}

#[test]
fn a_waiting_play_launches_on_its_beat_in_time_order() {
    let start = |slot, when| EngineCommand::Start {
        play: play(slot),
        look: 7,
        rank: Rank::Manual,
        when,
    };
    let keep = EngineCommand::Keep { play: play(0) };
    let release = EngineCommand::Release { play: play(0) };
    let tap = EngineCommand::Tap;
    let (beat, bar, now) = (When::Beat, When::Bar, When::Now);
    let waits = |beat| PlayState::Waiting { beat };
    let (playing, kept) = (PlayState::Playing, PlayState::Kept);
    type Wakes<'a> = &'a [(u64, &'a [(u64, EngineCommand)])];
    // (name, wakes, plays after the last, the latest start that sounds)
    type Case<'a> = (&'a str, Wakes<'a>, &'a [(usize, PlayState)], Option<usize>);
    let cases: [Case; 23] = [
        (
            "a tap supplies a crossing at the instant of a later request: it wins",
            &[
                (1100, &[(1100, start(0, beat))]),
                (1300, &[(1300, start(1, now)), (1300, tap)]),
            ],
            &[(0, playing), (1, playing)],
            Some(1),
        ),
        (
            "a tap supplies a crossing asked at its instant, after a start at once",
            &[(
                1300,
                &[(1300, start(1, now)), (1300, start(0, beat)), (1300, tap)],
            )],
            &[(0, playing), (1, playing)],
            Some(0),
        ),
        (
            "a launched play, then a later start: it wins",
            &[
                (1100, &[(1100, start(0, beat))]),
                (1500, &[]),
                (1500, &[(1500, start(1, now))]),
            ],
            &[(0, playing), (1, playing)],
            Some(1),
        ),
        (
            "two launched at one instant by a tap, then a later start",
            &[
                (1100, &[(1100, start(0, beat)), (1100, start(2, beat))]),
                (1300, &[(1300, tap), (1300, start(1, now))]),
            ],
            &[(0, playing), (1, playing), (2, playing)],
            Some(1),
        ),
        (
            "a beat, before it",
            &[(1100, &[(1100, start(0, beat))]), (1499, &[])],
            &[(0, waits(3))],
            None,
        ),
        (
            "a beat, on it",
            &[(1100, &[(1100, start(0, beat))]), (1500, &[])],
            &[(0, playing)],
            Some(0),
        ),
        (
            "a beat, asked on the grid",
            &[(1500, &[(1500, start(0, beat))])],
            &[(0, playing)],
            Some(0),
        ),
        (
            "a beat, asked on the grid, late",
            &[(1600, &[(1500, start(0, beat))])],
            &[(0, playing)],
            Some(0),
        ),
        (
            "a bar, through the beats",
            &[
                (2600, &[(2600, start(0, bar))]),
                (3000, &[]),
                (3500, &[]),
                (3999, &[]),
            ],
            &[(0, waits(8))],
            None,
        ),
        (
            "a bar, on it",
            &[(2600, &[(2600, start(0, bar))]), (3000, &[]), (4000, &[])],
            &[(0, playing)],
            Some(0),
        ),
        (
            "kept while waiting: cancelled",
            &[
                (1100, &[(1100, start(0, beat))]),
                (1200, &[(1200, keep)]),
                (2000, &[]),
            ],
            &[],
            None,
        ),
        (
            "released while waiting",
            &[
                (1100, &[(1100, start(0, beat))]),
                (1200, &[(1200, release)]),
                (2000, &[]),
            ],
            &[],
            None,
        ),
        (
            "kept after its crossing, in a late wake",
            &[(1100, &[(1100, start(0, beat))]), (1600, &[(1550, keep)])],
            &[(0, kept)],
            Some(0),
        ),
        (
            "kept before its crossing, in a late wake",
            &[(1100, &[(1100, start(0, beat))]), (1600, &[(1450, keep)])],
            &[],
            None,
        ),
        (
            "kept on its crossing: it launched first",
            &[(1100, &[(1100, start(0, beat))]), (1600, &[(1500, keep)])],
            &[(0, kept)],
            Some(0),
        ),
        (
            "two boundaries in one late wake: the later beat starts later",
            &[
                (1100, &[(1100, start(0, bar)), (1100, start(1, beat))]),
                (2100, &[]),
            ],
            &[(0, playing), (1, playing)],
            Some(0),
        ),
        (
            "two on one beat: the later request starts later",
            &[
                (1100, &[(1100, start(0, beat)), (1100, start(1, beat))]),
                (1500, &[]),
            ],
            &[(0, playing), (1, playing)],
            Some(1),
        ),
        (
            "sent before the crossing, at once: the launch is later",
            &[
                (1100, &[(1100, start(0, beat))]),
                (1600, &[(1200, start(1, now))]),
            ],
            &[(0, playing), (1, playing)],
            Some(0),
        ),
        (
            "sent after the crossing, at once: it is later",
            &[
                (1100, &[(1100, start(0, beat))]),
                (1600, &[(1550, start(1, now))]),
            ],
            &[(0, playing), (1, playing)],
            Some(1),
        ),
        (
            "a tap supplies the crossing",
            &[(1100, &[(1100, start(0, beat))]), (1300, &[(1300, tap)])],
            &[(0, playing)],
            Some(0),
        ),
        (
            "an off-grid tap moves a bar's crossing: before it",
            &[
                (2600, &[(2600, start(0, bar))]),
                (3150, &[(3150, tap)]),
                (3649, &[]),
            ],
            &[(0, waits(8))],
            None,
        ),
        (
            "an off-grid tap moves a bar's crossing: on it",
            &[
                (2600, &[(2600, start(0, bar))]),
                (3150, &[(3150, tap)]),
                (3650, &[]),
            ],
            &[(0, playing)],
            Some(0),
        ),
        (
            "started again while waiting: waits afresh",
            &[
                (1100, &[(1100, start(0, beat))]),
                (1200, &[(1200, start(0, bar))]),
                (1500, &[]),
            ],
            &[(0, waits(4))],
            None,
        ),
    ];
    for (name, wakes, plays, latest) in cases {
        let mut state = at_120();
        for (now, sent) in wakes {
            state = wake(state, *now, sent);
        }
        let now: Vec<_> = state
            .plays
            .iter()
            .map(|play| (play.play.slot(), play.state))
            .collect();
        assert_eq!(now, plays, "{name}");
        let mut orders: Vec<_> = state.plays.sounding().map(|play| play.order).collect();
        orders.sort_unstable();
        orders.dedup();
        assert_eq!(
            orders.len(),
            state.plays.sounding().count(),
            "{name}: a tie"
        );
        let last = state.plays.sounding().max_by_key(|play| play.order);
        assert_eq!(last.map(|play| play.play.slot()), latest, "{name}");
    }
}

#[test]
fn a_cancelled_play_ends_and_frees_its_slot() {
    let start = EngineCommand::Start {
        play: play(0),
        look: 7,
        rank: Rank::Show,
        when: When::Bar,
    };
    let state = wake(at_120(), 1100, &[(1100, start)]);
    let state = wake(
        state,
        1200,
        &[(1200, EngineCommand::Keep { play: play(0) })],
    );
    assert!(!state.plays.has(0));
    // and it never sounds: past its bar, nothing launched
    let later = wake(state, 2500, &[]);
    assert!(later.plays.iter().next().is_none());
}

/// Every cell of "A play on the tick" (`docs/engine.md`): beat n at n * 500 ms, the
/// play in slot 0 set up at 1100 ms, each event in a later wake.
#[test]
fn a_play_on_the_tick_follows_its_table() {
    let (first, second) = (play(0), PlayId::new(0, 2).unwrap());
    let start = |play, rank, when| EngineCommand::Start {
        play,
        look: 7,
        rank,
        when,
    };
    let removing = MixRules {
        manual_end: ManualEnd::UntilStop,
        ..MixRules::default()
    };
    let staying = MixRules {
        other_end: OtherEnd::Stay,
        ..MixRules::default()
    };
    let set_up = |commands: &[EngineCommand]| {
        let sent: Vec<_> = commands.iter().map(|command| (1100, *command)).collect();
        wake(at_120(), 1100, &sent)
    };
    let keep = EngineCommand::Keep { play: first };
    let manual = |when| start(first, Rank::Manual, when);
    let (off, waiting) = (set_up(&[]), set_up(&[manual(When::Beat)]));
    let (playing, kept) = (
        set_up(&[manual(When::Now)]),
        set_up(&[manual(When::Now), keep]),
    );
    // (event, wake at, its command): the table's columns, in order
    let events = [
        (
            "Start, Now",
            1200,
            Some(start(second, Rank::Manual, When::Now)),
        ),
        (
            "Start, Beat, not reached",
            1200,
            Some(start(second, Rank::Manual, When::Beat)),
        ),
        (
            "Start, boundary reached",
            1500,
            Some(start(second, Rank::Manual, When::Beat)),
        ),
        ("grid reaches the beat", 1600, None),
        ("Keep", 1200, Some(keep)),
        (
            "Release",
            1200,
            Some(EngineCommand::Release { play: first }),
        ),
        ("ReleaseAll", 1200, Some(EngineCommand::ReleaseAll)),
        (
            "Rules, Manual ends on a stop",
            1200,
            Some(EngineCommand::Rules { rules: removing }),
        ),
        (
            "Rules, Manual kept",
            1200,
            Some(EngineCommand::Rules { rules: staying }),
        ),
    ];
    let first_is = |state| Some((1, state));
    let (fresh, fresh_waiting) = (
        Some((2, PlayState::Playing)),
        Some((2, PlayState::Waiting { beat: 3 })),
    );
    let (on_3, plays, held) = (
        first_is(PlayState::Waiting { beat: 3 }),
        first_is(PlayState::Playing),
        first_is(PlayState::Kept),
    );
    // (state, slot 0 after each event as (generation, state), takes a plan for it)
    let rows = [
        (
            "off",
            off,
            [
                fresh,
                fresh_waiting,
                fresh,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            false,
        ),
        (
            "Waiting",
            waiting,
            [
                fresh,
                fresh_waiting,
                fresh,
                plays,
                None,
                None,
                None,
                on_3,
                on_3,
            ],
            true,
        ),
        (
            "Playing",
            playing,
            [
                fresh,
                fresh_waiting,
                fresh,
                plays,
                held,
                None,
                None,
                plays,
                plays,
            ],
            true,
        ),
        (
            "Kept",
            kept,
            [
                fresh,
                fresh_waiting,
                fresh,
                held,
                held,
                None,
                None,
                None,
                held,
            ],
            false,
        ),
    ];
    for (name, state, after, takes_plan) in rows {
        assert_eq!(state.plays.takes_plan(first), takes_plan, "{name}: Plan");
        assert!(
            !state.plays.takes_plan(second),
            "{name}: Plan of another play"
        );
        for ((event, at, command), expected) in events.iter().zip(after) {
            let sent: Vec<_> = command.iter().map(|command| (*at, *command)).collect();
            let slot = wake(state, *at, &sent).plays.at(0);
            let got = slot.map(|play| (play.play.generation, play.state));
            assert_eq!(got, expected, "{name} + {event}");
        }
    }
    // ReleaseAll lets only Manual go; the rules end a kept play of their rank only
    let show_kept = set_up(&[start(first, Rank::Show, When::Now), keep]);
    let show_cases = [
        (EngineCommand::ReleaseAll, Some(PlayState::Kept)),
        (
            EngineCommand::Rules { rules: staying },
            Some(PlayState::Kept),
        ),
        (
            EngineCommand::Rules {
                rules: MixRules::default(),
            },
            None,
        ),
    ];
    for (command, expected) in show_cases {
        let state = wake(show_kept, 1200, &[(1200, command)]);
        let got = state.plays.at(0).map(|play| play.state);
        assert_eq!(got, expected, "Show, Kept + {command:?}");
    }
}

#[test]
fn a_play_starts_at_its_crossing_and_a_tap_moves_no_running_play() {
    let start = |when| EngineCommand::Start {
        play: play(0),
        look: 7,
        rank: Rank::Manual,
        when,
    };
    // at once: beat zero in its applying frame, the wake at 1600
    let state = wake(at_120(), 1600, &[(1100, start(When::Now))]);
    assert!(close(state.plays.iter().next().unwrap().start, 3.2));
    // a change of tempo after it, in the same wake: still beat zero in that frame
    let tempo = |sent: &[(u64, EngineCommand)]| {
        let mut sent = sent.to_vec();
        sent.insert(0, (100, start(When::Now)));
        let state = wake(at_120(), 500, &sent);
        let play = *state.plays.iter().next().unwrap();
        (play.start, state.clock.beat_at(state.clock.at()))
    };
    let slow = EngineCommand::Feed {
        bpm: Bpm::new(60).unwrap(),
    };
    let keep = EngineCommand::Keep { play: play(0) };
    let changes: [(&str, &[(u64, EngineCommand)]); 5] = [
        ("the feed at 60", &[(200, slow)]),
        ("a tap", &[(200, EngineCommand::Tap)]),
        ("a tap at its frame", &[(500, EngineCommand::Tap)]),
        ("released", &[(200, EngineCommand::ReleaseTempo)]),
        ("kept, then the feed at 60", &[(150, keep), (200, slow)]),
    ];
    for (name, sent) in changes {
        let (start, elapsed) = tempo(sent);
        assert_eq!(start, elapsed, "{name}");
    }
    // on a beat in a late wake: from its crossing at 1500, 0.2 beats on by the frame
    let state = wake(at_120(), 1100, &[(1100, start(When::Beat))]);
    let state = wake(state, 1600, &[]);
    assert!(close(state.plays.iter().next().unwrap().start, 3.0));
    // a tap at 1700 moves the grid from 3.4 to 4; the elapsed count runs on
    let untapped = wake(state, 1700, &[]);
    let tapped = wake(state, 1700, &[(1700, EngineCommand::Tap)]);
    assert_eq!(tapped.clock.elapsed_now(), untapped.clock.elapsed_now());
    assert!(close(untapped.clock.grid(), 3.4) && close(tapped.clock.grid(), 4.0));
    assert_eq!(tapped.plays, untapped.plays);
}

#[test]
fn a_press_sent_before_its_boundary_but_seen_after_plays_from_the_frame() {
    let start = |when| EngineCommand::Start {
        play: play(0),
        look: 7,
        rank: Rank::Manual,
        when,
    };
    let playing = PlayState::Playing;
    // 120 BPM: beat n at n * 500 ms, a bar every 2000 ms. The wake at `missed` counted
    // its commands before the press came in; the wake at `seen` takes it.
    // (name, when, sent, missed, seen, state, start in beats when playing)
    type Case = (&'static str, When, u64, u64, u64, PlayState, Option<f64>);
    let cases: [Case; 4] = [
        (
            "a beat 1 ms before beat 3",
            When::Beat,
            1499,
            1501,
            1520,
            playing,
            Some(3.04),
        ),
        (
            "a beat on beat 3",
            When::Beat,
            1500,
            1501,
            1520,
            playing,
            Some(3.04),
        ),
        (
            "a bar 1 ms before bar 1",
            When::Bar,
            1999,
            2001,
            2020,
            playing,
            Some(4.04),
        ),
        (
            "a beat 1 ms after beat 3",
            When::Beat,
            1501,
            1502,
            1520,
            PlayState::Waiting { beat: 4 },
            None,
        ),
    ];
    for (name, when, sent, missed, seen, expected, beats) in cases {
        let state = wake(wake(at_120(), missed, &[]), seen, &[(sent, start(when))]);
        let play = *state.plays.iter().next().unwrap();
        assert_eq!(play.state, expected, "{name}");
        if let Some(beats) = beats {
            assert!(close(play.start, beats), "{name}: {:?}", play.start);
        }
    }
}

#[test]
fn taps_drained_in_one_wake_count_at_their_own_times() {
    let cases = [
        (
            "750 ms apart",
            1100,
            1850,
            Source::Tapped {
                at: Duration::from_millis(1850),
            },
            750,
        ),
        (
            "sent at once: a bounce",
            1100,
            1100,
            Source::OneTap {
                at: Duration::from_millis(1100),
            },
            500,
        ),
    ];
    for (name, first, second, source, period) in cases {
        let tap = EngineCommand::Tap;
        let state = wake(at_120(), 2000, &[(first, tap), (second, tap)]);
        assert_eq!(state.clock.source(), source, "{name}");
        assert_eq!(
            state.clock.period(),
            Duration::from_millis(period),
            "{name}"
        );
    }
}

#[test]
fn update_turns_a_command_into_a_new_state() {
    let set = |universe, address, value| EngineCommand::SetChannel {
        channel: channel(universe, address),
        value,
    };
    let release = |universe, address| EngineCommand::ReleaseChannel {
        channel: channel(universe, address),
    };
    let max = State {
        frame: u64::MAX,
        ..CALM
    };
    // (state, held before, command, state after, held after)
    let cases = [
        (
            CALM,
            raw(&[]),
            tick(0),
            State { frame: 8, ..CALM },
            raw(&[]),
        ), // the beat is not due yet: it stays
        (
            CALM,
            raw(&[]),
            EngineCommand::Tick { now: BEAT },
            State {
                frame: 8,
                now: BEAT,
                beat: BEAT * 2,
                ..CALM
            },
            raw(&[]),
        ), // the beat is due: it moves on
        (max, raw(&[]), tick(0), State { frame: 0, ..CALM }, raw(&[])), // wraps
        (CALM, raw(&[]), set(1, 1, 255), CALM, raw(&[(1, 1, 255)])),
        (
            CALM,
            raw(&[(2, 512, 9)]),
            set(2, 512, 10),
            CALM,
            raw(&[(2, 512, 10)]),
        ),
        (
            CALM,
            raw(&[(1, 3, 9), (1, 4, 5)]),
            release(1, 3),
            CALM,
            raw(&[(1, 4, 5)]),
        ),
        (CALM, raw(&[]), release(1, 3), CALM, raw(&[])), // nothing held: no change
        (
            CALM,
            raw(&[(1, 1, 1), (2, 2, 2)]),
            EngineCommand::ReleaseAll,
            CALM,
            raw(&[]),
        ),
        (
            // ReleaseAll lets holds go, never Master
            State {
                master: 128,
                ..CALM
            },
            raw(&[(1, 1, 1)]),
            EngineCommand::ReleaseAll,
            State {
                master: 128,
                ..CALM
            },
            raw(&[]),
        ),
        (
            CALM,
            raw(&[(1, 1, 80)]),
            EngineCommand::Blackout { on: true },
            DARK,
            raw(&[(1, 1, 80)]),
        ),
        (
            DARK,
            raw(&[(1, 1, 80)]),
            EngineCommand::Blackout { on: false },
            CALM,
            raw(&[(1, 1, 80)]),
        ),
        (
            CALM,
            raw(&[(1, 1, 80)]),
            EngineCommand::Master { level: 128 },
            State {
                master: 128,
                ..CALM
            },
            raw(&[(1, 1, 80)]),
        ),
        (
            State {
                master: 128,
                ..DARK
            },
            raw(&[]),
            EngineCommand::Master { level: 255 },
            DARK,
            raw(&[]),
        ),
        (DARK, raw(&[]), set(1, 2, 50), DARK, raw(&[(1, 2, 50)])), // held in a blackout
        (CALM, raw(&[]), set(1, 2, 0), CALM, raw(&[(1, 2, 0)])),   // held at 0 is held
        (CALM, raw(&[(1, 2, 0)]), release(1, 2), CALM, raw(&[])),
        (
            CALM,
            raw(&[(1, 2, 0)]),
            release(1, 3),
            CALM,
            raw(&[(1, 2, 0)]),
        ),
    ];
    for (state, (mut universes, mut holds), command, after, raw_after) in cases {
        let result = update(state, &mut universes, &mut holds, &mut no_grabs(), command);
        assert_eq!(result, after, "{command:?}");
        assert_eq!((universes, holds), raw_after, "{command:?}");
    }
}

#[test]
fn a_grab_command_changes_its_entry_and_a_start_takes_the_next_order() {
    let entry = GrabEntry::new(4).unwrap();
    let grab = |change| EngineCommand::Grab { entry, change };
    let live = |order| {
        Some(Grab {
            order,
            holding: Holding::Live,
        })
    };
    let until_stop = MixRules {
        manual_end: ManualEnd::UntilStop,
        ..CALM.rules
    };
    // (row, commands, the grab after, the next start order)
    let cases = [
        ("start", vec![grab(GrabChange::Start)], live(CALM.order), 1),
        (
            "new offers keep the start",
            vec![grab(GrabChange::Start), grab(GrabChange::Offers)],
            live(CALM.order),
            1,
        ),
        (
            "a second start is the latest",
            vec![grab(GrabChange::Start), grab(GrabChange::Start)],
            live(CALM.order + 1),
            2,
        ),
        (
            "kept",
            vec![grab(GrabChange::Start), grab(GrabChange::Keep)],
            Some(Grab {
                order: CALM.order,
                holding: Holding::Kept,
            }),
            1,
        ),
        (
            "freed",
            vec![grab(GrabChange::Start), grab(GrabChange::Free)],
            None,
            1,
        ),
        (
            "release all",
            vec![grab(GrabChange::Start), EngineCommand::ReleaseAll],
            None,
            1,
        ),
        (
            "rules let a kept one go",
            vec![
                grab(GrabChange::Start),
                grab(GrabChange::Keep),
                EngineCommand::Rules { rules: until_stop },
            ],
            None,
            1,
        ),
        (
            "rules keep a live one",
            vec![
                grab(GrabChange::Start),
                EngineCommand::Rules { rules: until_stop },
            ],
            live(CALM.order),
            1,
        ),
    ];
    for (row, commands, after, order) in cases {
        let mut grabs = Grabs::NONE;
        let mut state = CALM;
        for command in commands {
            state = update(state, &mut [], &mut [], &mut grabs, command);
        }
        let order = CALM.order + order;
        assert_eq!((grabs.get(entry), state.order), (after, order), "{row}");
    }
}

#[test]
fn a_light_run_stays_in_the_lights_of_the_engine() {
    let all = Lights::new(4).unwrap();
    let number = |n| LightNumber::new(n, all).unwrap();
    // (first, count, made)
    let cases = [
        (0, 4, true),
        (3, 1, true),
        (3, 2, false),
        (0, 0, false),
        (2, u16::MAX, false),
    ];
    for (first, count, made) in cases {
        assert_eq!(
            LightRun::new(number(first), count, all).is_some(),
            made,
            "{first} {count}"
        );
    }
    // a shorter buffer of values: only the lights it has
    let mut two = [Light::DARK; 2];
    assert_eq!(
        LightRun::new(number(1), 3, all).unwrap().of(&mut two).len(),
        1
    );
}

#[test]
fn a_channel_on_a_shorter_buffer_changes_nothing() {
    let (mut one, mut holds) = ([Levels::ZERO], [ChannelSet::NONE]);
    let command = EngineCommand::SetChannel {
        channel: channel(2, 1),
        value: 9,
    };
    assert_eq!(
        update(CALM, &mut one, &mut holds, &mut no_grabs(), command),
        CALM
    );
    assert_eq!((one, holds), ([Levels::ZERO], [ChannelSet::NONE]));
}

/// Both universes patched, at rest 0, fading and following Master and Blackout, but
/// channel 1/2 at 200: a shutter held open, which jumps and follows Blackout only.
fn two() -> Vec<Configured> {
    (0..2 * 512)
        .map(|at| Configured {
            at,
            rest: if at == 1 { 200 } else { 0 },
            dark: 0,
            flags: if at == 1 { Flags::NONE } else { Flags::FADE },
            follows: if at == 1 {
                Follows::BLACKOUT
            } else {
                Follows::MASTER.with(Follows::BLACKOUT)
            },
        })
        .collect()
}

#[test]
fn frame_says_who_set_each_channel_and_a_release_gives_it_back() {
    // a grab offers 255 on the dimmer at 1/1; 1/2 rests at 200
    let entry = GrabEntry::new(0).unwrap();
    let grabbed = EngineCommand::Grab {
        entry,
        change: GrabChange::Start,
    };
    let lit = SourceTag::of(PLAYS).unwrap();
    let set = |universe, address, value| EngineCommand::SetChannel {
        channel: channel(universe, address),
        value,
    };
    let release = |universe, address| EngineCommand::ReleaseChannel {
        channel: channel(universe, address),
    };
    let blackout = |on| EngineCommand::Blackout { on };
    let master = |level| EngineCommand::Master { level };
    use SourceTag as S;
    // (row, commands, channels 1/1, 1/2 and 2/512: byte and source)
    type Seen = [(u8, SourceTag); 3];
    let cases: [(&str, Vec<EngineCommand>, Seen); 10] = [
        (
            // the shutter jumps and follows Blackout only: Master at 0 leaves it
            "master at 0: what follows it goes dark, the shutter stays",
            vec![set(2, 512, 91), master(0)],
            [(0, lit), (200, S::REST), (0, S::RAW)],
        ),
        (
            // 255 x 128 / 255 = 128; 91 x 128 / 255 = 45.7
            "master half: the light and the raw scale, the shutter stays",
            vec![set(2, 512, 91), master(128)],
            [(128, lit), (200, S::REST), (46, S::RAW)],
        ),
        (
            "master full again",
            vec![set(2, 512, 91), master(128), master(255)],
            [(255, lit), (200, S::REST), (91, S::RAW)],
        ),
        (
            "nothing held",
            vec![],
            [(255, lit), (200, S::REST), (0, S::REST)],
        ),
        (
            "raw 91 over the light",
            vec![set(1, 1, 91)],
            [(91, S::RAW), (200, S::REST), (0, S::REST)],
        ),
        (
            "raw 91 released",
            vec![set(1, 1, 91), release(1, 1)],
            [(255, lit), (200, S::REST), (0, S::REST)],
        ),
        (
            "raw on the last channel",
            vec![set(2, 512, 91)],
            [(255, lit), (200, S::REST), (91, S::RAW)],
        ),
        (
            "blackout over raw and light",
            vec![set(2, 512, 91), blackout(true)],
            [(0, lit), (0, S::REST), (0, S::RAW)],
        ),
        (
            "blackout off, the hold comes back",
            vec![set(2, 512, 91), blackout(true), blackout(false)],
            [(255, lit), (200, S::REST), (91, S::RAW)],
        ),
        (
            "every raw channel released",
            vec![
                set(1, 1, 91),
                set(2, 512, 91),
                EngineCommand::ReleaseChannels,
            ],
            [(255, lit), (200, S::REST), (0, S::REST)],
        ),
    ];
    for (row, commands, expected) in cases {
        let (mut universes, mut holds) = raw(&[]);
        let mut grabs = Grabs::NONE;
        let mut state = CALM;
        for command in core::iter::once(grabbed).chain(commands) {
            state = update(state, &mut universes, &mut holds, &mut grabs, command);
        }
        let mut channels = Box::new(Channels::NEW);
        let patched = two();
        let followers: Vec<Field> = patched.iter().map(|c| Field::Byte(c.at)).collect();
        channels.configure(&[], &patched);
        frame(
            &state,
            &grabs,
            (&universes, &holds),
            (&patched, &followers),
            &mut channels,
            |offers| offers.grab(entry, Field::Byte(0), 0xff00),
        );
        let seen = [(1, 1), (1, 2), (2, 512)].map(|(universe, address)| {
            let at = channel(universe, address);
            let byte = channels.values()[usize::from(universe - 1)].0[usize::from(address - 1)];
            (byte, channels.source(at))
        });
        assert_eq!(seen, expected, "{row}");
    }
}

fn output(number: u8) -> OutputNumber {
    OutputNumber::try_from(number).unwrap()
}

fn set_output(number: u8, rate: Rate) -> EngineCommand {
    EngineCommand::Attach {
        output: output(number),
        universes: RunNumbers { first: 1, count: 1 }
            .check(Universes::ONE)
            .unwrap(),
        rate,
    }
}

/// Per wake: the time in ms and the outputs served.
type Wakes = Vec<(u64, Vec<u8>)>;

/// Runs the schedule like the output thread does, on time: wake, hand every due
/// output a frame. Returns, per wake, the time in ms and the outputs served.
fn run(mut state: State, until_ms: u64, mut at: impl FnMut(u64) -> Vec<EngineCommand>) -> Wakes {
    let mut wakes = vec![];
    let mut universes = [Levels::ZERO];
    loop {
        let now = state.next_wake();
        let ms = u64::try_from(now.as_millis()).unwrap();
        if ms > until_ms {
            return wakes;
        }
        state = update(
            state,
            &mut universes,
            &mut [],
            &mut no_grabs(),
            EngineCommand::Tick { now },
        );
        for command in at(ms) {
            state = update(state, &mut universes, &mut [], &mut no_grabs(), command);
        }
        let served: Vec<u8> = due(&state, now).map(|n| n.slot() as u8 + 1).collect();
        for &number in &served {
            let handed = EngineCommand::Handed {
                output: output(number),
                at: now,
            };
            state = update(state, &mut universes, &mut [], &mut no_grabs(), handed);
        }
        wakes.push((ms, served));
    }
}

#[test]
fn the_engine_wakes_when_the_next_output_is_due() {
    let calm = State::new(Priority::RealTime);
    let (thirty, forty_four, sixty) = (Rate::Thirty, Rate::FortyFour, Rate::Sixty);
    // (outputs set on the first wake, until ms, wakes: ms and outputs served)
    // 60 and 30 a second meet at 56.06 ms, 1 ns apart: one wake serves both.
    let cases: [(Vec<EngineCommand>, u64, Wakes); 4] = [
        (vec![], 70, vec![(22, vec![]), (45, vec![]), (68, vec![])]), // the beat
        (
            vec![set_output(1, sixty)],
            40,
            vec![(22, vec![1]), (39, vec![1])],
        ),
        (
            vec![set_output(1, sixty), set_output(2, thirty)],
            60,
            vec![(22, vec![1, 2]), (39, vec![1]), (56, vec![1, 2])],
        ),
        (
            vec![
                set_output(1, sixty),
                set_output(2, thirty),
                set_output(3, forty_four),
            ],
            60,
            vec![
                (22, vec![1, 2, 3]),
                (39, vec![1]),
                (45, vec![3]),
                (56, vec![1, 2]),
            ],
        ),
    ];
    for (commands, until, expected) in cases {
        let mut first = Some(commands.clone());
        let wakes = run(calm, until, |_| first.take().unwrap_or_default());
        assert_eq!(wakes, expected, "{commands:?}");
    }
}

#[test]
fn outputs_added_or_removed_while_running_change_the_wakes() {
    let calm = State::new(Priority::RealTime);
    let remove = EngineCommand::RemoveOutput { output: output(1) };
    let wakes = run(calm, 100, |ms| match ms {
        22 => vec![set_output(1, Rate::Sixty)],
        39 => vec![set_output(2, Rate::Thirty)],
        56 => vec![remove],
        _ => vec![],
    });
    let expected = vec![
        (22, vec![1]),    // added on the beat: served at once
        (39, vec![1, 2]), // the second one too
        (56, vec![]),     // 1 removed before it was served
        (72, vec![2]),    // only 2 is left, on its own rate; no beat while it is set
    ];
    assert_eq!(wakes, expected);
}

#[test]
fn a_new_output_joins_the_phase_of_its_rate() {
    let calm = State::new(Priority::RealTime);
    let (thirty, sixty) = (Rate::Thirty, Rate::Sixty);
    // (output added on the wake at 39 ms, wakes: ms and outputs served)
    let cases = [
        // a rate that runs: its next frame, with the others of its rate
        (
            set_output(3, thirty),
            vec![(22, vec![1, 2]), (39, vec![1]), (56, vec![1, 2, 3])],
        ),
        (
            set_output(3, sixty),
            vec![(22, vec![1, 2]), (39, vec![1, 3]), (56, vec![1, 2, 3])],
        ),
        // a rate of its own: at once
        (
            set_output(3, Rate::FortyFour),
            vec![(22, vec![1, 2]), (39, vec![1, 3]), (56, vec![1, 2])],
        ),
    ];
    for (added, expected) in cases {
        let wakes = run(calm, 60, |ms| match ms {
            22 => vec![set_output(1, sixty), set_output(2, thirty)],
            39 => vec![added],
            _ => vec![],
        });
        assert_eq!(wakes, expected, "{added:?}");
    }
}

#[test]
fn after_the_last_output_is_removed_the_engine_wakes_within_one_beat() {
    let calm = State::new(Priority::RealTime);
    let one_beat = u64::try_from(BEAT.as_millis()).unwrap() + 1;
    let (thirty, forty_four, sixty) = (Rate::Thirty, Rate::FortyFour, Rate::Sixty);
    // (outputs set on the first wake, all removed on the first wake after ms)
    let cases: [(&[(u8, Rate)], u64); 5] = [
        (&[(1, sixty)], 50),
        (&[(1, sixty)], 17_000), // about 1000 wakes
        (&[(1, sixty), (2, forty_four)], 17_000),
        (&[(1, thirty)], 17_000),
        (&[(1, sixty), (2, thirty), (3, forty_four)], 17_000),
    ];
    for (outputs, after) in cases {
        let (mut added, mut removed) = (false, None);
        let wakes = run(calm, after + 100, |ms| {
            if !added {
                added = true;
                return outputs
                    .iter()
                    .map(|&(n, rate)| set_output(n, rate))
                    .collect();
            }
            if removed.is_some() || ms < after {
                return vec![];
            }
            removed = Some(ms);
            let remove = |&(n, _): &(u8, Rate)| EngineCommand::RemoveOutput { output: output(n) };
            outputs.iter().map(remove).collect()
        });
        let removed = removed.unwrap();
        let next = wakes.iter().map(|&(ms, _)| ms).find(|&ms| ms > removed);
        assert!(
            next.is_some_and(|ms| ms - removed <= one_beat),
            "{outputs:?}: removed at {removed} ms, next wake {next:?}"
        );
    }
}

#[test]
fn after_a_long_stall_each_output_starts_over_from_now() {
    let mut universes = [Levels::ZERO];
    let mut state = State::new(Priority::RealTime);
    state = update(
        state,
        &mut universes,
        &mut [],
        &mut no_grabs(),
        set_output(1, Rate::Sixty),
    );
    state = update(
        state,
        &mut universes,
        &mut [],
        &mut no_grabs(),
        set_output(2, Rate::Thirty),
    );
    // the machine slept for a second
    let now = Duration::from_secs(1);
    state = update(
        state,
        &mut universes,
        &mut [],
        &mut no_grabs(),
        EngineCommand::Tick { now },
    );
    let served: Vec<_> = due(&state, now).collect();
    assert_eq!(served, vec![output(1), output(2)]);
    for number in served {
        let handed = EngineCommand::Handed {
            output: number,
            at: now,
        };
        state = update(state, &mut universes, &mut [], &mut no_grabs(), handed);
    }
    assert_eq!(state.next_wake(), now + Rate::Sixty.period());
    let thirty = state.outputs[1].unwrap();
    assert_eq!(thirty.due, now + Rate::Thirty.period());
}

#[test]
fn next_frame_catches_up_when_late_and_resyncs_after_a_stall() {
    let period = Duration::from_millis(10);
    let due = Duration::from_secs(5);
    // (frames late, when the next frame is due)
    let cases = [
        (0, due + period),              // on time
        (3, due + period),              // oversleep: make it up at once
        (MAX_BEHIND, due + period),     // still catching up
        (MAX_BEHIND + 1, due + period), // the last frame it still makes up
        (20, due + period * 21),        // machine slept: start over from now
    ];
    for (late, expected) in cases {
        let now = due + period * late;
        assert_eq!(next_frame(due, now, period), expected, "{late} frames late");
    }
    assert_eq!(
        next_frame(Duration::MAX, Duration::MAX, period),
        Duration::MAX
    );
}

#[test]
fn a_wake_that_serves_an_output_spins_the_last_millisecond_a_beat_only_sleeps() {
    let mut universes = [Levels::ZERO];
    let calm = State::new(Priority::RealTime);
    let due = BEAT; // the beat, and the output's first frame
    let with_output = update(
        calm,
        &mut universes,
        &mut [],
        &mut no_grabs(),
        set_output(1, Rate::Sixty),
    );
    let schedule = with_output.attached_output(output(1)).unwrap().due_at(due);
    let with_output = with_output.with_output_schedule(output(1), schedule);
    let us = Duration::from_micros;
    // (state, µs before due, µs to sleep, spin until due)
    let cases = [
        (with_output, 22_727, 21_727, true), // a whole frame ahead
        (with_output, 1_001, 1, true),
        (with_output, 1_000, 0, true), // spin all of it
        (with_output, 400, 0, true),
        (with_output, 0, 0, true),
        (calm, 22_727, 22_727, false), // no output: sleep to the beat
        (calm, 400, 400, false),
        (calm, 0, 0, false),
    ];
    for (state, ahead, sleep, spins) in cases {
        let now = due - us(ahead);
        let spin = spins.then_some(due);
        assert_eq!(
            wait(&state, now),
            (us(sleep), spin),
            "{ahead} µs ahead, spins {spins}"
        );
    }
    // past due: no sleep
    assert_eq!(wait(&with_output, due * 2), (Duration::ZERO, Some(due)));
    assert_eq!(wait(&calm, due * 2), (Duration::ZERO, None));
}

#[test]
fn bucket_counts_by_100_microseconds() {
    // (time in µs, bucket of 251, bucket of 501)
    let cases = [
        (0, 0, 0),
        (99, 0, 0),
        (100, 1, 1),
        (250, 2, 2),
        (9_999, 99, 99),
        (24_999, 249, 249),
        (25_000, 250, 250),
        (49_999, 250, 499),
        (u64::MAX, 250, 500),
    ];
    for (micros, lateness, latency) in cases {
        let time = Duration::from_micros(micros);
        assert_eq!(bucket::<LATENESS_BUCKETS>(time), lateness, "{micros} µs");
        assert_eq!(bucket::<LATENCY_BUCKETS>(time), latency, "{micros} µs");
    }
}

fn counts(counts: &[(usize, u64)]) -> Histogram<LATENESS_BUCKETS> {
    let mut histogram = [0; LATENESS_BUCKETS];
    for (bucket, count) in counts {
        histogram[*bucket] = *count;
    }
    histogram
}

#[test]
fn rank_is_the_nearest_rank() {
    // (total, percent, rank): ceil(percent / 100 x total), the textbook nearest rank
    let full = 501 * u128::from(u64::MAX); // every bucket of a window at its most
    let cases = [
        (0, 99, 0),
        (1, 0, 0),
        (1, 99, 1),
        (99, 99, 99),   // 98.01
        (100, 99, 99),  // 99
        (101, 99, 100), // 99.99
        (100, 95, 95),  // 95
        (101, 95, 96),  // 95.95
        (100, 100, 100),
        (100, 0, 0),
        (100, 150, 100), // above 100 is 100
        (full, 99, 9_149_400_593_119_200_505_524),
    ];
    for (total, percent, expected) in cases {
        assert_eq!(rank(total, percent), expected, "{total}, {percent}");
    }
}

#[test]
fn percentile_is_the_upper_edge_of_its_bucket_and_never_past_the_worst() {
    let last = LATENESS_BUCKETS - 1;
    let late = 30_000; // past the last edge, 25 ms
    // (bucket and count, the worst time counted in µs, percent, result in µs)
    let cases = [
        (counts(&[]), 0, 99, None),
        (counts(&[(0, 1)]), 99, 99, Some(99)), // the worst is in the bucket
        (counts(&[(0, 99), (5, 1)]), 550, 99, Some(100)),
        (counts(&[(0, 98), (5, 2)]), 550, 99, Some(550)),
        (counts(&[(0, 98), (5, 2)]), 599, 99, Some(599)),
        (counts(&[(3, 1000), (100, 11)]), 10_050, 99, Some(10_050)),
        (counts(&[(3, 1000), (100, 11)]), late, 99, Some(10_100)),
        (counts(&[(0, 95), (5, 5)]), 550, 95, Some(100)),
        (counts(&[(0, 94), (5, 6)]), 550, 95, Some(550)),
        (counts(&[(0, 1), (9, 1)]), 950, 95, Some(950)),
        (counts(&[(0, 1), (last, 1)]), 2_000_000, 95, Some(2_000_000)), // a stall of 2 s
        (counts(&[(last, 3)]), late, 50, Some(late)),
        (counts(&[(0, 99), (last, 1)]), late, 99, Some(100)),
        // half of 2 x u64::MAX counts in each: p99 is in the second, capped at the worst
        (counts(&[(0, u64::MAX), (1, u64::MAX)]), 150, 99, Some(150)),
        // a total that is no multiple of 100: the rank is rounded up
        (counts(&[(0, 75), (5, 75)]), 550, 50, Some(100)), // rank 75
        (counts(&[(0, 74), (5, 76)]), 550, 50, Some(550)),
        (counts(&[(0, 76), (5, 75)]), 550, 50, Some(100)), // rank 76 of 151
        (counts(&[(0, 76), (5, 75)]), 550, 51, Some(550)), // rank 78
    ];
    for (histogram, worst, percent, expected) in cases {
        let worst = Duration::from_micros(worst);
        assert_eq!(
            percentile(&histogram, percent, worst),
            expected.map(Duration::from_micros),
            "p{percent} worst {worst:?}"
        );
    }
}
