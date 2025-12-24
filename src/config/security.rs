//! Security configuration for cloaking, rate limiting, and anti-abuse.

use rand::Rng;
use rand::distributions::Alphanumeric;
use serde::Deserialize;

use super::types::default_true;

/// Security configuration for cloaking, rate limiting, and anti-abuse.
#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    /// Secret key for HMAC-based host cloaking.
    /// MUST be kept private and should be at least 32 characters.
    #[serde(default = "default_cloak_secret")]
    pub cloak_secret: String,
    /// Suffix for cloaked IP addresses (default: "ip").
    #[serde(default = "default_cloak_suffix")]
    pub cloak_suffix: String,
    /// Enable spam detection for message content (default: true).
    #[serde(default = "default_spam_detection_enabled")]
    pub spam_detection_enabled: bool,
    /// Spam detection configuration.
    #[serde(default)]
    pub spam: SpamConfig,
    /// Rate limiting configuration.
    #[serde(default)]
    pub rate_limits: RateLimitConfig,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            cloak_secret: default_cloak_secret(),
            cloak_suffix: default_cloak_suffix(),
            spam_detection_enabled: default_spam_detection_enabled(),
            spam: SpamConfig::default(),
            rate_limits: RateLimitConfig::default(),
        }
    }
}

fn default_cloak_secret() -> String {
    let secret: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    tracing::warn!(
        "No cloak_secret configured - using ephemeral random secret. \
         Cloaked hostnames will NOT be consistent across server restarts. \
         Set [security].cloak_secret in config.toml for production use."
    );
    secret
}

fn default_cloak_suffix() -> String {
    "ip".to_string()
}

fn default_spam_detection_enabled() -> bool {
    true
}

/// Spam detection configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct SpamConfig {
    /// Enable DNS Blocklist checks (default: true).
    #[serde(default = "default_true")]
    pub dnsbl_enabled: bool,
    /// Enable Reputation system (default: true).
    #[serde(default = "default_true")]
    pub reputation_enabled: bool,
    /// Heuristics configuration.
    #[serde(default)]
    pub heuristics: HeuristicsConfig,
}

impl Default for SpamConfig {
    fn default() -> Self {
        Self {
            dnsbl_enabled: true,
            reputation_enabled: true,
            heuristics: HeuristicsConfig::default(),
        }
    }
}

/// Configuration for behavioral heuristics
#[derive(Debug, Clone, Deserialize)]
pub struct HeuristicsConfig {
    /// Enable heuristics engine (default: true).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Window size for velocity tracking (seconds)
    #[serde(default = "default_velocity_window")]
    pub velocity_window: u64,
    /// Max messages allowed in velocity window before penalty
    #[serde(default = "default_max_velocity")]
    pub max_velocity: usize,
    /// Window size for fan-out tracking (seconds)
    #[serde(default = "default_fanout_window")]
    pub fanout_window: u64,
    /// Max unique recipients allowed in fan-out window before penalty
    #[serde(default = "default_max_fanout")]
    pub max_fanout: usize,
    /// Decay factor for repetition score (0.0 - 1.0)
    #[serde(default = "default_repetition_decay")]
    pub repetition_decay: f32,
}

impl Default for HeuristicsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            velocity_window: default_velocity_window(),
            max_velocity: default_max_velocity(),
            fanout_window: default_fanout_window(),
            max_fanout: default_max_fanout(),
            repetition_decay: default_repetition_decay(),
        }
    }
}

fn default_velocity_window() -> u64 {
    10
}

fn default_max_velocity() -> usize {
    5
}

fn default_fanout_window() -> u64 {
    60
}

fn default_max_fanout() -> usize {
    10
}

fn default_repetition_decay() -> f32 {
    0.8
}

/// Rate limiting configuration for anti-flood protection.
#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    /// Messages allowed per client per second (default: 2).
    #[serde(default = "default_message_rate")]
    pub message_rate_per_second: u32,
    /// Connection burst allowed per IP in 10 seconds (default: 3).
    #[serde(default = "default_connection_burst")]
    pub connection_burst_per_ip: u32,
    /// Channel join burst allowed per client in 10 seconds (default: 5).
    #[serde(default = "default_join_burst")]
    pub join_burst_per_client: u32,
    /// CTCP messages allowed per client per second (default: 1).
    #[serde(default = "default_ctcp_rate")]
    pub ctcp_rate_per_second: u32,
    /// CTCP burst allowed per client (default: 2).
    #[serde(default = "default_ctcp_burst")]
    pub ctcp_burst_per_client: u32,
    /// Maximum concurrent connections allowed per IP (default: 10).
    #[serde(default = "default_max_connections")]
    pub max_connections_per_ip: u32,
    /// IP addresses exempt from all rate limiting and connection limits.
    /// These IPs get unlimited connections and no flood protection.
    /// Use sparingly - only for trusted operators/bots.
    #[serde(default)]
    pub exempt_ips: Vec<String>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            message_rate_per_second: default_message_rate(),
            connection_burst_per_ip: default_connection_burst(),
            join_burst_per_client: default_join_burst(),
            ctcp_rate_per_second: default_ctcp_rate(),
            ctcp_burst_per_client: default_ctcp_burst(),
            max_connections_per_ip: default_max_connections(),
            exempt_ips: Vec::new(),
        }
    }
}

