use super::estimate::{Estimate, Gaps};
use super::{Event, Format, PartialFrame, Samples, Signal, Step, Tempo};
use alloc::vec::Vec;

const MIN_ENERGY: f64 = 0.0025;
const MIN_ONSET_GAP: u64 = 100_000_000;
const MAX_STAMP_ERROR: u128 = 20_000_000;

/// Follows one input. Block boundaries may fall anywhere within a hop.
pub struct Tracker {
    format: Format,
    previous_block: Option<BlockTimes>,
    hop: Hop,
    history: HopHistory,
    second_hop: bool,
    onset_signal: Signal,
    quiet_through: u64,
    onsets: Onsets,
}

struct BlockTimes {
    first: u64,
    end: u128,
}

#[derive(Default)]
struct Hop {
    frames: u32,
    energy_sum: f64,
    first_loud_at: Option<u64>,
}

#[derive(Default)]
struct HopHistory {
    newest_first: [f64; 16],
    len: usize,
}

impl HopHistory {
    fn push(&mut self, energy: f64) {
        self.newest_first.rotate_right(1);
        self.newest_first[0] = energy;
        self.len = (self.len + 1).min(self.newest_first.len());
    }

    fn median(&self) -> f64 {
        let mut sorted = self.newest_first;
        let hops = sorted.get_mut(..self.len).unwrap_or_default();
        hops.sort_unstable_by(f64::total_cmp);
        hops.get(hops.len().saturating_sub(1) / 2)
            .zip(hops.get(hops.len() / 2))
            .map_or(0.0, |(lower, upper)| (lower + upper) / 2.0)
    }
}

/// History used to decide the next onset on its capture clock.
#[derive(Default)]
struct Onsets {
    last: Option<u64>,
    gaps: Gaps,
    estimate: Estimate,
    reference: Option<Tempo>,
}

impl Tracker {
    pub fn new(format: Format) -> Self {
        Self {
            format,
            previous_block: None,
            hop: Hop::default(),
            history: HopHistory::default(),
            second_hop: false,
            onset_signal: Signal::None,
            quiet_through: 0,
            onsets: Onsets::default(),
        }
    }

    /// Start counting hops again at the next block's first frame.
    pub fn gap(&mut self) {
        *self = Self {
            onset_signal: self.onset_signal.next(Step::Gap),
            ..Self::new(self.format)
        };
    }

    /// Append the events heard in this block. A partial frame refuses the whole block.
    /// An empty block leaves the tracker unchanged.
    pub fn push(
        &mut self,
        first: u64,
        samples: Samples<'_>,
        out: &mut Vec<Event>,
    ) -> Result<(), PartialFrame> {
        match samples {
            Samples::F32(block) => self.push_frames(
                first,
                block,
                |x| if x.is_finite() { f64::from(x) } else { 0.0 },
                out,
            ),
            Samples::I16(block) => self.push_frames(first, block, |x| f64::from(x) / 32_768.0, out),
            Samples::U16(block) => {
                self.push_frames(first, block, |x| (f64::from(x) - 32_768.0) / 32_768.0, out)
            }
        }
    }

    fn push_frames<T: Copy>(
        &mut self,
        first: u64,
        samples: &[T],
        value: impl Fn(T) -> f64,
        out: &mut Vec<Event>,
    ) -> Result<(), PartialFrame> {
        if samples.is_empty() {
            return Ok(());
        }
        let channels = self.format.channels;
        let frames = samples.chunks_exact(usize::from(channels));
        if !frames.remainder().is_empty() {
            return Err(PartialFrame);
        }

        let rate = self.format.rate;
        if let Some(previous) = &self.previous_block
            && (first < previous.first
                || previous.end.abs_diff(u128::from(first)) > MAX_STAMP_ERROR)
        {
            self.gap();
        }
        self.previous_block = Some(BlockTimes {
            first,
            end: frame_time(first, frames.len(), rate),
        });
        // Preserve the full prediction above. Clamp only when reporting on the u64 clock.
        let time_of = |index| u64::try_from(frame_time(first, index, rate)).unwrap_or(u64::MAX);
        let hop_size = (rate + 50) / 100;
        for (index, frame) in frames.enumerate() {
            let energy = frame
                .iter()
                .map(|&x| {
                    let x = value(x);
                    x * x
                })
                .sum::<f64>()
                / f64::from(channels);
            if energy > MIN_ENERGY {
                self.hop.first_loud_at.get_or_insert_with(|| time_of(index));
            }
            self.hop.energy_sum += energy;
            self.hop.frames += 1;
            if self.hop.frames == hop_size {
                self.finish_hop(time_of(index + 1), out);
            }
        }
        Ok(())
    }

    fn finish_hop(&mut self, end: u64, out: &mut Vec<Event>) {
        let hop = core::mem::take(&mut self.hop);
        let energy = hop.energy_sum / f64::from(hop.frames);
        let previous = self.history.newest_first[0];
        let onset = hop
            .first_loud_at
            .filter(|_| {
                energy >= MIN_ENERGY
                    && energy - previous >= MIN_ENERGY.max(2.0 * self.history.median())
            })
            .filter(|&at| {
                self.onsets
                    .last
                    .is_none_or(|last| at.saturating_sub(last) >= MIN_ONSET_GAP)
            });
        if let Some(at) = onset {
            self.hear_onset(at, out);
            self.quiet_through = at;
        } else {
            // Overlapping silent blocks cannot shorten the silence already observed.
            self.quiet_through = self.quiet_through.max(end);
        }
        if self.second_hop {
            let level = level(energy.max(previous));
            self.age_onsets(self.quiet_through);
            out.push(Event::Reading {
                at: end,
                level,
                signal: self.onset_signal,
                confidence: self.onsets.estimate.confidence,
                tempo: self.onsets.estimate.tempo,
            });
        }
        self.second_hop = !self.second_hop;
        self.history.push(energy);
    }

    fn age_onsets(&mut self, at: u64) {
        let Some(last) = self.onsets.last else {
            return;
        };
        self.onset_signal = self.onset_signal.next(Step::Quiet {
            since_onset: at.saturating_sub(last),
        });
        if self.onset_signal == Signal::Lost {
            self.onsets = Onsets::default();
        }
    }

    fn hear_onset(&mut self, at: u64, out: &mut Vec<Event>) {
        self.age_onsets(at);
        let before = self.onset_signal;
        if let Some(last) = self.onsets.last.replace(at) {
            self.onsets.gaps.push(at - last);
        }
        let estimate = self.onsets.gaps.estimate(self.onsets.reference);
        self.onsets.estimate = estimate;
        let confidence = estimate.confidence;
        self.onset_signal = before.next(Step::Onset { confidence });
        out.push(Event::Onset { at, confidence });
        if self.onset_signal == Signal::Locked
            && let Some(tempo) = estimate.tempo
        {
            self.onsets.reference = Some(tempo);
            if before != Signal::Locked || estimate.newest_inlier {
                out.push(Event::Beat { at, tempo });
            }
        }
    }
}

fn frame_time(first: u64, index: usize, rate: u32) -> u128 {
    u128::from(first) + index as u128 * 1_000_000_000 / u128::from(rate)
}

/// The meter level of `energy`, a mean of squares: `100 * sqrt(energy)` rounded half
/// away from zero, at most 100. The smallest `n` with `10000 * energy < (n + 0.5)^2`,
/// so no square root (`core` has none).
fn level(energy: f64) -> u8 {
    let scaled = 10_000.0 * energy;
    (0..100u8)
        .find(|&n| {
            let edge = f64::from(n) + 0.5;
            scaled < edge * edge
        })
        .unwrap_or(100)
}
