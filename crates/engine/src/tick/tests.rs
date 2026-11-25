use super::*;
#[cfg(feature = "test-support")]
use std::time::Instant;

#[cfg(feature = "test-support")]
#[expect(
    clippy::unwrap_used,
    reason = "test inputs and stepped wakes must succeed"
)]
mod health {
    use super::*;
    use engine_core::health::{HealthSample, press_target};

    // The outcome is selected by the DMX byte that actually reaches the device.
    pub(super) struct ByLevel;

    impl Device for ByLevel {
        fn take(&mut self, frame: &[u8]) -> Outcome {
            match frame.first() {
                Some(253) => Outcome::Dropped,
                Some(254) => Outcome::Failed,
                _ => Outcome::Accepted,
            }
        }
    }

    struct Desk {
        wakes: Wakes,
        engine: Engine,
        sender: Sender,
        start: Duration,
    }

    impl Desk {
        fn new() -> Self {
            Self::with_open(Universes::ONE, Box::new(outputs::open))
        }

        fn with_open(universes: Universes, open: Open) -> Self {
            let (engine, sender, wakes) = Engine::start_stepped(universes, open).unwrap();
            wakes.hold().unwrap();
            let start = wakes.wake().unwrap();
            Self {
                wakes,
                engine,
                sender,
                start,
            }
        }

        fn attach(&mut self, number: u8, rate: Rate) {
            let run = RunNumbers { first: 1, count: 1 }
                .check(Universes::ONE)
                .unwrap();
            self.sender
                .attach(number.try_into().unwrap(), Box::new(ByLevel), run, rate)
                .unwrap();
        }

        fn press(&mut self, level: u8) {
            self.sender
                .send(Command::SetChannel {
                    channel: ChannelNumbers {
                        universe: 1,
                        address: 1,
                    },
                    value: level,
                })
                .unwrap();
        }

        // Keep the simulated time still until the tick has received every result.
        fn frames(&mut self, at: Duration, count: usize) {
            let started = Instant::now();
            let mut came = 0;
            while came < count {
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "{came} of {count} frames came back"
                );
                self.wakes.wake_at(at).unwrap();
                came += self
                    .wakes
                    .events()
                    .iter()
                    .filter(|e| matches!(e, Event::Came { .. }))
                    .count();
            }
            assert_eq!(came, count);
        }

