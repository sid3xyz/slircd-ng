//! SASL authentication helpers for IRC.
//!
//! This module provides utilities for encoding SASL authentication
//! credentials using common mechanisms.
//!
//! # Supported Mechanisms
//!
//! - **PLAIN**: Simple username/password authentication (RFC 4616)
//! - **EXTERNAL**: Certificate-based authentication (client cert)
//! - **SCRAM-SHA-256**: Challenge-response authentication (RFC 7677) - *partial support*
//!
//! # SCRAM-SHA-256 Support
//!
//! SCRAM-SHA-256 is recognized and preferred by [`choose_mechanism`], but full
//! client-side implementation requires cryptographic dependencies (sha2, hmac, pbkdf2).
//! The [`ScramClient`] struct provides the state machine; actual payload generation
//! will be added in a future release with an optional `scram` feature flag.
//!
//! # Reference
//! - IRCv3 SASL: <https://ircv3.net/specs/extensions/sasl-3.2>
//! - RFC 4616 (PLAIN): <https://tools.ietf.org/html/rfc4616>
//! - RFC 7677 (SCRAM-SHA-256): <https://tools.ietf.org/html/rfc7677>
//!
//! # Example
//!
//! ```
//! use slirc_proto::sasl::{SaslMechanism, encode_plain};
//!
//! // Encode PLAIN credentials
//! let encoded = encode_plain("myuser", "mypassword");
//! assert!(!encoded.is_empty());
//!
//! // Check mechanism support
//! let mech = SaslMechanism::parse("PLAIN");
//! assert_eq!(mech, SaslMechanism::Plain);
//! ```

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

mod external;
mod plain;
mod scram;

// Re-export mechanism implementations
pub use external::encode_external;
pub use plain::{encode_plain, encode_plain_with_authzid};
pub use scram::{ScramClient, ScramError, ScramState};

/// Maximum length of a single SASL message chunk (400 bytes).
///
/// SASL responses that exceed this length must be split into multiple
/// AUTHENTICATE commands.
pub const SASL_CHUNK_SIZE: usize = 400;

/// Supported SASL authentication mechanisms.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SaslMechanism {
    /// PLAIN mechanism (RFC 4616) - simple username/password.
    Plain,
    /// EXTERNAL mechanism - uses TLS client certificate.
    External,
    /// SCRAM-SHA-256 mechanism (RFC 7677).
    ScramSha256,
    /// Unknown or unsupported mechanism.
    Unknown(String),
}

impl SaslMechanism {
    /// Parse a mechanism name string.
    pub fn parse(name: &str) -> Self {
        match name.to_ascii_uppercase().as_str() {
            "PLAIN" => Self::Plain,
            "EXTERNAL" => Self::External,
            "SCRAM-SHA-256" => Self::ScramSha256,
            _ => Self::Unknown(name.to_owned()),
        }
    }

    /// Returns the canonical name of this mechanism.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Plain => "PLAIN",
            Self::External => "EXTERNAL",
            Self::ScramSha256 => "SCRAM-SHA-256",
            Self::Unknown(s) => s,
        }
    }

    /// Check if this mechanism is supported for encoding.
    #[cfg(feature = "scram")]
    pub fn is_supported(&self) -> bool {
        matches!(self, Self::Plain | Self::External | Self::ScramSha256)
    }

    /// Check if this mechanism is supported for encoding.
    #[cfg(not(feature = "scram"))]
    pub fn is_supported(&self) -> bool {
        matches!(self, Self::Plain | Self::External)
    }
}

impl std::fmt::Display for SaslMechanism {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parse a list of mechanisms from a server's `RPL_SASLMECHS` (908) response.
///
/// The mechanisms are typically comma-separated.
pub fn parse_mechanisms(list: &str) -> Vec<SaslMechanism> {
    list.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(SaslMechanism::parse)
        .collect()
}

/// Choose the best supported mechanism from a list.
///
/// Preference order: EXTERNAL > SCRAM-SHA-256 > PLAIN
pub fn choose_mechanism(available: &[SaslMechanism]) -> Option<SaslMechanism> {
    // Prefer EXTERNAL (certificate-based) over password-based
    if available.contains(&SaslMechanism::External) {
        return Some(SaslMechanism::External);
    }
    // SCRAM-SHA-256 is more secure than PLAIN
    if available.contains(&SaslMechanism::ScramSha256) {
        return Some(SaslMechanism::ScramSha256);
    }
    // Fall back to PLAIN
    if available.contains(&SaslMechanism::Plain) {
        return Some(SaslMechanism::Plain);
    }
    None
}

/// Split an encoded SASL response into chunks for transmission.
///
/// IRC SASL requires responses longer than 400 bytes to be split
/// across multiple AUTHENTICATE commands.
pub fn chunk_response(encoded: &str) -> impl Iterator<Item = &str> {
    encoded.as_bytes().chunks(SASL_CHUNK_SIZE).map(|chunk| {
        // Safe because base64 is always ASCII
        std::str::from_utf8(chunk).unwrap()
    })
}

/// Check if a SASL response needs chunking.
#[inline]
pub fn needs_chunking(encoded: &str) -> bool {
    encoded.len() > SASL_CHUNK_SIZE
}

/// Decode a base64-encoded SASL challenge or response.
///
/// # Returns
///
/// The decoded bytes, or an error if decoding fails.
pub fn decode_base64(encoded: &str) -> Result<Vec<u8>, base64::DecodeError> {
    if encoded == "+" {
        return Ok(Vec::new());
    }
    BASE64.decode(encoded)
}

/// SASL authentication state machine.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SaslState {
    /// Initial state, not yet started.
    Initial,
    /// Sent AUTHENTICATE with mechanism, waiting for challenge.
    MechanismSent(SaslMechanism),
    /// Received challenge, need to send credentials.
    ChallengeReceived,
    /// Sent credentials, waiting for result.
    CredentialsSent,
    /// Authentication succeeded.
    Success,
    /// Authentication failed.
    Failed(String),
    /// Authentication aborted.
    Aborted,
}

