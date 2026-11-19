//! The beat clock: one continuous count of elapsed beats drives every play, and a grid
//! of beats and bars, which a tap may move, schedules launches. Data and calculations:
//! the tick thread settles it in time order, a tap or a tempo change at the time it was
//! sent (`update`).
//!
//! ```text
//! Feed   --tap: aligns the grid-------------------------------------> OneTap
//! OneTap --a second tap, 200 ms to 2 s on: its interval------------> Tapped
//! OneTap --2 s with no tap (lapse), ReleaseTempo-------------------> Feed
//! Tapped --tap: a valid interval sets the tempo, every tap aligns---> Tapped
//! Tapped --ReleaseTempo: the feed's tempo at the grid's phase-------> Feed
//! ```
//!
//! A tap under 200 ms after the last is a bounce: ignored. A tap more than 2 s after the
//! last starts a fresh interval at the current tempo. Intervals and the lapse count from
//! the time each tap was sent, even when it missed a frame. A tap marks a beat: at an
//! exact beat the grid keeps its number, otherwise it moves on to the next one. The
//! elapsed count never jumps; only its rate changes. A tempo is kept exactly (124 BPM is
//! 124 beats a minute), and every decision on the grid is made by [`Clock::crossing`].

use core::time::Duration;
use serde::{Deserialize, Serialize, Serializer};

/// Beats in a bar of the grid.
pub const BEATS_PER_BAR: u64 = 4;

/// One beat, in the fixed point the clock counts in.
const ONE: u64 = 1 << 32;

/// Taps closer than this are one tap bouncing.
const BOUNCE: Duration = Duration::from_millis(200);

/// Taps further apart than this make no interval; one tap lapses after it.
const LAPSE: Duration = Duration::from_secs(2);

/// A tempo the internal feed takes: 30 to 300 beats a minute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Bpm(u16);

impl Bpm {
    /// The internal feed's tempo until a client sets one.
    pub const FEED: Self = Self(124);

    /// `None` outside 30 to 300.
    pub const fn new(bpm: u16) -> Option<Self> {
        if 30 <= bpm && bpm <= 300 {
            Some(Self(bpm))
        } else {
            None
        }
    }

    /// Its beats in a minute, exactly: 124 BPM is no whole number of nanoseconds a beat.
    const fn tempo(self) -> Tempo {
        Tempo {
            span: 60_000_000_000,
            beats: self.0 as u64,
        }
    }
}

/// How fast the clock counts: `beats` beats in `span` nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tempo {
    span: u64,
    beats: u64,
}

impl Tempo {
    /// One beat in `gap`, a tap's interval: 200 ms to 2 s.
    fn every(gap: Duration) -> Self {
        Self {
            span: u64::try_from(gap.as_nanos()).unwrap_or(u64::MAX),
            beats: 1,
        }
    }

    /// Whether both count at the same rate.
    fn same(self, other: Self) -> bool {
        u128::from(self.span) * u128::from(other.beats)
            == u128::from(other.span) * u128::from(self.beats)
    }
}

impl TryFrom<u16> for Bpm {
    type Error = &'static str;

    fn try_from(bpm: u16) -> Result<Self, Self::Error> {
        Self::new(bpm).ok_or("a tempo is 30 to 300 beats a minute")
    }
}

/// When a play starts: at once, or on the first beat or bar of the grid on or after the
/// request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum When {
    #[default]
    Now,
    Beat,
    Bar,
}

/// Who sets the tempo, and the last tap that counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Feed,
    OneTap { at: Duration },
    Tapped { at: Duration },
}

impl Serialize for Source {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (index, name) = match self {
            Self::Feed => (0, "Feed"),
            Self::OneTap { .. } => (1, "OneTap"),
            Self::Tapped { .. } => (2, "Tapped"),
        };
        serializer.serialize_unit_variant("Source", index, name)
    }
}

