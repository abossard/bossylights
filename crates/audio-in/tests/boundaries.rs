use audio_in::{Event, Format, Samples, Signal, Tempo, Tracker};

const START: u64 = 1_000_000_000;
const MS: u64 = 1_000_000;
const FRAME_NS: u64 = 125_000; // 8 kHz
const HOP: usize = 80;

fn tracker(channels: u16) -> Tracker {
    Tracker::new(Format::new(8_000, channels).unwrap())
}

fn wav() -> Vec<i16> {
    include_bytes!("data/click-93_75.wav")[44..]
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect()
}

// Positions and gaps are in frames at 8 kHz.
fn click_events(
    start: u64,
    first: usize,
    gaps: &[usize],
    click: &[i16],
    frames: usize,
) -> Vec<Event> {
    let mut samples = vec![0; frames];
    let mut frame = first;
    for gap in std::iter::once(0).chain(gaps.iter().copied()) {
        frame += gap;
        samples[frame..frame + click.len()].copy_from_slice(click);
    }
    let mut events = Vec::new();
    tracker(1)
        .push(start, Samples::I16(&samples), &mut events)
        .unwrap();
    events
}

// End on a reading after the last pulse.
fn pulses(first: usize, gaps: &[usize]) -> Vec<Event> {
    let last = first + gaps.iter().sum::<usize>();
    let pair = 2 * HOP;
    click_events(START, first, gaps, &[i16::MIN], (last / pair + 1) * pair)
}

fn clicks(gaps_ms: &[usize]) -> (u64, Vec<Event>) {
    let last_ms = 200 + gaps_ms.iter().sum::<usize>();
    let gaps: Vec<_> = gaps_ms.iter().map(|ms| ms * 8).collect();
    let events = click_events(0, 200 * 8, &gaps, &[24_576; 40], (last_ms + 40) * 8);
    (last_ms as u64 * MS, events)
}

#[test]
fn one_frame_either_side_of_the_gap_limits() {
    let tempo = |span, beats| Some(Tempo { span, beats });
    // Gaps in frames, onset count, final exact tempo, confidence.
    let cases: &[(&[usize], usize, Option<Tempo>, u8)] = &[
        (&[799], 1, None, 0),
        (&[800], 2, None, 0), // 100 ms
        (&[1599], 2, None, 0),
        (&[1600], 2, tempo(200_000_000, 1), 25),     // 200 ms
        (&[16_000], 2, tempo(2_000_000_000, 1), 25), // 2 s
        (&[16_001], 2, None, 0),
        // The gap outside the candidate range still counts toward confidence.
        (
            &[1600, 1600, 1600, 1600, 16_001],
            6,
            tempo(800_000_000, 4),
            80,
        ),
        (&[8000, 8400], 3, tempo(2_050_000_000, 2), 50), // 5% above the median
        (&[8000, 8401], 3, tempo(1_000_000_000, 1), 25),
        (&[8000, 8000, 7600], 4, tempo(2_950_000_000, 3), 75), // 5% below
        (&[8000, 8000, 7599], 4, tempo(2_000_000_000, 2), 50),
    ];
    for &(gaps, count, expected_tempo, expected_confidence) in cases {
        let events = pulses(HOP, gaps);
        let onsets = events
            .iter()
            .filter(|e| matches!(e, Event::Onset { .. }))
            .count();
        let Some(Event::Reading {
            tempo, confidence, ..
        }) = events.last()
        else {
            panic!("missing final reading for {gaps:?}");
        };
        assert_eq!(
            (onsets, *tempo, *confidence),
            (count, expected_tempo, expected_confidence),
            "{gaps:?}"
        );
    }
}

