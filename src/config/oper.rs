//! Operator and WEBIRC block configuration.

use serde::Deserialize;

/// Operator block configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct OperBlock {
    /// Operator name (used in OPER command).
    pub name: String,
    /// Password (plaintext or Argon2 hash).
    pub password: String,
    /// Optional hostmask restriction (e.g., "*!*@trusted.host").
    pub hostmask: Option<String>,
    /// Require TLS connection to use this oper block.
    #[serde(default)]
    pub require_tls: bool,
}

impl OperBlock {
    /// Verify the provided password against the stored password (plaintext or Argon2).
    /// Verify the provided password against the stored password (plaintext or Argon2).
    pub async fn verify_password(&self, password: &str) -> bool {
        if self.password.starts_with("$argon2") {
            // Verify using Argon2 via the same mechanism as user passwords
            crate::security::password::verify_password(password.to_string(), self.password.clone())
                .await
                .unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_oper(password: &str) -> OperBlock {
        OperBlock {
            name: "testoper".to_string(),
            password: password.to_string(),
            hostmask: None,
            require_tls: false,
        }
    }

    #[tokio::test]
    async fn verify_password_plaintext_match() {
        let oper = make_oper("hunter2");
        assert!(oper.verify_password("hunter2").await);
    }

    #[tokio::test]
    async fn verify_password_plaintext_mismatch() {
        let oper = make_oper("hunter2");
        assert!(!oper.verify_password("wrongpass").await);
    }

    #[tokio::test]
    async fn verify_password_argon2_match() {
        // Generate Argon2 hash at runtime
        let hash = crate::security::password::hash_password("secret123".to_string()).await.unwrap();
        let oper = make_oper(&hash);
        assert!(oper.verify_password("secret123").await);
    }

    #[tokio::test]
    async fn verify_password_argon2_mismatch() {
        let hash = crate::security::password::hash_password("secret123".to_string()).await.unwrap();
        let oper = make_oper(&hash);
        assert!(!oper.verify_password("wrongpassword").await);
    }

    #[tokio::test]
    async fn verify_password_invalid_argon2_hash() {
        // Starts with $argon2 but is not a valid hash
        let oper = make_oper("$argon2_invalid_not_a_real_hash");
        assert!(!oper.verify_password("anything").await);
    }

    #[tokio::test]
    async fn verify_password_empty_password() {
        let oper = make_oper("");
        // Empty password should match empty input
        assert!(oper.verify_password("").await);
        // But not match non-empty input
        assert!(!oper.verify_password("something").await);
    }

    #[tokio::test]
    async fn verify_password_empty_input() {
        let oper = make_oper("secret");
        assert!(!oper.verify_password("").await);
    }
}
