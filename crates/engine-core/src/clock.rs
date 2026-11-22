//! Playing looks follow a continuous count of elapsed beats. Waiting looks start on a
//! grid of beats and bars. A tap or a heard beat can move the grid while elapsed beats
//! keep counting from the same position.
//!
//! Who sets the tempo
//!
//! ```text
//! Feed   --tap: aligns the grid-------------------------------------> OneTap
//! OneTap --a second tap, 200 ms to 2 s on: its interval------------> Tapped
//! OneTap --first nanosecond past 2 s, release, or set the list------> Feed
//! Tapped --tap: a valid interval sets the tempo, every tap aligns---> Tapped
//! Tapped --ReleaseTempo, a new list: the list's driver--------------> Feed
//! ```
//!
//! How the beat moves
//!
//! ```text
//! Free    --taps end and Tap does not drive: anchor on the beat reached--> Heard
//! Heard   --the grid reaches anchor + 1 unheard: half tempo-------------> Slowing
//! Slowing --the grid reaches anchor + 2: stands on it, not taken--------> Stopped
//! any     --a beat aligns the grid: anchor on its nearest beat----------> Heard
//! any     --Tap drives, or taps hold-----------------------------------> Free
//! ```
//!
//! The feed list holds 1 to 5 feeds in order, Tap only last. The first with a lock
//! drives. Tap always has a lock at the internal tempo. When one tap lapses, the
//! clock releases it at that instant and follows the current driver. A waiting look
//! keeps the time of a beat crossed before the lapse, even on a later wake (`Dating`:
//! Lapsed until the grid moves again). Each heard beat
//! records its feed's lock. The driver can move the grid by up to half a beat when taps
//! are released, the beat was heard since the last change and it keeps every beat
//! already reached.
//!
//! A tap under 200 ms after the last is a bounce: ignored. A tap more than 2 s after the
//! last starts a fresh interval at the current tempo. Intervals and the lapse count from
//! the time each tap was sent, even when it missed a frame. A tap marks a beat: at an
//! exact beat the grid keeps its number, otherwise it moves on to the next one. A heard
//! beat marks the nearest beat. The elapsed count never jumps. Only its rate changes. A
//! tempo is kept exactly (124 BPM is 124 beats a minute), and every decision on the grid
//! is made by [`Clock::crossing`].

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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
pub struct Tempo {
    span: u64,
    beats: u64,
}

impl Tempo {
    /// A measured tempo of 1 to 1000 beats, each between 100 ms and 4 s inclusive.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "the tick takes measured tempos in ticket D, #242")
    )]
    pub const fn new(span: u64, beats: u64) -> Option<Self> {
        if beats >= 1
            && beats <= 1000
            && span >= 100_000_000 * beats
            && span <= 4_000_000_000 * beats
        {
            Some(Self { span, beats })
        } else {
            None
        }
    }

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

    fn rate(self) -> u128 {
        u128::from(ONE) * u128::from(self.beats)
    }

    fn travel(self, time: Duration) -> u128 {
        time.as_nanos().saturating_mul(self.rate())
    }
}

/// A possible source of beats, in the order chosen by the feed list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "external feeds reach the tick in ticket D, #242")
)]
pub enum TempoFeed {
    Tap,
    Audio,
    MidiClock,
    VirtualDJ,
    Timecode,
}

/// The feeds to try in order. Unused slots always hold Tap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeedList {
    slots: [TempoFeed; 5],
    len: u8,
}

/// The first entry that makes a feed list invalid, or an empty list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "ticket C adds the feed list command")
)]
pub enum FeedListError {
    Empty,
    Repeated { feed: TempoFeed },
    TapNotLast,
}

impl FeedList {
    pub const TAP: Self = Self {
        slots: [TempoFeed::Tap; 5],
        len: 1,
    };

    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "ticket C adds the feed list command")
    )]
    pub fn new(feeds: &[TempoFeed]) -> Result<Self, FeedListError> {
        if feeds.is_empty() {
            return Err(FeedListError::Empty);
        }
        let mut list = Self {
            len: 0,
            ..Self::TAP
        };
        for (i, &feed) in feeds.iter().enumerate() {
            if list.feeds().contains(&feed) {
                return Err(FeedListError::Repeated { feed });
            }
            if feed == TempoFeed::Tap && i + 1 != feeds.len() {
                return Err(FeedListError::TapNotLast);
            }
            if let Some(slot) = list.slots.get_mut(i) {
                *slot = feed;
                list.len += 1;
            }
        }
        Ok(list)
    }

    pub fn feeds(&self) -> &[TempoFeed] {
        self.slots.get(..usize::from(self.len)).unwrap_or_default()
    }
}

