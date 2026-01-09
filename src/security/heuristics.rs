//! Behavioral heuristics engine for spam detection.
//!
//! Tracks per-user behavioral metrics to identify spam patterns:
//! - Message velocity (messages per time window)
//! - Fan-out detection (many unique recipients)
//! - Content repetition scoring
//!
//! Used by the spam detection service to trigger rate limiting or bans.

use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::HeuristicsConfig;

/// Tracks behavioral metrics for a single user session
#[derive(Debug)]
struct UserMetrics {
    /// Timestamps of recent messages for velocity tracking
    message_timestamps: VecDeque<Instant>,
    /// Timestamps of recent unique recipients for fan-out tracking
    recipient_timestamps: VecDeque<Instant>,
    /// Hash of the last message content
    last_message_hash: u64,
    /// Current repetition score
    repetition_score: f32,
}

impl UserMetrics {
    fn new() -> Self {
        Self {
            message_timestamps: VecDeque::new(),
            recipient_timestamps: VecDeque::new(),
            last_message_hash: 0,
            repetition_score: 0.0,
        }
    }

    /// Prune old timestamps based on window size
    fn prune(&mut self, now: Instant, velocity_window: u64, fanout_window: u64) {
        let velocity_cutoff = now - Duration::from_secs(velocity_window);
        while let Some(&t) = self.message_timestamps.front() {
            if t < velocity_cutoff {
                self.message_timestamps.pop_front();
            } else {
                break;
            }
        }

        let fanout_cutoff = now - Duration::from_secs(fanout_window);
        while let Some(&t) = self.recipient_timestamps.front() {
            if t < fanout_cutoff {
                self.recipient_timestamps.pop_front();
            } else {
                break;
            }
        }
    }
}

/// Manages behavioral heuristics for spam detection
#[derive(Clone)]
pub struct HeuristicsEngine {
    config: HeuristicsConfig,
    /// Maps Session ID (or IP) to metrics
    metrics: Arc<DashMap<String, UserMetrics>>,
}

impl HeuristicsEngine {
    pub fn new(config: HeuristicsConfig) -> Self {
        Self {
            config,
            metrics: Arc::new(DashMap::new()),
        }
    }

