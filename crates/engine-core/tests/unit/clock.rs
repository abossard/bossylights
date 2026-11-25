use super::*;

const fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

const fn bpm(bpm: u16) -> Bpm {
    match Bpm::new(bpm) {
        Some(bpm) => bpm,
        None => Bpm::FEED,
    }
}

/// What happens to the clock, each at its time.
#[derive(Clone, Copy, Debug)]
enum Event {
    Tap(u64),
    Release(u64),
    Feed(u64, u16),
    /// Time passes, with nothing sent.
    Settle(u64),
    /// A tap sent at the first time, after a frame at the second took the clock on.
    Late(u64, u64),
}

fn run(events: &[Event]) -> Clock {
    let mut clock = Clock::START.set_feed(bpm(120));
    for event in events {
        clock = match *event {
            Event::Tap(t) => clock.settle(ms(t)).tap(),
            Event::Late(sent, frame) => clock.settle(ms(frame)).settle(ms(sent)).tap(),
            Event::Release(t) => clock.settle(ms(t)).release(),
            Event::Feed(t, b) => clock.settle(ms(t)).set_feed(bpm(b)),
            Event::Settle(t) => clock.settle(ms(t)),
        };
    }
    clock
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Feed,
    OneTap,
    Tapped,
}

fn kind(source: Source) -> Kind {
    match source {
        Source::Feed => Kind::Feed,
        Source::OneTap { .. } => Kind::OneTap,
        Source::Tapped { .. } => Kind::Tapped,
    }
}

use Event::{Feed, Late, Release, Settle, Tap};

#[test]
fn a_wave_started_between_counts_keeps_its_midpoint_across_a_tap() {
    use crate::{Binding, Node, Program, Rgb, Use};

    let feed = Clock::START.set_feed(bpm(125));
    let tap_at =
        |clock: Clock, at: Duration, period| clock.settle(at - period).tap().settle(at).tap();
    let tapped = tap_at(feed, ms(480), ms(250));
    // Two tap periods with no common factor. Each change is on a whole
    // beat so the absolute clock is exact. Test both orders of their spans.
    let short = Duration::from_nanos(240_000_006);
    let long = Duration::from_nanos(250_000_001);
    let a = tap_at(feed, ms(480), short);
    let change = ms(480) + 12 * short;
    let b = tap_at(a, change, long);
    let back = change + 12 * long;
    let c = tap_at(b, back, short);
    let red = Rgb {
        red: u16::MAX,
        green: 0,
        blue: 0,
    };
    let green = Rgb {
        red: 0,
        green: u16::MAX,
        blue: 0,
    };
    let nodes = [Node::Wave {
        period: 1,
        swatches: 3,
    }];
    // red, green and blue of each swatch: the words of one RGB target
    let words: Vec<Option<u16>> = [red, green, green]
        .iter()
        .flat_map(|c| [Some(c.red), Some(c.green), Some(c.blue)])
        .collect();
    let wave = Program {
        nodes: &nodes,
        items: &[],
        root: 0,
        words: &words,
    };
    let bindings = [Binding {
        node: 0,
        index: 0,
        count: 1,
        words: 0,
    }];
    let target = crate::Sampled {
        bindings: &bindings,
        fades: &[true; 3],
    };
    // A three-swatch Wave has red/green midpoints at 1/6 and 5/6 beat.
    for (start, end) in [
        (feed.beat_at(ms(80)), feed.beat_at(ms(480))),
        (feed.beat_at(ms(80)), tapped.beat_at(ms(480))),
        (a.beat_at(ms(480) + short / 6), b.beat_at(change)),
        (b.beat_at(change), c.beat_at(back + short / 6)),
    ] {
        let (mut out, mut spare) = ([None; 3], [None; 3 * crate::MAX_DEPTH]);
        let elapsed = crate::Elapsed::between(end, start);
        wave.sample(target, elapsed, Use::default(), &mut out, &mut spare);
        assert_eq!(out, [Some(32_768), Some(32_768), Some(0)]);
    }
}

