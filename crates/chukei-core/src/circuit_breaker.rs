//! Upstream circuit breaker (pattern reused from kafka-backup, PRD §7.3).
//!
//! After `threshold` consecutive upstream failures the breaker opens for
//! `cooldown`; while open, requests fast-fail with 503 instead of stacking
//! timeouts. First request after cooldown is the half-open probe.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct CircuitBreaker {
    threshold: u32,
    cooldown: Duration,
    consecutive_failures: AtomicU32,
    open_until_ms: AtomicU64,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, cooldown: Duration) -> Self {
        Self {
            threshold,
            cooldown,
            consecutive_failures: AtomicU32::new(0),
            open_until_ms: AtomicU64::new(0),
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// May a request proceed right now?
    pub fn allow(&self) -> bool {
        Self::now_ms() >= self.open_until_ms.load(Ordering::Relaxed)
    }

    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.open_until_ms.store(0, Ordering::Relaxed);
    }

    pub fn record_failure(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        if failures >= self.threshold {
            self.open_until_ms.store(
                Self::now_ms() + self.cooldown.as_millis() as u64,
                Ordering::Relaxed,
            );
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(5, Duration::from_secs(2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_after_threshold_and_recovers() {
        let breaker = CircuitBreaker::new(3, Duration::from_millis(50));
        assert!(breaker.allow());
        breaker.record_failure();
        breaker.record_failure();
        assert!(breaker.allow(), "below threshold stays closed");
        breaker.record_failure();
        assert!(!breaker.allow(), "threshold reached → open");
        std::thread::sleep(Duration::from_millis(60));
        assert!(
            breaker.allow(),
            "cooldown elapsed → half-open probe allowed"
        );
        breaker.record_success();
        assert!(breaker.allow());
        assert_eq!(
            breaker
                .consecutive_failures
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }
}
