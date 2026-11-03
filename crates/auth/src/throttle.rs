//! Failed-login throttling (approved deviation D2; the reference had none).
//!
//! Five attempts for one key inside 15 minutes lock that key until the oldest of them is
//! 15 minutes old. Callers build the key from the username (case-insensitive) and whether the
//! request comes from this PC, so a LAN client cannot lock the main PC out, nor the whole
//! office. State is in memory: a restart clears it.
//!
//! [`LoginThrottle::begin`] checks and counts an attempt in one step, before the slow password
//! check runs. Counting afterwards let many requests sent at once all pass the check before any
//! failure was recorded. A successful attempt calls [`LoginThrottle::reset`].

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MAX_FAILURES: usize = 5;
pub const WINDOW: Duration = Duration::from_secs(15 * 60);
/// Keys remembered at most. When full, the key with the fewest recent attempts is forgotten
/// first, so throw-away usernames cannot push out a username that is being guessed.
pub const MAX_KEYS: usize = 10_000;

#[derive(Default)]
pub struct LoginThrottle {
    failures: Mutex<HashMap<String, Vec<Instant>>>,
}

impl LoginThrottle {
    fn key(username: &str) -> String {
        username.trim().to_lowercase()
    }

    /// Counts an attempt for `key` and answers `Err(wait)` instead when it is locked. The
    /// attempt stays counted as a failure unless [`reset`](Self::reset) follows.
    pub fn begin(&self, key: &str) -> Result<(), Duration> {
        self.begin_at(key, Instant::now())
    }

    pub fn begin_at(&self, key: &str, now: Instant) -> Result<(), Duration> {
        let mut map = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        let key = Self::key(key);
        if let Some(list) = map.get_mut(&key) {
            list.retain(|t| now.duration_since(*t) < WINDOW);
            if list.len() >= MAX_FAILURES {
                let oldest = list.iter().min().copied().unwrap_or(now);
                return Err(WINDOW.saturating_sub(now.duration_since(oldest)));
            }
        }
        Self::push(&mut map, key, now);
        Ok(())
    }

    /// `Err(wait)` while `key` is locked; counts nothing.
    pub fn check_at(&self, key: &str, now: Instant) -> Result<(), Duration> {
        let mut map = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        let Some(list) = map.get_mut(&Self::key(key)) else { return Ok(()) };
        list.retain(|t| now.duration_since(*t) < WINDOW);
        if list.len() >= MAX_FAILURES {
            let oldest = list.iter().min().copied().unwrap_or(now);
            return Err(WINDOW.saturating_sub(now.duration_since(oldest)));
        }
        Ok(())
    }

    pub fn record_failure_at(&self, key: &str, at: Instant) {
        let mut map = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        Self::push(&mut map, Self::key(key), at);
    }

    fn push(map: &mut HashMap<String, Vec<Instant>>, key: String, at: Instant) {
        if !map.contains_key(&key) && map.len() >= MAX_KEYS {
            map.retain(|_, v| v.iter().any(|t| at.duration_since(*t) < WINDOW));
            if map.len() >= MAX_KEYS {
                let weakest =
                    map.iter().min_by_key(|(_, v)| (v.len(), v.iter().max().copied())).map(|(k, _)| k.clone());
                if let Some(k) = weakest {
                    map.remove(&k);
                }
            }
        }
        map.entry(key).or_default().push(at);
    }

    pub fn reset(&self, key: &str) {
        let mut map = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(&Self::key(key));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.failures.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_after_five_attempts_and_unlocks_after_window() {
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
    fn begin_counts_before_the_slow_check_so_parallel_attempts_stop_at_five() {
        let t = LoginThrottle::default();
        let now = Instant::now();
        // Many requests arrive together; none has finished checking its password yet.
        let allowed = (0..300).filter(|_| t.begin_at("admin", now).is_ok()).count();
        assert_eq!(allowed, MAX_FAILURES);
    }

    #[test]
    fn success_resets() {
        let t = LoginThrottle::default();
        for _ in 0..5 {
            assert!(t.begin("a").is_ok());
        }
        assert!(t.begin("a").is_err());
        t.reset("a");
        assert!(t.begin("a").is_ok());
    }

    #[test]
    fn memory_is_bounded_and_junk_names_do_not_evict_a_guessed_username() {
        let t = LoginThrottle::default();
        let now = Instant::now();
        for _ in 0..4 {
            assert!(t.begin_at("admin", now).is_ok());
        }
        for i in 0..(MAX_KEYS + 500) {
            let _ = t.begin_at(&format!("junk-{i}"), now);
        }
        assert!(t.len() <= MAX_KEYS);
        assert!(t.begin_at("admin", now).is_ok(), "fifth attempt");
        assert!(t.begin_at("admin", now).is_err(), "admin's count survived the junk");
    }
}