#[test]
fn the_inlier_window_extends_past_the_median_candidate_limits() {
    // Only candidates for the median are limited to 200 to 2000 ms.
    for (gaps, span) in [
        ([200, 200, 210, 199], 809),
        ([1950, 2000, 2000, 2001], 7951),
    ] {
        let (heard, events) = clicks(&gaps);
        let last_reading = events.iter().rev().find_map(|event| match event {
            Event::Reading {
                signal,
                confidence,
                tempo,
                ..
            } => Some((*signal, *confidence, *tempo)),
            Event::Onset { .. } | Event::Beat { .. } => None,
        });
        let last_beat = events.iter().rev().find_map(|event| match event {
            Event::Beat { at, tempo } => Some((*at, *tempo)),
            Event::Onset { .. } | Event::Reading { .. } => None,
        });
        let tempo = Tempo {
            span: span * MS,
            beats: 4,
        };
        assert_eq!(
            last_reading,
            Some((Signal::Locked, 100, Some(tempo))),
            "{gaps:?}"
        );
        assert_eq!(last_beat, Some((heard, tempo)), "{gaps:?}");
    }
}

#[test]
fn the_half_or_double_hold_uses_the_exact_candidate_tempo() {
    // The lock is 640 ms. Its doubled tempo has a 320 ms period.
    for (new_gaps, confidence) in [
        ([327, 327, 337, 337, 337], 59), // mean 333 ms, median 337 ms
        ([335, 335, 335, 335, 351], 62), // mean 338.2 ms, median 335 ms
    ] {
        let mut gaps = vec![640; 7];
        gaps.extend(new_gaps);
        let (heard, events) = clicks(&gaps);
        let actual = events.iter().find_map(|event| match event {
            Event::Onset { at, confidence } => (*at == heard).then_some(*confidence),
            Event::Beat { .. } | Event::Reading { .. } => None,
        });
        assert_eq!(actual, Some(confidence), "{new_gaps:?}");
    }
}

#[test]
fn the_half_or_double_hold_keeps_fractional_nanoseconds() {
    // 336 and 1344 ms are the upper 5% edges of a 640 ms lock's double and half tempo.
    for new_gap in [336, 1344] {
        for (last_shift, confidence) in [(0, 59), (1, 62)] {
            let mut tracker = tracker(1);
            let mut events = Vec::new();
            let mut stamp = 200 * MS;
            let gaps = std::iter::repeat_n(640, 7).chain(std::iter::repeat_n(new_gap, 5));
            for gap in gaps {
                let mut samples = vec![0; gap * 8];
                samples[..40].fill(24_576);
                tracker
                    .push(stamp, Samples::I16(&samples), &mut events)
                    .unwrap();
                stamp += gap as u64 * MS;
            }
            // Shifting the last onset by 1 ns increases the five-gap mean by 0.2 ns.
            stamp += last_shift;
            events.clear();
            tracker
                .push(stamp, Samples::I16(&[24_576; HOP]), &mut events)
                .unwrap();
            assert!(
                events.contains(&Event::Onset {
                    at: stamp,
                    confidence
                }),
                "{new_gap} ms, last click shifted by {last_shift} ns: {events:?}"
            );
        }
    }
}

#[test]
fn the_onset_rise_uses_twice_the_median_of_sixteen_previous_hops() {
    // Start, leading silent hops, loud hops, trailing silent hops, final sample, heard.
    let cases = [
        (START, 0, 9, 8, 10_240, true),  // the oldest loud hop has fallen out
        (START, 1, 8, 7, 10_240, true),  // the oldest silent hop still counts in the tie
        (START, 0, 9, 7, 10_240, false), // loud median, rise between once and twice that energy
        (0, 1, 8, 7, 6144, false), // middle energies 0 and 0.0625: rise below twice their mean
        (0, 1, 8, 7, 8192, true),  // rise equals twice their mean
    ];
    for (start, leading, loud, trailing, sample, heard) in cases {
        let mut samples = vec![0; leading * HOP];
        samples.extend(std::iter::repeat_n(8192, loud * HOP));
        samples.extend(std::iter::repeat_n(0, trailing * HOP));
        let at = start + samples.len() as u64 * FRAME_NS;
        samples.extend([sample; HOP]);
        let mut events = Vec::new();
        tracker(1)
            .push(start, Samples::I16(&samples), &mut events)
            .unwrap();
        let onset = events
            .iter()
            .any(|e| matches!(e, Event::Onset { at: t, .. } if *t == at));
        assert_eq!(
            onset, heard,
            "{leading} silent, {loud} loud, {trailing} silent, final sample {sample}"
        );
    }
}

