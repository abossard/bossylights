//! The tracker of `docs/design/tempo-feeds.md` (5a, 4b), against the table of section 7.
//! Times in the tables are milliseconds of sample time; the tracker speaks nanoseconds.

use audio_in::{Event, Format, PartialFrame, Samples, Signal, Step, Tempo, Tracker};

const MS: u64 = 1_000_000;
const SECOND: u64 = 1_000_000_000;
const CLICK: i16 = 24_576;

/// The 8 kHz mono WAV made by `scripts/make-tempo-clicks.mjs`.
fn wav() -> Vec<i16> {
    let bytes = include_bytes!("data/click-93_75.wav");
    assert_eq!(bytes.len(), 160_044, "44 header bytes and 10 s of 16 bit samples");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..16], b"WAVEfmt ");
    assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1, "mono");
    assert_eq!(u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]), 8_000);
    assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16, "16 bit");
    assert_eq!(&bytes[36..40], b"data");
    bytes[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

/// Silence with a click of 5 ms (alternating +24576, -24576) starting at each time.
fn clicks(rate: u32, starts_ms: &[u64], length_ms: u64) -> Vec<i16> {
    let at = |ms: u64| usize::try_from(ms * u64::from(rate) / 1000).unwrap();
    let mut samples = vec![0_i16; at(length_ms)];
    let click = (rate as usize * 5 + 500) / 1000; // round(rate × 0.005): 40, 55, 221, 240
    for &start in starts_ms {
        for (i, sample) in samples.iter_mut().skip(at(start)).take(click).enumerate() {
            *sample = if i % 2 == 0 { CLICK } else { -CLICK };
        }
    }
    samples
}

/// The clicks of the WAV: the first at 200 ms, then every 640 ms.
fn steady() -> Vec<u64> {
    (0..16).map(|k| 200 + 640 * k).collect()
}

/// The time of frame `i`: what the capture side would stamp a block starting there with.
fn frame_time(rate: u32, i: usize) -> u64 {
    u64::try_from(u128::try_from(i).unwrap() * u128::from(SECOND) / u128::from(rate)).unwrap()
}

/// Feeds interleaved samples in blocks of the given frame counts (cycled), each stamped
/// with the exact time of its first frame.
fn feed(rate: u32, channels: u16, samples: &[i16], sizes: &[usize]) -> Vec<Event> {
    let mut tracker = Tracker::new(Format::new(rate, channels).unwrap());
    let mut events = Vec::new();
    let frames = samples.len() / usize::from(channels);
    let mut at = 0;
    for &size in sizes.iter().cycle() {
        if at >= frames {
            break;
        }
        let end = (at + size).min(frames);
        let block = &samples[at * usize::from(channels)..end * usize::from(channels)];
        tracker.push(frame_time(rate, at), Samples::I16(block), &mut events).unwrap();
        at = end;
    }
    events
}

fn mono(samples: &[i16]) -> Vec<Event> {
    feed(8_000, 1, samples, &[256])
}

fn onsets(events: &[Event]) -> Vec<(u64, u8)> {
    events
        .iter()
        .filter_map(|e| match *e {
            Event::Onset { at, confidence } => Some((at, confidence)),
            Event::Beat { .. } | Event::Reading { .. } => None,
        })
        .collect()
}

fn beats(events: &[Event]) -> Vec<(u64, Tempo)> {
    events
        .iter()
        .filter_map(|e| match *e {
            Event::Beat { at, tempo } => Some((at, tempo)),
            Event::Onset { .. } | Event::Reading { .. } => None,
        })
        .collect()
}

/// (time, level, signal, confidence, tempo) of every reading.
fn readings(events: &[Event]) -> Vec<(u64, u8, Signal, u8, Option<Tempo>)> {
    events
        .iter()
        .filter_map(|e| match *e {
            Event::Reading { at, level, signal, confidence, tempo } => Some((at, level, signal, confidence, tempo)),
            Event::Onset { .. } | Event::Beat { .. } => None,
        })
        .collect()
}

/// The reading where the signal changes, and the first one.
fn signal_changes(events: &[Event]) -> Vec<(u64, Signal)> {
    let mut changes: Vec<(u64, Signal)> = Vec::new();
    for (at, _, signal, _, _) in readings(events) {
        if changes.last().is_none_or(|&(_, last)| last != signal) {
            changes.push((at, signal));
        }
    }
    changes
}

fn level_at(events: &[Event], ms: u64) -> u8 {
    readings(events).into_iter().find(|r| r.0 == ms * MS).map(|r| r.1).unwrap()
}

fn in_ns<T: Copy>(table: &[(u64, T)]) -> Vec<(u64, T)> {
    table.iter().map(|&(ms, x)| (ms * MS, x)).collect()
}

/// (time, span, beats) in milliseconds.
fn tempos(table: &[(u64, u64, u64)]) -> Vec<(u64, Tempo)> {
    table.iter().map(|&(at, span, beats)| (at * MS, Tempo { span: span * MS, beats })).collect()
}

const WAV_ONSETS: &[(u64, u8)] = &[
    (200, 0), (840, 25), (1480, 50), (2120, 75), (2760, 100), (3400, 100), (4040, 100), (4680, 100),
    (5320, 100), (5960, 100), (6600, 100), (7240, 100), (7880, 100), (8520, 100), (9160, 100), (9800, 100),
];
const WAV_BEATS: &[(u64, u64, u64)] = &[
    (2760, 2560, 4), (3400, 3200, 5), (4040, 3840, 6), (4680, 4480, 7), (5320, 5120, 8), (5960, 5120, 8),
    (6600, 5120, 8), (7240, 5120, 8), (7880, 5120, 8), (8520, 5120, 8), (9160, 5120, 8), (9800, 5120, 8),
];

#[test]
fn the_wav_gives_its_known_answers() {
    let samples = wav();
    assert_eq!(samples, clicks(8_000, &steady(), 10_000), "the WAV is the click track");
    let events = mono(&samples);

    assert_eq!(onsets(&events), in_ns(WAV_ONSETS));
    assert_eq!(beats(&events), tempos(WAV_BEATS));
    assert_eq!(
        signal_changes(&events),
        in_ns(&[(20, Signal::None), (220, Signal::Weak), (2780, Signal::Locked)]),
    );
    assert_eq!(readings(&events).len(), 500, "a reading every 20 ms");
    assert_eq!((level_at(&events, 200), level_at(&events, 220), level_at(&events, 240)), (0, 53, 0));
    let locked = readings(&events).into_iter().find(|r| r.0 == 2780 * MS).unwrap();
    assert_eq!(
        (locked.3, locked.4),
        (100, Some(Tempo { span: 2560 * MS, beats: 4 })),
        "93.75 BPM, kept exact",
    );
}

#[test]
fn every_chunking_gives_the_same_events() {
    let samples = wav();
    let whole = feed(8_000, 1, &samples, &[samples.len()]);
    let cases: &[(&str, &[usize])] = &[
        ("1", &[1]),
        ("31", &[31]),
        ("256", &[256]),
        ("irregular", &[1, 7, 300, 13, 64, 2, 999]),
    ];
    for (name, sizes) in cases {
        assert_eq!(feed(8_000, 1, &samples, sizes), whole, "chunks of {name}");
    }
}

#[test]
fn every_sample_format_gives_the_same_events() {
    let samples = wav();
    let reference = mono(&samples);
    let inverted: Vec<i16> = samples.iter().flat_map(|&s| [s, -s]).collect();
    let as_f32: Vec<f32> = samples.iter().map(|&s| f32::from(s) / 32_768.0).collect();
    let as_f32_inverted: Vec<f32> = inverted.iter().map(|&s| f32::from(s) / 32_768.0).collect();
    let as_u16: Vec<u16> = samples.iter().map(|&s| (i32::from(s) + 32_768).try_into().unwrap()).collect();
    let cases: &[(&str, u16, Samples<'_>)] = &[
        ("i16 mono", 1, Samples::I16(&samples)),
        ("i16 stereo, right inverted", 2, Samples::I16(&inverted)),
        ("f32 mono", 1, Samples::F32(&as_f32)),
        ("f32 stereo, right inverted", 2, Samples::F32(&as_f32_inverted)),
        ("u16 mono", 1, Samples::U16(&as_u16)),
    ];
    for &(name, channels, block) in cases {
        let mut tracker = Tracker::new(Format::new(8_000, channels).unwrap());
        let mut events = Vec::new();
        tracker.push(0, block, &mut events).unwrap();
        assert_eq!(events, reference, "{name}");
    }
}

#[test]
fn every_rate_hears_the_same_beats() {
    let reference = mono(&wav());
    // rate, first reading (two hops of round(rate / 100) samples), every event equal
    let cases: &[(u32, u64, bool)] = &[
        (8_000, 20 * MS, true),
        (11_025, 19_954_648, false), // hops of 110 samples: whole samples, not whole milliseconds
        (44_100, 20 * MS, true),
        (48_000, 20 * MS, true),
    ];
    for &(rate, first, all_equal) in cases {
        let events = feed(rate, 1, &clicks(rate, &steady(), 10_000), &[512]);
        assert_eq!(onsets(&events), onsets(&reference), "{rate} Hz");
        assert_eq!(beats(&events), beats(&reference), "{rate} Hz");
        assert_eq!(readings(&events)[0].0, first, "{rate} Hz");
        if all_equal {
            assert_eq!(events, reference, "{rate} Hz");
        }
    }
}

struct Pattern {
    name: &'static str,
    starts: Vec<u64>,
    length_ms: u64,
    onsets: &'static [(u64, u8)],
    beats: &'static [(u64, u64, u64)],
    signals: &'static [(u64, Signal)],
}

/// Clicks every `first` ms for 8 clicks, then every `then` ms.
fn switch(first: u64, then: u64, until: u64) -> Vec<u64> {
    let mut starts = vec![200];
    while let Some(&last) = starts.last() {
        let next = last + if starts.len() < 8 { first } else { then };
        if next >= until {
            break;
        }
        starts.push(next);
    }
    starts
}

#[test]
fn the_tempo_follows_what_it_hears() {
    use Signal::{Locked, None, Weak};
    let cases = [
        Pattern {
            name: "one click missing (the ninth)",
            starts: steady().into_iter().filter(|&t| t != 5320).collect(),
            length_ms: 10_000,
            onsets: &[
                (200, 0), (840, 25), (1480, 50), (2120, 75), (2760, 100), (3400, 100), (4040, 100), (4680, 100),
                (5960, 87), (6600, 87), (7240, 87), (7880, 87), (8520, 87), (9160, 87), (9800, 87),
            ],
            beats: &[
                (2760, 2560, 4), (3400, 3200, 5), (4040, 3840, 6), (4680, 4480, 7),
                (6600, 4480, 7), (7240, 4480, 7), (7880, 4480, 7), (8520, 4480, 7), (9160, 4480, 7), (9800, 4480, 7),
            ],
            signals: &[(20, None), (220, Weak), (2780, Locked)],
        },
        Pattern {
            name: "one click 30 ms early (the eighth)",
            starts: steady().into_iter().map(|t| if t == 4680 { 4650 } else { t }).collect(),
            length_ms: 10_000,
            onsets: &[
                (200, 0), (840, 25), (1480, 50), (2120, 75), (2760, 100), (3400, 100), (4040, 100), (4650, 100),
                (5320, 100), (5960, 100), (6600, 100), (7240, 100), (7880, 100), (8520, 100), (9160, 100), (9800, 100),
            ],
            beats: &[
                (2760, 2560, 4), (3400, 3200, 5), (4040, 3840, 6), (4650, 4450, 7), (5320, 5120, 8), (5960, 5120, 8),
                (6600, 5120, 8), (7240, 5120, 8), (7880, 5120, 8), (8520, 5120, 8), (9160, 5120, 8), (9800, 5150, 8),
            ],
            signals: &[(20, None), (220, Weak), (2780, Locked)],
        },
        Pattern {
            name: "half time: the spacing doubles to 1280 ms",
            starts: switch(640, 1280, 20_000),
            length_ms: 20_000,
            onsets: &[
                (200, 0), (840, 25), (1480, 50), (2120, 75), (2760, 100), (3400, 100), (4040, 100), (4680, 100),
                (5960, 87), (7240, 75), (8520, 62), (9800, 50), (11_080, 59), (12_360, 75), (13_640, 87),
                (14_920, 100), (16_200, 100), (17_480, 100), (18_760, 100),
            ],
            beats: &[
                (2760, 2560, 4), (3400, 3200, 5), (4040, 3840, 6), (4680, 4480, 7),
                (13_640, 8960, 7), (14_920, 10_240, 8), (16_200, 10_240, 8), (17_480, 10_240, 8), (18_760, 10_240, 8),
            ],
            signals: &[(20, None), (220, Weak), (2780, Locked), (9820, Weak), (13_660, Locked)],
        },
        Pattern {
            name: "double time: the spacing halves to 320 ms",
            starts: switch(640, 320, 10_000),
            length_ms: 10_000,
            onsets: &[
                (200, 0), (840, 25), (1480, 50), (2120, 75), (2760, 100), (3400, 100), (4040, 100), (4680, 100),
                (5000, 87), (5320, 75), (5640, 62), (5960, 50), (6280, 59), (6600, 75), (6920, 87), (7240, 100),
                (7560, 100), (7880, 100), (8200, 100), (8520, 100), (8840, 100), (9160, 100), (9480, 100), (9800, 100),
            ],
            beats: &[
                (2760, 2560, 4), (3400, 3200, 5), (4040, 3840, 6), (4680, 4480, 7), (6920, 2240, 7), (7240, 2560, 8),
                (7560, 2560, 8), (7880, 2560, 8), (8200, 2560, 8), (8520, 2560, 8), (8840, 2560, 8), (9160, 2560, 8),
                (9480, 2560, 8), (9800, 2560, 8),
            ],
            signals: &[(20, None), (220, Weak), (2780, Locked), (5980, Weak), (6940, Locked)],
        },
        Pattern {
            name: "slow drift: each gap 4 ms longer than the last",
            starts: (0..29_u64).scan(200, |t, k| Some(std::mem::replace(t, *t + 640 + 4 * k))).collect(),
            length_ms: 20_000,
            onsets: &[
                (200, 0), (840, 25), (1484, 50), (2132, 75), (2784, 100), (3440, 100), (4100, 100), (4764, 100),
                (5432, 100), (6104, 100), (6780, 100), (7460, 100), (8144, 100), (8832, 100), (9524, 100),
                (10_220, 100), (10_920, 100), (11_624, 100), (12_332, 100), (13_044, 100), (13_760, 100),
                (14_480, 100), (15_204, 100), (15_932, 100), (16_664, 100), (17_400, 100), (18_140, 100),
                (18_884, 100), (19_632, 100),
            ],
            beats: &[
                (2784, 2584, 4), (3440, 3240, 5), (4100, 3900, 6), (4764, 4564, 7), (5432, 5232, 8), (6104, 5264, 8),
                (6780, 5296, 8), (7460, 5328, 8), (8144, 5360, 8), (8832, 5392, 8), (9524, 5424, 8),
                (10_220, 5456, 8), (10_920, 5488, 8), (11_624, 5520, 8), (12_332, 5552, 8), (13_044, 5584, 8),
                (13_760, 5616, 8), (14_480, 5648, 8), (15_204, 5680, 8), (15_932, 5712, 8), (16_664, 5744, 8),
                (17_400, 5776, 8), (18_140, 5808, 8), (18_884, 5840, 8), (19_632, 5872, 8),
            ],
            signals: &[(20, None), (220, Weak), (2800, Locked)],
        },
    ];
    for case in cases {
        let events = mono(&clicks(8_000, &case.starts, case.length_ms));
        assert_eq!(onsets(&events), in_ns(case.onsets), "{}: onsets", case.name);
        assert_eq!(beats(&events), tempos(case.beats), "{}: beats", case.name);
        assert_eq!(signal_changes(&events), in_ns(case.signals), "{}: signal", case.name);
    }
}

/// name, samples, onsets (ns, confidence), signal changes (ms)
type Quiet<'a> = (&'static str, Samples<'a>, &'static [(u64, u8)], &'static [(u64, Signal)]);

#[test]
fn no_rhythm_no_beat() {
    use Signal::{Lost, None, Weak};
    let tone: Vec<f32> = (0..20_u16)
        .cycle()
        .take(80_000)
        .map(|i| 0.5 * (2.0 * core::f32::consts::PI * f32::from(i) / 20.0).sin())
        .collect();
    let mut state: u32 = 0x2545_f491;
    let noise: Vec<i16> = (0..80_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 16) as u16 as i16 / 4
        })
        .collect();
    let silence = vec![0_i16; 80_000];
    let cases: &[Quiet<'_>] = &[
        ("silence", Samples::I16(&silence), &[], &[(20, None)]),
        ("a steady tone from the start", Samples::F32(&tone), &[(125_000, 0)], &[(20, Weak), (3020, Lost)]),
        ("steady noise from the start", Samples::I16(&noise), &[(0, 0)], &[(20, Weak), (3000, Lost)]),
    ];
    for &(name, block, expected_onsets, signals) in cases {
        let mut tracker = Tracker::new(Format::new(8_000, 1).unwrap());
        let mut events = Vec::new();
        tracker.push(0, block, &mut events).unwrap();
        assert_eq!(onsets(&events), expected_onsets, "{name}");
        assert!(beats(&events).is_empty(), "{name}");
        assert_eq!(signal_changes(&events), in_ns(signals), "{name}");
        let mut lost = readings(&events).into_iter().filter(|r| r.2 == Lost);
        assert!(lost.all(|r| r.3 == 0 && r.4.is_none()), "{name}: Lost has no estimate");
    }
    let silent = mono(&silence);
    assert!(readings(&silent).iter().all(|r| r.1 == 0), "silence reads 0");
}

