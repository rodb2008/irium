use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct RateLimiter {
    requests_per_minute: u32,
    buckets: HashMap<String, (u32, f64)>,
    cleanup_interval: f64,
    last_cleanup: f64,
}

impl RateLimiter {
    pub fn new(requests_per_minute: u32) -> RateLimiter {
        let now = now_secs();
        RateLimiter {
            requests_per_minute,
            buckets: HashMap::new(),
            cleanup_interval: 300.0,
            last_cleanup: now,
        }
    }

    pub fn is_allowed(&mut self, key: &str) -> bool {
        let now = now_secs();
        if now - self.last_cleanup > self.cleanup_interval {
            self.cleanup(now);
        }

        let (mut tokens, mut last_update) = self
            .buckets
            .get(key)
            .cloned()
            .unwrap_or((self.requests_per_minute, now));

        let elapsed = now - last_update;
        let to_add = (elapsed * (self.requests_per_minute as f64 / 60.0)).floor() as u32;
        tokens = std::cmp::min(self.requests_per_minute, tokens.saturating_add(to_add));

        if tokens > 0 {
            tokens -= 1;
            last_update = now;
            self.buckets.insert(key.to_string(), (tokens, last_update));
            true
        } else {
            self.buckets.insert(key.to_string(), (0, now));
            false
        }
    }

    fn cleanup(&mut self, now: f64) {
        self.buckets.retain(|_, (_, last)| now - *last <= 3600.0);
        self.last_cleanup = now;
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