#[test]
fn locking_in_a_reading_hop_appends_onset_then_beat_then_reading() {
    let events = pulses(HOP, &[5120; 4]); // 640 ms gaps, one full-scale frame per click
    let at = START + 2_570_000_000;
    let tempo = Tempo {
        span: 2_560_000_000,
        beats: 4,
    };
    assert!(events.ends_with(&[
        Event::Onset {
            at,
            confidence: 100
        },
        Event::Beat { at, tempo },
        Event::Reading {
            at: at + 10_000_000,
            level: 11,
            signal: Signal::Locked,
            confidence: 100,
            tempo: Some(tempo)
        },
    ]));
}

#[test]
fn a_gap_partway_through_a_stereo_hop_starts_a_new_meter_pair() {
    let mut tracker = tracker(2);
    let mut events = Vec::new();
    let loud: Vec<i16> = [0, i16::MIN].repeat(HOP + HOP / 2);
    tracker
        .push(START, Samples::I16(&loud), &mut events)
        .unwrap();
    tracker.gap();
    let restart = START + 15_000_000;
    let after = [[0, i16::MIN].repeat(HOP), vec![0; HOP * 2]].concat();
    tracker
        .push(restart, Samples::I16(&after), &mut events)
        .unwrap();
    assert_eq!(
        events,
        [
            Event::Onset {
                at: START,
                confidence: 0
            },
            Event::Onset {
                at: restart,
                confidence: 0
            },
            Event::Reading {
                at: restart + 20_000_000,
                level: 71,
                signal: Signal::Weak,
                confidence: 0,
                tempo: None
            },
        ]
    );
}

#[test]
fn rounded_hops_and_the_meter_ceiling() {
    // Rate, frames in two hops, amplitude, level. A half frame rounds up.
    for (rate, frames, value, level) in [(11_049, 220, 0.5, 50), (11_050, 222, 1.5, 100)] {
        let mut tracker = Tracker::new(Format::new(rate, 1).unwrap());
        let mut events = Vec::new();
        tracker
            .push(START, Samples::F32(&vec![value; frames]), &mut events)
            .unwrap();
        let at = START + frames as u64 * 1_000_000_000 / u64::from(rate);
        assert_eq!(
            events.last(),
            Some(&Event::Reading {
                at,
                level,
                signal: Signal::Weak,
                confidence: 0,
                tempo: None,
            }),
            "{rate} Hz, amplitude {value}"
        );
    }
}

#[test]
fn empty_blocks_leave_the_click_events_unchanged() {
    let samples = wav();
    let mut expected = Vec::new();
    tracker(1)
        .push(0, Samples::I16(&samples), &mut expected)
        .unwrap();

    // Insertion frame and how late the empty block is stamped.
    let cases = [
        ("first, 7 s late", 0, 7_000_000_000),
        ("first, within the gap tolerance", 0, FRAME_NS),
        (
            "partway through a locked hop, 1 s late",
            40_001,
            1_000_000_000,
        ),
    ];
    let mut mismatches = Vec::new();
    for (name, frame, late_by) in cases {
        let (before, after) = samples.split_at(frame);
        let at = frame as u64 * FRAME_NS;
        let mut tracker = tracker(1);
        let mut events = Vec::new();
        if !before.is_empty() {
            tracker.push(0, Samples::I16(before), &mut events).unwrap();
        }
        tracker
            .push(at + late_by, Samples::I16(&[]), &mut events)
            .unwrap();
        tracker.push(at, Samples::I16(after), &mut events).unwrap();
        if events != expected {
            mismatches.push(name);
        }
    }
    assert!(mismatches.is_empty(), "events changed for {mismatches:?}");
}

