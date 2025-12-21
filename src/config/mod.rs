//! Configuration loading and management.
//!
//! This module provides structured configuration for slircd-ng, including:
//! - Server identity and network settings
//! - TLS and WebSocket listeners
//! - Operator and WEBIRC blocks
//! - Security settings (cloaking, rate limiting, spam detection)
//! - History and database configuration
//! - Server linking configuration

mod defaults;
mod security;
mod types;

use serde::Deserialize;
use std::path::Path;
use thiserror::Error;

// Re-export all types for public API
pub use security::{HeuristicsConfig, RateLimitConfig, SecurityConfig};
pub use types::{
    AccountRegistrationConfig, ClientAuth, DatabaseConfig, HistoryConfig, IdleTimeoutsConfig,
    LimitsConfig, LinkBlock, ListenConfig, MotdConfig, OperBlock, ServerConfig, TlsConfig,
    WebSocketConfig, WebircBlock,
};

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
