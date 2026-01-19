//! Configuration validation.
//!
//! Validates configuration at startup to catch common errors early.

use super::Config;
use std::path::Path;
use thiserror::Error;

/// Validation errors for configuration.
#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("server.name is required")]
    MissingServerName,
    #[error("server.network is required")]
    MissingNetworkName,
    #[error("server.sid must be exactly 3 characters, got {0}")]
    InvalidSid(usize),
    #[error("server.sid must match pattern [0-9][A-Z0-9][A-Z0-9], got '{0}'")]
    InvalidSidFormat(String),
    #[error("tls.cert_path does not exist: {0}")]
    TlsCertNotFound(String),
    #[error("tls.key_path does not exist: {0}")]
    TlsKeyNotFound(String),
    #[error("database.path parent directory does not exist: {0}")]
    DatabasePathInvalid(String),
}

/// Validate a configuration, returning all errors found.
pub fn validate(config: &Config) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    // Required fields
    if config.server.name.is_empty() {
        errors.push(ValidationError::MissingServerName);
    }
    if config.server.network.is_empty() {
        errors.push(ValidationError::MissingNetworkName);
    }

    // SID validation (TS6 format)
    let sid = &config.server.sid;
    if sid.len() != 3 {
        errors.push(ValidationError::InvalidSid(sid.len()));
    } else {
        let chars: Vec<char> = sid.chars().collect();
        let valid = chars[0].is_ascii_digit()
            && (chars[1].is_ascii_uppercase() || chars[1].is_ascii_digit())
            && (chars[2].is_ascii_uppercase() || chars[2].is_ascii_digit());
        if !valid {
            errors.push(ValidationError::InvalidSidFormat(sid.clone()));
        }
    }

    // TLS validation
    if let Some(ref tls) = config.tls {
        if !Path::new(&tls.cert_path).exists() {
            errors.push(ValidationError::TlsCertNotFound(tls.cert_path.clone()));
        }
        if !Path::new(&tls.key_path).exists() {
            errors.push(ValidationError::TlsKeyNotFound(tls.key_path.clone()));
        }
    }

    // Database path validation
    if let Some(ref db) = config.database {
        let db_path = Path::new(&db.path);
        if let Some(parent) = db_path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            errors.push(ValidationError::DatabasePathInvalid(db.path.clone()));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_valid_config() -> String {
        r#"
[server]
name = "test.server"
network = "TestNet"
sid = "00T"
description = "Test"

[listen]
address = "127.0.0.1:6667"
"#
        .to_string()
    }

    #[test]
    fn test_valid_config_passes() {
        let config: Config = toml::from_str(&minimal_valid_config()).unwrap();
        assert!(validate(&config).is_ok());
    }

    #[test]
    fn test_empty_server_name_fails() {
        let toml = r#"
[server]
name = ""
network = "TestNet"
sid = "00T"
description = "Test"

[listen]
address = "127.0.0.1:6667"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        let errors = validate(&config).unwrap_err();
        assert!(errors.iter().any(|e| matches!(e, ValidationError::MissingServerName)));
    }

    #[test]
    fn test_invalid_sid_length_fails() {
        let toml = r#"
[server]
name = "test"
network = "TestNet"
sid = "0T"
description = "Test"

[listen]
address = "127.0.0.1:6667"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        let errors = validate(&config).unwrap_err();
        assert!(errors.iter().any(|e| matches!(e, ValidationError::InvalidSid(_))));
    }

    #[test]
    fn test_missing_tls_cert_fails() {
        let toml = r#"
[server]
name = "test"
network = "TestNet"
sid = "00T"
description = "Test"

[listen]
address = "127.0.0.1:6667"

[tls]
address = "127.0.0.1:6697"
cert_path = "/nonexistent/cert.pem"
key_path = "/nonexistent/key.pem"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        let errors = validate(&config).unwrap_err();
        assert!(errors.iter().any(|e| matches!(e, ValidationError::TlsCertNotFound(_))));
    }
}