/// How fast the grid and elapsed beats advance at the settled time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    Free,
    Heard,
    Slowing,
    Stopped,
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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

impl Source {
    fn lapse(self, by: Duration) -> Option<Duration> {
        match self {
            Self::OneTap { at } if by.saturating_sub(at) > LAPSE => {
                Some(at + LAPSE + Duration::from_nanos(1))
            }
            Self::Feed | Self::OneTap { .. } | Self::Tapped { .. } => None,
        }
    }
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Reading {
    /// The tempo in charge.
    pub bpm: f64,
    /// The internal feed's tempo.
    pub feed: Bpm,
    #[cfg_attr(feature = "ts", ts(type = "\"Feed\" | \"OneTap\" | \"Tapped\""))]
    pub source: Source,
    /// Where the grid is, in beats: a whole number is a beat, a multiple of four a bar.
    pub beat: f64,
    /// Beats elapsed since the engine started. A play's own beat is this less its start.
    pub elapsed: f64,
}

/// A Q32 beat count and the fraction of its next count, in tempo.span parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Count {
    whole: u64,
    part: u64,
}

impl Count {
    const fn whole(whole: u64) -> Self {
        Self { whole, part: 0 }
    }

    fn beat(beat: u64) -> Self {
        Self::whole(beat.saturating_mul(ONE))
    }

    fn parts(self, tempo: Tempo) -> u128 {
        u128::from(self.whole) * u128::from(tempo.span) + u128::from(self.part)
    }

    fn advance(self, parts: u128, tempo: Tempo) -> Self {
        let total = parts.saturating_add(u128::from(self.part));
        let span = u128::from(tempo.span);
        Self {
            whole: self
                .whole
                .saturating_add(u64::try_from(total / span).unwrap_or(u64::MAX)),
            part: (total % span) as u64,
        }
    }

    fn at_tempo(self, from: Tempo, to: Tempo) -> Self {
        // All internal feeds share one span, so changing BPM carries the part exactly.
        // A tap can use another span. Round up to its next part as for a grid crossing.
        let (_, part) = self.part_at_tempo(from, to);
        if part >= u128::from(to.span) {
            Self::whole(self.whole.saturating_add(1))
        } else {
            Self {
                whole: self.whole,
                part: part as u64,
            }
        }
    }

    /// The fraction of a Q32 count in the new tempo's parts, rounded down and up.
    fn part_at_tempo(self, from: Tempo, to: Tempo) -> (u128, u128) {
        let scaled = u128::from(self.part) * u128::from(to.span);
        let span = u128::from(from.span);
        (scaled / span, scaled.div_ceil(span))
    }
}

/// Elapsed Q32 beat counts, retaining the fraction measured by the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeatTime {
    count: Count,
    span: u64,
}

impl From<u64> for BeatTime {
    fn from(whole: u64) -> Self {
        Self {
            count: Count::whole(whole),
            span: 1,
        }
    }
}

impl BeatTime {
    /// Count in beats for the wire picture.
    pub(crate) fn beats(self) -> f64 {
        (self.count.whole as f64 + self.count.part as f64 / self.span as f64) / ONE as f64
    }

    pub(crate) fn after(self, other: Self) -> bool {
        (
            self.count.whole,
            u128::from(self.count.part) * u128::from(other.span),
        ) > (
            other.count.whole,
            u128::from(other.count.part) * u128::from(self.span),
        )
    }

    pub(crate) fn parts(self) -> (u64, u64, u64) {
        (self.count.whole, self.count.part, self.span)
    }
}

/// Both counts at a change of grid or tempo. Their origin preserves fractional crossings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Origin {
    from: Duration,
    elapsed: Count,
    grid: Count,
    tempo: Tempo,
}

impl Origin {
    fn elapsed_after(&self, parts: u128) -> BeatTime {
        BeatTime {
            count: self.elapsed.advance(parts, self.tempo),
            span: self.tempo.span,
        }
    }

