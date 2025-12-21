//! Configuration type definitions.
//!
//! All the sub-config structs used by the main Config.

use serde::Deserialize;
use std::net::SocketAddr;

use super::defaults::{
    default_channel_mailbox_capacity, default_history_backend, default_history_path,
    default_max_list_channels, default_max_names_channels, default_max_who_results,
    default_ping_interval, default_ping_timeout, default_registration_timeout, default_true,
};

// =============================================================================
// Link Configuration (S2S)
// =============================================================================

/// Link block configuration for server-to-server connections.
#[derive(Debug, Clone, Deserialize)]
pub struct LinkBlock {
    /// Remote server name (e.g., "hub.straylight.net").
    pub name: String,
    /// Remote server IP/hostname to connect to.
    pub hostname: String,
    /// Remote server port.
    #[allow(dead_code)]
    pub port: u16,
    /// Password for authentication (must match remote's password).
    pub password: String,
    /// Whether to use TLS for this link.
    #[serde(default)]
    #[allow(dead_code)]
    pub tls: bool,
    /// Whether to initiate connection to this server automatically.
    #[serde(default)]
    pub autoconnect: bool,
    /// Expected remote SID (optional, for validation).
    #[allow(dead_code)]
    pub sid: Option<String>,
}

// =============================================================================
// Account Registration
// =============================================================================

/// Account registration configuration (draft/account-registration).
#[derive(Debug, Clone, Deserialize)]
pub struct AccountRegistrationConfig {
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
            before_connect: true,
            email_required: false,
            custom_account_name: true,
        }
    }
}

// =============================================================================
// MOTD Configuration
// =============================================================================

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

// =============================================================================
// Database Configuration
// =============================================================================

/// Database configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Path to SQLite database file.
    pub path: String,
}

// =============================================================================
// History Configuration
// =============================================================================

/// History configuration (Innovation 5: Event-Sourced History).
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryConfig {
    /// Whether history is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Backend type: "redb", "sqlite", "none".
    #[serde(default = "default_history_backend")]
    pub backend: String,
    /// Path to history database file.
    #[serde(default = "default_history_path")]
    pub path: String,
    /// Event type configuration.
    #[serde(default)]
    pub events: HistoryEventsConfig,
}

/// Configuration for which event types to store in history.
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryEventsConfig {
    /// Store PRIVMSG messages.
    #[serde(default = "default_true")]
    pub privmsg: bool,
    /// Store NOTICE messages.
    #[serde(default = "default_true")]
    pub notice: bool,
    /// Store TOPIC changes (requires event-playback to replay).
    #[serde(default = "default_true")]
    pub topic: bool,
    /// Store TAGMSG (only with +draft/persist tag, requires event-playback).
    #[serde(default = "default_true")]
    pub tagmsg: bool,
    /// Store JOIN/PART/QUIT events (future, requires event-playback).
    #[serde(default)]
    pub membership: bool,
    /// Store MODE changes (future, requires event-playback).
    #[serde(default)]
    pub mode: bool,
}

impl Default for HistoryEventsConfig {
    fn default() -> Self {
        Self {
            privmsg: true,
            notice: true,
            topic: true,
            tagmsg: true,
            membership: false,
            mode: false,
        }
    }
}

impl HistoryConfig {
    /// Check if a specific event type should be stored.
    pub fn should_store_event(&self, event_type: &str) -> bool {
        if !self.enabled {
            return false;
        }
        match event_type {
            "PRIVMSG" => self.events.privmsg,
            "NOTICE" => self.events.notice,
            "TOPIC" => self.events.topic,
            "TAGMSG" => self.events.tagmsg,
            "JOIN" | "PART" | "QUIT" | "KICK" => self.events.membership,
            "MODE" => self.events.mode,
            _ => false,
        }
    }
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: "none".to_string(),
            path: "history.db".to_string(),
            events: HistoryEventsConfig::default(),
        }
    }
}

// =============================================================================
// Operator Configuration
// =============================================================================

/// Operator block configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct OperBlock {
    /// Operator name (used in OPER command).
    pub name: String,
    /// Password (plaintext or bcrypt hash).
    pub password: String,
    /// Optional hostmask restriction (e.g., "*!*@trusted.host").
    pub hostmask: Option<String>,
    /// Require TLS connection to use this oper block.
    #[serde(default)]
    pub require_tls: bool,
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