fn default_message_rate() -> u32 {
    2
}

fn default_connection_burst() -> u32 {
    3
}

fn default_join_burst() -> u32 {
    5
}

fn default_ctcp_rate() -> u32 {
    1
}

fn default_ctcp_burst() -> u32 {
    2
}

fn default_max_connections() -> u32 {
    10
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Default Function Tests ===

    #[test]
    fn default_cloak_suffix_is_ip() {
        assert_eq!(default_cloak_suffix(), "ip");
    }

    #[test]
    fn default_spam_detection_enabled_is_true() {
        assert!(default_spam_detection_enabled());
    }

    #[test]
    fn default_velocity_window_value() {
        assert_eq!(default_velocity_window(), 10);
    }

    #[test]
    fn default_max_velocity_value() {
        assert_eq!(default_max_velocity(), 5);
    }

    #[test]
    fn default_fanout_window_value() {
        assert_eq!(default_fanout_window(), 60);
    }

    #[test]
    fn default_max_fanout_value() {
        assert_eq!(default_max_fanout(), 10);
    }

    #[test]
    fn default_repetition_decay_value() {
        assert!((default_repetition_decay() - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn default_message_rate_value() {
        assert_eq!(default_message_rate(), 2);
    }

    #[test]
    fn default_connection_burst_value() {
        assert_eq!(default_connection_burst(), 3);
    }

    #[test]
    fn default_join_burst_value() {
        assert_eq!(default_join_burst(), 5);
    }

    #[test]
    fn default_ctcp_rate_value() {
        assert_eq!(default_ctcp_rate(), 1);
    }

    #[test]
    fn default_ctcp_burst_value() {
        assert_eq!(default_ctcp_burst(), 2);
    }

    #[test]
    fn default_max_connections_value() {
        assert_eq!(default_max_connections(), 10);
    }

    // === SpamConfig Default Tests ===

    #[test]
    fn spam_config_default_dnsbl_enabled() {
        let config = SpamConfig::default();
        assert!(config.dnsbl_enabled);
    }

    #[test]
    fn spam_config_default_reputation_enabled() {
        let config = SpamConfig::default();
        assert!(config.reputation_enabled);
    }

    #[test]
    fn spam_config_default_heuristics_enabled() {
        let config = SpamConfig::default();
        assert!(config.heuristics.enabled);
    }

    // === HeuristicsConfig Default Tests ===

    #[test]
    fn heuristics_config_default_values() {
        let config = HeuristicsConfig::default();
        assert!(config.enabled);
        assert_eq!(config.velocity_window, 10);
        assert_eq!(config.max_velocity, 5);
        assert_eq!(config.fanout_window, 60);
        assert_eq!(config.max_fanout, 10);
        assert!((config.repetition_decay - 0.8).abs() < f32::EPSILON);
    }

    // === RateLimitConfig Default Tests ===

    #[test]
    fn rate_limit_config_default_message_rate() {
        let config = RateLimitConfig::default();
        assert_eq!(config.message_rate_per_second, 2);
    }

    #[test]
    fn rate_limit_config_default_connection_burst() {
        let config = RateLimitConfig::default();
        assert_eq!(config.connection_burst_per_ip, 3);
    }

    #[test]
    fn rate_limit_config_default_join_burst() {
        let config = RateLimitConfig::default();
        assert_eq!(config.join_burst_per_client, 5);
    }

    #[test]
    fn rate_limit_config_default_ctcp_rate() {
        let config = RateLimitConfig::default();
        assert_eq!(config.ctcp_rate_per_second, 1);
    }

    #[test]
    fn rate_limit_config_default_ctcp_burst() {
        let config = RateLimitConfig::default();
        assert_eq!(config.ctcp_burst_per_client, 2);
    }

    #[test]
    fn rate_limit_config_default_max_connections() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_connections_per_ip, 10);
    }

    #[test]
    fn rate_limit_config_default_exempt_ips_empty() {
        let config = RateLimitConfig::default();
        assert!(config.exempt_ips.is_empty());
    }

    // === SecurityConfig Default Tests ===

    #[test]
    fn security_config_default_cloak_suffix() {
        let config = SecurityConfig::default();
        assert_eq!(config.cloak_suffix, "ip");
    }

    #[test]
    fn security_config_default_spam_detection_enabled() {
        let config = SecurityConfig::default();
        assert!(config.spam_detection_enabled);
    }

    #[test]
    fn security_config_default_cloak_secret_is_32_chars() {
        // Note: This generates an ephemeral secret - we just verify its length
        let config = SecurityConfig::default();
        assert_eq!(config.cloak_secret.len(), 32);
    }

    #[test]
    fn security_config_default_cloak_secret_is_alphanumeric() {
        let config = SecurityConfig::default();
        assert!(config.cloak_secret.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