#[test]
fn a_beat_uses_the_stamp_of_the_block_that_heard_it() {
    let fifth = HOP + 4 * 5120;
    let mut before = vec![0; fifth];
    for frame in (HOP..fifth).step_by(5120) {
        before[frame] = i16::MIN;
    }
    let mut after = [0; 2 * HOP];
    after[4] = i16::MIN;

    for shift in [0, 10_000_000] {
        let mut tracker = tracker(1);
        let mut events = Vec::new();
        tracker
            .push(START, Samples::I16(&before), &mut events)
            .unwrap();
        let stamp = START + fifth as u64 * FRAME_NS + shift;
        tracker
            .push(stamp, Samples::I16(&after), &mut events)
            .unwrap();

        let heard = stamp + 4 * FRAME_NS;
        assert_eq!(
            events.iter().find(|e| matches!(e, Event::Beat { .. })),
            Some(&Event::Beat {
                at: heard,
                tempo: Tempo {
                    span: heard - (START + HOP as u64 * FRAME_NS),
                    beats: 4,
                },
            }),
            "capture stamp {shift} ns later than the sample count predicts"
        );
    }
}

#[test]
fn a_click_320_ms_into_a_shifted_block_keeps_lock_at_either_jump_limit() {
    let samples = wav();
    let (first, second) = samples.split_at(40_000);
    for (stamp_ms, heard_ms) in [(5020, 5340), (4980, 5300)] {
        let mut tracker = tracker(1);
        let mut events = Vec::new();
        tracker.push(0, Samples::I16(first), &mut events).unwrap();
        events.clear();
        tracker
            .push(stamp_ms * MS, Samples::I16(second), &mut events)
            .unwrap();
        let first_onset = events.iter().find_map(|e| match *e {
            Event::Onset { at, confidence } => Some((at, confidence)),
            Event::Beat { .. } | Event::Reading { .. } => None,
        });
        assert_eq!(
            first_onset,
            Some((heard_ms * MS, 100)),
            "stamp {stamp_ms} ms"
        );
    }
}

#[test]
fn capture_stamp_and_frame_offset_each_round_down_at_fractional_rates() {
    for rate in [11_025, 44_100] {
        let mut samples = vec![0; rate as usize];
        let click_length = (rate as usize * 5 + 500) / 1000;
        for ms in [200, 840] {
            let first = rate as usize * ms / 1000;
            samples[first..first + click_length].fill(24_576);
        }
        let mut events = Vec::new();
        let mut tracker = Tracker::new(Format::new(rate, 1).unwrap());
        for (i, block) in samples.chunks(512).enumerate() {
            let stamp = i as u64 * 512 * 1_000_000_000 / u64::from(rate);
            tracker
                .push(stamp, Samples::I16(block), &mut events)
                .unwrap();
        }
        let onsets: Vec<_> = events
            .iter()
            .filter_map(|e| match *e {
                Event::Onset { at, .. } => Some(at),
                Event::Beat { .. } | Event::Reading { .. } => None,
            })
            .collect();
        assert_eq!(onsets, [199_999_999, 839_999_999], "{rate} Hz");
    }
}

#[test]
fn the_onset_and_reading_use_the_stamps_of_their_own_frames() {
    // Split frame, pulse frame, shift of the second block, heard offset from START.
    for (split, pulse, shift, heard) in [
        (HOP, HOP + 8, 0, 11 * MS),
        (HOP, HOP + 8, 5 * MS, 16 * MS),
        (120, 88, 10 * MS, 11 * MS), // the pulse precedes the split in the same hop
        (120, 128, 10 * MS, 26 * MS),
    ] {
        let mut samples = [0; 2 * HOP];
        samples[pulse] = i16::MIN;
        let (first, second) = samples.split_at(split);
        let stamp = START + split as u64 * FRAME_NS + shift;
        let mut tracker = tracker(1);
        let mut events = Vec::new();
        tracker
            .push(START, Samples::I16(first), &mut events)
            .unwrap();
        tracker
            .push(stamp, Samples::I16(second), &mut events)
            .unwrap();
        assert!(
            matches!(
                events.as_slice(),
                [Event::Onset { at, .. }, Event::Reading { at: end, .. }]
                    if *at == START + heard && *end == START + 20 * MS + shift
            ),
            "split {split}, pulse {pulse}, shift {shift}: {events:?}"
        );
    }
}