    /// When a freely running grid reaches this beat.
    fn crossing(&self, beat: u64) -> Duration {
        let nanos = self.distance_to(beat).div_ceil(self.tempo.rate());
        self.from.saturating_add(Duration::from_nanos(
            u64::try_from(nanos).unwrap_or(u64::MAX),
        ))
    }

    /// Distance from the stored grid to a whole beat, in tempo.span parts.
    fn distance_to(&self, beat: u64) -> u128 {
        Count::beat(beat)
            .parts(self.tempo)
            .saturating_sub(self.grid.parts(self.tempo))
    }
}

/// Which grid dates a time before the clock's origin: one state, one transition
/// ([`Dating::after`]), table in `tempo-feeds.md` 4a ("Dating").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dating {
    /// The origin's grid: a time before it is taken at its instant.
    Current,
    /// A one tap lapsed and its release moved the origin: a time before the origin is
    /// on the tapped grid, so a beat crossed before the lapse keeps its time.
    Lapsed { tapped: Origin },
}

/// What changes the dating: the only two writers are a lapse and a rebase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DatingEvent {
    /// A one tap lapsed: `before` is the origin it had, `moved` whether the release
    /// moved the origin.
    Lapse { before: Origin, moved: bool },
    /// The counts are taken again at a new origin (tap, release, feed, list, lock, a
    /// heard beat off its crossing, a new tempo).
    Rebase,
}

impl Dating {
    /// Every transition.
    const fn after(self, event: DatingEvent) -> Self {
        match event {
            DatingEvent::Lapse {
                before,
                moved: true,
            } => Self::Lapsed { tapped: before },
            DatingEvent::Lapse { moved: false, .. } => self,
            DatingEvent::Rebase => Self::Current,
        }
    }
}

/// The beat clock. Every change happens at the time it was settled to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// The time the clock was settled to, since the engine started.
    at: Duration,
    /// The time the last settle asked for: when the event it comes before was sent.
    /// Earlier than `at` for an event sent before a frame it missed.
    sent: Duration,
    origin: Origin,
    /// Which grid dates a time before the origin (`tempo-feeds.md` 4a, "Dating").
    dating: Dating,
    /// The last change of the grid, the tempo, the list or the taps: a beat heard
    /// before it moves nothing (spec 5b, step 4). Kept apart from the origin, which a beat on
    /// its own crossing leaves alone so that no crossing moves.
    changed: Duration,
    feed: Bpm,
    feeds: FeedList,
    locks: [Option<Tempo>; 5],
    /// The whole beat we listen from. Taps and the internal driver run freely.
    anchor: Option<u64>,
    /// A one tap that lapsed stays here: [`Clock::source`] tells it as the feed.
    source: Source,
}

impl Clock {
    /// A new engine's clock: the internal feed at [`Bpm::FEED`], on beat zero.
    pub const START: Self = Self {
        at: Duration::ZERO,
        sent: Duration::ZERO,
        origin: Origin {
            from: Duration::ZERO,
            elapsed: Count::whole(0),
            grid: Count::whole(0),
            tempo: Bpm::FEED.tempo(),
        },
        dating: Dating::Current,
        changed: Duration::ZERO,
        feed: Bpm::FEED,
        feeds: FeedList::TAP,
        locks: [None; 5],
        anchor: None,
        source: Source::Feed,
    };

    /// Who sets the tempo now: one tap lapses 2 s after it was sent.
    pub fn source(&self) -> Source {
        if self.source.lapse(self.at).is_some() {
            Source::Feed
        } else {
            self.source
        }
    }

    /// How long a beat lasts now, to the nanosecond below.
    pub fn period(&self) -> Duration {
        Duration::from_nanos(self.origin.tempo.span / self.origin.tempo.beats.max(1))
    }

    /// The time the clock was settled to.
    pub const fn at(&self) -> Duration {
        self.at
    }

    /// The clock at `t`, if that is later, for an event sent at `t`.
    pub fn settle(self, t: Duration) -> Self {
        let at = self.at.max(t);
        Self {
            at,
            sent: t,
            ..self.lapse_between(self.at, at).unwrap_or(self)
        }
    }

