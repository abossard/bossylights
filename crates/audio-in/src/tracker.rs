use super::estimate::{Estimate, Gaps};
use super::{Event, Format, PartialFrame, Samples, Signal, Step, Tempo};

const MIN_ENERGY: f64 = 0.0025;
const MIN_ONSET_GAP: u64 = 100_000_000;
const MAX_STAMP_ERROR: u128 = 20_000_000;

/// Follows one input. Block boundaries may fall anywhere within a hop.
pub struct Tracker {
    format: Format,
    anchor: Option<u64>,
    next_frame: u64,
    hop: Hop,
    history: HopHistory,
    signal: Signal,
    onsets: Onsets,
}

#[derive(Default)]
struct Hop {
    frames: u32,
    energy_sum: f64,
    first_loud_frame: Option<u64>,
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

    fn lower_median(&self) -> f64 {
        let mut sorted = self.newest_first;
        let hops = sorted.get_mut(..self.len).unwrap_or_default();
        hops.sort_unstable_by(f64::total_cmp);
        hops.get(hops.len().saturating_sub(1) / 2)
            .copied()
            .unwrap_or(0.0)
    }
}

/// All the history that expires after three quiet seconds.
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
            anchor: None,
            next_frame: 0,
            hop: Hop::default(),
            history: HopHistory::default(),
            signal: Signal::None,
            onsets: Onsets::default(),
        }
    }

    /// Start counting hops again at the next block's first frame.
    pub fn gap(&mut self) {
        *self = Self {
            signal: self.signal.next(Step::Gap),
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
            Samples::F32(block) => self.push_frames(first, block, f64::from, out),
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
        if let Some(anchor) = self.anchor
            && frame_time(anchor, self.next_frame, rate).abs_diff(u128::from(first))
                > MAX_STAMP_ERROR
        {
            self.gap();
        }
        let anchor = *self.anchor.get_or_insert(first);
        let hop_size = (rate + 50) / 100;
        for frame in frames {
            let energy = frame.iter().map(|&x| value(x).powi(2)).sum::<f64>() / f64::from(channels);
            if energy >= MIN_ENERGY {
                self.hop.first_loud_frame.get_or_insert(self.next_frame);
            }
            self.hop.energy_sum += energy;
            self.hop.frames += 1;
            self.next_frame += 1;
            if self.hop.frames == hop_size {
                self.finish_hop(anchor, out);
            }
        }
        Ok(())
    }

    fn finish_hop(&mut self, anchor: u64, out: &mut Vec<Event>) {
        let hop = std::mem::take(&mut self.hop);
        let energy = hop.energy_sum / f64::from(hop.frames);
        let previous = self.history.newest_first[0];
        // Clamp only when reporting on the u64 clock. Gap detection uses the full prediction.
        let time_of =
            |frame| u64::try_from(frame_time(anchor, frame, self.format.rate)).unwrap_or(u64::MAX);
        let end = time_of(self.next_frame);
        let onset = hop
            .first_loud_frame
            .filter(|_| {
                energy >= MIN_ENERGY
                    && energy - previous >= MIN_ENERGY.max(2.0 * self.history.lower_median())
            })
            .map(time_of)
            .filter(|&at| {
                self.onsets
                    .last
                    .is_none_or(|last| at - last >= MIN_ONSET_GAP)
            });
        if let Some(at) = onset {
            self.hear_onset(at, out);
        } else if let Some(last) = self.onsets.last {
            self.signal = self.signal.next(Step::Quiet {
                since_onset: end - last,
            });
            if self.signal == Signal::Lost {
                self.onsets = Onsets::default();
            }
        }
        if self.next_frame.is_multiple_of(2 * u64::from(hop.frames)) {
            let level = (100.0 * energy.max(previous).sqrt()).round().min(100.0) as u8;
            out.push(Event::Reading {
                at: end,
                level,
                signal: self.signal,
                confidence: self.onsets.estimate.confidence,
                tempo: self.onsets.estimate.tempo,
            });
        }
        self.history.push(energy);
    }

    fn hear_onset(&mut self, at: u64, out: &mut Vec<Event>) {
        if let Some(last) = self.onsets.last.replace(at) {
            self.onsets.gaps.push(at - last);
        }
        let estimate = self.onsets.gaps.estimate(self.onsets.reference);
        self.onsets.estimate = estimate;
        let confidence = estimate.confidence;
        let before = self.signal;
        self.signal = before.next(Step::Onset { confidence });
        out.push(Event::Onset { at, confidence });
        if self.signal == Signal::Locked
            && let Some(tempo) = estimate.tempo
        {
            self.onsets.reference = Some(tempo);
            if before != Signal::Locked || estimate.newest_inlier {
                out.push(Event::Beat { at, tempo });
            }
        }
    }
}

fn frame_time(anchor: u64, frame: u64, rate: u32) -> u128 {
    u128::from(anchor) + u128::from(frame) * 1_000_000_000 / u128::from(rate)
}
