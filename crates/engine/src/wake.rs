//! When the tick wakes, and the one clock the whole engine reads (#165). The sender stamps
//! a command, the tick its frame and each hand-over, an output's thread what came of a
//! frame: all from one [`Clock`], since the engine started.
//!
//! | Clock | Its time | The tick waits |
//! |---|---|---|
//! | the wall: the only one without the feature `test-support` | since the engine started | sleeps, then spins the last stretch until the next output is due |
//! | stepped, with the feature `test-support` | what the test let pass: each wake exactly when the engine asked for it | until the test lets the next wake through ([`Wakes`]), or, running, as long as the wall would |
//!
//! One wake, and where a test may hold it:
//!
//! ```text
//! wait --> take the picture asked for and the commands published so far
//!      --> cutoff: read the frame time          (Wakes::cut holds the tick here)
//!      --> apply those commands, make the frame, hand it to each due output --> wait
//! ```
//!
//! A command published before the cutoff is in this frame, one published after it in the
//! next. Without the feature the stepped parts are not compiled: the tick pays nothing.

use engine_core::{Outcome, OutputNumber};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use std::{hint, thread};

#[cfg(feature = "test-support")]
pub use stepped::{Event, Wakes};

/// What the engine reads the time from. Cloned to the sender and each output's thread.
#[derive(Clone)]
pub(crate) struct Clock {
    start: Instant,
    #[cfg(feature = "test-support")]
    stepped: Option<std::sync::Arc<stepped::Steps>>,
}

impl Clock {
    /// The wall, from now.
    pub(crate) fn wall() -> Self {
        Self {
            start: Instant::now(),
            #[cfg(feature = "test-support")]
            stepped: None,
        }
    }

    /// Since the engine started.
    pub(crate) fn now(&self) -> Duration {
        #[cfg(feature = "test-support")]
        if let Some(steps) = &self.stepped {
            return steps.now();
        }
        self.start.elapsed()
    }

    /// Waits for the next wake: sleeps `sleep`, then spins until `spin`, if any. A sleep
    /// can wake late, a spin cannot. A stepped clock also returns once `stop` is set.
    #[cfg_attr(
        not(feature = "test-support"),
        expect(unused_variables, reason = "only a stepped clock waits for the test")
    )]
    pub(crate) fn wait(&self, sleep: Duration, spin: Option<Duration>, stop: &AtomicBool) {
        #[cfg(feature = "test-support")]
        if let Some(steps) = &self.stepped {
            steps.wait(
                spin.unwrap_or_else(|| steps.now().saturating_add(sleep)),
                stop,
            );
            return;
        }
        thread::sleep(sleep);
        while spin.is_some_and(|until| self.start.elapsed() < until) {
            hint::spin_loop();
        }
    }

    /// The frame time, read at the cutoff: the wake has taken the picture asked for and
    /// the commands published so far. A stepped clock may hold here until the test lets
    /// the wake on, or `stop` is set.
    #[cfg_attr(
        not(feature = "test-support"),
        expect(unused_variables, reason = "only a stepped clock holds at the cutoff")
    )]
    pub(crate) fn cutoff(&self, stop: &AtomicBool) -> Duration {
        #[cfg(feature = "test-support")]
        if let Some(steps) = &self.stepped {
            steps.cutoff(stop);
        }
        self.now()
    }
}

/// Where the tick writes down each due hand-over and what came of it. Without the
/// feature `test-support` it holds nothing and writes nothing.
pub(crate) struct Trace {
    #[cfg(feature = "test-support")]
    events: Option<rtrb::Producer<Event>>,
}

impl Trace {
    /// Writes nothing down.
    pub(crate) const fn none() -> Self {
        Self {
            #[cfg(feature = "test-support")]
            events: None,
        }
    }

    /// `output` was handed its frame due at `due`, in the wake at `at`.
    #[cfg_attr(
        not(feature = "test-support"),
        expect(unused_variables, reason = "only a test reads the trace")
    )]
    pub(crate) fn handed(&mut self, output: OutputNumber, due: Duration, at: Duration) {
        #[cfg(feature = "test-support")]
        self.write(Event::Handed { output, due, at });
    }

    /// What came of `output`'s frame due at `due`.
    #[cfg_attr(
        not(feature = "test-support"),
        expect(unused_variables, reason = "only a test reads the trace")
    )]
    pub(crate) fn came(&mut self, output: OutputNumber, outcome: Outcome, due: Duration) {
        #[cfg(feature = "test-support")]
        self.write(Event::Came {
            output,
            outcome,
            due,
        });
    }

    /// Wait-free, never allocates: a full trace loses the event.
    #[cfg(feature = "test-support")]
    fn write(&mut self, event: Event) {
        if let Some(events) = &mut self.events {
            let _ = events.push(event);
        }
    }
}