#[test]
fn three_quiet_seconds_lose_the_beat() {
    let events = mono(&clicks(8_000, &steady(), 14_000));
    use Signal::{Locked, Lost, None, Weak};
    assert_eq!(
        signal_changes(&events),
        in_ns(&[(20, None), (220, Weak), (2780, Locked), (12_800, Lost)]),
        "the last click at 9800 ms, Lost at the first hop 3 s after it",
    );
    let lost = readings(&events).into_iter().find(|r| r.0 == 12_800 * MS).unwrap();
    assert_eq!((lost.3, lost.4), (0, Option::None), "no estimate once Lost");
}

/// How a block at 5000 ms (frame 40 000 of the WAV) arrives.
#[derive(Clone, Copy, Debug)]
enum Arrival {
    /// stamped this many ns later than its samples say (negative: earlier)
    Stamped(i64),
    /// `gap()` is called before it
    AfterGap,
    /// 100 ms of samples never arrive; the block is stamped with its true time
    Dropped100Ms,
}

/// Every event, and how many came before the block at 5000 ms.
fn with_arrival(arrival: Arrival) -> (Vec<Event>, usize) {
    let samples = wav();
    let split = 40_000;
    let mut tracker = Tracker::new(Format::new(8_000, 1).unwrap());
    let mut events = Vec::new();
    tracker.push(0, Samples::I16(&samples[..split]), &mut events).unwrap();
    let before = events.len();
    let (time, rest) = match arrival {
        Arrival::Stamped(shift) => (5000 * MS).checked_add_signed(shift).map(|t| (t, &samples[split..])).unwrap(),
        Arrival::AfterGap => {
            tracker.gap();
            (5000 * MS, &samples[split..])
        }
        Arrival::Dropped100Ms => (5100 * MS, &samples[split + 800..]),
    };
    tracker.push(time, Samples::I16(rest), &mut events).unwrap();
    (events, before)
}