#[test]
fn every_source_takes_every_event() {
    // feed at 120 (500 ms); one tap at 1000; tapped at 1000 and 1600 (600 ms)
    let one: &[Event] = &[Tap(1000)];
    let tapped: &[Event] = &[Tap(1000), Tap(1600)];
    // (name, from, event, source after, beat after)
    let cases: [(&str, &[Event], Event, Kind, Duration); 26] = [
        (
            "one tap, 200 ms sent, after a frame 1 ms on",
            &[Late(1000, 1001)],
            Late(1200, 1200),
            Kind::Tapped,
            ms(200),
        ),
        (
            "one tap, exactly 2 s sent, after a frame 1 ms on",
            one,
            Late(3000, 3001),
            Kind::Tapped,
            ms(2000),
        ),
        (
            "tapped, lost phase, then 750 ms: the fresh tap counts",
            &[Tap(1000), Tap(1600), Tap(5000)],
            Tap(5750),
            Kind::Tapped,
            ms(750),
        ),
        (
            "tapped, lost phase, then 750 ms after a bounce",
            &[Tap(1000), Tap(1600), Tap(5000), Tap(5100)],
            Tap(5750),
            Kind::Tapped,
            ms(750),
        ),
        ("feed, tap", &[], Tap(1000), Kind::OneTap, ms(500)),
        ("feed, silence", &[], Settle(5000), Kind::Feed, ms(500)),
        ("feed, release", &[], Release(1000), Kind::Feed, ms(500)),
        ("feed, new feed", &[], Feed(1000, 60), Kind::Feed, ms(1000)),
        ("one tap, bounce", one, Tap(1199), Kind::OneTap, ms(500)),
        ("one tap, 200 ms", one, Tap(1200), Kind::Tapped, ms(200)),
        ("one tap, 750 ms", one, Tap(1750), Kind::Tapped, ms(750)),
        (
            "one tap, exactly 2 s",
            one,
            Tap(3000),
            Kind::Tapped,
            ms(2000),
        ),
        ("one tap, past 2 s", one, Tap(3001), Kind::OneTap, ms(500)),
        (
            "one tap, 2 s silence",
            one,
            Settle(3000),
            Kind::OneTap,
            ms(500),
        ),
        ("one tap, lapse", one, Settle(3001), Kind::Feed, ms(500)),
        ("one tap, release", one, Release(1100), Kind::Feed, ms(500)),
        (
            "one tap, new feed",
            one,
            Feed(1100, 60),
            Kind::OneTap,
            ms(1000),
        ),
        ("tapped, bounce", tapped, Tap(1700), Kind::Tapped, ms(600)),
        ("tapped, 800 ms", tapped, Tap(2400), Kind::Tapped, ms(800)),
        (
            "tapped, exactly 2 s",
            tapped,
            Tap(3600),
            Kind::Tapped,
            ms(2000),
        ),
        ("tapped, past 2 s", tapped, Tap(3601), Kind::Tapped, ms(600)),
        (
            "tapped, silence",
            tapped,
            Settle(60_000),
            Kind::Tapped,
            ms(600),
        ),
        (
            "tapped, release",
            tapped,
            Release(2000),
            Kind::Feed,
            ms(500),
        ),
        (
            "tapped, new feed",
            tapped,
            Feed(2000, 60),
            Kind::Tapped,
            ms(600),
        ),
        (
            "tapped, new feed, release",
            &[Tap(1000), Tap(1600), Feed(2000, 60)],
            Release(2100),
            Kind::Feed,
            ms(1000),
        ),
        (
            "one tap lapsed, tap",
            &[Tap(1000), Settle(3500)],
            Tap(3600),
            Kind::OneTap,
            ms(500),
        ),
    ];
    for (name, from, event, source, period) in cases {
        let mut events = from.to_vec();
        events.push(event);
        let clock = run(&events);
        assert_eq!(
            (kind(clock.source()), clock.period()),
            (source, period),
            "{name}"
        );
    }
}

#[test]
fn a_bounce_leaves_the_last_tap_as_it_was() {
    // a bounce at 1100 must not restart the interval: 1600 is 600 ms after 1000
    let clock = run(&[Tap(1000), Tap(1100), Tap(1600)]);
    assert_eq!(clock.period(), ms(600));
    // the next interval counts from the last valid tap, not from a bounce
    let clock = run(&[Tap(1000), Tap(1600), Tap(1650), Tap(2300)]);
    assert_eq!(clock.period(), ms(700));
    // and moves nothing: the grid stays where time took it. (name, from, bounce at)
    let cases: [(&str, &[Event], u64); 2] = [
        ("one tap, 100 ms", &[Tap(1000)], 1100),
        ("tapped, 100 ms", &[Tap(1000), Tap(1600)], 1700),
    ];
    for (name, from, at) in cases {
        let mut bounced = from.to_vec();
        bounced.push(Tap(at));
        let mut waited = from.to_vec();
        waited.push(Settle(at));
        assert_eq!(run(&bounced), run(&waited), "{name}");
    }
}

