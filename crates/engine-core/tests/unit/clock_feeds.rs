use super::*;
use TempoFeed::{Audio, MidiClock, Timecode, VirtualDJ};

/// The Tap feed: `Tap` is a step here.
const TAP: TempoFeed = TempoFeed::Tap;

const fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

fn bpm(bpm: u16) -> Bpm {
    Bpm::new(bpm).unwrap()
}

/// One beat in `period` ms.
fn every(period: u64) -> Tempo {
    Tempo::new(period * 1_000_000, 1).unwrap()
}

fn list(feeds: &[TempoFeed]) -> FeedList {
    FeedList::new(feeds).unwrap()
}

/// What happens, each at its time in ms. Each step settles the clock to its time first.
#[derive(Clone, Copy, Debug)]
enum Step {
    /// Time passes, with nothing sent.
    Settle(u64),
    /// A beat of `feed`, heard at the first time, taken at the second (a frame at or
    /// after it), one beat in the third ms.
    Beat(TempoFeed, u64, u64, u64),
    /// `feed` gains a lock of one beat in that many ms, or loses it.
    Lock(u64, TempoFeed, Option<u64>),
    /// A new feed list.
    List(u64, &'static [TempoFeed]),
    Tap(u64),
    Release(u64),
    /// `SetInternalTempo`.
    Internal(u64, u16),
}

impl Step {
    /// The time the clock is settled to before the step.
    const fn at(self) -> u64 {
        match self {
            Self::Settle(t)
            | Self::Beat(_, _, t, _)
            | Self::Lock(t, ..)
            | Self::List(t, _)
            | Self::Tap(t)
            | Self::Release(t)
            | Self::Internal(t, _) => t,
        }
    }

