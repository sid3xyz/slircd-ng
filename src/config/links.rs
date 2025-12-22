//! Server-to-server link configuration.

use serde::Deserialize;

use super::types::default_true;

/// Link block configuration for server-to-server connections.
#[derive(Debug, Clone, Deserialize)]
pub struct LinkBlock {
    /// Remote server name (e.g., "hub.straylight.net").
    pub name: String,
    /// Remote server IP/hostname to connect to.
    pub hostname: String,
    /// Remote server port.
    pub port: u16,
    /// Password for authentication (must match remote's password).
    pub password: String,
    /// Whether to use TLS for this link.
    #[serde(default)]
    pub tls: bool,
    /// Whether to verify the remote certificate (only applies when tls = true).
    /// Defaults to true for security. Set to false only for testing or self-signed certs.
    #[serde(default = "default_true")]
    pub verify_cert: bool,
    /// Whether to initiate connection to this server automatically.
    #[serde(default)]
    pub autoconnect: bool,
    /// Expected remote SID (optional, for validation).
    #[allow(dead_code)] // Will be used for SID validation in handshake
    pub sid: Option<String>,
}