#[test]
fn the_grid_decides_by_its_crossings() {
    let at_60_s = Duration::from_secs(60);
    // 124 BPM: 60 s is beat 124 exactly, a bar
    let fed = Clock::START.settle(at_60_s);
    assert_eq!(fed.next(When::Bar), Some(124), "124 BPM, a bar at 60 s");
    assert_eq!(fed.crossing(124), at_60_s);
    // 120 to 60 BPM at 100 ms: 0.2 beats, then 0.8 of a second each: beat 1 at 900 ms
    let slowed = run(&[Feed(100, 60)]);
    assert_eq!(slowed.crossing(1), ms(900), "slowed at 100 ms, beat 1");
    let on = slowed.settle(ms(900));
    assert!(on.reached(1));
    assert_eq!(on.next(When::Beat), Some(1), "a beat asked on its crossing");
    assert_eq!(
        beats(on.tap().grid()),
        1.0,
        "a tap on a crossing keeps the beat"
    );
    // the same tempo again, or the feed while tapped: no crossing moves.
    // (name, events, the beat waited for, its crossing)
    let cases = [
        ("the feed at 120 again", vec![Feed(100, 120)], 1, ms(500)),
        (
            "the feed while tapped",
            vec![Tap(0), Tap(750), Feed(850, 100)],
            3,
            ms(1500),
        ),
        (
            "released at the feed's tempo",
            vec![Release(100)],
            1,
            ms(500),
        ),
    ];
    for (name, events, beat, crossing) in cases {
        assert_eq!(run(&events).crossing(beat), crossing, "{name}");
    }
}

#[test]
fn a_change_on_a_crossing_keeps_the_grid_on_its_beat() {
    // 124 BPM: no beat is a whole nanosecond. (name, the change on the crossing of
    // `on`, asked, waits for)
    let on = |beat| Clock::START.settle(Clock::START.crossing(beat));
    let cases: [(&str, Clock, When, u64); 5] = [
        ("on beat 16, a bar", on(16), When::Bar, 16),
        (
            "the feed on beat 16, a bar",
            on(16).set_feed(bpm(120)),
            When::Bar,
            16,
        ),
        (
            "the feed on beat 16, a beat",
            on(16).set_feed(bpm(120)),
            When::Beat,
            16,
        ),
        (
            "the feed and a tap on beat 3, a beat",
            on(3).set_feed(bpm(120)).tap(),
            When::Beat,
            3,
        ),
        (
            "the feed and a tap on beat 3, a bar",
            on(3).set_feed(bpm(120)).tap(),
            When::Bar,
            4,
        ),
    ];
    for (name, clock, when, beat) in cases {
        assert_eq!(clock.next(when), Some(beat), "{name}");
        assert_eq!(clock.reached(beat), beat != 4, "{name}");
    }
}

#[test]
fn a_tap_marks_a_beat_and_the_elapsed_count_runs_on() {
    // feed at 120: beat n at n * 500 ms. (name, events, grid after, elapsed after)
    let cases = [
        (
            "at an exact beat it keeps the number",
            vec![Tap(1000)],
            2.0,
            2.0,
        ),
        (
            "off the grid it moves on to the next",
            vec![Tap(1100)],
            3.0,
            2.2,
        ),
        (
            "just past a beat, on to the next",
            vec![Tap(1001)],
            3.0,
            2.002,
        ),
        (
            // the beat it marks is the frame's, not the one after its send time
            "sent just before a beat, seen after it: on to the next",
            vec![Late(1499, 1520)],
            4.0,
            3.04,
        ),
        (
            // 1100 aligns to 3; 1850 is 750 ms on: 1.5 beats at 120, so 4.5 -> 5
            "a second tap aligns again, at the old tempo up to it",
            vec![Tap(1100), Tap(1850)],
            5.0,
            3.7,
        ),
        (
            // tapped at 750 ms from 1100: beat 5 at 1850, beat 6 at 2600
            "the tapped tempo runs on",
            vec![Tap(1100), Tap(1850), Settle(2600)],
            6.0,
            3.7 + 1.0,
        ),
        (
            // lost phase: after 2 s the tempo stays, the grid aligns afresh
            "a tap after a long gap keeps the tempo",
            vec![Tap(1000), Tap(1600), Tap(5000)],
            // 1600 aligned to 4 (from 3.2), 600 ms a beat: 5000 is 4 + 3400/600
            10.0,
            3.2 + 3400.0 / 600.0,
        ),
        (
            // release at 1850 + 375: half a beat of 750 ms, resumed at 500 ms a beat
            "a release keeps the grid's phase",
            vec![Tap(1100), Tap(1850), Release(2225), Settle(2475)],
            6.0,
            3.7 + 0.5 + 0.5,
        ),
    ];
    for (name, events, grid, elapsed) in cases {
        let clock = run(&events);
        assert!(
            (beats(clock.grid()) - grid).abs() < 1e-6,
            "{name}: grid {}",
            beats(clock.grid())
        );
        assert!(
            (beats(clock.elapsed_now()) - elapsed).abs() < 1e-6,
            "{name}: elapsed {}",
            beats(clock.elapsed_now())
        );
    }
}