// =============================================================================
// WEBIRC Configuration
// =============================================================================

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
}

// =============================================================================
// Server Configuration
// =============================================================================

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
    /// Admin info line 1 (RPL_ADMINLOC1) - typically organization name.
    #[serde(default)]
    pub admin_info1: Option<String>,
    /// Admin info line 2 (RPL_ADMINLOC2) - typically location.
    #[serde(default)]
    pub admin_info2: Option<String>,
    /// Admin email address (RPL_ADMINEMAIL).
    #[serde(default)]
    pub admin_email: Option<String>,
    /// Idle timeout configuration for ping/pong keepalive.
    #[serde(default)]
    pub idle_timeouts: IdleTimeoutsConfig,
}

/// Idle timeout configuration for client connection keepalive.
///
/// IRC servers send periodic PING messages to detect dead connections.
/// If the client doesn't respond with PONG within the timeout, they are
/// disconnected with "Ping timeout".
///
/// Based on Ergo's three-phase model:
/// - `ping`: Seconds of idle before sending PING (default: 90)
/// - `timeout`: Seconds to wait for PONG after PING (default: 120)
/// - `registration`: Seconds allowed for initial registration (default: 60)
#[derive(Debug, Clone, Deserialize)]
pub struct IdleTimeoutsConfig {
    /// Seconds of idle before sending PING to client (default: 90).
    #[serde(default = "default_ping_interval")]
    pub ping: u64,

    /// Seconds to wait for PONG after sending PING before disconnect (default: 120).
    /// Total idle time before disconnect = ping + timeout.
    #[serde(default = "default_ping_timeout")]
    pub timeout: u64,

    /// Seconds allowed for registration handshake (NICK/USER/CAP) before disconnect (default: 60).
    #[serde(default = "default_registration_timeout")]
    pub registration: u64,
}

impl Default for IdleTimeoutsConfig {
    fn default() -> Self {
        Self {
            ping: default_ping_interval(),
            timeout: default_ping_timeout(),
            registration: default_registration_timeout(),
        }
    }
}

// =============================================================================
// Network Listener Configuration
// =============================================================================

/// Network listener configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ListenConfig {
    /// Address to bind to (e.g., "0.0.0.0:6667").
    pub address: SocketAddr,
}

/// Client certificate authentication mode.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClientAuth {
    /// No client certificate requested.
    #[default]
    None,
    /// Client certificate optional (SASL EXTERNAL available if provided).
    Optional,
    /// Client certificate required (connection rejected without valid cert).
    Required,
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
    /// Whether to require TLS 1.3 only (disables TLS 1.2).
    #[serde(default)]
    pub tls13_only: bool,
    /// Client certificate verification mode.
    #[serde(default)]
    pub client_auth: ClientAuth,
    /// Path to CA certificate file for client verification (PEM format).
    /// Required if client_auth is "optional" or "required".
    pub ca_path: Option<String>,
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

// =============================================================================
// Limits Configuration
// =============================================================================

/// Command output limits configuration.
///
/// These limits prevent pathologically large result sets from exhausting
/// server resources or causing slow clients to back up.
#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    /// Maximum results returned by WHO command (default: 500).
    /// Applies to both channel WHO and mask-based WHO queries.
    #[serde(default = "default_max_who_results")]
    pub max_who_results: usize,
    /// Maximum channels returned by LIST command (default: 1000).
    #[serde(default = "default_max_list_channels")]
    pub max_list_channels: usize,
    /// Maximum channels listed by NAMES without argument (default: 50).
    /// NAMES #channel is unlimited since it's a single channel.
    #[serde(default = "default_max_names_channels")]
    pub max_names_channels: usize,
    /// Channel actor mailbox capacity (default: 500).
    /// Higher values provide burst tolerance during floods.
    #[serde(default = "default_channel_mailbox_capacity")]
    pub channel_mailbox_capacity: usize,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_who_results: default_max_who_results(),
            max_list_channels: default_max_list_channels(),
            max_names_channels: default_max_names_channels(),
            channel_mailbox_capacity: default_channel_mailbox_capacity(),
        }
    }
}
