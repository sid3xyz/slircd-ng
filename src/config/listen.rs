//! Network listener configuration.

use serde::Deserialize;
use std::net::SocketAddr;

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