#[test]
fn taps_and_release_carry_elapsed_fractions_across_different_tempo_spans() {
    // At the second tap the feed has supplied 1.42 beats, the tap period is
    // 700 ms, and the next half beat is 56 ms away.
    let tapped = run(&[Tap(10), Tap(710)]);
    // 4.2 ms later the tapped clock has added 0.006 beats. Releasing to
    // 60 BPM leaves 74 ms to the same 1.5-beat edge.
    let released = tapped
        .set_feed(bpm(60))
        .settle(Duration::from_micros(714_200))
        .release();
    for (clock, edge) in [(tapped, 766_000), (released, 788_200)] {
        for (micros, reached) in [(edge - 1, false), (edge, true), (edge + 1, true)] {
            assert_eq!(
                clock.elapsed_at(Duration::from_micros(micros)) >= 3 * ONE / 2,
                reached
            );
        }
    }
}

#[test]
fn a_launch_waits_for_the_next_beat_or_bar() {
    // feed at 120. (name, request at, when, beat, crossing)
    let cases = [
        ("now", 1100, When::Now, None, None),
        (
            "a beat, off the grid",
            1100,
            When::Beat,
            Some(3),
            Some(1500),
        ),
        ("a beat, on the grid", 1500, When::Beat, Some(3), Some(1500)),
        (
            "a bar, through the beats",
            2600,
            When::Bar,
            Some(8),
            Some(4000),
        ),
        ("a bar, on a bar", 2000, When::Bar, Some(4), Some(2000)),
        ("a bar, on a beat", 2500, When::Bar, Some(8), Some(4000)),
    ];
    for (name, at, when, beat, crossing) in cases {
        let clock = run(&[Settle(at)]);
        let next = clock.next(when);
        assert_eq!(next, beat, "{name}");
        assert_eq!(
            next.map(|beat| clock.crossing(beat)),
            crossing.map(ms),
            "{name}"
        );
        assert_eq!(
            next.map(|beat| clock.reached(beat)),
            beat.map(|_| crossing == Some(at)),
            "{name}"
        );
    }
}

#[test]
fn an_off_grid_tap_moves_a_waiting_crossing_once() {
    // waiting for bar 8 (4000 ms at 120); a tap at 3150 (6.3) aligns to 7
    let clock = run(&[Tap(3150)]);
    assert_eq!(clock.crossing(8), ms(3650));
    assert!(!clock.reached(8));
    assert!(clock.settle(ms(3650)).reached(8));
    // a tap at 3150 waiting for beat 7: the tap supplies it
    assert!(clock.reached(7));
    assert_eq!(clock.crossing(7), ms(3150));
}

#[test]
fn bpm_is_30_to_300() {
    let cases = [(29, false), (30, true), (300, true), (301, false)];
    for (bpm, ok) in cases {
        assert_eq!(Bpm::new(bpm).is_some(), ok, "{bpm}");
        assert_eq!(Bpm::try_from(bpm).is_ok(), ok, "{bpm}");
    }
}

#[test]
fn the_clock_never_goes_back() {
    let clock = run(&[Settle(2000), Settle(1000)]);
    assert_eq!(beats(clock.elapsed_now()), 4.0);
}
