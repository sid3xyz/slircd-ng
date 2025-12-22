//! Operator and WEBIRC block configuration.

use serde::Deserialize;

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
