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
/// time the request was sent.
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
        let grid = self
            .origin
            .grid
            .advance(self.advance(t).1, self.origin.tempo);
        let below = grid.whole / ONE;
        let beat = Count::beat(below);
        if self.origin.grid <= beat && self.grid_crossing(below) == t {
            beat
        } else {
            grid
        }
    }

    /// The beat a launch waits for: the first matching one on or after the time the
    /// launch was sent (the last settle), `None` for at once.
    pub fn next(&self, when: When) -> Option<u64> {
        let beat = self.first_beat(self.sent);
        match when {
            When::Now => None,
            When::Beat => Some(beat),
            When::Bar => Some(beat.div_ceil(BEATS_PER_BAR).saturating_mul(BEATS_PER_BAR)),
        }
    }

    /// The first beat whose crossing is at `t` or later: the one the grid is on, else the
    /// next. Decided by [`Clock::crossing`], as a launch is.
    fn first_beat(&self, t: Duration) -> u64 {
        match self.position_at(t) {
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
        let beat = self.first_beat(self.at);
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
            self.origin
                .grid
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
#[path = "../tests/unit/clock.rs"]
mod tests;

/// The feed list, the driving feed, a heard beat and the pace: spec
/// `docs/design/tempo-feeds.md` 4a, 4d and 5b (#241). Every clock starts at 120 BPM
/// (500 ms a beat) on beat 0 at 0 ms, then takes its feed list at 0 ms.
#[cfg(test)]
#[path = "../tests/unit/clock_feeds.rs"]
pub(crate) mod feeds;
