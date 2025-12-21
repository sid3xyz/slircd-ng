//! Security-related configuration types.
//!
//! Includes cloaking, spam detection, heuristics, and rate limiting.

use serde::Deserialize;

use super::defaults::{
    default_cloak_secret, default_cloak_suffix, default_ctcp_burst, default_ctcp_rate,
    default_connection_burst, default_fanout_window, default_join_burst, default_max_connections,
    default_max_fanout, default_max_velocity, default_message_rate, default_repetition_decay,
    default_spam_detection_enabled, default_true, default_velocity_window,
};

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

/// Configuration for behavioral heuristics.
#[derive(Debug, Clone, Deserialize)]
pub struct HeuristicsConfig {
    /// Enable heuristics engine (default: true).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Window size for velocity tracking (seconds).
    #[serde(default = "default_velocity_window")]
    pub velocity_window: u64,
    /// Max messages allowed in velocity window before penalty.
    #[serde(default = "default_max_velocity")]
    pub max_velocity: usize,
    /// Window size for fan-out tracking (seconds).
    #[serde(default = "default_fanout_window")]
    pub fanout_window: u64,
    /// Max unique recipients allowed in fan-out window before penalty.
    #[serde(default = "default_max_fanout")]
    pub max_fanout: usize,
    /// Decay factor for repetition score (0.0 - 1.0).
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