#[test]
fn a_gap_restarts_the_tracker() {
    let reference = mono(&wav());
    let twenty = i64::try_from(20 * MS).unwrap();
    // arrival, is it a gap
    let cases = [
        (Arrival::Stamped(0), false),
        (Arrival::Stamped(twenty), false),
        (Arrival::Stamped(twenty + 1), true),
        (Arrival::Stamped(-twenty), false),
        (Arrival::Stamped(-twenty - 1), true),
        (Arrival::AfterGap, true),
        (Arrival::Dropped100Ms, true),
    ];
    for (arrival, gap) in cases {
        let (events, before) = with_arrival(arrival);
        if !gap {
            assert_eq!(events, reference, "{arrival:?}: the sample clock is kept");
            continue;
        }
        let after = &events[before..];
        let first = readings(after)[0];
        assert_eq!((first.2, first.3, first.4), (Signal::Lost, 0, Option::None), "{arrival:?}: Lost, no estimate");
        let confidences: Vec<u8> = onsets(after).iter().map(|o| o.1).take(5).collect();
        assert_eq!(confidences, [0, 25, 50, 75, 100], "{arrival:?}: it listens again from nothing");
        let fifth = onsets(after)[4].0;
        assert_eq!(beats(after)[0].0, fifth, "{arrival:?}: the fifth click locks again");
    }
    use Signal::{Locked, Lost, None, Weak};
    let (after_gap, _) = with_arrival(Arrival::AfterGap);
    assert_eq!(
        signal_changes(&after_gap),
        in_ns(&[(20, None), (220, Weak), (2780, Locked), (5020, Lost), (5340, Weak), (7900, Locked)]),
        "hops count again from the first sample after the gap",
    );
    let (dropped, _) = with_arrival(Arrival::Dropped100Ms);
    assert_eq!(
        signal_changes(&dropped),
        in_ns(&[(20, None), (220, Weak), (2780, Locked), (5120, Lost), (5340, Weak), (7900, Locked)]),
    );
    assert_eq!(onsets(&dropped)[8..], in_ns(&[(5320, 0), (5960, 25), (6600, 50), (7240, 75), (7880, 100), (8520, 100), (9160, 100), (9800, 100)]));
}

