use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

/// Time source used by Covey for wall-clock timestamps.
///
/// Lease ordering is maintained separately inside SQLite via a monotonic
/// logical clock, so wall-clock skew cannot resurrect expired leases.
pub trait Clock: Send + Sync {
    /// Returns the current wall-clock time in Unix milliseconds.
    fn wall_now_ms(&self) -> i64;
}

/// Production clock backed by [`SystemTime`].
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn wall_now_ms(&self) -> i64 {
        wall_clock_now_ms().unwrap_or(0)
    }
}

/// Test clock with manually controlled time.
#[derive(Debug, Clone)]
pub struct ManualClock {
    now_ms: Arc<AtomicI64>,
}

impl ManualClock {
    /// Creates a manual clock fixed at the provided Unix millisecond timestamp.
    pub fn new(now_ms: i64) -> Self {
        Self {
            now_ms: Arc::new(AtomicI64::new(now_ms)),
        }
    }

    /// Sets the current timestamp directly.
    pub fn set(&self, now_ms: i64) {
        self.now_ms.store(now_ms, Ordering::SeqCst);
    }

    /// Advances the current timestamp by the provided delta.
    pub fn advance(&self, delta_ms: i64) {
        self.now_ms.fetch_add(delta_ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn wall_now_ms(&self) -> i64 {
        self.now_ms.load(Ordering::SeqCst)
    }
}

fn wall_clock_now_ms() -> Option<i64> {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => Some(duration.as_millis() as i64),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, ManualClock};

    #[test]
    fn manual_clock_supports_set_and_advance() {
        let clock = ManualClock::new(1_000);

        assert_eq!(clock.wall_now_ms(), 1_000);

        clock.set(2_500);
        assert_eq!(clock.wall_now_ms(), 2_500);

        clock.advance(125);
        assert_eq!(clock.wall_now_ms(), 2_625);
    }

    #[test]
    fn manual_clock_clones_share_the_same_time_source() {
        let clock = ManualClock::new(10);
        let clone = clock.clone();

        clone.advance(15);
        assert_eq!(clock.wall_now_ms(), 25);

        clock.set(40);
        assert_eq!(clone.wall_now_ms(), 40);
    }
}
