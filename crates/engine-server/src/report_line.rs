//! The words of a report about an output, one source: the stop report of the process
//! (`main.rs`) and the hands-on examples take this file by its path.

#![allow(dead_code, reason = "each user takes its own part")]

use engine::{Counters, Lateness};
use engine_core::OFF_BEAT;

/// "lateness p99 120 µs, worst 3500 µs".
pub fn lateness(lateness: Lateness) -> String {
    format!(
        "lateness p99 {} µs, worst {} µs",
        lateness.p99.as_micros(),
        lateness.worst.as_micros()
    )
}

/// What an output counted and how late it was. The 2 ms is [`OFF_BEAT`], the one the
/// counter uses.
pub fn counts(c: Counters, late: Lateness) -> String {
    format!(
        "accepted {}, dropped {}, failed {}, off beat (> {} ms) {}, {}",
        c.accepted,
        c.dropped,
        c.failed,
        OFF_BEAT.as_millis(),
        c.off_beat,
        lateness(late)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_report_says_off_beat_and_microseconds() {
        let counters = Counters {
            accepted: 10,
            dropped: 2,
            failed: 3,
            off_beat: 4,
        };
        let late = Lateness {
            p99: Duration::from_micros(120),
            worst: Duration::from_micros(3500),
        };
        assert_eq!(
            counts(counters, late),
            "accepted 10, dropped 2, failed 3, off beat (> 2 ms) 4, lateness p99 120 µs, worst 3500 µs"
        );
        assert_eq!(lateness(late), "lateness p99 120 µs, worst 3500 µs");
    }
}