        fn sample(&mut self, at: Duration, number: u8) -> HealthSample {
            self.engine.latest(); // ask for the next wake
            self.wakes.wake_at(at).unwrap();
            let mut sample = HealthSample::new(Duration::MAX, u64::MAX, Status::NotFound);
            assert!(
                self.engine
                    .latest()
                    .health_sample(number.try_into().unwrap(), &mut sample)
            );
            sample
        }
    }

    #[test]
    fn an_accepted_frame_counts_just_its_prefix_on_its_output() {
        let mut desk = Desk::new();
        let start = desk.start;
        desk.attach(1, Rate::Thirty);
        desk.attach(2, Rate::Sixty);
        desk.frames(start, 2);
        let slow = desk.sample(start, 1);
        let fast = desk.sample(start, 2);

        let at = start + Rate::Sixty.period();
        let latency = Duration::from_millis(3);
        desk.wakes.set_time(at - latency);
        desk.press(91);
        desk.wakes.wake_at(at).unwrap(); // the frame carrying 91 is now in the lane
        desk.press(17); // applied before that frame's result is collected
        desk.frames(at, 1);
        for (number, mut expected, added) in [(1, slow, 0), (2, fast, 1)] {
            *expected
                .press
                .get_mut(bucket::<LATENCY_BUCKETS>(latency))
                .unwrap() += added;
            let sample = desk.sample(at, number);
            assert_eq!(sample.press, expected.press, "output {number}");
            assert_eq!(sample.press_worst, latency * added as u32);
            assert_eq!(
                sample.press_under_target,
                expected.press_under_target + added
            );
        }
    }

    #[test]
    fn attaching_another_output_does_not_count_as_a_press() {
        let mut desk = Desk::new();
        let rate = Rate::Sixty;
        let start = desk.start;
        desk.attach(1, rate);
        desk.frames(start, 1);
        let first = desk.sample(start, 1);
        assert_eq!(
            (first.press.iter().sum::<u64>(), first.press_under_target),
            (0, 0)
        );
        assert_eq!(
            desk.engine.latest().send_to_frame_p95(),
            Some(Duration::ZERO)
        );

        let due = start + rate.period();
        desk.wakes.set_time(due - Duration::from_millis(3));
        desk.press(91);
        desk.attach(2, rate);
        desk.frames(due, 2);

        for (output, presses) in [(1, 1), (2, 0)] {
            let sample = desk.sample(due, output);
            assert_eq!(
                (sample.press.iter().sum::<u64>(), sample.press_under_target),
                (presses, presses),
                "output {output}"
            );
        }
    }

    #[test]
    fn removing_another_output_does_not_count_as_a_press() {
        let mut desk = Desk::new();
        let rate = Rate::Sixty;
        let start = desk.start;
        desk.attach(1, rate);
        desk.attach(2, rate);
        desk.frames(start, 2);

        let due = start + rate.period();
        desk.wakes.set_time(due - Duration::from_millis(3));
        desk.sender
            .send(Command::RemoveOutput {
                output: 2.try_into().unwrap(),
            })
            .unwrap();
        desk.frames(due, 1);
        let sample = desk.sample(due, 1);
        assert_eq!(
            (sample.press.iter().sum::<u64>(), sample.press_under_target),
            (0, 0)
        );
    }

    #[test]
    fn attaching_another_output_is_not_a_press() {
        struct AcceptLevel(u8);
        impl Device for AcceptLevel {
            fn take(&mut self, frame: &[u8]) -> Outcome {
                if frame.first() == Some(&self.0) {
                    Outcome::Accepted
                } else {
                    Outcome::Failed
                }
            }
        }

        let universes = Universes::new(2).unwrap();
        let mut desk = Desk::with_open(universes, Box::new(outputs::open));
        let start = desk.start;
        let rate = Rate::Sixty;
        let attach = |desk: &mut Desk, number: u8, level| {
            let run = RunNumbers {
                first: u16::from(number),
                count: 1,
            }
            .check(universes)
            .unwrap();
            desk.sender
                .attach(
                    number.try_into().unwrap(),
                    Box::new(AcceptLevel(level)),
                    run,
                    rate,
                )
                .unwrap();
        };
        attach(&mut desk, 1, 91);
        desk.press(91);
        desk.frames(start, 1);
        let before = desk.sample(start, 1);
        assert_eq!(
            (before.press.iter().sum::<u64>(), before.press_under_target),
            (1, 1)
        );

        let due = start + rate.period();
        desk.wakes.set_time(due - Duration::from_millis(3));
        attach(&mut desk, 2, 0);
        desk.frames(due, 2);
        let running = desk.sample(due, 1);
        let new = desk.sample(due, 2);
        assert_eq!(
            (running.status, new.status),
            (Status::Running, Status::Connecting)
        );
        assert_eq!(
            (new.press.iter().sum::<u64>(), new.press_under_target),
            (0, 0)
        );
        assert_eq!(
            (
                running.press,
                running.press_worst,
                running.press_under_target
            ),
            (before.press, before.press_worst, before.press_under_target)
        );
    }

    #[test]
    fn dropped_and_failed_frames_leave_the_press_for_the_next_acceptance() {
        let rate = Rate::Sixty;
        for (level, dropped, failed) in [(253, 1, 0), (254, 0, 1)] {
            let mut desk = Desk::new();
            let start = desk.start;
            desk.attach(1, rate);
            desk.frames(start, 1);
            let mut expected = desk.sample(start, 1).press;

            let due = start + rate.period();
            let sent = due - Duration::from_millis(3);
            desk.wakes.set_time(sent);
            desk.press(level);
            desk.frames(due, 1);
            let sample = desk.sample(due, 1);
            assert_eq!(sample.press, expected);
            assert_eq!((sample.dropped, sample.failed), (dropped, failed));
            assert_eq!(sample.frames_handed, 2, "level {level}");

            let next = due + rate.period();
            desk.wakes.set_time(next);
            desk.press(42);
            desk.frames(next, 1);
            for time in [next - sent, Duration::ZERO] {
                *expected.get_mut(bucket::<LATENCY_BUCKETS>(time)).unwrap() += 1;
            }
            let sample = desk.sample(next, 1);
            assert_eq!(sample.press, expected, "level {level}");
            assert_eq!(sample.press_worst, next - sent);
        }
    }

    #[test]
    fn exact_press_target_and_existing_frame_counters_reach_the_snapshot() {
        for rate in [Rate::Thirty, Rate::FortyFour, Rate::Sixty] {
            let mut desk = Desk::new();
            let start = desk.start;
            desk.attach(1, rate);
            desk.frames(start, 1);
            let target = press_target(rate);
            let late = Duration::from_millis(7);
            let at = start + rate.period() + late;
            let ns = Duration::from_nanos(1);
            let mut expected = HealthSample::new(at, 1, Status::Running);
            for latency in [target + ns, target, target - ns] {
                desk.wakes.set_time(at - latency);
                desk.press(90);
                *expected
                    .press
                    .get_mut(bucket::<LATENCY_BUCKETS>(latency))
                    .unwrap() += 1;
            }
            desk.frames(at, 1);
            // The attach is no press: only target minus one nanosecond is under it.
            expected.press_under_target = 1;
            expected.press_worst = target + ns;
            expected.frames_handed = 2;
            expected.off_beat = 1;
            expected.lateness_worst = late;
            for latency in [Duration::ZERO, late] {
                *expected
                    .lateness
                    .get_mut(bucket::<LATENESS_BUCKETS>(latency))
                    .unwrap() += 1;
            }
            assert_eq!(desk.sample(at, 1), expected, "{rate:?}");
        }
    }

    #[test]
    fn only_entries_into_reconnecting_count_as_outages() {
        let mut desk = Desk::new();
        let mut at = desk.start;
        let rate = Rate::Sixty;
        desk.attach(1, rate);
        for (level, status, outages) in [
            (0, Status::Connecting, 0),
            (254, Status::Reconnecting { failures: 1 }, 1),
            (254, Status::Reconnecting { failures: 2 }, 1),
            (91, Status::Running, 1),
            (254, Status::Reconnecting { failures: 1 }, 2),
            (254, Status::Reconnecting { failures: 2 }, 2),
            (91, Status::Running, 2),
        ] {
            desk.wakes.set_time(at);
            desk.press(level);
            desk.frames(at, 1);
            let sample = desk.sample(at, 1);
            assert_eq!((sample.status, sample.outages), (status, outages));
            at += rate.period();
        }
    }

    #[test]
    fn a_returned_failure_counts_an_outage_only_in_its_own_generation() {
        #[derive(Clone, Default)]
        struct HoldFirst {
            entered: Arc<AtomicBool>,
            release: Arc<AtomicBool>,
        }

        impl Device for HoldFirst {
            fn take(&mut self, frame: &[u8]) -> Outcome {
                if frame.first() == Some(&91) {
                    self.entered.store(true, Ordering::Release);
                    let began = Instant::now();
                    while !self.release.load(Ordering::Acquire) {
                        if began.elapsed() > Duration::from_secs(10) {
                            return Outcome::Failed;
                        }
                        thread::yield_now();
                    }
                }
                ByLevel.take(frame)
            }
        }

        let rate = outputs::default_rate(engine_core::TEST_KIND);
        let set_output = |rate| Command::SetOutput {
            output: 1.try_into().unwrap(),
            kind: engine_core::TEST_KIND,
            universes: RunNumbers { first: 1, count: 1 },
            rate,
        };
        for (reconfigure, generation, outages) in [(true, 2, 0), (false, 1, 1)] {
            let gate = HoldFirst::default();
            let device = gate.clone();
            let mut desk = Desk::with_open(
                Universes::new(2).unwrap(),
                Box::new(move |_| Ok(Box::new(device.clone()))),
            );
            desk.sender.send(set_output(None)).unwrap();
            desk.press(91);
            desk.wakes.wake_at(desk.start).unwrap();
            let began = Instant::now();
            while !gate.entered.load(Ordering::Acquire) {
                assert!(began.elapsed() < Duration::from_secs(10), "first frame");
                thread::yield_now();
            }

            // Queue the failure behind the held acceptance. Both return in order
            // after the optional reconfigure: NotFound -> Connecting -> Reconnecting.
            let due = desk.start + rate.period();
            desk.press(254);
            desk.wakes.wake_at(due).unwrap();
            if reconfigure {
                desk.sender.send(set_output(Some(rate))).unwrap();
            }
            let before = desk.sample(due, 1);
            assert_eq!(
                (before.generation, before.status),
                (generation, Status::NotFound)
            );
            gate.release.store(true, Ordering::Release);
            desk.frames(due, 2);
            let sample = desk.sample(due, 1);
            assert_eq!(
                (sample.status, sample.failed, sample.outages),
                (Status::Reconnecting { failures: 1 }, 1, outages),
                "generation {generation}"
            );
        }
    }

    #[test]
    fn a_frame_returned_after_reconfigure_adds_no_press_or_outage() {
        let rate = Rate::Sixty;
        for (old_level, status, failures) in
            [(91, Status::Connecting, 0), (254, Status::NotFound, 1)]
        {
            let mut desk = Desk::new();
            let start = desk.start;
            desk.attach(1, rate);
            desk.frames(start, 1);
            desk.press(254);
            let failed = start + rate.period();
            desk.frames(failed, 1); // an outage in the old generation
            desk.press(old_level);
            let old = failed + rate.period();
            desk.wakes.wake_at(old).unwrap(); // still in flight, or waiting in the return queue
            desk.press(42); // after the old frame's prefix, before re-attach
            // The command prepared by SetOutput when the same kind keeps its lane.
            let base = desk.sender.base();
            let keep = Prepared::keep_output(
                base,
                1.try_into().unwrap(),
                RunNumbers { first: 1, count: 1 }
                    .check(Universes::ONE)
                    .unwrap(),
                rate,
            );
            desk.sender.admit(keep).unwrap();
            desk.press(7); // the new generation's first press
            desk.frames(old, 1);
            let sample = desk.sample(old, 1);
            assert_eq!(sample.generation, 2);
            // Status, counters and lateness take the old frame as before.
            assert_eq!(sample.status, status);
            assert_eq!(
                (sample.lateness.iter().sum::<u64>(), sample.failed),
                (1, failures)
            );
            assert_eq!(sample.press.iter().sum::<u64>(), 0);
            assert_eq!(sample.press_worst, Duration::ZERO);
            assert_eq!(
                (
                    sample.press_under_target,
                    sample.press_missing,
                    sample.outages
                ),
                (0, 0, 0)
            );

            let next = old + rate.period();
            desk.frames(next, 1);
            let sample = desk.sample(next, 1);
            assert_eq!(
                sample.press.iter().sum::<u64>(),
                1,
                "only the press after it"
            );
            assert_eq!(sample.press_worst, rate.period());
            assert_eq!(sample.press_under_target, 1);
        }
    }

    #[test]
    fn overwritten_presses_count_as_missing_once_and_reset_on_attach() {
        // Hold the frame carrying 91 while later commands overwrite its prefix.
        struct Hold(Arc<AtomicBool>);
        impl Device for Hold {
            fn take(&mut self, frame: &[u8]) -> Outcome {
                if frame.first() == Some(&91) {
                    let started = Instant::now();
                    while !self.0.load(Ordering::Acquire) {
                        if started.elapsed() > Duration::from_secs(10) {
                            return Outcome::Failed;
                        }
                        thread::yield_now();
                    }
                }
                Outcome::Accepted
            }
        }
        let mut desk = Desk::new();
        let start = desk.start;
        let rate = Rate::Sixty;
        let release = Arc::new(AtomicBool::new(false));
        desk.sender
            .attach(
                1.try_into().unwrap(),
                Box::new(Hold(Arc::clone(&release))),
                RunNumbers { first: 1, count: 1 }
                    .check(Universes::ONE)
                    .unwrap(),
                rate,
            )
            .unwrap();
        desk.frames(start, 1);
        desk.press(91);
        let held = start + rate.period();
        desk.wakes.wake_at(held).unwrap();
        for press in 1..=CARRY + 3 {
            desk.press(42);
            if press % QUEUE == 0 {
                desk.wakes.wake_at(held).unwrap();
            }
        }
        desk.wakes.wake_at(held).unwrap();
        release.store(true, Ordering::Release);
        desk.frames(held, 1);
        let early = desk.sample(held, 1);
        assert_eq!(early.press_missing, 1, "only 91 was in the accepted frame");
        assert_eq!(early.press.iter().sum::<u64>(), 0, "the attach is no press");
        let at = held + rate.period();
        desk.frames(at, 1);
        let next = at + rate.period();
        desk.frames(next, 1); // the same prefix a second time
        let sample = desk.sample(next, 1);
        assert_eq!(sample.press_missing, 4);
        assert_eq!(
            sample.press.get(bucket::<LATENCY_BUCKETS>(at - held)),
            Some(&(CARRY as u64))
        );
        assert_eq!(sample.press_under_target, CARRY as u64);
        desk.sender
            .send(Command::RemoveOutput {
                output: 1.try_into().unwrap(),
            })
            .unwrap();
        desk.attach(1, rate);
        let replacement = desk.sample(next, 1);
        assert_eq!(replacement.generation, 2, "only attaches advance the slot");
        assert_eq!(replacement.press_missing, 0);
        assert_eq!(replacement.press_worst, Duration::ZERO);
    }

    #[test]
    fn presses_are_counted_after_every_due_hand_over() {
        // Counts the frames its device took.
        struct Took(Arc<std::sync::atomic::AtomicUsize>);
        impl Device for Took {
            fn take(&mut self, _: &[u8]) -> Outcome {
                self.0.fetch_add(1, Ordering::Release);
                Outcome::Accepted
            }
        }
        let mut desk = Desk::new();
        let rate = Rate::Sixty;
        let took = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for number in [1, 2] {
            desk.sender
                .attach(
                    number.try_into().unwrap(),
                    Box::new(Took(Arc::clone(&took))),
                    RunNumbers { first: 1, count: 1 }
                        .check(Universes::ONE)
                        .unwrap(),
                    rate,
                )
                .unwrap();
        }
        let mut at = desk.start;
        let mut before = 0;
        for frame in 0..20 {
            // Send and take at the same stepped time: every press goes in bucket zero.
            desk.wakes.set_time(at);
            desk.press(91);
            desk.engine.latest(); // request the snapshot at the end of this wake
            desk.wakes.wake_at(at).unwrap();
            let at_handoff = desk
                .engine
                .shared
                .presses_at_handoff
                .load(Ordering::Relaxed);
            assert_eq!(at_handoff, before, "frame {frame}");
            let picture = desk.engine.latest();
            before = 0;
            for number in [1, 2] {
                let output = OutputNumber::try_from(number).unwrap();
                let mut sample = HealthSample::new(at, 0, Status::NotFound);
                assert!(picture.health_sample(output, &mut sample));
                assert_eq!(sample.frames_handed, frame as u64 + 1);
                before += sample.press.iter().sum::<u64>();
            }
            let events = desk.wakes.events();
            if before > at_handoff {
                let handed = events
                    .iter()
                    .rposition(|e| matches!(e, Event::Handed { .. }))
                    .unwrap();
                let counted = events
                    .iter()
                    .position(|e| matches!(e, Event::Counted { .. }))
                    .unwrap();
                assert!(handed < counted, "frame {frame}: {events:?}");
            }
            // Both devices take this wake's frames, returned to the next wake.
            let started = Instant::now();
            while took.load(Ordering::Acquire) < 2 * (frame + 1) {
                assert!(started.elapsed() < Duration::from_secs(10), "frame {frame}");
                thread::yield_now();
            }
            at += rate.period();
        }
        assert!(before > 0, "no wake counted a press");
    }

    #[test]
    fn an_in_flight_frame_counts_only_its_partially_retained_prefix() {
        struct Hold(Arc<AtomicBool>);

        impl Device for Hold {
            fn take(&mut self, frame: &[u8]) -> Outcome {
                if frame.first() == Some(&91) {
                    let started = Instant::now();
                    while !self.0.load(Ordering::Acquire) {
                        if started.elapsed() > Duration::from_secs(10) {
                            return Outcome::Failed;
                        }
                        thread::yield_now();
                    }
                }
                Outcome::Accepted
            }
        }

        let mut desk = Desk::new();
        let start = desk.start;
        let rate = Rate::Sixty;
        let release = Arc::new(AtomicBool::new(false));
        desk.sender
            .attach(
                1.try_into().unwrap(),
                Box::new(Hold(Arc::clone(&release))),
                RunNumbers { first: 1, count: 1 }
                    .check(Universes::ONE)
                    .unwrap(),
                rate,
            )
            .unwrap();
        desk.frames(start, 1);

        let held = start + rate.period();
        for (level, milliseconds) in [(11, 6), (22, 4), (33, 2), (91, 0)] {
            desk.wakes
                .set_time(held - Duration::from_millis(milliseconds));
            desk.press(level);
        }
        desk.wakes.wake_at(held).unwrap();
        // The ring now has two stamps from the held frame and CARRY - 2
        // from commands that were applied after that frame.
        for press in 1..=CARRY - 2 {
            desk.press(42);
            if press % QUEUE == 0 {
                desk.wakes.wake_at(held).unwrap();
            }
        }
        desk.wakes.wake_at(held).unwrap();
        release.store(true, Ordering::Release);
        desk.frames(held, 1);

        let sample = desk.sample(held, 1);
        let mut expected = [0; LATENCY_BUCKETS];
        for milliseconds in [0, 2] {
            *expected
                .get_mut(bucket::<LATENCY_BUCKETS>(Duration::from_millis(
                    milliseconds,
                )))
                .unwrap() += 1;
        }
        assert_eq!(sample.press_missing, 2);
        assert_eq!(sample.press, expected);
        assert_eq!(sample.press_worst, Duration::from_millis(2));
        assert_eq!(sample.press_under_target, 2);
    }

    #[test]
    fn same_device_reconfigure_preserves_existing_return_accounting() {
        let number = OutputNumber::try_from(1).unwrap();
        let rate = outputs::default_rate(engine_core::TEST_KIND);
        let set_output = |rate| Command::SetOutput {
            output: number,
            kind: engine_core::TEST_KIND,
            universes: RunNumbers { first: 1, count: 1 },
            rate,
        };
        let mut samples = Vec::new();
        let mut expected = Vec::new();
        for (level, outcome, status, counts) in [
            (91, Outcome::Accepted, Status::Connecting, (1, 0, 0)),
            (253, Outcome::Dropped, Status::NotFound, (0, 1, 0)),
            (254, Outcome::Failed, Status::NotFound, (0, 0, 1)),
        ] {
            let (mut engine, mut sender, mut wakes) = Engine::start_stepped(
                Universes::new(2).unwrap(),
                Box::new(|_| Ok(Box::new(ByLevel))),
            )
            .unwrap();
            wakes.hold().unwrap();
            let start = wakes.wake().unwrap();
            sender.send(set_output(None)).unwrap();

            let wait_for_return = |engine: &mut Engine, wakes: &mut Wakes, at, due, outcome| {
                let began = Instant::now();
                loop {
                    engine.latest();
                    wakes.wake_at(at).unwrap();
                    if wakes.events().contains(&Event::Came {
                        output: number,
                        outcome,
                        due,
                    }) {
                        break;
                    }
                    assert!(began.elapsed() < Duration::from_secs(10), "frame {due:?}");
                }
            };
            wait_for_return(&mut engine, &mut wakes, start, start, Outcome::Accepted);
            sender
                .send(Command::SetChannel {
                    channel: ChannelNumbers {
                        universe: 1,
                        address: 1,
                    },
                    value: level,
                })
                .unwrap();
            let due = start + rate.period();
            let late = Duration::from_millis(4);
            let at = due + late;
            wakes.wake_at(at).unwrap();
            sender.send(set_output(Some(rate))).unwrap(); // keeps the lane: a new generation

            wait_for_return(&mut engine, &mut wakes, at, due, outcome);
            let snapshot = engine.latest();
            let output = snapshot.output(number);
            assert!(output.is_some(), "output {number:?} is attached");
            let Some(output) = output else {
                return;
            };
            assert_eq!(
                output.universes,
                RunNumbers { first: 1, count: 1 }
                    .check(Universes::new(2).unwrap())
                    .unwrap()
            );
            let counters = output.counters;
            samples.push((
                level,
                output.connection(),
                (counters.accepted, counters.dropped, counters.failed),
                snapshot.lateness(number).map(|lateness| lateness.worst),
            ));
            expected.push((level, status, counts, Some(late)));
        }
        assert_eq!(samples, expected);
    }
}

