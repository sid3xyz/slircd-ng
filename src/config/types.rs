//! Core configuration types and loading.

use serde::Deserialize;
use std::path::Path;
use thiserror::Error;

use super::history::HistoryConfig;
use super::limits::LimitsConfig;
use super::links::LinkBlock;
use super::listen::{ListenConfig, TlsConfig, WebSocketConfig};
use super::oper::{OperBlock, WebircBlock};
use super::security::SecurityConfig;

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
    /// History configuration.
    #[serde(default)]
    pub history: HistoryConfig,
    /// Security configuration (cloaking, rate limiting, anti-abuse).
    #[serde(default)]
    pub security: SecurityConfig,
    /// Account registration (draft/account-registration) configuration.
    #[serde(default)]
    pub account_registration: AccountRegistrationConfig,
    /// Message of the Day configuration.
    #[serde(default)]
    pub motd: MotdConfig,
    /// Command output limits (WHO, LIST, NAMES result caps).
    #[serde(default)]
    pub limits: LimitsConfig,
    /// Link blocks for server peering.
    #[serde(default)]
    pub links: Vec<LinkBlock>,
}

impl Config {
    /// Load configuration from a TOML file.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
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
    /// Default user modes applied to new connections (e.g., "+i" for invisible).
    /// Supports: i (invisible), w (wallops), R (registered-only PM), T (no CTCP), B (bot).
    /// Modes o, r, Z, s, S are special and cannot be set via default.
    #[serde(default)]
    pub default_user_modes: Option<String>,
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

fn default_ping_interval() -> u64 {
    90
}

fn default_ping_timeout() -> u64 {
    120
}

fn default_registration_timeout() -> u64 {
    60
}

/// Database configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Path to SQLite database file.
    pub path: String,
}

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

pub(super) fn default_true() -> bool {
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
