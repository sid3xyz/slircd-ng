//! Configuration loading and management.

use rand::Rng;
use rand::distributions::Alphanumeric;
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::Path;
use thiserror::Error;

/// Configuration errors.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),
}

/// Server configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Server information.
    pub server: ServerConfig,
    /// Network listen configuration.
    pub listen: ListenConfig,
    /// Optional TLS listen configuration.
    pub tls: Option<TlsConfig>,
    /// Optional WebSocket listen configuration.
    pub websocket: Option<WebSocketConfig>,
    /// Operator blocks.
    #[serde(default)]
    pub oper: Vec<OperBlock>,
    /// WEBIRC blocks for trusted gateway clients.
    #[serde(default)]
    pub webirc: Vec<WebircBlock>,
    /// Database configuration.
    pub database: Option<DatabaseConfig>,
    /// Security configuration (cloaking, rate limiting, anti-abuse).
    #[serde(default)]
    pub security: SecurityConfig,
    /// Account registration (draft/account-registration) configuration.
    #[serde(default)]
    pub account_registration: AccountRegistrationConfig,
    /// Message of the Day configuration.
    #[serde(default)]
    pub motd: MotdConfig,
}

/// Account registration configuration (draft/account-registration).
#[derive(Debug, Clone, Deserialize)]
pub struct AccountRegistrationConfig {
    /// Whether account registration is enabled.
    #[serde(default = "default_true")]
    #[allow(dead_code)]
    pub enabled: bool,
    /// Allow registration before connection is complete (before CAP END).
    #[serde(default = "default_true")]
    pub before_connect: bool,
    /// Require email address for registration.
    #[serde(default)]
    pub email_required: bool,
    /// Allow custom account names (different from nick).
    #[serde(default = "default_true")]
    pub custom_account_name: bool,
}

impl Default for AccountRegistrationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            before_connect: true,
            email_required: false,
            custom_account_name: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Message of the Day (MOTD) configuration.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MotdConfig {
    /// Path to MOTD file (one line per MOTD line).
    pub file: Option<String>,
    /// Inline MOTD lines (used when `file` is not set).
    #[serde(default)]
    pub lines: Vec<String>,
}

impl MotdConfig {
    /// Load MOTD lines from file, or return default MOTD.
    pub fn load_lines(&self) -> Vec<String> {
        if let Some(ref path) = self.file {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    return content.lines().map(|s| s.to_string()).collect();
                }
                Err(e) => {
                    tracing::warn!("Failed to read MOTD file {}: {}", path, e);
                }
            }
        }

        if !self.lines.is_empty() {
            return self.lines.clone();
        }

        // Default MOTD
        vec![
            "Welcome to slircd-ng!".to_string(),
            "A high-performance IRC daemon.".to_string(),
        ]
    }
}

/// Database configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Path to SQLite database file.
    pub path: String,
}

/// Operator block configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct OperBlock {
    /// Operator name (used in OPER command).
    pub name: String,
    /// Password (plaintext or bcrypt hash).
    pub password: String,
    /// Optional hostmask restriction (e.g., "*!*@trusted.host").
    pub hostmask: Option<String>,
}

impl OperBlock {
    /// Verify the provided password against the stored password (plaintext or bcrypt).
    pub fn verify_password(&self, password: &str) -> bool {
        if self.password.starts_with("$2") {
            bcrypt::verify(password, &self.password).unwrap_or(false)
        } else {
            // Fallback to plaintext check
            self.password == password
        }
    }
}

/// WEBIRC block configuration for trusted gateway clients.
///
/// WEBIRC allows trusted proxies (web clients, bouncers) to forward
/// the real user's IP/host to the IRC server.
#[derive(Debug, Clone, Deserialize)]
pub struct WebircBlock {
    /// Password for WEBIRC authentication.
    pub password: String,
    /// Allowed host/IP patterns for the gateway (glob patterns supported).
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Description of this WEBIRC gateway (for admin reference).
    #[allow(dead_code)] // Used for admin logging/inspection
    pub description: Option<String>,
}

/// Server identity configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Server name (e.g., "irc.straylight.net").
    pub name: String,
    /// Network name (e.g., "Straylight").
    pub network: String,
    /// Server ID for TS6 (3 characters).
    pub sid: String,
    /// Server description.
    pub description: String,
    /// Global connection password (optional).
    pub password: Option<String>,
    /// Prometheus metrics HTTP port (default: 9090).
    pub metrics_port: Option<u16>,
}

/// Network listener configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ListenConfig {
    /// Address to bind to (e.g., "0.0.0.0:6667").
    pub address: SocketAddr,
}

/// TLS listener configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct TlsConfig {
    /// Address to bind to for TLS (e.g., "0.0.0.0:6697").
    pub address: SocketAddr,
    /// Path to certificate file (PEM format).
    pub cert_path: String,
    /// Path to private key file (PEM format).
    pub key_path: String,
}

/// WebSocket listener configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct WebSocketConfig {
    /// Address to bind to for WebSocket (e.g., "0.0.0.0:8080").
    pub address: SocketAddr,
    /// Allowed origins for CORS (e.g., `["https://example.com"]`).
    /// Empty list allows all origins.
    #[serde(default)]
    pub allow_origins: Vec<String>,
}

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

impl Config {
    /// Load configuration from a TOML file.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