    /// Follow the driver at the lapse, even when the frame skips over it. Keep the
    /// tap's send time so a second tap sent within the interval still counts.
    fn lapse_between(self, since: Duration, by: Duration) -> Option<Self> {
        let lapse = self.source.lapse(by)?;
        if lapse <= since {
            return None;
        }
        // A tap delivered after its own lapse first marks the grid at delivery.
        let clock = Self {
            at: lapse.max(self.origin.from),
            ..self
        };
        let released = clock.follow();
        Some(Self {
            dating: released.dating.after(DatingEvent::Lapse {
                before: self.origin,
                moved: released.origin.from > self.origin.from,
            }),
            ..released
        })
    }

    /// The elapsed position at `t`, including the fraction of a Q32 count.
    pub fn beat_at(&self, t: Duration) -> BeatTime {
        if t < self.origin.from
            && let Dating::Lapsed { tapped } = self.dating
        {
            let since = t.saturating_sub(tapped.from);
            return tapped.elapsed_after(tapped.tempo.travel(since));
        }
        let clock = self.lapse_between(self.at, t).unwrap_or(*self);
        clock.origin.elapsed_after(clock.advance(t).1)
    }

    /// The elapsed count at `t`, which is not before the last change, in [`ONE`]s.
    pub fn elapsed_at(&self, t: Duration) -> u64 {
        self.beat_at(t).count.whole
    }

    /// The elapsed count now, in [`ONE`]s.
    pub fn elapsed_now(&self) -> u64 {
        self.elapsed_at(self.at)
    }

    /// Where the grid is now, in [`ONE`]s.
    pub fn grid(&self) -> u64 {
        self.position_at(self.at).whole
    }

    /// Where the grid is now: whole [`ONE`]s, and the part of one in `tempo.span`ths. On
    /// a beat's crossing, exactly on that beat: the grid is there all that nanosecond.
    fn position_at(&self, t: Duration) -> Count {
        let grid = self.origin.grid.advance(self.advance(t).1, self.origin.tempo);
        let below = grid.whole / ONE;
        let beat = Count::beat(below);
        if self.origin.grid <= beat && self.grid_crossing(below) == t {
            beat
        } else {
            grid
        }
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
        match self.position_at(self.at) {
            Count { whole, part: 0 } if whole.is_multiple_of(ONE) => whole / ONE,
            count => (count.whole / ONE).saturating_add(1),
        }
    }

    /// When the grid reaches beat `beat`. An automatic release preserves earlier crossings.
    pub fn crossing(&self, beat: u64) -> Duration {
        let crossing = self.grid_crossing(beat);
        // The released grid dates every beat already reached to its own origin.
        if crossing == self.origin.from
            && let Dating::Lapsed { tapped } = self.dating
        {
            return tapped.crossing(beat).min(crossing);
        }
        self.lapse_between(self.at, crossing)
            .map_or(crossing, |released| released.grid_crossing(beat))
    }

    /// The crossing on this grid, before any pending release.
    fn grid_crossing(&self, beat: u64) -> Duration {
        if self.waits_for_beat(beat) {
            return Duration::MAX;
        }
        self.origin.crossing(beat)
    }

    fn waits_for_beat(&self, beat: u64) -> bool {
        self.anchor.is_some_and(|anchor| beat >= anchor + 2)
    }

    /// Whether the grid reached beat `beat` by now.
    pub fn reached(&self, beat: u64) -> bool {
        !self.waits_for_beat(beat) && self.crossing(beat) <= self.at
    }

    /// A tap, sent when the last settle asked for: its interval counts from the time it
    /// was sent, the beat it marks is now.
    pub fn tap(self) -> Self {
        let sent = self.sent;
        let (source, tempo) = match self.source {
            Source::Feed => (Source::OneTap { at: sent }, self.origin.tempo),
            Source::OneTap { at: last } | Source::Tapped { at: last } => {
                let gap = sent.saturating_sub(last);
                if gap < BOUNCE {
                    return self;
                }
                match self.source {
                    _ if gap <= LAPSE => (Source::Tapped { at: sent }, Tempo::every(gap)),
                    Source::Tapped { .. } => (Source::Tapped { at: sent }, self.origin.tempo),
                    Source::Feed | Source::OneTap { .. } => {
                        (Source::OneTap { at: sent }, self.origin.tempo)
                    }
                }
            }
        };
        let beat = self.first_beat();
        let mut clock = Self {
            anchor: None,
            source,
            ..self.rebase(tempo)
        };
        clock.origin.grid = Count::beat(beat);
        clock.lapse_between(sent, self.at).unwrap_or(clock)
    }