#[cfg(feature = "test-support")]
#[test]
#[expect(
    clippy::unwrap_used,
    reason = "a missing frame or command fails this test"
)]
fn scorecard_stamps_the_admission_and_the_device_handoff() {
    struct Gated {
        // Each frame carries a different first byte. Hold its take until
        // the driver permits it to finish, including during a later wake.
        byte: Arc<AtomicU64>,
        permits: Arc<AtomicU64>,
        calls: u64,
    }
    impl Device for Gated {
        fn take(&mut self, frame: &[u8]) -> Outcome {
            self.calls += 1;
            let level = frame.first().copied().unwrap();
            self.byte.store(u64::from(level), Ordering::Release);
            let started = Instant::now();
            while self.permits.load(Ordering::Acquire) < self.calls {
                if started.elapsed() > Duration::from_secs(10) {
                    return Outcome::Failed;
                }
                thread::yield_now();
            }
            Outcome::Accepted
        }
    }

    let slot = engine_slot::take().unwrap();
    let (clock, trace, wakes) = wake::stepped();
    let (writer, mut timings) = timing::TimingWriter::new(clock.clone());
    let (mut engine, mut sender) = Engine::start_on(
        slot,
        Universes::ONE,
        Box::new(outputs::open),
        clock,
        trace,
        Some(writer),
    )
    .unwrap();
    wakes.hold().unwrap();
    let byte = Arc::new(AtomicU64::new(u64::MAX));
    let permits = Arc::new(AtomicU64::new(0));
    let heard = |value| {
        let started = Instant::now();
        while byte.load(Ordering::Acquire) != value {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "device never saw {value}"
            );
            thread::yield_now();
        }
    };
    let device = Gated {
        byte: byte.clone(),
        permits: permits.clone(),
        calls: 0,
    };
    let output = OutputNumber::try_from(1).unwrap();
    let run = RunNumbers { first: 1, count: 1 }
        .check(Universes::ONE)
        .unwrap();
    let rate = Rate::Sixty;
    let first = timings.now() + Duration::from_secs(1);
    wakes.set_time(first);
    sender.attach(output, Box::new(device), run, rate).unwrap();
    wakes.wake_at(first).unwrap();
    heard(0);
    let due = first + rate.period();
    let send = |sender: &mut Sender, value| {
        sender
            .send(Command::SetChannel {
                channel: ChannelNumbers {
                    universe: 1,
                    address: 1,
                },
                value,
            })
            .unwrap()
    };

    let admitted = first + Duration::from_millis(3);
    wakes.set_time(admitted);
    send(&mut sender, 91);
    assert_eq!(wakes.cut().unwrap(), due);
    let after_cut = due + Duration::from_millis(1);
    wakes.set_time(after_cut);
    send(&mut sender, 17);
    wakes.finish().unwrap();

    // Frame two waits behind the first device call. Its own device
    // handoff, completion and the next tick poll each have a distinct stamp.
    let handed = due + Duration::from_millis(5);
    wakes.set_time(handed);
    permits.store(1, Ordering::Release);
    heard(91);
    wakes.set_time(due + Duration::from_millis(8));
    permits.store(u64::MAX, Ordering::Release);
    let poll = due + rate.period();
    wakes.wake_at(poll).unwrap();
    heard(17); // frame two's result has been returned before this take
    engine.latest();
    wakes.wake_at(poll).unwrap();
    let mut health = engine_core::health::HealthSample::new(poll, 0, Status::NotFound);
    assert!(engine.latest().health_sample(output, &mut health));
    let latency = handed - admitted;
    assert_eq!(
        health.press.get(bucket::<LATENCY_BUCKETS>(latency)),
        Some(&1)
    );
    assert_eq!(health.press_worst, latency);

    let (mut commands, mut due_frames, mut finished) = (vec![], vec![], vec![]);
    while let Some(event) = timings.pop().unwrap() {
        match event {
            Timing::Applied { number, at } => commands.push((number, at)),
            Timing::Due {
                output,
                due,
                applied,
            } => due_frames.push((output, due, applied)),
            Timing::Finished {
                output,
                due,
                at,
                outcome,
            } if due < poll => {
                finished.push((output, due, at, outcome));
            }
            Timing::Finished { .. } => {}
        }
    }
    assert_eq!(commands, [(1, first), (2, admitted), (3, after_cut)]);
    assert_eq!(
        due_frames,
        [(output, first, 1), (output, due, 2), (output, poll, 3)]
    );
    assert_eq!(
        finished,
        [
            (output, first, first, Outcome::Accepted),
            (output, due, handed, Outcome::Accepted),
        ]
    );
}