#[test]
fn the_signal_machine_has_every_cell() {
    use Signal::{Locked, Lost, None, Weak};
    let steps = [
        Step::Onset { confidence: 0 },
        Step::Onset { confidence: 59 },
        Step::Onset { confidence: 60 },
        Step::Onset { confidence: 79 },
        Step::Onset { confidence: 80 },
        Step::Onset { confidence: 100 },
        Step::Quiet { since_onset: 3 * SECOND - 1 },
        Step::Quiet { since_onset: 3 * SECOND },
        Step::Quiet { since_onset: 3 * SECOND + 1 },
        Step::Gap,
    ];
    // spec 4b: one row per state, one column per step above
    let table = [
        (None, [Weak, Weak, Weak, Weak, Weak, Weak, None, None, None, None]),
        (Weak, [Weak, Weak, Weak, Weak, Locked, Locked, Weak, Lost, Lost, Lost]),
        (Locked, [Weak, Weak, Locked, Locked, Locked, Locked, Locked, Lost, Lost, Lost]),
        (Lost, [Weak, Weak, Weak, Weak, Weak, Weak, Lost, Lost, Lost, Lost]),
    ];
    for (from, row) in table {
        for (step, to) in steps.iter().zip(row) {
            assert_eq!(from.next(*step), to, "{from:?} on {step:?}");
        }
    }
}

#[test]
fn a_format_outside_the_range_cannot_be_built() {
    // rate, channels, can it be built
    let cases = [
        (7_999, 1, false),
        (8_000, 1, true),
        (192_000, 32, true),
        (192_001, 1, false),
        (48_000, 0, false),
        (48_000, 33, false),
    ];
    for (rate, channels, built) in cases {
        assert_eq!(Format::new(rate, channels).is_some(), built, "{rate} Hz, {channels} channels");
    }
}

#[test]
fn a_partial_frame_is_refused_and_changes_nothing() {
    let samples = wav();
    let stereo: Vec<i16> = samples.iter().flat_map(|&s| [s, s]).collect();
    let mut tracker = Tracker::new(Format::new(8_000, 2).unwrap());
    let mut events = Vec::new();
    assert_eq!(tracker.push(0, Samples::I16(&stereo[..3]), &mut events), Err(PartialFrame));
    assert!(events.is_empty());
    tracker.push(0, Samples::I16(&stereo), &mut events).unwrap();
    assert_eq!(events, mono(&samples));
}