    /// Release the taps and follow the list at the current grid phase.
    pub fn release(self) -> Self {
        let released = Self {
            source: Source::Feed,
            ..self
        };
        if self.source() == Source::Feed {
            released
        } else {
            released.follow()
        }
    }

    /// The internal tempo follows this setting even while another feed drives.
    pub fn set_feed(self, bpm: Bpm) -> Self {
        let clock = Self { feed: bpm, ..self };
        if clock.driving() == Some(TempoFeed::Tap)
            && !matches!(self.source(), Source::Tapped { .. })
        {
            clock.at_tempo(bpm.tempo())
        } else {
            clock
        }
    }

    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "ticket C publishes the feed list")
    )]
    pub fn feeds(&self) -> FeedList {
        self.feeds
    }

    /// The first feed with a lock, even while taps override its grid.
    pub fn driving(&self) -> Option<TempoFeed> {
        self.feeds
            .feeds()
            .iter()
            .copied()
            .find(|&feed| self.locked(feed).is_some())
    }

    fn locked(&self, feed: TempoFeed) -> Option<Tempo> {
        if feed == TempoFeed::Tap {
            Some(self.feed.tempo())
        } else {
            self.locks.get(feed as usize).copied().flatten()
        }
    }

    pub fn set_feeds(self, feeds: FeedList) -> Self {
        if feeds == self.feeds {
            return self.release();
        }
        Self {
            feeds,
            source: Source::Feed,
            ..self
        }
        .follow()
    }

    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "the tick takes beats and locks in ticket D, #242")
    )]
    pub fn lock(self, feed: TempoFeed, tempo: Option<Tempo>) -> Self {
        self.record_lock(feed, tempo).follow_lock(self.driving())
    }

    fn record_lock(mut self, feed: TempoFeed, tempo: Option<Tempo>) -> Self {
        if feed != TempoFeed::Tap
            && let Some(slot) = self.locks.get_mut(feed as usize)
        {
            *slot = tempo;
        }
        self
    }

    /// Keeping the same driver only updates its row.
    fn follow_lock(self, previous: Option<TempoFeed>) -> Self {
        if self.driving() == previous {
            self
        } else {
            self.follow()
        }
    }

    /// Apply a release or a change of driver (4a). A new estimate from the same
    /// external driver takes effect with its next beat.
    fn follow(self) -> Self {
        let clock = Self {
            changed: self.at,
            ..self
        };
        if clock.source() != Source::Feed {
            return clock;
        }
        let driver = clock.driving();
        let tempo = driver
            .and_then(|feed| clock.locked(feed))
            .unwrap_or(clock.origin.tempo);
        if driver == Some(TempoFeed::Tap) {
            let free = if clock.at_full_tempo() {
                clock.at_tempo(tempo)
            } else {
                clock.rebase(tempo)
            };
            Self {
                anchor: None,
                ..free
            }
        } else {
            Self {
                anchor: clock.anchor.or_else(|| Some(clock.grid() / ONE)),
                ..clock.at_tempo(tempo)
            }
        }
    }

    /// Align an external driver's beat on the grid it heard (5b). A rejected beat
    /// can still supply the first lock in the list.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "the tick takes beats and locks in ticket D, #242")
    )]
    pub fn beat(self, feed: TempoFeed, heard: Duration, tempo: Tempo) -> Self {
        let old = self.settle(self.at.max(heard));
        let locked = old.record_lock(feed, Some(tempo));
        if feed != TempoFeed::Tap
            && old.source() == Source::Feed
            && locked.driving() == Some(feed)
            && heard >= old.changed
            && let Some(aligned) = locked.align(heard, tempo)
        {
            return aligned;
        }
        locked.follow_lock(old.driving())
    }

    /// The lock has been recorded, but the grid still follows its old trajectory.
    fn align(self, heard: Duration, tempo: Tempo) -> Option<Self> {
        let heard_grid = self.position_at(heard);
        let beat = heard_grid.whole / ONE + u64::from(heard_grid.whole % ONE >= ONE / 2);
        let on_crossing = self.crossing(beat) == heard
            && heard_grid == Count::beat(beat)
            && self.origin.tempo.same(tempo)
            && !self.waits_for_beat(beat);
        if on_crossing && self.at_full_tempo() && self.at <= self.crossing(beat + 1) {
            // The old origin retains the fraction of a nanosecond at this crossing.
            return Some(Self {
                anchor: Some(beat),
                changed: self.at,
                ..self
            });
        }
        let grid_at_beat = if on_crossing {
            self.origin.grid
                .advance(self.advance(heard).1, self.origin.tempo)
                .at_tempo(self.origin.tempo, tempo)
        } else {
            Count::beat(beat)
        };
        let since = self.at - heard;
        let candidate = grid_at_beat.advance(tempo.travel(since), tempo);
        let old = self.position_at(self.at);
        let whole = old.whole / ONE;
        let reached = self.anchor.map_or(whole, |anchor| whole.min(anchor + 1));
        if (candidate.whole / ONE).min(beat + 1) < reached {
            return None;
        }
        // Round both limits inward when the old phase falls between two new parts.
        let (down, up) = old.part_at_tempo(self.origin.tempo, tempo);
        let whole_parts = Count::whole(old.whole).parts(tempo);
        let half = Count::whole(ONE / 2).parts(tempo);
        let parts = candidate.parts(tempo).clamp(
            (whole_parts + up).saturating_sub(half),
            whole_parts + down + half,
        );
        let grid = Count::whole(0).advance(parts, tempo);
        let mut aligned = Self {
            anchor: Some(beat),
            ..self.rebase(tempo)
        };
        aligned.origin.grid = grid.min(Count::beat(beat + 2));
        Some(aligned)
    }

    /// The same clock at `tempo` from now on. The same rate changes nothing: a count
    /// taken again rounds, and would move a crossing.
    fn at_tempo(self, tempo: Tempo) -> Self {
        if self.origin.tempo.same(tempo) {
            return self;
        }
        self.rebase(tempo)
    }

    /// Take both counts at the change instant, carrying their fractional parts.
    fn rebase(self, tempo: Tempo) -> Self {
        Self {
            origin: Origin {
                from: self.at,
                elapsed: self
                    .origin
                    .elapsed_after(self.advance(self.at).1)
                    .count
                    .at_tempo(self.origin.tempo, tempo),
                grid: self.position_at(self.at).at_tempo(self.origin.tempo, tempo),
                tempo,
            },
            dating: self.dating.after(DatingEvent::Rebase),
            changed: self.at,
            ..self
        }
    }

    pub fn pace(&self) -> Pace {
        self.advance(self.at).0
    }

    /// Both counts still have their full-tempo phase on the crossing nanosecond.
    fn at_full_tempo(&self) -> bool {
        self.anchor
            .is_none_or(|anchor| self.at <= self.grid_crossing(anchor + 1))
    }

    /// The pace and distance travelled by both counts (4d). Take the first unheard
    /// beat on time, including its crossing nanosecond, then count at half tempo.
    fn advance(&self, t: Duration) -> (Pace, u128) {
        let since = t.saturating_sub(self.origin.from);
        let full = self.origin.tempo.travel(since);
        let Some(anchor) = self.anchor else {
            return (Pace::Free, full);
        };
        let pivot = anchor + 1;
        let on_time = self.origin.distance_to(pivot);
        if full < on_time {
            return (Pace::Heard, full);
        }
        let advance = if since.as_nanos() == on_time.div_ceil(self.origin.tempo.rate()) {
            full
        } else {
            on_time + (full - on_time) / 2
        };
        let remaining = self.origin.distance_to(anchor + 2);
        if advance >= remaining {
            (Pace::Stopped, remaining)
        } else {
            (Pace::Slowing, advance)
        }
    }

    /// The clock as a picture shows it.
    pub fn reading(&self) -> Reading {
        Reading {
            bpm: 60e9 * self.origin.tempo.beats as f64 / self.origin.tempo.span.max(1) as f64,
            feed: self.feed,
            source: self.source(),
            beat: beats(self.grid()),
            elapsed: self.beat_at(self.at).beats(),
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
    fn a_wave_started_between_counts_keeps_its_midpoint_across_a_tap() {
        use crate::{Binding, Node, Program, Rgb, Swatches, Use};

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
            swatches: Swatches::new(&[red, green, green]).unwrap(),
        }];
        let wave = Program {
            nodes: &nodes,
            items: &[],
            root: 0,
        };
        let target = [Binding {
            node: 0,
            index: 0,
            count: 1,
            discrete_colour: false,
        }];
        // A three-swatch Wave has red/green midpoints at 1/6 and 5/6 beat.
        for (start, end) in [
            (feed.beat_at(ms(80)), feed.beat_at(ms(480))),
            (feed.beat_at(ms(80)), tapped.beat_at(ms(480))),
            (a.beat_at(ms(480) + short / 6), b.beat_at(change)),
            (b.beat_at(change), c.beat_at(back + short / 6)),
        ] {
            assert_eq!(
                wave.sample(&target, end.since(start), Use::default())
                    .colour,
                Some(Rgb {
                    red: 32_768,
                    green: 32_768,
                    blue: 0
                })
            );
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
}

/// The feed list, the driving feed, a heard beat and the pace: spec
/// `docs/design/tempo-feeds.md` 4a, 4d and 5b (#241). Every clock starts at 120 BPM
/// (500 ms a beat) on beat 0 at 0 ms, then takes its feed list at 0 ms.
#[cfg(test)]
mod feeds {
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
        let cases = [(400, 0.8), (500, 1.0), (1000, 1.5), (1500, 2.0), (2600, 2.0), (60_000, 2.0)];
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
        let cases = [(When::Now, None), (When::Beat, Some(2)), (When::Bar, Some(4))];
        for (when, beat) in cases {
            assert_eq!(clock.next(when), beat, "{when:?}");
        }
    }

    #[test]
    fn a_beat_on_its_crossing_at_the_same_tempo_moves_no_crossing() {
        // 124 BPM: no beat is a whole nanosecond
        let free = Clock::START;
        let heard = free.crossing(1);
        let clock = Clock::START
            .set_feeds(list(&[Audio]))
            .settle(heard)
            .beat(Audio, heard, Tempo::new(60_000_000_000, 124).unwrap());
        assert_eq!(clock.crossing(1), heard);
        assert_eq!(clock.crossing(2), free.crossing(2));
        assert_eq!(clock.crossing(3), Duration::MAX);
    }

    #[test]
    fn a_new_engine_has_tap_alone_and_runs_free() {
        assert_eq!(
            (Clock::START.feeds(), Clock::START.driving(), Clock::START.pace()),
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
            ("four, then none", &[VirtualDJ, Audio, MidiClock, Timecode], Ok(())),
            ("empty", &[], Err(FeedListError::Empty)),
            ("a repeat", &[Audio, Audio], Err(FeedListError::Repeated { feed: Audio })),
            ("a repeat later", &[Audio, MidiClock, Audio, TAP], Err(FeedListError::Repeated { feed: Audio })),
            ("six: one repeats", &[Audio, MidiClock, VirtualDJ, Timecode, Audio, TAP], Err(FeedListError::Repeated { feed: Audio })),
            ("Tap first", &[TAP, Audio], Err(FeedListError::TapNotLast)),
            ("Tap twice: the first is not last", &[TAP, TAP], Err(FeedListError::TapNotLast)),
            ("Tap in the middle comes before the repeat", &[Audio, TAP, Audio], Err(FeedListError::TapNotLast)),
            ("the repeat comes before Tap", &[Audio, Audio, TAP, MidiClock], Err(FeedListError::Repeated { feed: Audio })),
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
        assert_eq!(run(&[Audio], &[List(10, &[Audio, TAP])]).feeds(), list(&[Audio, TAP]));
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
            assert_eq!(Tempo::new(span, beats).is_some(), made, "{span} ns, {beats} beats");
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
    fn review_a_waiting_play_keeps_its_crossing_when_the_same_wake_passes_the_lapse() {
        use crate::{EngineCommand, PlayId, PlayState, Priority, Rank, State, update};

        // At 512 ms per beat the tapped grid would cross at 2148 ms. The
        // release at 2100 ms + 1 ns moves that crossing to 2160 ms.
        for (tap_period, crossing, late_wake) in
            [(497, ms(2088), ms(2120)), (512, ms(2160), ms(2200))]
        {
            let clock = run(
                &[Audio],
                &[
                    Lock(0, Audio, Some(tap_period)),
                    Tap(100),
                    Lock(1000, Audio, Some(640)),
                    Settle(2080),
                ],
            );
            let state = State {
                clock,
                ..State::new(Priority::RealTime)
            };
            let apply = |state, command| update(state, &mut [], &mut [], &mut [], command);
            let waiting = apply(
                state,
                EngineCommand::Start {
                    play: PlayId::new(0, 1).unwrap(),
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
}