    /// The step on a clock already settled to its time.
    fn on(self, clock: Clock) -> Clock {
        match self {
            Self::Settle(_) => clock,
            Self::Beat(feed, heard, _, period) => clock.beat(feed, ms(heard), every(period)),
            Self::Lock(_, feed, period) => clock.lock(feed, period.map(every)),
            Self::List(_, feeds) => clock.set_feeds(list(feeds)),
            Self::Tap(_) => clock.tap(),
            Self::Release(_) => clock.release(),
            Self::Internal(_, b) => clock.set_feed(bpm(b)),
        }
    }
}

fn start(feeds: &[TempoFeed]) -> Clock {
    Clock::START.set_feed(bpm(120)).set_feeds(list(feeds))
}

fn run(feeds: &[TempoFeed], steps: &[Step]) -> Clock {
    steps.iter().fold(start(feeds), |clock, step| {
        step.on(clock.settle(ms(step.at())))
    })
}

pub(crate) fn waiting_play_lapse_clock(tap_period: u64) -> Clock {
    run(
        &[Audio],
        &[
            Lock(0, Audio, Some(tap_period)),
            Tap(100),
            Lock(1000, Audio, Some(640)),
            Settle(2080),
        ],
    )
}

use Step::{Beat, Internal, List, Lock, Release, Settle, Tap};

/// (name, feed list at 0 ms, steps, pace, driving feed, grid in beats, period in ms,
/// beats and their crossings in ms, `None` for never)
type Row = (
    &'static str,
    &'static [TempoFeed],
    &'static [Step],
    Pace,
    Option<TempoFeed>,
    f64,
    u64,
    &'static [(u64, Option<u64>)],
);

#[rustfmt::skip]
/// One beat on time, one at half the tempo, then it stands on the next without
/// taking it (4d). Anchor 0: beat 1 at 500 ms, beat 2 is never crossed.
const SILENCE: [Row; 8] = [
    ("silence, at the start", &[Audio], &[], Pace::Heard, None, 0.0, 500, &[(0, Some(0)), (1, Some(500)), (2, None)]),
    ("silence, 1 ms before anchor + 1", &[Audio], &[Settle(499)], Pace::Heard, None, 0.998, 500, &[(1, Some(500)), (2, None)]),
    ("silence, on anchor + 1: taken on time", &[Audio], &[Settle(500)], Pace::Slowing, None, 1.0, 500, &[(1, Some(500)), (2, None)]),
    ("silence, 1 ms past anchor + 1: half the tempo", &[Audio], &[Settle(501)], Pace::Slowing, None, 1.001, 500, &[(2, None)]),
    ("silence, half way to anchor + 2", &[Audio], &[Settle(1000)], Pace::Slowing, None, 1.5, 500, &[(2, None)]),
    ("silence, 1 ms before anchor + 2", &[Audio], &[Settle(1499)], Pace::Slowing, None, 1.999, 500, &[(2, None)]),
    ("silence, on anchor + 2: stands on it, not taken", &[Audio], &[Settle(1500)], Pace::Stopped, None, 2.0, 500, &[(2, None), (3, None)]),
    ("silence, a minute on: still standing", &[Audio], &[Settle(60_000)], Pace::Stopped, None, 2.0, 500, &[(2, None), (3, None)]),
];

#[rustfmt::skip]
/// A heard beat moves the grid like a tap that brings its tempo (5b), and every
/// cell of 4a.
const BEATS: [Row; 39] = [
    ("a beat on its crossing", &[Audio], &[Beat(Audio, 500, 500, 500)], Pace::Heard, Some(Audio), 1.0, 500, &[(1, Some(500)), (2, Some(1000)), (3, None)]),
    ("heard 2 ms before its crossing, taken in a frame 10 ms later", &[Audio], &[Beat(Audio, 498, 510, 500)], Pace::Heard, Some(Audio), 1.024, 500, &[(2, Some(998)), (3, None)]),
    ("heard 2 ms after its crossing: the nearest beat, not counted twice", &[Audio], &[Beat(Audio, 502, 502, 500)], Pace::Heard, Some(Audio), 1.0, 500, &[(2, Some(1002)), (3, None)]),
    ("heard half way while slowing: the tie goes to the later beat", &[Audio], &[Settle(1000), Beat(Audio, 1000, 1000, 500)], Pace::Heard, Some(Audio), 2.0, 500, &[(2, Some(1000)), (3, Some(1500)), (4, None)]),
    ("heard while stopped: takes the beat it stands on", &[Audio], &[Settle(2000), Beat(Audio, 2000, 2000, 500)], Pace::Heard, Some(Audio), 2.0, 500, &[(2, Some(2000)), (3, Some(2500)), (4, None)]),
    ("a beat brings its tempo", &[Audio], &[Beat(Audio, 500, 500, 640)], Pace::Heard, Some(Audio), 1.0, 640, &[(2, Some(1140)), (3, None)]),
    ("a late beat that would go back past a reached beat: its row only", &[Audio], &[Beat(Audio, 500, 500, 500), Settle(1010), Beat(Audio, 740, 1010, 640)], Pace::Slowing, Some(Audio), 2.01, 500, &[(3, None)]),
    ("a beat heard before the last beat that moved the grid: its row only", &[Audio], &[Beat(Audio, 500, 500, 500), Beat(Audio, 450, 520, 640)], Pace::Heard, Some(Audio), 1.04, 500, &[(2, Some(1000)), (3, None)]),
    ("a beat heard while one tap held, taken after it lapsed: its row only", &[Audio], &[Tap(100), Beat(Audio, 2050, 2150, 500)], Pace::Heard, Some(Audio), 5.1, 500, &[(6, Some(2600)), (7, None)]),
    ("a beat heard before a new list, same driver: its row only", &[Audio], &[Beat(Audio, 500, 500, 500), List(600, &[Audio, TAP]), Beat(Audio, 550, 610, 640)], Pace::Heard, Some(Audio), 1.22, 500, &[(2, Some(1000)), (3, None)]),
    ("a beat heard before its feed took over from Tap at the same tempo: its row only", &[Audio, TAP], &[Lock(1000, Audio, Some(500)), Beat(Audio, 900, 1010, 640)], Pace::Heard, Some(Audio), 2.02, 500, &[(3, Some(1500)), (4, None)]),
    ("a beat heard before its feed took over from another: its row only", &[MidiClock, Audio], &[Beat(Audio, 500, 500, 500), Lock(600, MidiClock, Some(640)), Beat(MidiClock, 550, 610, 640)], Pace::Heard, Some(MidiClock), 1.215625, 640, &[(2, Some(1112)), (3, None)]),
    ("a release with no taps changes nothing: a beat heard before it still counts", &[Audio], &[Beat(Audio, 500, 500, 500), Release(600), Beat(Audio, 550, 610, 640)], Pace::Heard, Some(Audio), 1.09375, 640, &[(2, Some(1190)), (3, None)]),
    ("a beat of a feed that does not drive: its row only", &[MidiClock, Audio], &[Lock(0, MidiClock, Some(500)), Beat(Audio, 300, 300, 640)], Pace::Heard, Some(MidiClock), 0.6, 500, &[(1, Some(500)), (2, None)]),
    ("a beat while two taps hold: its row only", &[Audio], &[Tap(100), Tap(700), Beat(Audio, 1000, 1000, 640)], Pace::Free, Some(Audio), 3.5, 600, &[(4, Some(1300)), (5, Some(1900))]),
    ("release after taps: the driving feed's tempo at the grid's phase, anchor now", &[Audio], &[Tap(100), Tap(700), Beat(Audio, 1000, 1000, 640), Release(1000)], Pace::Heard, Some(Audio), 3.5, 640, &[(4, Some(1320)), (5, None)]),
    ("release after taps, no lock: the tapped tempo, anchor now", &[Audio], &[Tap(100), Tap(700), Release(1000)], Pace::Heard, None, 3.5, 600, &[(4, Some(1300)), (5, None)]),
    ("release after taps, no lock, then silence: slows and stops", &[Audio], &[Tap(100), Tap(700), Release(1000), Settle(2500)], Pace::Stopped, None, 5.0, 600, &[(5, None)]),
    ("Tap last drives while Audio has no lock: it never stops", &[Audio, TAP], &[Settle(60_000)], Pace::Free, Some(TAP), 120.0, 500, &[(121, Some(60_500))]),
    ("Audio's first beat takes over from Tap last", &[Audio, TAP], &[Beat(Audio, 500, 500, 640)], Pace::Heard, Some(Audio), 1.0, 640, &[(2, Some(1140)), (3, None)]),
    ("Audio's first beat, then silence: slows and stops", &[Audio, TAP], &[Beat(Audio, 500, 500, 640), Settle(3000)], Pace::Stopped, Some(Audio), 3.0, 640, &[(3, None)]),
    ("stopped, Audio loses its lock: Tap last takes the beat it stands on at once", &[Audio, TAP], &[Beat(Audio, 500, 500, 640), Settle(3000), Lock(3000, Audio, None)], Pace::Free, Some(TAP), 3.0, 500, &[(3, Some(3000)), (4, Some(3500))]),
    ("stopped, Audio at the internal tempo loses its lock: Tap last takes the beat it stands on", &[Audio, TAP], &[Beat(Audio, 500, 500, 500), Settle(3000), Lock(3000, Audio, None)], Pace::Free, Some(TAP), 3.0, 500, &[(3, Some(3000)), (4, Some(3500))]),
    ("Audio locks while Tap last drives: its tempo at the grid's phase, anchor now", &[Audio, TAP], &[Lock(1100, Audio, Some(640))], Pace::Heard, Some(Audio), 2.2, 640, &[(3, Some(1612)), (4, None)]),
    ("Audio locks while Tap last drives, then silence: slows and stops", &[Audio, TAP], &[Lock(1100, Audio, Some(640)), Settle(2892)], Pace::Stopped, Some(Audio), 4.0, 640, &[(4, None)]),
    ("a lost lock with nothing after it: slows from the anchor it had", &[Audio], &[Beat(Audio, 500, 500, 500), Lock(500, Audio, None), Settle(1500)], Pace::Slowing, None, 2.5, 500, &[(3, None)]),
    ("a new lock tempo of the driving feed waits for its beat", &[Audio], &[Beat(Audio, 500, 500, 500), Lock(600, Audio, Some(640))], Pace::Heard, Some(Audio), 1.2, 500, &[(2, Some(1000)), (3, None)]),
    ("a new list while stopped, nothing drives: stays stopped", &[Audio], &[Settle(2000), List(2000, &[MidiClock])], Pace::Stopped, None, 2.0, 500, &[(2, None)]),
    ("a new list while slowing, a feed drives: its tempo, still slowing", &[Audio], &[Lock(0, MidiClock, Some(640)), List(1000, &[MidiClock])], Pace::Slowing, Some(MidiClock), 1.5, 640, &[(2, None)]),
    ("the same list again is a release", &[Audio], &[Tap(100), Tap(700), List(1000, &[Audio])], Pace::Heard, None, 3.5, 600, &[(4, Some(1300)), (5, None)]),
    ("a new list releases the taps", &[Audio], &[Tap(100), Tap(700), List(1000, &[Audio, TAP])], Pace::Free, Some(TAP), 3.5, 500, &[(4, Some(1250)), (5, Some(1750))]),
    ("the internal tempo while Audio drives is not the tempo", &[Audio], &[Beat(Audio, 500, 500, 640), Internal(600, 60)], Pace::Heard, Some(Audio), 1.15625, 640, &[(2, Some(1140)), (3, None)]),
    ("the internal tempo while Tap last drives is the tempo", &[Audio, TAP], &[Internal(500, 60)], Pace::Free, Some(TAP), 1.0, 1000, &[(2, Some(1500))]),
    ("Tap's lock cannot be taken away", &[Audio, TAP], &[Lock(0, TAP, None), Settle(1000)], Pace::Free, Some(TAP), 2.0, 500, &[(3, Some(1500))]),
    ("one tap runs free for its 2 s", &[Audio], &[Tap(100), Settle(2100)], Pace::Free, None, 5.0, 500, &[(6, Some(2600))]),
    ("one tap lapsed: anchor at the beat reached at the lapse", &[Audio], &[Tap(100), Settle(2101)], Pace::Heard, None, 5.002, 500, &[(6, Some(2600)), (7, None)]),
    ("one tap lapsed, then silence: slows and stops", &[Audio], &[Tap(100), Settle(3600)], Pace::Stopped, None, 7.0, 500, &[(7, None)]),
    ("two taps never stop", &[Audio], &[Tap(100), Tap(700), Settle(60_100)], Pace::Free, None, 102.0, 600, &[(103, Some(60_700))]),
    ("a bar: a grid stopped on beat 4 takes it from the next heard beat", &[Audio], &[Beat(Audio, 500, 500, 500), Beat(Audio, 1000, 1000, 500), Settle(2500), Beat(Audio, 2600, 2600, 500)], Pace::Heard, Some(Audio), 4.0, 500, &[(4, Some(2600)), (5, Some(3100)), (6, None)]),
];

fn check((name, feeds, steps, pace, driving, grid, period, crossings): Row) {
    let clock = run(feeds, steps);
    assert_eq!(
        (clock.pace(), clock.driving(), clock.period()),
        (pace, driving, ms(period)),
        "{name}"
    );
    assert!(
        (beats(clock.grid()) - grid).abs() < 1e-6,
        "{name}: grid {}",
        beats(clock.grid())
    );
    for &(beat, at) in crossings {
        assert_eq!(
            clock.crossing(beat),
            at.map_or(Duration::MAX, ms),
            "{name}: the crossing of beat {beat}"
        );
        assert_eq!(
            clock.reached(beat),
            at.is_some_and(|at| ms(at) <= clock.at()),
            "{name}: beat {beat} reached"
        );
    }
}

#[test]
fn in_silence_the_beat_slows_and_stops() {
    SILENCE.into_iter().for_each(check);
}

#[test]
fn a_heard_beat_moves_the_grid_and_the_feed_list_picks_who_drives() {
    BEATS.into_iter().for_each(check);
}

#[test]
fn the_elapsed_count_slows_and_stands_with_the_grid() {
    // (settled to, elapsed in beats)
    let cases = [
        (400, 0.8),
        (500, 1.0),
        (1000, 1.5),
        (1500, 2.0),
        (2600, 2.0),
        (60_000, 2.0),
    ];
    for (at, elapsed) in cases {
        let clock = run(&[Audio], &[Settle(at)]);
        assert!(
            (beats(clock.elapsed_now()) - elapsed).abs() < 1e-6,
            "settled to {at}: elapsed {}",
            beats(clock.elapsed_now())
        );
    }
    // and looked at ahead of the clock, from 400 ms
    let clock = run(&[Audio], &[Settle(400)]);
    for (at, elapsed) in [(1000, 1.5), (2000, 2.0)] {
        assert!(
            (clock.beat_at(ms(at)).beats() - elapsed).abs() < 1e-6,
            "at {at}: elapsed {}",
            clock.beat_at(ms(at)).beats()
        );
    }
}

#[test]
fn the_elapsed_count_never_jumps() {
    for (name, feeds, steps, ..) in SILENCE.into_iter().chain(BEATS) {
        let mut clock = start(feeds);
        for step in steps {
            let settled = clock.settle(ms(step.at()));
            clock = step.on(settled);
            let (before, after) = (settled.beat_at(settled.at()), clock.beat_at(clock.at()));
            assert!(
                (after.beats() - before.beats()).abs() < 1e-9,
                "{name}, {step:?}: elapsed {} then {}",
                before.beats(),
                after.beats()
            );
        }
    }
}

#[test]
fn a_stopped_grid_waits_on_the_beat_it_stands_on() {
    let clock = run(&[Audio], &[Settle(1500)]);
    // (asked, waits for)
    let cases = [
        (When::Now, None),
        (When::Beat, Some(2)),
        (When::Bar, Some(4)),
    ];
    for (when, beat) in cases {
        assert_eq!(clock.next(when), beat, "{when:?}");
    }
}

#[test]
fn a_beat_on_its_crossing_at_the_same_tempo_moves_no_crossing() {
    // 124 BPM: no beat is a whole nanosecond
    let free = Clock::START;
    let heard = free.crossing(1);
    let clock = Clock::START.set_feeds(list(&[Audio])).settle(heard).beat(
        Audio,
        heard,
        Tempo::new(60_000_000_000, 124).unwrap(),
    );
    assert_eq!(clock.crossing(1), heard);
    assert_eq!(clock.crossing(2), free.crossing(2));
    assert_eq!(clock.crossing(3), Duration::MAX);
}

#[test]
fn a_new_engine_has_tap_alone_and_runs_free() {
    assert_eq!(
        (
            Clock::START.feeds(),
            Clock::START.driving(),
            Clock::START.pace()
        ),
        (FeedList::TAP, Some(TAP), Pace::Free)
    );
}

#[test]
fn a_feed_list_is_1_to_5_feeds_no_repeats_tap_only_last() {
    let five: &[TempoFeed] = &[VirtualDJ, Audio, MidiClock, Timecode, TAP];
    // (name, feeds, what comes back)
    let cases: [(&str, &[TempoFeed], Result<(), FeedListError>); 13] = [
        ("Tap", &[TAP], Ok(())),
        ("Audio, then none", &[Audio], Ok(())),
        ("Audio, then Tap", &[Audio, TAP], Ok(())),
        ("five", five, Ok(())),
        (
            "four, then none",
            &[VirtualDJ, Audio, MidiClock, Timecode],
            Ok(()),
        ),
        ("empty", &[], Err(FeedListError::Empty)),
        (
            "a repeat",
            &[Audio, Audio],
            Err(FeedListError::Repeated { feed: Audio }),
        ),
        (
            "a repeat later",
            &[Audio, MidiClock, Audio, TAP],
            Err(FeedListError::Repeated { feed: Audio }),
        ),
        (
            "six: one repeats",
            &[Audio, MidiClock, VirtualDJ, Timecode, Audio, TAP],
            Err(FeedListError::Repeated { feed: Audio }),
        ),
        ("Tap first", &[TAP, Audio], Err(FeedListError::TapNotLast)),
        (
            "Tap twice: the first is not last",
            &[TAP, TAP],
            Err(FeedListError::TapNotLast),
        ),
        (
            "Tap in the middle comes before the repeat",
            &[Audio, TAP, Audio],
            Err(FeedListError::TapNotLast),
        ),
        (
            "the repeat comes before Tap",
            &[Audio, Audio, TAP, MidiClock],
            Err(FeedListError::Repeated { feed: Audio }),
        ),
    ];
    for (name, feeds, want) in cases {
        assert_eq!(
            FeedList::new(feeds).map(|list| list.feeds() == feeds),
            want.map(|()| true),
            "{name}"
        );
    }
    // the same feeds in the same order are the same list
    assert_eq!(list(&[Audio, TAP]), list(&[Audio, TAP]));
    assert_ne!(list(&[Audio, TAP]), list(&[Audio]));
    assert_ne!(list(&[Audio, MidiClock]), list(&[MidiClock, Audio]));
    assert_eq!(list(&[TAP]), FeedList::TAP);
    assert_eq!(
        run(&[Audio], &[List(10, &[Audio, TAP])]).feeds(),
        list(&[Audio, TAP])
    );
}

#[test]
fn a_tempo_is_1_to_1000_beats_of_100_ms_to_4_s() {
    // (span ns, beats, made)
    let cases = [
        (500_000_000, 1, true),
        (0, 1, false),
        (500_000_000, 0, false),
        (100_000_000, 1, true),
        (99_999_999, 1, false),
        (4_000_000_000, 1, true),
        (4_000_000_001, 1, false),
        (60_000_000_000, 300, true),
        (60_000_000_000, 15, true),
        (60_000_000_000, 601, false),
        (500_000_000_000, 1000, true),
        (500_500_000_000, 1001, false),
    ];
    for (span, beats, made) in cases {
        assert_eq!(
            Tempo::new(span, beats).is_some(),
            made,
            "{span} ns, {beats} beats"
        );
    }
}

#[test]
fn equivalent_tempos_keep_each_fractional_crossing_and_its_stale_beat_cutoff() {
    let mut clock = Clock::START.set_feeds(list(&[Audio]));
    let tempo = Tempo::new(15_000_000_000, 31).unwrap();
    for beat in 1..=4 {
        let heard = Clock::START.crossing(beat);
        assert_eq!(clock.crossing(beat), heard);
        clock = clock.beat(Audio, heard, tempo);
        assert_eq!(clock.next(When::Beat), Some(beat));
        clock = clock.beat(Audio, heard - Duration::from_nanos(1), every(640));
        assert_eq!(clock.period(), Clock::START.period());
    }
}

#[test]
fn beats_locks_and_taps_on_a_moving_or_stopped_grid() {
    let cases: &[Row] = &[
        (
            "a delayed beat restores the full grid",
            &[Audio],
            &[Beat(Audio, 500, 800, 500)],
            Pace::Heard,
            Some(Audio),
            1.6,
            500,
            &[(2, Some(1000)), (3, None)],
        ),
        (
            "half speed continues at the new driver's tempo",
            &[Audio],
            &[
                Lock(0, MidiClock, Some(640)),
                List(1000, &[MidiClock]),
                Settle(1639),
            ],
            Pace::Slowing,
            Some(MidiClock),
            1.99921875,
            640,
            &[(2, None)],
        ),
        (
            "a tap takes the stopped beat",
            &[Audio],
            &[
                Lock(0, MidiClock, Some(640)),
                List(1000, &[MidiClock]),
                Tap(1800),
            ],
            Pace::Free,
            Some(MidiClock),
            2.0,
            640,
            &[(2, Some(1800)), (3, Some(2440))],
        ),
        (
            "one tap lapses at the locked feed's tempo",
            &[Audio],
            &[Tap(100), Lock(1000, Audio, Some(640)), Settle(2101)],
            Pace::Heard,
            Some(Audio),
            5.001_562_500_437_5,
            640,
            &[(6, Some(2740)), (7, None)],
        ),
    ];
    cases.iter().copied().for_each(check);
}

#[test]
fn a_stale_first_beat_still_locks_at_the_current_phase() {
    let clock = run(
        &[Audio, TAP],
        &[
            List(400, &[MidiClock, Audio, TAP]),
            Beat(Audio, 350, 420, 640),
        ],
    );
    assert_eq!(clock.driving(), Some(Audio));
    assert_eq!(clock.crossing(1), Duration::from_micros(522_400));
    assert_eq!(clock.crossing(2), Duration::MAX);
}

#[test]
fn time_alone_takes_elapsed_through_a_lapse_and_never_takes_the_stopped_beat() {
    let tapped = run(&[Audio], &[Tap(100)]);
    for (at, elapsed) in [(ms(2100), 4.2), (ms(3100), 5.7), (Duration::MAX, 6.2)] {
        assert!((tapped.beat_at(at).beats() - elapsed).abs() < 1e-9);
        assert_eq!(tapped.beat_at(at), tapped.settle(at).beat_at(at));
    }
    let stopped = tapped.settle(Duration::MAX);
    assert_eq!(stopped.next(When::Beat), Some(7));
    assert!(!stopped.reached(7));
}

#[test]
fn a_late_first_beat_keeps_elapsed_and_every_beat_already_taken() {
    for (delivery, grid, pace, next_crossing) in [
        (750, 1.5, Pace::Slowing, None),
        (1750, 3.5, Pace::Heard, Some(2000)),
    ] {
        let free = start(&[Audio, TAP]).settle(ms(delivery));
        let clock = free.beat(Audio, ms(0), every(500));
        assert_eq!(
            clock.beat_at(clock.at()).beats(),
            free.beat_at(free.at()).beats()
        );
        assert_eq!(beats(clock.grid()), grid);
        assert_eq!(clock.pace(), pace);
        assert!(clock.reached(grid as u64));
        assert_eq!(
            clock.crossing(grid as u64 + 1),
            next_crossing.map_or(Duration::MAX, ms)
        );
    }
}

#[test]
fn a_beat_heard_before_tap_took_over_only_supplies_a_lock() {
    let clock = run(
        &[Audio, TAP],
        &[
            Beat(Audio, 500, 500, 500),
            Lock(700, Audio, None),
            Beat(Audio, 600, 800, 700),
        ],
    );
    assert_eq!(clock.driving(), Some(Audio));
    assert_eq!(clock.crossing(2), ms(1080));
}

#[test]
fn unrelated_locks_keep_the_tap_fallback_at_the_internal_tempo() {
    let clock = run(
        &[Audio, TAP],
        &[
            Beat(Audio, 500, 500, 640),
            Tap(600),
            Lock(700, Audio, None),
            Settle(2700),
        ],
    );
    for (feed, tempo) in [(TAP, None), (MidiClock, Some(every(700)))] {
        assert_eq!(clock.lock(feed, tempo).period(), ms(500));
    }
    assert_eq!(clock.release().period(), ms(500));
}

#[test]
fn fractional_beats_slow_and_stop_at_the_crossing_nanosecond() {
    let clock = Clock::START.set_feeds(list(&[Audio]));
    for (nanos, pace) in [
        (483_870_967, Pace::Heard),
        (483_870_968, Pace::Slowing),
        (483_870_969, Pace::Slowing),
        (1_451_612_903, Pace::Slowing),
        (1_451_612_904, Pace::Stopped),
    ] {
        assert_eq!(
            clock.settle(Duration::from_nanos(nanos)).pace(),
            pace,
            "{nanos}"
        );
    }
}

#[test]
fn tap_taking_over_on_a_fractional_crossing_keeps_the_grid() {
    let free = Clock::START;
    let locked = free
        .set_feeds(list(&[Audio, TAP]))
        .lock(Audio, Tempo::new(60_000_000_000, 124));
    assert_eq!(
        locked.beat_at(free.crossing(1)),
        free.beat_at(free.crossing(1))
    );
    let clock = locked.settle(free.crossing(1)).lock(Audio, None);
    for beat in 1..=4 {
        assert_eq!(clock.crossing(beat), free.crossing(beat), "beat {beat}");
    }
}

#[test]
fn every_external_feed_keeps_its_lock_until_it_drives() {
    let periods = [
        (Timecode, 400),
        (VirtualDJ, 480),
        (MidiClock, 600),
        (Audio, 640),
    ];
    let mut clock = start(&[TAP]);
    for (feed, period) in periods {
        clock = clock.lock(feed, Some(every(period)));
        assert_eq!(clock.driving(), Some(TAP));
    }
    clock = clock.set_feeds(list(&[Timecode, VirtualDJ, MidiClock, Audio, TAP]));
    for (feed, period) in periods {
        assert_eq!((clock.driving(), clock.period()), (Some(feed), ms(period)));
        clock = clock.lock(feed, None);
    }
    assert_eq!((clock.driving(), clock.period()), (Some(TAP), ms(500)));
}

#[test]
fn a_rejected_beat_keeps_its_tempo_for_the_next_driver_change() {
    let clock = run(
        &[MidiClock, Audio],
        &[
            Beat(Audio, 500, 500, 500),
            Beat(Audio, 450, 520, 640),
            Lock(600, MidiClock, Some(1000)),
            Lock(700, MidiClock, None),
        ],
    );
    assert_eq!(clock.crossing(2), ms(1148));
    for (at, pace) in [
        (700, Pace::Heard),
        (1300, Pace::Slowing),
        (2500, Pace::Stopped),
    ] {
        let clock = clock.settle(ms(at));
        assert_eq!(clock.pace(), pace);
        assert_eq!(clock.reading().bpm, 93.75);
    }
}

#[test]
fn a_delayed_beat_moves_the_grid_by_at_most_half_a_beat() {
    let cases = [
        (
            "missed beat, 20 ms late",
            run(&[Audio], &[Beat(Audio, 500, 500, 500), Settle(1520)]),
            1500,
            500,
            3,
            true,
        ),
        (
            "missed beat, 10 ms late",
            run(&[Audio], &[Beat(Audio, 0, 0, 500), Settle(1010)]),
            1000,
            500,
            2,
            true,
        ),
        (
            "slower first beat",
            run(&[Audio, TAP], &[Settle(450)]),
            200,
            1000,
            0,
            false,
        ),
    ];
    for (name, before, heard, period, reached, forward) in cases {
        let after = before.beat(Audio, ms(heard), every(period));
        let expected = if forward {
            before.grid() + ONE / 2
        } else {
            before.grid() - ONE / 2
        };
        assert_eq!(after.grid(), expected, "{name}");
        assert!(after.reached(reached), "{name}");
        assert_eq!(after.elapsed_now(), before.elapsed_now(), "{name}");
    }
}

#[test]
fn the_half_beat_limit_includes_fractions_between_grid_counts() {
    // 139_687_244 × 2 × ONE leaves a remainder of 500_000_016 in the old span.
    let old = start(&[Audio])
        .lock(Audio, Tempo::new(500_000_017, 2))
        .settle(Duration::from_nanos(139_687_244));
    for (heard, period, grid) in [
        (0, 100, old.grid() + ONE / 2),
        (120, 400, old.grid() - ONE / 2 + 1),
    ] {
        assert_eq!(old.beat(Audio, ms(heard), every(period)).grid(), grid);
    }
}

#[test]
fn a_delayed_beat_on_its_crossing_preserves_that_crossing() {
    let clock = run(&[Audio, TAP], &[Beat(Audio, 500, 750, 500)]);
    assert_eq!(clock.crossing(1), ms(500));
    assert_eq!(clock.crossing(2), ms(1000));
}

#[test]
fn a_candidate_below_a_reached_beat_only_supplies_a_lock() {
    // The candidate is 0.675. Clamping first would hide that it fell below beat 1.
    let clock = run(&[Audio, TAP], &[Beat(Audio, 200, 875, 1000)]);
    assert_eq!(clock.driving(), Some(Audio));
    assert_eq!(beats(clock.grid()), 1.75);
    assert_eq!(clock.crossing(2), ms(1125));
}

#[test]
fn one_tap_lapses_to_the_current_driver() {
    let fallback = run(
        &[Audio, TAP],
        &[
            Beat(Audio, 500, 500, 640),
            Tap(600),
            Lock(700, Audio, None),
            Settle(2600),
        ],
    );
    let promoted = run(
        &[MidiClock, Audio],
        &[
            Beat(Audio, 500, 500, 640),
            Tap(600),
            Lock(700, MidiClock, Some(1000)),
            Settle(2600),
        ],
    );
    let locked = run(
        &[Audio],
        &[Tap(100), Lock(1000, Audio, Some(640)), Settle(2100)],
    );
    for (held, old_period, at, driver, period, reached_six) in [
        (fallback, 640, 2700, TAP, 500, false),
        (promoted, 640, 2700, MidiClock, 1000, false),
        (fallback, 640, 3100, TAP, 500, true),
        (locked, 500, 2650, Audio, 640, false),
    ] {
        assert!(matches!(held.source(), Source::OneTap { .. }));
        assert_eq!(held.period(), ms(old_period));
        let clock = held.settle(ms(at));
        assert_eq!(clock.source(), Source::Feed);
        assert_eq!(clock.driving(), Some(driver));
        assert_eq!(clock.period(), ms(period), "{driver:?} at {at} ms");
        assert_eq!(clock.reached(6), reached_six, "{driver:?} at {at} ms");
    }
}

#[test]
fn a_one_tap_lapse_matches_release_at_that_nanosecond() {
    let ns = Duration::from_nanos(1);
    let sent = ms(100);
    let frame = sent + ns;
    let lapse = ms(2100) + ns;
    let measured = Tempo::new(1_900_000_000, 3).unwrap();
    let audio_drives = start(&[Audio, TAP]).lock(Audio, Some(every(640)));
    let cases = [
        ("Tap", Clock::START, None),
        (
            "Audio locks",
            start(&[Audio]),
            Some((Audio, Some(measured))),
        ),
        ("Audio loses its lock", audio_drives, Some((Audio, None))),
        ("no driver", start(&[Audio]), None),
    ];
    for (name, clock, lock) in cases {
        // Both taps take effect in the same frame. The second was sent one ns
        // later and still holds when we explicitly release it at the first's lapse.
        let tap = |sent| {
            let held = clock.settle(frame).settle(sent).tap().settle(ms(1000));
            match lock {
                Some((feed, tempo)) => held.lock(feed, tempo),
                None => held,
            }
        };
        let held = tap(sent);
        assert_eq!(
            held.settle(lapse - ns).source(),
            Source::OneTap { at: sent }
        );
        let later = tap(frame).settle(lapse);
        assert_eq!(later.source(), Source::OneTap { at: frame });
        let released = later.release();
        assert_eq!(held.crossing(2), frame + clock.crossing(1), "{name}");
        if clock.feeds() == FeedList::TAP {
            // Tap keeps the 124 BPM grid's origin, even for its past crossings.
            assert_eq!(held.settle(lapse).crossing(2), held.crossing(2));
        }
        for beat in 5..=8 {
            assert_eq!(
                held.crossing(beat),
                released.crossing(beat),
                "{name}: predict beat {beat}"
            );
        }
        for at in [lapse, lapse + ns, ms(2900), ms(4000), Duration::MAX] {
            let actual = held.settle(at);
            let expected = released.settle(at);
            assert_eq!(actual.reading(), expected.reading(), "{name} at {at:?}");
            assert_eq!(
                (actual.pace(), actual.grid()),
                (expected.pace(), expected.grid()),
                "{name}"
            );
            assert_eq!(held.beat_at(at), expected.beat_at(at), "{name}");
            assert_eq!(held.elapsed_at(at), expected.elapsed_now(), "{name}");
            for when in [When::Beat, When::Bar] {
                assert_eq!(actual.next(when), expected.next(when), "{name} at {at:?}");
            }
            for beat in 5..=8 {
                assert_eq!(actual.crossing(beat), expected.crossing(beat), "{name}");
                assert_eq!(actual.reached(beat), expected.reached(beat), "{name}");
            }
        }
    }
}

#[test]
fn a_beat_heard_before_a_driver_change_or_lapse_keeps_its_phase() {
    let cases = [
        (
            "higher priority feed gained its lock",
            MidiClock,
            run(
                &[MidiClock, Audio],
                &[
                    Beat(Audio, 500, 500, 500),
                    Lock(600, MidiClock, Some(500)),
                    Settle(610),
                ],
            ),
            550,
            640,
            None,
        ),
        (
            "higher priority feed lost its lock",
            Audio,
            run(
                &[MidiClock, Audio],
                &[
                    Lock(0, Audio, Some(500)),
                    Beat(MidiClock, 500, 500, 500),
                    Lock(600, MidiClock, None),
                    Settle(610),
                ],
            ),
            550,
            640,
            None,
        ),
        (
            "one tap lapsed while Tap was the fallback",
            Audio,
            run(&[Audio, TAP], &[Tap(100), Settle(2110)]),
            2050,
            640,
            None,
        ),
        (
            "one tap lapsed and the beat supplies a same-rate lock",
            Audio,
            run(&[Audio, TAP], &[Tap(100), Settle(2150)]),
            2050,
            500,
            Some((6, 2600)),
        ),
    ];
    for (name, feed, old, heard, period, crossing) in cases {
        let clock = old.beat(feed, ms(heard), every(period));
        assert_eq!(clock.driving(), Some(feed), "{name}");
        assert_eq!(clock.grid(), old.grid(), "{name}");
        if let Some((beat, at)) = crossing {
            assert_eq!(old.crossing(beat), ms(at), "{name}");
            assert_eq!(clock.crossing(beat), ms(at), "{name}");
        }
    }
}

#[test]
fn internal_tempo_during_one_tap_changes_the_rate_only_if_tap_drives() {
    let cases: [(&[TempoFeed], &[Step], u64); 2] = [
        (&[TAP], &[Tap(100), Internal(200, 60)], 1000),
        (
            &[Audio, TAP],
            &[Lock(0, Audio, Some(500)), Tap(100), Internal(200, 60)],
            500,
        ),
    ];
    for (feeds, steps, period) in cases {
        let clock = run(feeds, steps);
        assert!(matches!(clock.source(), Source::OneTap { .. }));
        assert_eq!(clock.reading().feed, bpm(60));
        assert_eq!(clock.period(), ms(period), "{feeds:?}");
    }
}

#[test]
fn a_beat_heard_after_one_tap_lapses_aligns_the_grid() {
    let clock = run(&[Audio], &[Tap(100), Beat(Audio, 2150, 2150, 640)]);
    assert_eq!(clock.source(), Source::Feed);
    assert_eq!(clock.driving(), Some(Audio));
    assert_eq!(clock.crossing(6), ms(2790));
}

#[test]
fn a_new_list_applies_the_current_drivers_lock() {
    let old = run(
        &[Audio],
        &[
            Beat(Audio, 500, 500, 500),
            Lock(600, Audio, Some(640)),
            Settle(700),
        ],
    );
    let clock = old.set_feeds(list(&[Audio, TAP]));
    assert_eq!(clock.grid(), old.grid());
    assert_eq!((clock.period(), clock.crossing(2)), (ms(640), ms(1084)));
}

#[test]
fn a_beat_while_one_tap_holds_only_records_its_lock() {
    let old = run(&[Audio], &[Tap(100), Settle(300)]);
    let clock = old.beat(Audio, ms(300), every(640));
    assert_eq!(clock.driving(), Some(Audio));
    assert_eq!(
        (clock.pace(), clock.period(), beats(clock.grid())),
        (Pace::Free, old.period(), beats(old.grid()))
    );
}

#[test]
fn a_lock_updated_at_the_lapse_does_not_release_the_tap_again() {
    let lapse = ms(2100) + Duration::from_nanos(1);
    let held = run(&[Audio], &[Tap(100), Lock(1000, Audio, Some(640))]);
    let released = held.settle(lapse).lock(Audio, Some(every(1000)));
    for at in [lapse, lapse + Duration::from_nanos(1)] {
        assert_eq!(released.settle(at).period(), ms(640), "at {at:?}");
    }
}

#[test]
fn a_beat_heard_before_the_last_beat_was_taken_moves_nothing() {
    let taken = run(
        &[Audio],
        &[Beat(Audio, 500, 500, 500), Beat(Audio, 990, 1100, 500)],
    );
    let stale = taken.beat(Audio, ms(1050), every(640));
    assert_eq!(
        (stale.grid(), stale.period(), stale.crossing(3)),
        (taken.grid(), taken.period(), taken.crossing(3))
    );
}

#[test]
fn the_next_tap_after_a_lapse_takes_the_stopped_beat_at_its_own_time() {
    let stopped = run(
        &[Audio],
        &[Tap(100), Lock(1000, Audio, Some(640)), Settle(5000)],
    );
    assert!(!stopped.reached(7));
    assert_eq!(stopped.tap().crossing(7), ms(5000));
}

#[test]
fn dating_follows_its_table() {
    let (a, b) = (Clock::START.origin, start(&[Audio]).origin);
    let lapse = |before, moved| DatingEvent::Lapse { before, moved };
    let lapsed = |tapped| Dating::Lapsed { tapped };
    // (dating, event, after): every cell of tempo-feeds.md 4a, "Dating"
    let cases = [
        (Dating::Current, lapse(a, true), lapsed(a)),
        (Dating::Current, lapse(a, false), Dating::Current),
        (Dating::Current, DatingEvent::Rebase, Dating::Current),
        (lapsed(a), lapse(b, true), lapsed(b)),
        (lapsed(a), lapse(b, false), lapsed(a)),
        (lapsed(a), DatingEvent::Rebase, Dating::Current),
    ];
    for (dating, event, after) in cases {
        assert_eq!(dating.after(event), after, "{dating:?} + {event:?}");
    }
}

#[test]
fn a_lapse_that_moves_the_grid_keeps_the_tapped_grid_until_the_grid_moves_again() {
    // One tap at 100 ms at 512 ms a beat, Audio locked at 640 ms from 1000 ms: the
    // tap lapses at 2100 ms and the release moves the origin to Audio's tempo.
    let held = run(
        &[Audio],
        &[
            Lock(0, Audio, Some(512)),
            Tap(100),
            Lock(1000, Audio, Some(640)),
            Settle(2080),
        ],
    );
    assert_eq!(held.dating, Dating::Current);
    let lapsed = held.settle(ms(2200));
    assert_eq!(
        lapsed.dating,
        Dating::Lapsed {
            tapped: held.origin
        }
    );
    // (step at 2300 ms, whether the grid moves, by tempo-feeds.md 4a)
    let cases = [
        (Settle(2300), false),                     // time alone
        (Tap(2300), true),                         // a tap aligns the grid
        (Release(2300), false),                    // source already Feed: stays
        (Internal(2300, 90), false),               // Audio drives: only the internal tempo
        (List(2300, &[TAP]), true),                // a new list: its driver at the phase now
        (List(2300, &[Audio]), false),             // the same list: a release, stays
        (Lock(2300, Audio, Some(700)), false),     // same driver: only its row
        (Lock(2300, MidiClock, Some(700)), false), // not in the list: recorded
        (Beat(Audio, 2240, 2300, 640), true),      // a beat from the driver aligns
    ];
    for (step, moves) in cases {
        let clock = step.on(lapsed.settle(ms(step.at())));
        let expected = if moves {
            Dating::Current
        } else {
            lapsed.dating
        };
        assert_eq!(clock.origin.from != lapsed.origin.from, moves, "{step:?}");
        assert_eq!(clock.dating, expected, "{step:?}");
    }
}