/// The clock as a picture shows it, at the time it was settled to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Reading {
    /// The tempo in charge.
    pub bpm: f64,
    /// The internal feed's tempo.
    pub feed: Bpm,
    #[cfg_attr(test, ts(type = "\"Feed\" | \"OneTap\" | \"Tapped\""))]
    pub source: Source,
    /// Where the grid is, in beats: a whole number is a beat, a multiple of four a bar.
    pub beat: f64,
    /// Beats elapsed since the engine started. A play's own beat is this less its start.
    pub elapsed: f64,
}

/// The beat clock. Every change happens at the time it was settled to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// The time the clock was settled to, since the engine started.
    at: Duration,
    /// The time the last settle asked for: when the event it comes before was sent.
    /// Earlier than `at` for an event sent before a frame it missed.
    sent: Duration,
    /// From this time on, the clock counts at `tempo`; `elapsed` and `grid` are the
    /// counts then, in [`ONE`]s.
    from: Duration,
    elapsed: u64,
    grid: u64,
    tempo: Tempo,
    feed: Bpm,
    /// A one tap that lapsed stays here: [`Clock::source`] tells it as the feed.
    source: Source,
}

impl Clock {
    /// A new engine's clock: the internal feed at [`Bpm::FEED`], on beat zero.
    pub const START: Self = Self {
        at: Duration::ZERO,
        sent: Duration::ZERO,
        from: Duration::ZERO,
        elapsed: 0,
        grid: 0,
        tempo: Bpm::FEED.tempo(),
        feed: Bpm::FEED,
        source: Source::Feed,
    };

    /// Who sets the tempo now: one tap lapses 2 s after it was sent.
    pub fn source(&self) -> Source {
        match self.source {
            Source::OneTap { at: tap } if self.at.saturating_sub(tap) > LAPSE => Source::Feed,
            source @ (Source::Feed | Source::OneTap { .. } | Source::Tapped { .. }) => source,
        }
    }

    /// How long a beat lasts now, to the nanosecond below.
    pub fn period(&self) -> Duration {
        Duration::from_nanos(self.tempo.span / self.tempo.beats.max(1))
    }

    /// The time the clock was settled to.
    pub const fn at(&self) -> Duration {
        self.at
    }

    /// The clock at `t`, if that is later, for an event sent at `t`.
    pub fn settle(self, t: Duration) -> Self {
        Self {
            at: self.at.max(t),
            sent: t,
            ..self
        }
    }

    /// The elapsed count at `t`, which is not before the last change, in [`ONE`]s.
    pub fn elapsed_at(&self, t: Duration) -> u64 {
        self.elapsed.saturating_add(self.count(t))
    }

    /// The elapsed count now, in [`ONE`]s.
    pub fn elapsed_now(&self) -> u64 {
        self.elapsed_at(self.at)
    }

    /// Where the grid is now, in [`ONE`]s.
    pub fn grid(&self) -> u64 {
        self.grid.saturating_add(self.count(self.at))
    }

    /// The beat a launch asked for now waits for: `None` for at once. A request on the
    /// beat (or bar) waits for none.
    pub fn next(&self, when: When) -> Option<u64> {
        let beat = self.first_beat();
        match when {
            When::Now => None,
            When::Beat => Some(beat),
            When::Bar => Some(beat.div_ceil(BEATS_PER_BAR).saturating_mul(BEATS_PER_BAR)),
        }
    }

    /// The first beat whose crossing is now or later: the one the grid is on, else the
    /// next. Decided by [`Clock::crossing`], as a launch is.
    fn first_beat(&self) -> u64 {
        let below = self.grid() / ONE;
        let on = below.saturating_mul(ONE) >= self.grid && self.crossing(below) == self.at;
        if on { below } else { below.saturating_add(1) }
    }

