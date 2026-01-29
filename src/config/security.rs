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
    /// Require SASL authentication for all connections.
    /// When true, clients that don't authenticate via SASL will be disconnected
    /// after registration with ERR_SASLFAIL message.
    #[serde(default)]
    pub require_sasl: bool,
    /// Allow SASL PLAIN authentication on non-TLS (plaintext) connections.
    /// This is NOT RECOMMENDED as it exposes passwords to network eavesdroppers.
    /// Only enable this if you have a specific need for legacy clients on a
    /// trusted network.
    #[serde(default)]
    pub allow_plaintext_sasl_plain: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            cloak_secret: default_cloak_secret(),
            cloak_suffix: default_cloak_suffix(),
            spam_detection_enabled: default_spam_detection_enabled(),
            spam: SpamConfig::default(),
            rate_limits: RateLimitConfig::default(),
            require_sasl: false,
            allow_plaintext_sasl_plain: false,
        }
    }
}

impl SecurityConfig {
    /// Emit warnings for deprecated/unused config knobs that are still accepted for compatibility.
    pub fn warn_deprecated_and_unused(&self) {
        if let Some(value) = self.spam.dnsbl_enabled {
            tracing::warn!(
                value,
                "[security.spam].dnsbl_enabled is deprecated and ignored; use [security.spam.rbl].dns_enabled instead"
            );
        }

        if self.spam.rbl.stopforumspam_api_key.is_some() {
            tracing::warn!(
                "[security.spam.rbl].stopforumspam_api_key is configured but not currently used"
            );
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
        "No cloak_secret configured - using ephemeral random secret. Cloaked hostnames will NOT be consistent across server restarts. Set [security].cloak_secret in config.toml for production use."
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
    /// DEPRECATED: Use `rbl.dns_enabled` instead. This field is ignored.
    #[serde(default)]
    pub dnsbl_enabled: Option<bool>,
    /// Enable Reputation system (default: true).
    #[serde(default = "default_true")]
    pub reputation_enabled: bool,
    /// Heuristics configuration.
    #[serde(default)]
    pub heuristics: HeuristicsConfig,
    /// RBL (Realtime Blocklist) configuration.
    #[serde(default)]
    pub rbl: RblConfig,
    /// List of censored words for channel mode +G.
    #[serde(default)]
    pub censored_words: Vec<String>,
    /// List of regex patterns for spam detection.
    /// Uses Rust's regex crate (ReDoS safe).
    #[serde(default)]
    pub regex_patterns: Vec<String>,
}

impl Default for SpamConfig {
    fn default() -> Self {
        Self {
            dnsbl_enabled: None,
            reputation_enabled: true,
            heuristics: HeuristicsConfig::default(),
            rbl: RblConfig::default(),
            censored_words: Vec::new(),
            regex_patterns: Vec::new(),
        }
    }
}

/// RBL (Realtime Blocklist) configuration.
///
/// Supports both traditional DNS-based lookups and privacy-preserving HTTP APIs.
/// HTTP APIs are preferred as they don't leak user IPs to DNS resolvers.
#[derive(Debug, Clone, Deserialize)]
pub struct RblConfig {
    /// Enable HTTP-based RBL providers (default: true).
    /// These are privacy-preserving as queries go directly to the provider.
    #[serde(default = "default_true")]
    pub http_enabled: bool,
    /// Enable DNS-based RBL lookups (default: false for privacy).
    /// DNS lookups leak user IPs to your DNS resolver.
    #[serde(default)]
    pub dns_enabled: bool,
    /// Cache duration for RBL results in seconds (default: 300 = 5 minutes).
    #[serde(default = "default_rbl_cache_ttl")]
    pub cache_ttl_secs: u64,
    /// Maximum cache size in entries (default: 10000).
    #[serde(default = "default_rbl_cache_size")]
    pub cache_max_size: usize,
    /// StopForumSpam API key (optional, enables higher rate limits).
    /// Not currently used, but reserved for future rate limit bypass.
    pub stopforumspam_api_key: Option<String>,
    /// AbuseIPDB API key (optional, required for AbuseIPDB provider).
    pub abuseipdb_api_key: Option<String>,
    /// Minimum confidence score for AbuseIPDB to block (0-100, default: 50).
    #[serde(default = "default_abuseipdb_threshold")]
    pub abuseipdb_threshold: u8,
    /// DNS-based RBL lists to query (if dns_enabled = true).
    #[serde(default = "default_dns_rbl_lists")]
    pub dns_lists: Vec<String>,
}

impl Default for RblConfig {
    fn default() -> Self {
        Self {
            http_enabled: true,
            dns_enabled: false, // Privacy-preserving default
            cache_ttl_secs: default_rbl_cache_ttl(),
            cache_max_size: default_rbl_cache_size(),
            stopforumspam_api_key: None,
            abuseipdb_api_key: None,
            abuseipdb_threshold: default_abuseipdb_threshold(),
            dns_lists: default_dns_rbl_lists(),
        }
    }
}

fn default_rbl_cache_ttl() -> u64 {
    300 // 5 minutes
}

fn default_rbl_cache_size() -> usize {
    10000
}

fn default_abuseipdb_threshold() -> u8 {
    50
}

fn default_dns_rbl_lists() -> Vec<String> {
    vec![
        "dnsbl.dronebl.org".to_string(),
        "rbl.efnetrbl.org".to_string(),
    ]
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
    /// WHOIS queries allowed per client per second (default: 1).
    #[serde(default = "default_whois_rate")]
    pub whois_rate_per_second: u32,
    /// WHOIS burst allowed per client (default: 3).
    #[serde(default = "default_whois_burst")]
    pub whois_burst_per_client: u32,
    /// IP addresses exempt from all rate limiting and connection limits.
    /// These IPs get unlimited connections and no flood protection.
    /// Use sparingly - only for trusted operators/bots.
    #[serde(default)]
    pub exempt_ips: Vec<String>,

    // === Server-to-Server Rate Limiting ===
    /// S2S commands allowed per peer per second (default: 100).
    /// This is much higher than client rate limits since servers relay many users.
    #[serde(default = "default_s2s_command_rate")]
    pub s2s_command_rate_per_second: u32,
    /// S2S burst allowed per peer (default: 500).
    /// Allows for initial burst during netsplit recovery.
    #[serde(default = "default_s2s_burst")]
    pub s2s_burst_per_peer: u32,
    /// Number of rate limit violations before disconnecting peer (default: 10).
    /// Prevents cascading failures from a single flood event.
    #[serde(default = "default_s2s_disconnect_threshold")]
    pub s2s_disconnect_threshold: u32,
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
            whois_rate_per_second: default_whois_rate(),
            whois_burst_per_client: default_whois_burst(),
            exempt_ips: Vec::new(),
            s2s_command_rate_per_second: default_s2s_command_rate(),
            s2s_burst_per_peer: default_s2s_burst(),
            s2s_disconnect_threshold: default_s2s_disconnect_threshold(),
        }
    }
}

fn default_whois_rate() -> u32 {
    1
}

fn default_whois_burst() -> u32 {
    3
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

fn default_s2s_command_rate() -> u32 {
    100 // 100 commands/sec per peer - much higher than clients since they relay many users
}

fn default_s2s_burst() -> u32 {
    500 // Large burst for netsplit recovery when many users rejoin at once
}

fn default_s2s_disconnect_threshold() -> u32 {
    10 // Disconnect after 10 rate limit violations to prevent cascading failures
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
        assert!(config.dnsbl_enabled.is_none());
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

    #[test]
    fn rate_limit_config_default_whois_values() {
        let config = RateLimitConfig::default();
        assert_eq!(config.whois_rate_per_second, 1);
        assert_eq!(config.whois_burst_per_client, 3);
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
        assert!(
            config
                .cloak_secret
                .chars()
                .all(|c| c.is_ascii_alphanumeric())
        );
    }
}
