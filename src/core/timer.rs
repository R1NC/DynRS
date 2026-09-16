use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// A scheduled script callback, kept by name because both runtimes look the function up in their
/// globals when it fires.
struct TimerEntry {
    end_time: Instant,
    callback: String,
}

/// The timers a script runtime hosts.
///
/// Both bridges share this so that `addTimer`, `pollTimers` and `removeTimer` behave the same in
/// Lua and in JS. The API itself is a DynRS addition: DynXX has no engine level timer functions.
pub struct Timers {
    next_id: i32,
    active: HashMap<i32, TimerEntry>,
}

impl Timers {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            active: HashMap::new(),
        }
    }

    /// Schedules `callback` to run `delay_secs` from now and returns the handle for it. A delay
    /// that is not a positive, finite number fires on the next poll.
    pub fn add(&mut self, delay_secs: f64, callback: &str) -> i32 {
        let delay = if delay_secs.is_finite() && delay_secs > 0.0 {
            delay_secs
        } else {
            0.0
        };

        let handle = self.next_id;
        self.next_id += 1;
        self.active.insert(
            handle,
            TimerEntry {
                end_time: Instant::now() + Duration::from_secs_f64(delay),
                callback: callback.to_string(),
            },
        );
        handle
    }

    /// Drops every timer that is due and returns its callback, shortest delay first and ties by
    /// handle, so the order is reproducible.
    pub fn poll(&mut self) -> Vec<String> {
        let now = Instant::now();
        let mut due = Vec::new();

        self.active.retain(|handle, entry| {
            if entry.end_time <= now {
                due.push((entry.end_time, *handle, entry.callback.clone()));
                false
            } else {
                true
            }
        });

        due.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        due.into_iter().map(|(_, _, callback)| callback).collect()
    }

    /// Drops the timer behind `handle`, reporting whether it was still pending.
    pub fn remove(&mut self, handle: i32) -> bool {
        self.active.remove(&handle).is_some()
    }
}

impl Default for Timers {
    fn default() -> Self {
        Self::new()
    }
}

/// Locks the timers of a bridge, reporting an error rather than waiting forever when a callback
/// tries to touch the timers again while they are being polled.
pub fn lock_timers(timers: &Mutex<Timers>) -> Result<MutexGuard<'_, Timers>, String> {
    timers
        .try_lock()
        .map_err(|_| "the timers are busy running a callback".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn a_due_timer_is_reported_once_in_delay_order() {
        let mut timers = Timers::new();
        timers.add(0.05, "later");
        timers.add(0.0, "now");

        // A timer that is not due yet is not reported.
        assert_eq!(timers.poll(), ["now"]);

        sleep(Duration::from_millis(60));
        assert_eq!(timers.poll(), ["later"]);

        // A timer only ever runs once.
        assert!(timers.poll().is_empty());
    }

    #[test]
    fn removing_a_timer_keeps_it_from_running() {
        let mut timers = Timers::new();
        let handle = timers.add(0.0, "dropped");
        assert!(timers.remove(handle));
        assert!(timers.poll().is_empty());

        // Removing it again reports that there was nothing left.
        assert!(!timers.remove(handle));
        assert!(!timers.remove(999));
    }

    #[test]
    fn handles_are_unique_and_a_useless_delay_fires_at_once() {
        let mut timers = Timers::new();
        let negative = timers.add(-1.0, "negative");
        let not_a_number = timers.add(f64::NAN, "nan");

        assert_ne!(negative, not_a_number);
        assert_eq!(timers.poll(), ["negative", "nan"]);

        // A timer that is far in the future stays pending until its delay has passed.
        let soon = timers.add(3600.0, "an hour away");
        assert!(timers.poll().is_empty());
        assert!(timers.remove(soon));
    }

    #[test]
    fn reentering_the_timers_reports_a_busy_error() {
        let timers = Mutex::new(Timers::new());
        let guard = lock_timers(&timers).unwrap();

        // A callback that schedules another timer is told to wait, it is not allowed to deadlock.
        assert!(lock_timers(&timers).is_err());

        drop(guard);
        assert!(lock_timers(&timers).is_ok());
    }
}