#[test]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
fn each_output_counts_a_command_when_its_own_frame_carries_it() {
    let ms = Duration::from_millis;
    #[derive(Clone, Copy, Debug)]
    enum Step {
        Attach(usize),
        /// The `Attach` command sent at this time is applied, then the output's lane is fresh.
        Attached(u8, u64),
        Sent(u64),
        Handed(usize, u64),
    }
    use Step::{Attach, Attached, Handed, Sent};
    // (steps, times counted in ms)
    let cases = [
        (vec![Attach(0), Sent(10), Handed(0, 12)], vec![2]),
        // applied on a wake that served nothing: counted at the next frame
        (vec![Attach(0), Sent(10), Handed(0, 30)], vec![20]),
        (
            vec![Attach(0), Attach(1), Sent(10), Handed(0, 12), Handed(1, 33)],
            vec![2, 23],
        ),
        (vec![Attach(0), Sent(1), Sent(2), Handed(0, 5)], vec![4, 3]),
        (
            vec![Attach(0), Sent(1), Handed(0, 5), Handed(0, 9)],
            vec![4],
        ), // once
        (vec![Sent(1), Attach(0), Handed(0, 5)], vec![]), // sent before its device
        // every command counts on a running output, a fresh one nothing from before it
        (
            vec![
                Attached(0, 0),
                Sent(10),
                Attached(1, 11),
                Handed(1, 12),
                Handed(0, 13),
            ],
            vec![3, 2],
        ),
    ];
    let run = RunNumbers { first: 1, count: 1 }
        .check(Universes::ONE)
        .unwrap();
    for (steps, expected) in cases {
        let mut carried = Carried::new();
        let mut counted = vec![];
        for step in &steps {
            match *step {
                Attach(output) => carried.attached(output, false),
                Attached(slot, at) => {
                    let attach = EngineCommand::Attach {
                        output: OutputNumber::try_from(slot + 1).unwrap(),
                        universes: run,
                        rate: Rate::Sixty,
                    };
                    carried.applied(&attach, ms(at));
                    carried.attached(usize::from(slot), false);
                }
                Sent(at) => carried.applied(&EngineCommand::Tap, ms(at)),
                Handed(output, at) => carried.handed(output, ms(at), |t| counted.push(t)),
            }
        }
        assert_eq!(
            counted,
            expected.into_iter().map(ms).collect::<Vec<_>>(),
            "{steps:?}"
        );
    }
}

