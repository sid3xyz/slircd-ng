//! Rate limiting for flood protection.
//!
//! Implements a token bucket algorithm for rate limiting client messages.
//!
//! **Note**: This module is deprecated in favor of `security::rate_limit::RateLimitManager`
//! which provides a global, concurrent rate limiter. Kept for tests and potential fallback.

#![allow(dead_code)]

use std::time::Instant;

/// Token bucket rate limiter.
///
/// Uses a token bucket algorithm where:
/// - Tokens are added at a fixed rate per second
/// - Each message costs 1 token
/// - If no tokens available, the message is rejected (rate limit exceeded)
pub struct RateLimiter {
    tokens: f32,
    last_check: Instant,
    rate: f32,
    capacity: f32,
}

impl RateLimiter {
    /// Create a new rate limiter.
    ///
    /// # Arguments
    /// * `rate` - Tokens added per second
    /// * `capacity` - Maximum token capacity (burst size)
    pub fn new(rate: f32, capacity: f32) -> Self {
        Self {
            tokens: capacity,
            last_check: Instant::now(),
            rate,
            capacity,
        }
    }

    /// Check if a message can be processed.
    ///
    /// Returns `true` if the message is allowed (token consumed),
    /// `false` if rate limit exceeded.
    pub fn check(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_check).as_secs_f32();
        self.last_check = now;

        // Add tokens for elapsed time
        self.tokens = (self.tokens + elapsed * self.rate).min(self.capacity);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_initial_capacity() {
        let mut limiter = RateLimiter::new(10.0, 5.0);
        // Should allow up to capacity messages immediately
        for _ in 0..5 {
            assert!(limiter.check());
        }
        // Next should fail
        assert!(!limiter.check());
    }

    #[test]
    fn test_rate_replenish() {
        let mut limiter = RateLimiter::new(10.0, 5.0);
        // Consume all tokens
        for _ in 0..5 {
            limiter.check();
        }
        assert!(!limiter.check());

        // Wait for some tokens to replenish
        sleep(Duration::from_millis(200)); // Should add ~2 tokens
        assert!(limiter.check());
        assert!(limiter.check());
    }
}
