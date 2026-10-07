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
    /// that is not a positive, finite number fires on the next poll, and so does one too large to
    /// be represented: neither may panic, because this runs from inside a script engine callback
    /// where an escaping panic would abort the host.
    pub fn add(&mut self, delay_secs: f64, callback: &str) -> i32 {
        let delay = if delay_secs.is_finite() && delay_secs > 0.0 {
            // A finite float can still overflow a `Duration` (1e308 does), and `from_secs_f64`
            // panics on that rather than saturating.
            Duration::try_from_secs_f64(delay_secs).unwrap_or(Duration::ZERO)
        } else {
            Duration::ZERO
        };

        let handle = self.next_id;
        // Wrapping rather than `+=`: an overflow panics in debug and would otherwise silently
        // replace a pending timer in release. A handle is an opaque number the script hands back to
        // `removeTimer`, so a wrap after 2^31 registrations is a curiosity rather than a failure —
        // and the earlier comment here claimed handle 0 marked exhaustion, which it never did,
        // because `i32::MAX.wrapping_add(1)` is `i32::MIN`.
        self.next_id = self.next_id.wrapping_add(1);
        // `Instant + Duration` panics when the sum is unrepresentable, which a delay near
        // `Duration::MAX` reaches.
        let end_time = Instant::now()
            .checked_add(delay)
            .unwrap_or_else(Instant::now);
        self.active.insert(
            handle,
            TimerEntry {
                end_time,
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

    /// A delay that is finite but cannot be represented as a `Duration` used to panic inside
    /// `Duration::from_secs_f64`, and a delay just under the `Duration` ceiling used to panic in
    /// `Instant + Duration`. Either one aborted the host, because `add` runs from inside a script
    /// engine callback. `addTimer(1e308, "f")` is ordinary script input.
    #[test]
    fn an_unrepresentable_delay_fires_at_once_instead_of_panicking() {
        let mut timers = Timers::new();

        let overflow = timers.add(1e308, "overflowing");
        assert_eq!(timers.poll(), ["overflowing"]);

        let negative = timers.add(-1e308, "negative overflow");
        assert_eq!(timers.poll(), ["negative overflow"]);
        assert!(
            !timers.remove(negative),
            "a timer that already fired is gone"
        );

        let subnormal = timers.add(f64::MIN_POSITIVE, "subnormal");
        assert_eq!(timers.poll(), ["subnormal"]);

        assert_ne!(overflow, subnormal);
    }

    /// The delay is clamped rather than reported as a failure, so the handle is always usable and
    /// the id space does not run into the `i32` overflow that `+= 1` would panic on in debug.
    #[test]
    fn the_id_space_wraps_instead_of_panicking() {
        let mut timers = Timers::new();
        timers.next_id = i32::MAX;

        let last = timers.add(0.0, "last");
        assert_eq!(last, i32::MAX);

        let wrapped = timers.add(0.0, "wrapped");
        assert_eq!(wrapped, i32::MIN);
        assert_ne!(last, wrapped);
    }
}