#[test]
#[expect(clippy::unwrap_used, reason = "a test that cannot set itself up fails")]
fn a_command_is_a_press_unless_it_only_manages_outputs() {
    let output = OutputNumber::try_from(1).unwrap();
    let universes = RunNumbers { first: 1, count: 1 }
        .check(Universes::ONE)
        .unwrap();
    let channel = ChannelNumbers {
        universe: 1,
        address: 1,
    }
    .check(Universes::ONE)
    .unwrap();
    let entry = engine_core::GrabEntry::new(0).unwrap();
    let play = PlayId::new(0, 1).unwrap();
    // (command, presses it adds)
    let cases = [
        (EngineCommand::SetChannel { channel, value: 1 }, 1),
        (EngineCommand::ReleaseChannel { channel }, 1),
        (EngineCommand::ReleaseChannels, 1),
        (EngineCommand::ReleaseAll, 1),
        (EngineCommand::Blackout { on: true }, 1),
        (EngineCommand::Fields, 1),
        (
            EngineCommand::Grab {
                entry,
                change: engine_core::GrabChange::Start,
            },
            1,
        ),
        (EngineCommand::Grabs, 1),
        (
            EngineCommand::Start {
                play,
                look: 0,
                rank: Rank::Manual,
                when: When::default(),
            },
            1,
        ),
        (EngineCommand::Keep { play }, 1),
        (
            EngineCommand::Rules {
                rules: MixRules::default(),
            },
            1,
        ),
        (EngineCommand::Plan { play }, 1),
        (EngineCommand::Release { play }, 1),
        (EngineCommand::Tap, 1),
        (EngineCommand::ReleaseTempo, 1),
        (
            EngineCommand::Feed {
                bpm: Bpm::new(120).unwrap(),
            },
            1,
        ),
        (
            EngineCommand::Attach {
                output,
                universes,
                rate: Rate::Sixty,
            },
            0,
        ),
        (EngineCommand::RemoveOutput { output }, 0),
    ];
    for (command, presses) in cases {
        let mut carried = Carried::new();
        carried.applied(&command, Duration::ZERO);
        assert_eq!(
            (carried.commands.total, carried.presses.total),
            (1, presses),
            "{command:?}"
        );
    }
}
