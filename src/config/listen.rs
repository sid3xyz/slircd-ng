//! Network listener configuration.

use serde::Deserialize;
use std::net::SocketAddr;

/// Network listener configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ListenConfig {
    /// Address to bind to (e.g., "0.0.0.0:6667").
    pub address: SocketAddr,
    /// Enable PROXY protocol (v1/v2) support.
    #[serde(default)]
    pub proxy_protocol: bool,
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
    /// Enable PROXY protocol (v1/v2) support.
    #[serde(default)]
    pub proxy_protocol: bool,
    /// Strict Transport Security (STS) configuration.
    /// When enabled, advertises the STS capability to enforce TLS-only connections.
    #[serde(default)]
    pub sts: Option<StsConfig>,
}

/// Strict Transport Security (STS) configuration for IRCv3 sts capability.
///
/// STS allows servers to advertise that clients should only connect via TLS.
/// Reference: <https://ircv3.net/specs/extensions/sts>
#[derive(Debug, Clone, Deserialize)]
pub struct StsConfig {
    /// Port number for secure connections (required for insecure->secure upgrade).
    /// This should be the TLS port (typically 6697).
    pub port: u16,
    /// Duration in seconds for which clients must use secure connections.
    /// Recommended: 2592000 (30 days) to 31536000 (1 year).
    /// Set to 0 to disable STS persistence policy.
    #[serde(default = "default_sts_duration")]
    pub duration: u64,
    /// Whether to opt-in to STS preload lists.
    /// Only enable this if you're committed to offering TLS long-term.
    #[serde(default)]
    pub preload: bool,
}

fn default_sts_duration() -> u64 {
    2592000 // 30 days
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
    /// Enable PROXY protocol (v1/v2) support.
    #[serde(default)]
    pub proxy_protocol: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_auth_default_is_none() {
        assert_eq!(ClientAuth::default(), ClientAuth::None);
    }

    #[test]
    fn client_auth_equality() {
        assert_eq!(ClientAuth::None, ClientAuth::None);
        assert_eq!(ClientAuth::Optional, ClientAuth::Optional);
        assert_eq!(ClientAuth::Required, ClientAuth::Required);
        assert_ne!(ClientAuth::None, ClientAuth::Optional);
        assert_ne!(ClientAuth::None, ClientAuth::Required);
        assert_ne!(ClientAuth::Optional, ClientAuth::Required);
    }

    #[test]
    fn client_auth_clone_and_copy() {
        let auth = ClientAuth::Optional;
        let cloned = auth.clone();
        let copied = auth; // Copy
        assert_eq!(auth, cloned);
        assert_eq!(auth, copied);
    }

    #[test]
    fn client_auth_debug_format() {
        assert_eq!(format!("{:?}", ClientAuth::None), "None");
        assert_eq!(format!("{:?}", ClientAuth::Optional), "Optional");
        assert_eq!(format!("{:?}", ClientAuth::Required), "Required");
    }

    #[test]
    fn client_auth_deserialize_from_toml() {
        #[derive(Deserialize)]
        struct Wrapper {
            auth: ClientAuth,
        }

        let toml_str = r#"auth = "none""#;
        let w: Wrapper = toml::from_str(toml_str).unwrap();
        assert_eq!(w.auth, ClientAuth::None);

        let toml_str = r#"auth = "optional""#;
        let w: Wrapper = toml::from_str(toml_str).unwrap();
        assert_eq!(w.auth, ClientAuth::Optional);

        let toml_str = r#"auth = "required""#;
        let w: Wrapper = toml::from_str(toml_str).unwrap();
        assert_eq!(w.auth, ClientAuth::Required);
    }

    #[test]
    fn listen_config_deserialize() {
        let toml_str = r#"
            address = "0.0.0.0:6667"
        "#;
        let cfg: ListenConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.address.port(), 6667);
        assert!(!cfg.proxy_protocol); // default is false
    }

    #[test]
    fn listen_config_with_proxy_protocol() {
        let toml_str = r#"
            address = "0.0.0.0:6667"
            proxy_protocol = true
        "#;
        let cfg: ListenConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.proxy_protocol);
    }

    #[test]
    fn tls_config_deserialize_defaults() {
        let toml_str = r#"
            address = "0.0.0.0:6697"
            cert_path = "/path/to/cert.pem"
            key_path = "/path/to/key.pem"
        "#;
        let cfg: TlsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.address.port(), 6697);
        assert!(!cfg.tls13_only); // default
        assert_eq!(cfg.client_auth, ClientAuth::None); // default
        assert!(cfg.ca_path.is_none());
        assert!(!cfg.proxy_protocol); // default
    }

    #[test]
    fn tls_config_with_client_auth() {
        let toml_str = r#"
            address = "0.0.0.0:6697"
            cert_path = "/path/to/cert.pem"
            key_path = "/path/to/key.pem"
            client_auth = "required"
            ca_path = "/path/to/ca.pem"
        "#;
        let cfg: TlsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.client_auth, ClientAuth::Required);
        assert_eq!(cfg.ca_path.as_deref(), Some("/path/to/ca.pem"));
    }

    #[test]
    fn websocket_config_deserialize_defaults() {
        let toml_str = r#"
            address = "0.0.0.0:8080"
        "#;
        let cfg: WebSocketConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.address.port(), 8080);
        assert!(cfg.allow_origins.is_empty()); // default
        assert!(!cfg.proxy_protocol); // default
    }

    #[test]
    fn websocket_config_with_origins() {
        let toml_str = r#"
            address = "0.0.0.0:8080"
            allow_origins = ["https://example.com", "https://another.com"]
        "#;
        let cfg: WebSocketConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.allow_origins.len(), 2);
        assert_eq!(cfg.allow_origins[0], "https://example.com");
    }
}