    /// When the grid reaches beat `beat`: the last change if it was there already.
    pub fn crossing(&self, beat: u64) -> Duration {
        let to = u128::from(beat.saturating_mul(ONE).saturating_sub(self.grid));
        let nanos = (to * u128::from(self.tempo.span))
            .div_ceil(u128::from(ONE) * u128::from(self.tempo.beats.max(1)));
        self.from.saturating_add(Duration::from_nanos(
            u64::try_from(nanos).unwrap_or(u64::MAX),
        ))
    }

    /// Whether the grid reached beat `beat` by now.
    pub fn reached(&self, beat: u64) -> bool {
        self.crossing(beat) <= self.at
    }

    /// A tap, sent when the last settle asked for: its interval counts from the time it
    /// was sent, the beat it marks is now.
    pub fn tap(self) -> Self {
        let sent = self.sent;
        let (source, tempo) = match self.source {
            Source::Feed => (Source::OneTap { at: sent }, self.tempo),
            Source::OneTap { at: last } | Source::Tapped { at: last } => {
                let gap = sent.saturating_sub(last);
                if gap < BOUNCE {
                    return self;
                }
                match self.source {
                    _ if gap <= LAPSE => (Source::Tapped { at: sent }, Tempo::every(gap)),
                    Source::Tapped { .. } => (Source::Tapped { at: sent }, self.tempo),
                    Source::Feed | Source::OneTap { .. } => {
                        (Source::OneTap { at: sent }, self.tempo)
                    }
                }
            }
        };
        let beat = self.first_beat();
        Self {
            grid: beat.saturating_mul(ONE),
            tempo,
            source,
            ..self.rebase()
        }
    }

    /// The tempo goes back to the feed, at the grid's phase now.
    pub fn release(self) -> Self {
        Self {
            source: Source::Feed,
            ..self.at_tempo(self.feed.tempo())
        }
    }

    /// The internal feed takes `bpm`: the tempo now, unless taps set it.
    pub fn set_feed(self, bpm: Bpm) -> Self {
        let tempo = match self.source {
            Source::Feed | Source::OneTap { .. } => bpm.tempo(),
            Source::Tapped { .. } => self.tempo,
        };
        Self {
            feed: bpm,
            ..self.at_tempo(tempo)
        }
    }

    /// The same clock at `tempo` from now on. The same rate changes nothing: a count
    /// taken again rounds, and would move a crossing.
    fn at_tempo(self, tempo: Tempo) -> Self {
        if self.tempo.same(tempo) {
            self
        } else {
            Self {
                tempo,
                ..self.rebase()
            }
        }
    }

    /// The same clock, its counts taken now: ready for a change of tempo or grid.
    fn rebase(self) -> Self {
        Self {
            from: self.at,
            elapsed: self.elapsed_now(),
            grid: self.grid(),
            ..self
        }
    }

    /// Beats from the last change to `t`, in [`ONE`]s.
    fn count(&self, t: Duration) -> u64 {
        let since = t.saturating_sub(self.from).as_nanos();
        let count = since.saturating_mul(u128::from(ONE) * u128::from(self.tempo.beats))
            / u128::from(self.tempo.span.max(1));
        u64::try_from(count).unwrap_or(u64::MAX)
    }

    /// The clock as a picture shows it.
    pub fn reading(&self) -> Reading {
        Reading {
            bpm: 60e9 * self.tempo.beats as f64 / self.tempo.span.max(1) as f64,
            feed: self.feed,
            source: self.source(),
            beat: beats(self.grid()),
            elapsed: beats(self.elapsed_now()),
        }
    }
}

impl Serialize for Clock {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.reading().serialize(serializer)
    }
}

/// A count in [`ONE`]s as beats.
pub fn beats(count: u64) -> f64 {
    count as f64 / ONE as f64
}

#[cfg(test)]
mod tests {
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
        // 120 to 60 BPM at 100 ms: 0.2 beats, beat 1 at the clock's own crossing
        let slowed = run(&[Feed(100, 60)]);
        let on = slowed.settle(slowed.crossing(1));
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
}