/// A clock the test steps, and the trace it reads. Only with the feature `test-support`.
#[cfg(feature = "test-support")]
mod stepped {
    use super::{Clock, Instant, Trace};
    use engine_core::{Outcome, OutputNumber};
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering::SeqCst};
    use std::thread;
    use std::time::Duration;

    /// Events the trace holds until the test reads them.
    const EVENTS: usize = 1 << 16;
    /// How often a held tick, and a test waiting for the tick, look again.
    const LOOK: Duration = Duration::from_micros(50);
    /// The longest a running tick sleeps before it looks at the mode again.
    const PACE: Duration = Duration::from_millis(1);
    /// How long the test waits for the tick before it gives up.
    const GIVE_UP: Duration = Duration::from_secs(10);

    /// Each due hand-over and what came of it, in the order the tick saw them.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Event {
        /// `output` was handed its frame due at `due`, in the wake at `at`.
        Handed {
            output: OutputNumber,
            due: Duration,
            at: Duration,
        },
        /// What came of `output`'s frame due at `due`: from its thread, or `Dropped` at
        /// once when it had no spare box.
        Came {
            output: OutputNumber,
            outcome: Outcome,
            due: Duration,
        },
    }

    /// What the test lets the tick do.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        /// Wakes when the engine asks, as long after the last as the wall would take.
        Run = 0,
        /// Waits for the test to let each wake through.
        Hold = 1,
    }

    /// Between the tick and the test. Atomics only. Wait `n` comes before wake `n`; the
    /// tick is waiting while `waits == woke + 1`.
    ///
    /// ```text
    /// waiting (waits = n) --Run: the wall says so / Hold: granted >= n--> woke = n
    ///   --cut = n--> held at the cutoff (at_cut = n) --released >= n--> serving
    ///   --otherwise-------------------------------------------------> serving
    ///   serving --> waiting (waits = n + 1)          any --stop--> on, never held
    /// ```
    pub(super) struct Steps {
        /// The engine's time, nanoseconds since it started.
        now: AtomicU64,
        mode: AtomicU8,
        waits: AtomicU64,
        woke: AtomicU64,
        /// The wait in which the tick last saw `Hold`.
        held: AtomicU64,
        /// The last wake the test let through.
        granted: AtomicU64,
        /// The wake to hold at its cutoff, the wake held there, the last one let on.
        cut: AtomicU64,
        at_cut: AtomicU64,
        released: AtomicU64,
        /// The time the next wake takes in place of the one asked for, if not 0.
        jump: AtomicU64,
    }

    impl Steps {
        pub(super) fn now(&self) -> Duration {
            Duration::from_nanos(self.now.load(SeqCst))
        }

        fn mode(&self) -> Mode {
            if self.mode.load(SeqCst) == Mode::Hold as u8 {
                Mode::Hold
            } else {
                Mode::Run
            }
        }

        /// The tick's wait until its next wake at `until`.
        pub(super) fn wait(&self, until: Duration, stop: &AtomicBool) {
            let wait = self.waits.fetch_add(1, SeqCst) + 1;
            let entered = Instant::now();
            let rest = until.saturating_sub(self.now());
            while !stop.load(SeqCst) {
                match self.mode() {
                    Mode::Hold => {
                        self.held.store(wait, SeqCst);
                        if self.granted.load(SeqCst) >= wait {
                            break;
                        }
                        thread::sleep(LOOK);
                    }
                    Mode::Run => {
                        let gone = entered.elapsed();
                        if gone >= rest {
                            break;
                        }
                        thread::sleep((rest - gone).min(PACE));
                    }
                }
            }
            let until = match self.jump.swap(0, SeqCst) {
                0 => u64::try_from(until.as_nanos()).unwrap_or(u64::MAX),
                at => at,
            };
            self.now.fetch_max(until, SeqCst);
            self.woke.store(wait, SeqCst);
        }

        /// The tick at the cutoff of its wake: held there if the test asked.
        pub(super) fn cutoff(&self, stop: &AtomicBool) {
            let wake = self.woke.load(SeqCst);
            if self.cut.load(SeqCst) != wake {
                return;
            }
            self.at_cut.store(wake, SeqCst);
            while self.released.load(SeqCst) < wake && !stop.load(SeqCst) {
                thread::sleep(LOOK);
            }
        }

        fn waiting(&self) -> bool {
            self.waits.load(SeqCst) == self.woke.load(SeqCst) + 1
        }
    }

    /// The test's end of a stepped clock: lets the tick through one wake at a time and
    /// reads what it handed over. Starts running; dropped, it runs again.
    pub struct Wakes {
        steps: Arc<Steps>,
        events: rtrb::Consumer<Event>,
    }

    /// A stepped clock, its trace, and the test's end of both.
    pub(crate) fn clock() -> (Clock, Trace, Wakes) {
        let steps = Arc::new(Steps {
            now: AtomicU64::new(0),
            mode: AtomicU8::new(Mode::Run as u8),
            waits: AtomicU64::new(0),
            woke: AtomicU64::new(0),
            held: AtomicU64::new(0),
            granted: AtomicU64::new(0),
            cut: AtomicU64::new(0),
            at_cut: AtomicU64::new(0),
            released: AtomicU64::new(0),
            jump: AtomicU64::new(0),
        });
        let (events, read) = rtrb::RingBuffer::new(EVENTS);
        let clock = Clock {
            start: Instant::now(),
            stepped: Some(Arc::clone(&steps)),
        };
        let trace = Trace {
            events: Some(events),
        };
        (
            clock,
            trace,
            Wakes {
                steps,
                events: read,
            },
        )
    }

    impl Wakes {
        /// Holds the tick at its next wait: from now on each wake waits for
        /// [`Wakes::wake`] or [`Wakes::cut`]. Returns once the tick holds.
        pub fn hold(&self) -> io::Result<()> {
            self.steps.mode.store(Mode::Hold as u8, SeqCst);
            self.next().map(drop)
        }

        /// Lets the next wake through, at the time the engine asked for. Returns its
        /// frame time once the tick waits again.
        pub fn wake(&self) -> io::Result<Duration> {
            let wake = self.next()?;
            self.steps.granted.store(wake, SeqCst);
            self.until(|steps| steps.waits.load(SeqCst) > wake)?;
            Ok(self.steps.now())
        }

        /// Lets the next wake through at `at`, early or late, in place of the time the
        /// engine asked for; never before the last wake. Returns its frame time once the
        /// tick waits again.
        pub fn wake_at(&self, at: Duration) -> io::Result<Duration> {
            let at = u64::try_from(at.as_nanos()).unwrap_or(u64::MAX).max(1);
            self.steps.jump.store(at, SeqCst);
            self.wake()
        }

        /// Moves the engine's time on to `at` while the tick waits, with no wake: what
        /// is sent now is stamped `at` and drained by the next wake.
        pub fn set_time(&self, at: Duration) {
            let at = u64::try_from(at.as_nanos()).unwrap_or(u64::MAX);
            self.steps.now.fetch_max(at, SeqCst);
        }

        /// The number of the last wake: the frame it made.
        pub fn woke(&self) -> u64 {
            self.steps.woke.load(SeqCst)
        }

        /// Lets the next wake through and holds it at its cutoff: what is sent now is
        /// in the next frame. Returns its frame time. [`Wakes::finish`] lets it on.
        pub fn cut(&self) -> io::Result<Duration> {
            let wake = self.next()?;
            self.steps.cut.store(wake, SeqCst);
            self.steps.granted.store(wake, SeqCst);
            self.until(|steps| steps.at_cut.load(SeqCst) == wake)?;
            Ok(self.steps.now())
        }

        /// Lets the wake held at its cutoff on. Returns once the tick waits again.
        pub fn finish(&self) -> io::Result<()> {
            let wake = self.steps.at_cut.load(SeqCst);
            self.steps.released.store(wake, SeqCst);
            self.until(|steps| steps.waits.load(SeqCst) > wake)
        }

        /// The tick wakes on its own again, paced by the wall.
        pub fn run(&self) {
            self.steps.mode.store(Mode::Run as u8, SeqCst);
            let held = self.steps.at_cut.load(SeqCst);
            self.steps.released.fetch_max(held, SeqCst);
        }

        /// What happened since the last call: each due hand-over and what came of it,
        /// in order. The trace holds [`EVENTS`]; more are lost until it is read.
        pub fn events(&mut self) -> Vec<Event> {
            let mut events = Vec::new();
            while let Ok(event) = self.events.pop() {
                events.push(event);
            }
            events
        }

        /// The number of the next wake, once the tick holds before it.
        fn next(&self) -> io::Result<u64> {
            self.until(|steps| {
                steps.waiting() && steps.held.load(SeqCst) == steps.waits.load(SeqCst)
            })?;
            Ok(self.steps.woke.load(SeqCst) + 1)
        }

        /// Waits until `done`, at most [`GIVE_UP`].
        fn until(&self, done: impl Fn(&Steps) -> bool) -> io::Result<()> {
            let started = Instant::now();
            while !done(&self.steps) {
                if started.elapsed() > GIVE_UP {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "the tick did not get there within 10 s",
                    ));
                }
                thread::sleep(LOOK);
            }
            Ok(())
        }
    }

    impl Drop for Wakes {
        fn drop(&mut self) {
            self.run();
        }
    }
}

#[cfg(feature = "test-support")]
pub(crate) use stepped::clock as stepped;
