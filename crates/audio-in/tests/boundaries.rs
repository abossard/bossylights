use audio_in::{Event, Format, Samples, Signal, Tempo, Tracker};

const START: u64 = 1_000_000_000;
const FRAME_NS: u64 = 125_000; // 8 kHz
const HOP: usize = 80;

fn tracker(channels: u16) -> Tracker {
    Tracker::new(Format::new(8_000, channels).unwrap())
}

// The first pulse falls in the second hop. End on a reading after the last pulse.
fn pulses(gaps: &[usize]) -> Vec<Event> {
    let last = HOP + gaps.iter().sum::<usize>();
    let pair = 2 * HOP;
    let mut samples = vec![0; (last / pair + 1) * pair];
    let mut frame = HOP;
    samples[frame] = i16::MIN;
    for gap in gaps {
        frame += gap;
        samples[frame] = i16::MIN;
    }
    let mut events = Vec::new();
    tracker(1)
        .push(START, Samples::I16(&samples), &mut events)
        .unwrap();
    events
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
        let events = pulses(gaps);
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
fn the_onset_rise_uses_twice_the_lower_median_of_sixteen_previous_hops() {
    // Leading silent hops, loud hops, trailing silent hops, hear the final rise.
    let cases = [
        (0, 9, 8, true),  // the oldest loud hop has fallen out
        (1, 8, 7, true),  // the oldest silent hop still counts in the tie
        (0, 9, 7, false), // loud median, rise between once and twice that energy
    ];
    for (leading, loud, trailing, heard) in cases {
        let mut samples = vec![0; leading * HOP];
        samples.extend(std::iter::repeat_n(8192, loud * HOP));
        samples.extend(std::iter::repeat_n(0, trailing * HOP));
        let at = START + samples.len() as u64 * FRAME_NS;
        samples.extend([10_240; HOP]);
        let mut events = Vec::new();
        tracker(1)
            .push(START, Samples::I16(&samples), &mut events)
            .unwrap();
        let onset = events
            .iter()
            .any(|e| matches!(e, Event::Onset { at: t, .. } if *t == at));
        assert_eq!(
            onset, heard,
            "{leading} silent, {loud} loud, {trailing} silent"
        );
    }
}

#[test]
fn locking_in_a_reading_hop_appends_onset_then_beat_then_reading() {
    let events = pulses(&[5120; 4]); // 640 ms gaps, one full-scale frame per click
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
    let samples: Vec<_> = include_bytes!("data/click-93_75.wav")[44..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
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
