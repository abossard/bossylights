//! The immutable read contract for one completed engine wake.
//!
//! The tick owns mutable performance data. It publishes a [`Publication`] into a
//! preallocated reader box; callers receive only focused views and deltas from a
//! `Snapshot`.

use crate::{
    ChannelLevel, Histogram, LATENCY_BUCKETS, LATENESS_BUCKETS, Levels, MAX_OUTPUTS, MAX_UNIVERSES,
    Output, OutputNumber, Priority, State, Universes,
};
use engine_core::health::HealthSample;
use engine_core::{ChannelSet, Play};
use std::time::Duration;

/// Mutable tick data copied into one immutable publication. The tick owns every
/// instance; this packet is only the one-way handoff boundary.
pub(crate) struct Publication<'a> {
    pub state: State,
    pub universes: Universes,
    pub held: &'a [Levels; MAX_UNIVERSES as usize],
    pub holds: &'a [ChannelSet; MAX_UNIVERSES as usize],
    /// The frame's bytes, for tests: what the Mixer decided.
    #[cfg(feature = "test-support")]
    pub frame: &'a [Levels],
    pub output_health: &'a [OutputHealth],
    pub latency: Times<LATENESS_BUCKETS>,
    pub applied: u64,
    pub revision: u64,
}

/// Everything a reader can observe after one completed wake.
#[derive(Clone)]
pub struct Snapshot {
    state: State,
    universes: Universes,
    held: [Levels; MAX_UNIVERSES as usize],
    holds: [ChannelSet; MAX_UNIVERSES as usize],
    #[cfg(feature = "test-support")]
    frame: Box<[Levels; MAX_UNIVERSES as usize]>,
    output_health: [OutputHealth; MAX_OUTPUTS],
    latency: Times<LATENESS_BUCKETS>,
    applied: u64,
    revision: u64,
}

/// How late an output's device was handed its frames, so far: on the output's own
/// thread, as the send begins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lateness {
    pub p99: Duration,
    pub worst: Duration,
}

impl Snapshot {
    pub(crate) fn new(universes: Universes, priority: Priority) -> Self {
        Self {
            state: State::new(priority),
            universes,
            held: [Levels::ZERO; MAX_UNIVERSES as usize],
            holds: [ChannelSet::NONE; MAX_UNIVERSES as usize],
            #[cfg(feature = "test-support")]
            frame: Box::new([Levels::ZERO; MAX_UNIVERSES as usize]),
            output_health: [OutputHealth::ZERO; MAX_OUTPUTS],
            latency: Times::ZERO,
            applied: 0,
            revision: 0,
        }
    }

    /// Copies one completed wake into an existing publication box without allocating.
    pub(crate) fn publish(&mut self, from: Publication<'_>) {
        self.state = from.state;
        self.universes = from.universes;
        self.held = *from.held;
        self.holds = *from.holds;
        #[cfg(feature = "test-support")]
        for (to, from) in self.frame.iter_mut().zip(from.frame) {
            *to = *from;
        }
        self.output_health.copy_from_slice(from.output_health);
        self.latency = from.latency;
        self.applied = from.applied;
        self.revision = from.revision;
    }

    /// The immutable performance state used on the wire.
    pub fn performance(&self) -> State {
        self.state
    }

    pub fn frame(&self) -> u64 {
        self.state.frame
    }

    pub fn now(&self) -> Duration {
        self.state.now
    }

    pub fn blackout(&self) -> bool {
        self.state.blackout
    }

    pub fn output(&self, output: OutputNumber) -> Option<Output> {
        self.state.attached_output(output)
    }

    pub fn next_output_due(&self) -> Option<Duration> {
        self.state.next_output_due()
    }

    pub fn outputs_due_at(&self, at: Duration) -> usize {
        crate::due(&self.state, at).count()
    }