#[test]
fn small_capture_clock_drift_does_not_create_a_gap() {
    let mut samples = [0; 5120];
    samples[3 * HOP] = i16::MIN;
    let mut tracker = tracker(1);
    let mut events = Vec::new();
    for block in 0..202 {
        tracker
            .push(
                START + block * 640_100_000,
                Samples::I16(&samples),
                &mut events,
            )
            .unwrap();
    }

    assert!(events.iter().any(|e| matches!(e, Event::Beat { .. })));
    let lost = events.iter().find(|e| {
        matches!(
            e,
            Event::Reading {
                signal: Signal::Lost,
                ..
            }
        )
    });
    assert_eq!(
        lost, None,
        "adjacent block stamps differ from the 640 ms block duration by only 100 us"
    );
}

#[test]
fn overlapping_stamps_keep_an_onset_that_is_ahead_of_the_next_hop() {
    let mut before = [0; 2 * HOP];
    before[2 * HOP - 1] = i16::MIN;
    for loud_frames in [0, 4] {
        let mut after = [0; 2 * HOP];
        after[..loud_frames].fill(i16::MIN);
        let mut tracker = tracker(1);
        let mut events = Vec::new();
        tracker
            .push(START, Samples::I16(&before), &mut events)
            .unwrap();
        assert!(
            matches!(events.as_slice(), [Event::Onset { at, .. }, Event::Reading { .. }]
            if *at == START + 159 * FRAME_NS)
        );
        events.clear();
        // Nondecreasing stamp, exactly 20 ms early. Its first hop ends before the onset.
        tracker
            .push(START, Samples::I16(&after), &mut events)
            .unwrap();
        assert!(
            matches!(events.as_slice(), [Event::Reading { at, signal: Signal::Weak, .. }]
            if *at == START + 20 * MS),
            "{loud_frames} loud frames: {events:?}"
        );
    }
}

#[test]
fn a_backwards_capture_stamp_is_a_gap_inside_the_jump_tolerance() {
    let cases = [
        (START + 10 * MS, START + 20 * MS, Signal::Weak),
        (START - FRAME_NS, START + 20 * MS - FRAME_NS, Signal::Lost),
    ];
    for loud_frames in [1, HOP] {
        let mut click = [0; HOP];
        click[..loud_frames].fill(i16::MIN);
        for (stamp, expected_at, expected_signal) in cases {
            let mut tracker = tracker(1);
            let mut events = Vec::new();
            tracker
                .push(START, Samples::I16(&click), &mut events)
                .unwrap();
            tracker
                .push(stamp, Samples::I16(&[0; 2 * HOP]), &mut events)
                .unwrap();
            assert!(
                matches!(events.as_slice(),
                    [Event::Onset { .. }, Event::Reading { at, signal, .. }]
                        if (*at, *signal) == (expected_at, expected_signal)
                ),
                "stamp {stamp}, {loud_frames} loud frames: {events:?}"
            );
        }
    }
}

