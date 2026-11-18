use rt_thread::{BestEffort, Priority};
use std::time::Duration;

#[test]
fn the_thread_runs_with_the_priority_spawn_reports() {
    let (thread, priority) =
        rt_thread::spawn("rt-test", Duration::from_secs(1) / 44, |priority| priority)
            .expect("the system gives a thread");
    assert_eq!(thread.join().ok(), Some(priority));
    if cfg!(not(any(target_os = "macos", target_os = "linux"))) {
        assert_eq!(
            priority,
            Priority::BestEffort {
                reason: BestEffort::NotSupported
            }
        );
    }
}