    /// Analyze a message event and return a risk score (0.0 - 1.0)
    ///
    /// * `key`: Unique identifier for the user (e.g., IP or Session ID)
    /// * `content`: The message content
    /// * `is_private_msg`: True if this is a PRIVMSG to a user (affects fan-out)
    pub fn analyze(&self, key: &str, content: &str, is_private_msg: bool) -> f32 {
        let now = Instant::now();
        let mut metrics = self
            .metrics
            .entry(key.to_string())
            .or_insert_with(UserMetrics::new);

        // 1. Prune old data
        metrics.prune(now, self.config.velocity_window, self.config.fanout_window);

        // 2. Update Velocity
        metrics.message_timestamps.push_back(now);
        let velocity_score = if metrics.message_timestamps.len() > self.config.max_velocity {
            let excess = metrics.message_timestamps.len() - self.config.max_velocity;
            (excess as f32 / self.config.max_velocity as f32).min(1.0)
        } else {
            0.0
        };

        // 3. Update Fan-Out (only for private messages)
        let fanout_score = if is_private_msg {
            metrics.recipient_timestamps.push_back(now);
            if metrics.recipient_timestamps.len() > self.config.max_fanout {
                let excess = metrics.recipient_timestamps.len() - self.config.max_fanout;
                (excess as f32 / self.config.max_fanout as f32).min(1.0)
            } else {
                0.0
            }
        } else {
            0.0
        };

        // 4. Update Repetition
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        use std::hash::{Hash, Hasher};
        content.hash(&mut hasher);
        let current_hash = hasher.finish();

        if current_hash == metrics.last_message_hash {
            metrics.repetition_score += 1.0;
        } else {
            metrics.repetition_score *= self.config.repetition_decay;
        }
        metrics.last_message_hash = current_hash;

        // Normalize repetition score (e.g., > 5 repeats is 1.0)
        let repetition_risk = (metrics.repetition_score / 5.0).min(1.0);

        // 5. Aggregate Score (Weighted Average)
        // Velocity: 40%, Fan-Out: 40%, Repetition: 20%
        (velocity_score * 0.4) + (fanout_score * 0.4) + (repetition_risk * 0.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> HeuristicsConfig {
        HeuristicsConfig {
            enabled: true,
            velocity_window: 10,
            max_velocity: 5,
            fanout_window: 60,
            max_fanout: 10,
            repetition_decay: 0.5,
        }
    }

    #[test]
    fn test_initial_message_no_risk() {
        let engine = HeuristicsEngine::new(test_config());
        let risk = engine.analyze("user1", "Hello world", false);
        assert!(
            risk < 0.1,
            "First message should have near-zero risk, got {}",
            risk
        );
    }

    #[test]
    fn test_velocity_under_threshold() {
        let engine = HeuristicsEngine::new(test_config());
        // Send 5 messages (at threshold)
        for i in 0..5 {
            let risk = engine.analyze("user1", &format!("Message {}", i), false);
            assert!(
                risk < 0.5,
                "Messages at threshold should have low risk, got {}",
                risk
            );
        }
    }

    #[test]
    fn test_velocity_over_threshold() {
        let engine = HeuristicsEngine::new(test_config());
        // Send 10 messages rapidly (well over threshold of 5)
        for i in 0..10 {
            engine.analyze("user1", &format!("Message {}", i), false);
        }
        // The 11th message should trigger velocity risk
        let risk = engine.analyze("user1", "Message 10", false);
        assert!(
            risk > 0.2,
            "Excess velocity should increase risk, got {}",
            risk
        );
    }

    #[test]
    fn test_repetition_increases_risk() {
        let engine = HeuristicsEngine::new(test_config());
        let same_message = "Same message";

        // First message - no repetition
        let risk1 = engine.analyze("user1", same_message, false);

        // Same message again
        let risk2 = engine.analyze("user1", same_message, false);
        assert!(risk2 > risk1, "Repeated message should increase risk");

        // And again
        let risk3 = engine.analyze("user1", same_message, false);
        assert!(
            risk3 > risk2,
            "Repeated message should increase risk further"
        );
    }

    #[test]
    fn test_different_users_independent() {
        let engine = HeuristicsEngine::new(test_config());

        // User1 floods
        for i in 0..10 {
            engine.analyze("user1", &format!("Msg {}", i), false);
        }
        let user1_risk = engine.analyze("user1", "Final", false);

        // User2 sends first message - should be clean
        let user2_risk = engine.analyze("user2", "Hello", false);

        assert!(
            user2_risk < user1_risk,
            "Different users should have independent metrics"
        );
        assert!(
            user2_risk < 0.1,
            "New user should have low risk, got {}",
            user2_risk
        );
    }

    #[test]
    fn test_private_message_fanout() {
        let engine = HeuristicsEngine::new(test_config());

        // Send private messages (is_private_msg = true)
        for i in 0..15 {
            engine.analyze("user1", &format!("PM {}", i), true);
        }
        // Fanout should contribute to risk
        let risk = engine.analyze("user1", "Final PM", true);
        assert!(
            risk > 0.2,
            "Excessive fanout should increase risk, got {}",
            risk
        );
    }

    #[test]
    fn test_channel_message_no_fanout() {
        let engine = HeuristicsEngine::new(test_config());

        // Send channel messages (is_private_msg = false)
        for i in 0..15 {
            engine.analyze("user1", &format!("Chan msg {}", i), false);
        }
        // With velocity of 5, exceeding it should add some risk
        // but no fanout risk since is_private_msg = false
        let risk = engine.analyze("user1", "Final", false);
        // Should have velocity risk but not as high as if fanout was counted
        assert!(
            risk < 0.5,
            "Channel messages should not trigger fanout risk, got {}",
            risk
        );
    }

    #[test]
    fn test_repetition_decay_on_different_message() {
        let engine = HeuristicsEngine::new(test_config());

        // Build up repetition
        for _ in 0..5 {
            engine.analyze("user1", "Same", false);
        }

        // Now send different message - should decay
        engine.analyze("user1", "Different message", false);
        engine.analyze("user1", "Another different message", false);

        // Repetition score should have decayed
        let risk = engine.analyze("user1", "Yet another", false);
        // Risk should be lower due to decay
        assert!(
            risk < 0.5,
            "Repetition should decay on different messages, got {}",
            risk
        );
    }
}
