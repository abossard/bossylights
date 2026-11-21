use super::Tempo;

const GAP_COUNT: usize = 8;

#[derive(Default)]
pub(super) struct Gaps {
    newest_first: [Option<u64>; GAP_COUNT],
}

#[derive(Clone, Copy, Default)]
pub(super) struct Estimate {
    pub(super) tempo: Option<Tempo>,
    pub(super) confidence: u8,
    pub(super) newest_inlier: bool,
}

impl Gaps {
    pub(super) fn push(&mut self, gap: u64) {
        self.newest_first.rotate_right(1);
        self.newest_first[0] = Some(gap);
    }

    fn median_candidate(&self) -> Option<u64> {
        let mut sorted = self.newest_first;
        sorted.sort_unstable();
        let mut candidates = sorted.into_iter().flatten().filter(is_candidate);
        let middle = candidates.clone().count().checked_sub(1)? / 2;
        candidates.nth(middle)
    }

    pub(super) fn estimate(&self, reference: Option<Tempo>) -> Estimate {
        let Some(median) = self.median_candidate() else {
            return Estimate::default();
        };
        let inlier = |gap: &u64| is_candidate(gap) && within_five_percent(*gap, median);
        let gaps = self.newest_first.iter().flatten();
        let kept = gaps.clone().count() as u64;
        let inliers = gaps.copied().filter(inlier);
        let tempo = Tempo {
            span: inliers.clone().sum(),
            beats: inliers.count() as u64,
        };
        let mut confidence = (100 * tempo.beats / kept.max(4)) as u8;
        if tempo.beats < 6 && reference.is_some_and(|last| near_half_or_double(median, last)) {
            confidence = confidence.min(59);
        }
        Estimate {
            tempo: Some(tempo),
            confidence,
            newest_inlier: self.newest_first[0].as_ref().is_some_and(inlier),
        }
    }
}

fn is_candidate(gap: &u64) -> bool {
    (200_000_000..=2_000_000_000).contains(gap)
}

fn within_five_percent(value: u64, target: u64) -> bool {
    value.abs_diff(target) <= target / 20
}

fn near_half_or_double(median: u64, reference: Tempo) -> bool {
    // Compare whole spans so a fractional beat is never rounded.
    let span = median * reference.beats;
    within_five_percent(span, 2 * reference.span) || within_five_percent(2 * span, reference.span)
}
