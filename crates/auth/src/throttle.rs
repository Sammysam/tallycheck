//! Failed-login throttling (approved deviation D2; the reference had none).
//!
//! Five failures for one username inside 15 minutes lock that username until the oldest of
//! those failures is 15 minutes old. Counting is per username (case-insensitive) so a LAN
//! client cannot lock out the whole office. State is in memory: a restart clears it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MAX_FAILURES: usize = 5;
pub const WINDOW: Duration = Duration::from_secs(15 * 60);

#[derive(Default)]
pub struct LoginThrottle {
    failures: Mutex<HashMap<String, Vec<Instant>>>,
}

impl LoginThrottle {
    fn key(username: &str) -> String {
        username.trim().to_lowercase()
    }

    /// `Err(wait)` while the username is locked.
    pub fn check(&self, username: &str) -> Result<(), Duration> {
        self.check_at(username, Instant::now())
    }

    pub fn check_at(&self, username: &str, now: Instant) -> Result<(), Duration> {
        let mut map = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        let Some(list) = map.get_mut(&Self::key(username)) else { return Ok(()) };
        list.retain(|t| now.duration_since(*t) < WINDOW);
        if list.len() >= MAX_FAILURES {
            let oldest = list.iter().min().copied().unwrap_or(now);
            return Err(WINDOW.saturating_sub(now.duration_since(oldest)));
        }
        Ok(())
    }

    pub fn record_failure(&self, username: &str) {
        self.record_failure_at(username, Instant::now());
    }

    pub fn record_failure_at(&self, username: &str, at: Instant) {
        let mut map = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        // Bound memory: forget stale entries when the map grows.
        if map.len() > 10_000 {
            map.retain(|_, v| v.iter().any(|t| at.duration_since(*t) < WINDOW));
        }
        map.entry(Self::key(username)).or_default().push(at);
    }

    pub fn reset(&self, username: &str) {
        let mut map = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(&Self::key(username));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_after_five_failures_and_unlocks_after_window() {
        let t = LoginThrottle::default();
        let start = Instant::now();
        for i in 0..4u64 {
            t.record_failure_at("Admin", start + Duration::from_secs(i));
        }
        assert!(t.check_at("admin", start + Duration::from_secs(5)).is_ok());
        t.record_failure_at("ADMIN", start + Duration::from_secs(5));
        assert!(t.check_at("admin", start + Duration::from_secs(6)).is_err());
        assert!(t.check_at("someone-else", start + Duration::from_secs(6)).is_ok());
        assert!(t.check_at("admin", start + WINDOW + Duration::from_secs(1)).is_ok());
    }

    #[test]
    fn success_resets() {
        let t = LoginThrottle::default();
        for _ in 0..5 {
            t.record_failure("a");
        }
        assert!(t.check("a").is_err());
        t.reset("a");
        assert!(t.check("a").is_ok());
    }
}