    pub fn active_plays(&self) -> impl Iterator<Item = Play> + '_ {
        self.state.active_plays().copied()
    }

    pub fn play_at(&self, slot: usize) -> Option<Play> {
        self.state.play_at(slot)
    }

    pub fn has_play(&self, slot: usize) -> bool {
        self.state.has_play(slot)
    }

    /// The held levels of the engine's universes.
    pub fn universes(&self) -> &[Levels] {
        self.held
            .get(..usize::from(self.universes.count()))
            .unwrap_or_default()
    }

    /// Which raw channels of the engine's universes are held.
    pub fn holds(&self) -> &[ChannelSet] {
        self.holds
            .get(..usize::from(self.universes.count()))
            .unwrap_or_default()
    }

    /// The byte the Mixer decided for `address` of `universe` (both from 1) in this
    /// wake's frame, for tests.
    #[cfg(feature = "test-support")]
    pub fn sent(&self, universe: u16, address: u16) -> u8 {
        let (Some(universe), Some(address)) = (universe.checked_sub(1), address.checked_sub(1))
        else {
            return 0;
        };
        self.frame
            .get(usize::from(universe))
            .and_then(|row| row.0.get(usize::from(address)))
            .copied()
            .unwrap_or(0)
    }

    /// The channels whose held levels changed since `before`.
    pub fn channel_changes_since<'a>(
        &'a self,
        before: &'a Self,
    ) -> impl Iterator<Item = ChannelLevel> + 'a {
        crate::changes(before.universes(), self.universes())
    }

    /// Fills the cumulative measurements for this output at the snapshot's wake time.
    /// Returns false if the slot has no output.
    pub fn health_sample(&self, output: OutputNumber, sample: &mut HealthSample) -> bool {
        let Some(health) = self.output_health.get(output.slot()) else {
            return false;
        };
        if !self.state.record_output_health(output, sample) {
            return false;
        }
        sample.at = self.state.now;
        sample.generation = health.generation;
        sample.lateness = health.lateness.counts;
        sample.lateness_worst = health.lateness.worst;
        sample.press = health.press.counts;
        sample.press_worst = health.press.worst;
        sample.press_under_target = health.press_under_target;
        sample.press_missing = health.press_missing;
        sample.outages = health.outages;
        sample.prior_runs = health.prior_runs;
        sample.prior_outages = health.prior_outages;
        sample.frames_handed = health.handed;
        true
    }

    /// How late this output's device was handed its frames. `None` before its first.
    pub fn lateness(&self, output: OutputNumber) -> Option<Lateness> {
        let times = &self.output_health.get(output.slot())?.lateness;
        Some(Lateness {
            p99: times.percentile(99)?,
            worst: times.worst,
        })
    }

    /// Frames handed to this output's own thread since it was attached, this wake's
    /// too. What came of each is counted in its counters once its thread is done with it.
    pub fn handed(&self, output: OutputNumber) -> u64 {
        self.output_health
            .get(output.slot())
            .map_or(0, |health| health.handed)
    }

    /// The time from a send to the frame that carries it, for 95 of 100 commands.
    pub fn send_to_frame_p95(&self) -> Option<Duration> {
        self.latency.percentile(95)
    }

    /// How many commands this wake and the ones before applied.
    pub fn applied(&self) -> u64 {
        self.applied
    }

    /// The front revision read at this wake's cutoff.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn generation(&self, output: usize) -> u64 {
        self.output_health
            .get(output)
            .map_or(0, |health| health.generation)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct OutputHealth {
    pub lateness: Times<LATENESS_BUCKETS>,
    pub press: Times<LATENCY_BUCKETS>,
    pub generation: u64,
    pub handed: u64,
    pub press_under_target: u64,
    pub press_missing: u64,
    pub outages: u64,
    pub prior_runs: u64,
    pub prior_outages: u64,
}

impl OutputHealth {
    pub(crate) const ZERO: Self = Self {
        lateness: Times::ZERO,
        press: Times::ZERO,
        generation: 0,
        handed: 0,
        press_under_target: 0,
        press_missing: 0,
        outages: 0,
        prior_runs: 0,
        prior_outages: 0,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct Times<const N: usize> {
    pub counts: Histogram<N>,
    pub worst: Duration,
}

impl<const N: usize> Times<N> {
    pub(crate) const ZERO: Self = Self {
        counts: [0; N],
        worst: Duration::ZERO,
    };

    pub(crate) fn count(&mut self, time: Duration) {
        if let Some(count) = self.counts.get_mut(crate::bucket::<N>(time)) {
            *count += 1;
        }
        self.worst = self.worst.max(time);
    }

    fn percentile(&self, percent: u64) -> Option<Duration> {
        crate::percentile(&self.counts, percent, self.worst)
    }
}

#[cfg(all(test, target_pointer_width = "64"))]
#[test]
fn snapshot_has_one_press_histogram_and_six_counts_per_output() {
    // Tests see the frame's bytes through one more pointer.
    const BEFORE: usize = 75_184 + if cfg!(feature = "test-support") { 8 } else { 0 };
    let added = MAX_OUTPUTS * (size_of::<Times<LATENCY_BUCKETS>>() + 6 * size_of::<u64>());
    assert_eq!(size_of::<Snapshot>(), BEFORE + added);
}