impl SaslState {
    /// Check if authentication is complete (success or failure).
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Success | Self::Failed(_) | Self::Aborted)
    }

    /// Check if authentication succeeded.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mechanisms() {
        let mechs = parse_mechanisms("PLAIN,EXTERNAL,SCRAM-SHA-256");
        assert_eq!(mechs.len(), 3);
        assert!(mechs.contains(&SaslMechanism::Plain));
        assert!(mechs.contains(&SaslMechanism::External));
        assert!(mechs.contains(&SaslMechanism::ScramSha256));
    }

    #[test]
    fn test_choose_mechanism_prefers_external() {
        let available = vec![SaslMechanism::Plain, SaslMechanism::External];
        assert_eq!(choose_mechanism(&available), Some(SaslMechanism::External));
    }

    #[test]
    fn test_choose_mechanism_prefers_scram_over_plain() {
        let available = vec![SaslMechanism::Plain, SaslMechanism::ScramSha256];
        assert_eq!(
            choose_mechanism(&available),
            Some(SaslMechanism::ScramSha256)
        );
    }

    #[test]
    fn test_choose_mechanism_plain_fallback() {
        let available = vec![SaslMechanism::Plain];
        assert_eq!(choose_mechanism(&available), Some(SaslMechanism::Plain));
    }

    #[test]
    fn test_choose_mechanism_none() {
        let available = vec![SaslMechanism::Unknown("FOO".to_owned())];
        assert_eq!(choose_mechanism(&available), None);
    }

    #[test]
    fn test_chunk_response_short() {
        let short = "abc123";
        let chunks: Vec<_> = chunk_response(short).collect();
        assert_eq!(chunks, vec!["abc123"]);
    }

    #[test]
    fn test_chunk_response_long() {
        let long = "a".repeat(500);
        let chunks: Vec<_> = chunk_response(&long).collect();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 400);
        assert_eq!(chunks[1].len(), 100);
    }

    #[test]
    fn test_needs_chunking() {
        assert!(!needs_chunking("short"));
        assert!(needs_chunking(&"a".repeat(500)));
    }

    #[test]
    fn test_decode_base64_empty() {
        let decoded = decode_base64("+").unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_decode_base64_valid() {
        let encoded = BASE64.encode(b"hello");
        let decoded = decode_base64(&encoded).unwrap();
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn test_mechanism_parse() {
        assert_eq!(SaslMechanism::parse("PLAIN"), SaslMechanism::Plain);
        assert_eq!(SaslMechanism::parse("plain"), SaslMechanism::Plain);
        assert_eq!(SaslMechanism::parse("EXTERNAL"), SaslMechanism::External);
        assert_eq!(
            SaslMechanism::parse("SCRAM-SHA-256"),
            SaslMechanism::ScramSha256
        );
        assert_eq!(
            SaslMechanism::parse("UNKNOWN"),
            SaslMechanism::Unknown("UNKNOWN".to_owned())
        );
    }

    #[test]
    fn test_mechanism_as_str() {
        assert_eq!(SaslMechanism::Plain.as_str(), "PLAIN");
        assert_eq!(SaslMechanism::External.as_str(), "EXTERNAL");
        assert_eq!(SaslMechanism::ScramSha256.as_str(), "SCRAM-SHA-256");
    }

    #[test]
    fn test_mechanism_is_supported() {
        assert!(SaslMechanism::Plain.is_supported());
        assert!(SaslMechanism::External.is_supported());
        #[cfg(feature = "scram")]
        assert!(SaslMechanism::ScramSha256.is_supported());
        #[cfg(not(feature = "scram"))]
        assert!(!SaslMechanism::ScramSha256.is_supported());
        assert!(!SaslMechanism::Unknown("FOO".to_owned()).is_supported());
    }

    #[test]
    fn test_sasl_state() {
        assert!(!SaslState::Initial.is_complete());
        assert!(!SaslState::MechanismSent(SaslMechanism::Plain).is_complete());
        assert!(SaslState::Success.is_complete());
        assert!(SaslState::Success.is_success());
        assert!(SaslState::Failed("error".to_owned()).is_complete());
        assert!(!SaslState::Failed("error".to_owned()).is_success());
        assert!(SaslState::Aborted.is_complete());
    }
}