#[test]
fn the_captured_click_time_decides_the_three_second_timeout() {
    use Signal::{Locked, Weak};

    #[derive(Clone, Copy, Debug)]
    enum BeforeClick {
        LastReading(u64),
        Beat(u64),
    }
    #[derive(Clone, Copy, Debug)]
    enum Capture {
        Inside { frames: usize, reading_at: u64 },
        NextBlock(BeforeClick),
    }
    let pulse = &[i16::MIN][..];
    let click = &[24_576; 40][..];
    // Stream start, first click frame, steady gaps, waveform, block layout,
    // distance either side of the deadline (ns), confidence before it.
    let inside = |frames, reading_at| Capture::Inside { frames, reading_at };
    let after_reading = Capture::NextBlock(BeforeClick::LastReading(7480 * MS));
    let after_beat = Capture::NextBlock(BeforeClick::Beat(2760 * MS));
    let cases = [
        (
            START,
            HOP / 2,
            7,
            pulse,
            inside(60_000, START + 7500 * MS),
            FRAME_NS,
            87,
        ),
        (
            0,
            205 * 8,
            4,
            click,
            inside(5800 * 8, 5780 * MS),
            FRAME_NS,
            80,
        ),
        (0, HOP, 7, pulse, after_reading, MS, 87),
        (0, 200 * 8, 4, click, after_beat, 1, 80),
    ];
    for (start, first, steady, click, capture, step, held) in cases {
        let last = first + steady * 5120;
        let deadline_frame = last + 24_000;
        let deadline = start + deadline_frame as u64 * FRAME_NS;
        for (heard, confidence, signal) in [
            (deadline - step, held, Locked),
            (deadline, 0, Weak),
            (deadline + step, 0, Weak),
        ] {
            let mut events;
            let observed = match capture {
                Capture::Inside { frames, reading_at } => {
                    let mut gaps = vec![5120; steady];
                    gaps.push(((heard - start) / FRAME_NS) as usize - last);
                    events = click_events(start, first, &gaps, click, frames);
                    let reading = events
                        .iter()
                        .position(|e| matches!(e, Event::Reading { at, .. } if *at == reading_at))
                        .unwrap();
                    &events[reading - 1..=reading]
                }
                Capture::NextBlock(before_click) => {
                    let mut before = vec![0; deadline_frame];
                    for frame in (first..=last).step_by(5120) {
                        before[frame..frame + click.len()].copy_from_slice(click);
                    }
                    let mut tracker = tracker(1);
                    events = Vec::new();
                    tracker
                        .push(start, Samples::I16(&before), &mut events)
                        .unwrap();
                    match before_click {
                        BeforeClick::LastReading(expected) => assert!(matches!(events.last(),
                            Some(Event::Reading { at, signal: Locked, .. }) if *at == expected
                        )),
                        BeforeClick::Beat(expected) => assert!(
                            events
                                .iter()
                                .any(|e| matches!(e, Event::Beat { at, .. } if *at == expected))
                        ),
                    }
                    events.clear();
                    let mut after = [0; 2 * HOP];
                    after[..click.len()].copy_from_slice(click);
                    tracker
                        .push(heard, Samples::I16(&after), &mut events)
                        .unwrap();
                    events.as_slice()
                }
            };
            assert!(
                matches!(observed,
                    [Event::Onset { at, confidence: c }, Event::Reading { signal: s, .. }]
                        if (*at, *c, *s) == (heard, confidence, signal)
                ),
                "{capture:?}, click at {heard}, deadline {deadline}: {observed:?}"
            );
        }
    }
}

#[test]
fn overlapping_quiet_blocks_stay_lost_until_an_onset() {
    // The last click at 2775 ms times out at 5775 ms, before the reading at 5780 ms.
    let mut before = vec![0; 5780 * 8];
    for ms in (215..=2775).step_by(640) {
        before[ms * 8] = i16::MIN;
    }
    for (sample, signal, confidence) in [(0, Signal::Lost, 0), (i16::MIN, Signal::Locked, 80)] {
        let mut tracker = tracker(1);
        let mut events = Vec::new();
        tracker.push(0, Samples::I16(&before), &mut events).unwrap();
        assert!(
            matches!(events.last(), Some(Event::Reading { at, signal: Signal::Lost, .. })
            if *at == 5780 * MS)
        );
        events.clear();
        // Both hops overlap the end of the first block and precede the timeout.
        let mut hop = [0; HOP];
        tracker
            .push(5760 * MS, Samples::I16(&hop), &mut events)
            .unwrap();
        hop[0] = sample;
        tracker
            .push(5760 * MS, Samples::I16(&hop), &mut events)
            .unwrap();
        assert!(
            matches!(events.last(), Some(Event::Reading { at, signal: s, confidence: c, .. })
            if (*at, *s, *c) == (5770 * MS, signal, confidence)),
            "{events:?}"
        );
    }
}

#[test]
fn an_onset_uses_the_first_frame_strictly_over_the_energy_threshold() {
    const CHANNELS: usize = 25;
    let threshold_prefix = [72, 1].map(|lead| {
        let mut samples = [0.0_f32; HOP * CHANNELS];
        for (i, frame) in samples.chunks_exact_mut(CHANNELS).enumerate() {
            // 0.25 squared across 25 channels is exactly the threshold, 0.0025.
            frame[0] = if i < lead { 0.25 } else { 0.5 };
        }
        (samples, lead)
    });
    let first_sample = [(0.125, 1), (0.25, 1), (0.5, 0)].map(|(first, index)| {
        let mut samples = [0.0; HOP * CHANNELS];
        samples[0] = first;
        samples[CHANNELS..2 * CHANNELS].fill(0.5);
        (samples, index)
    });
    for (samples, onset_frame) in threshold_prefix.into_iter().chain(first_sample) {
        let mut events = Vec::new();
        tracker(CHANNELS as u16)
            .push(START, Samples::F32(&samples), &mut events)
            .unwrap();
        assert_eq!(
            events.as_slice(),
            [Event::Onset {
                at: START + onset_frame as u64 * FRAME_NS,
                confidence: 0,
            }],
            "first louder frame at {onset_frame}"
        );
    }
}

#[test]
fn nonfinite_samples_count_as_silence_and_keep_other_channels() {
    for sample in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut silence = [0.0_f32; 2 * HOP];
        silence[HOP - 1] = sample;
        silence[HOP] = sample;
        let stereo = [sample, 1.0].repeat(2 * HOP);
        for (channels, samples, level, signal) in [
            (1, silence.as_slice(), 0, Signal::None),
            (2, stereo.as_slice(), 71, Signal::Weak),
        ] {
            let mut events = Vec::new();
            tracker(channels)
                .push(START, Samples::F32(samples), &mut events)
                .unwrap();
            let mut expected = Vec::new();
            if signal == Signal::Weak {
                expected.push(Event::Onset {
                    at: START,
                    confidence: 0,
                });
            }
            expected.push(Event::Reading {
                at: START + (2 * HOP) as u64 * FRAME_NS,
                level,
                signal,
                confidence: 0,
                tempo: None,
            });
            assert_eq!(events, expected, "{channels} channels with {sample}");
        }
    }
}

#[test]
fn unsigned_pcm_one_quantum_either_side_of_a_meter_rounding_boundary() {
    let cases = [(36_863, 12), (36_864, 13), (28_673, 12), (28_672, 13)];
    let actual = cases.map(|(sample, _)| {
        let mut events = Vec::new();
        tracker(1)
            .push(START, Samples::U16(&[sample; 2 * HOP]), &mut events)
            .unwrap();
        let Some(Event::Reading { level, .. }) = events.last() else {
            panic!("missing reading for unsigned sample {sample}");
        };
        *level
    });
    assert_eq!(actual, cases.map(|(_, level)| level));
}

#[test]
fn onset_energy_rules_distinguish_nearby_samples() {
    let mut quiet_then_click = [0.02; 2 * HOP];
    quiet_then_click[3 * HOP / 2..].fill(0.5);
    let small_rise = [[0.0; HOP], [0.049; HOP], [0.051; HOP], [0.0; HOP]].concat();
    // A single 0.5 sample gives exactly the minimum mean energy in a 100-frame hop.
    let mut at_gate = [0.0; 200];
    at_gate[0] = 0.5;
    // 1638 and 1639 unsigned PCM quanta straddle the minimum frame amplitude.
    let cases = [
        (
            "subthreshold pre-roll",
            8_000,
            Samples::F32(&quiet_then_click),
            Some(15 * MS),
            35,
        ),
        (
            "rise from 0.049 to 0.051",
            8_000,
            Samples::F32(&small_rise),
            None,
            5,
        ),
        (
            "U16 positive below gate",
            8_000,
            Samples::U16(&[34_406; 2 * HOP]),
            None,
            5,
        ),
        (
            "U16 negative above gate",
            8_000,
            Samples::U16(&[31_129; 2 * HOP]),
            Some(0),
            5,
        ),
        (
            "hop energy exactly at the gate",
            10_000,
            Samples::F32(&at_gate),
            Some(0),
            5,
        ),
    ];
    for (name, rate, samples, expected_onset, expected_level) in cases {
        let mut events = Vec::new();
        Tracker::new(Format::new(rate, 1).unwrap())
            .push(0, samples, &mut events)
            .unwrap();
        let onsets: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::Onset { at, .. } => Some(*at),
                Event::Beat { .. } | Event::Reading { .. } => None,
            })
            .collect();
        assert_eq!(onsets, expected_onset.as_slice(), "{name}");
        assert!(
            matches!(events.last(), Some(Event::Reading { level, .. }) if *level == expected_level),
            "{name}: {events:?}"
        );
    }
}
