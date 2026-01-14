use crate::state::client::DeviceId;
use slirc_proto::Capability;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A secure string that is zeroized when dropped.
///
/// Used for passwords and other sensitive credential data to ensure
/// they don't linger in memory after use.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecureString(String);

impl SecureString {
    /// Create a new secure string.
    pub fn new(s: String) -> Self {
        Self(s)
    }

    /// Get the inner string (for passing to authentication functions).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecureString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print actual content
        f.debug_struct("SecureString")
            .field("len", &self.0.len())
            .finish()
    }
}

/// Capabilities we support (subset of slirc_proto::CAPABILITIES).
pub const SUPPORTED_CAPS: &[Capability] = &[
    Capability::MultiPrefix,
    Capability::UserhostInNames,
    Capability::ServerTime,
    Capability::EchoMessage,
    Capability::Sasl,
    Capability::Batch,
    Capability::MessageTags,
    Capability::LabeledResponse,
    Capability::SetName,
    Capability::AwayNotify,
    Capability::AccountNotify,
    Capability::ExtendedJoin,
    Capability::InviteNotify,
    Capability::ChgHost,
    Capability::Monitor,
    Capability::ExtendedMonitor,
    Capability::CapNotify,
    Capability::AccountTag,
    Capability::Multiline,
    Capability::AccountRegistration,
    Capability::ChatHistory,
    Capability::EventPlayback,
    Capability::DraftRelaymsg,
    Capability::Tls, // STARTTLS - only useful on plaintext connections
    Capability::Sts, // Strict Transport Security - advertised dynamically based on config
];

/// Maximum bytes allowed in a multiline batch message.
pub const MULTILINE_MAX_BYTES: u32 = 40000;
/// Maximum lines allowed in a multiline batch.
pub const MULTILINE_MAX_LINES: u32 = 100;

/// SASL authentication state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SaslState {
    #[default]
    None,
    /// Waiting for PLAIN credentials (base64-encoded).
    WaitingForData,
    /// Waiting for EXTERNAL response (empty or authzid).
    WaitingForExternal,
    /// Waiting for SCRAM client-first message.
    WaitingForScramClientFirst {
        /// The account name being authenticated.
        account_name: String,
    },
    /// Sent server-first, waiting for SCRAM client-final message.
    WaitingForScramClientFinal {
        /// The account name being authenticated.
        account_name: String,
        /// Device identifier extracted from SCRAM username (for bouncer/multiclient).
        device_id: Option<DeviceId>,
        /// Server nonce (combines client nonce + our random part).
        server_nonce: String,
        /// SCRAM verifiers from database.
        salt: Vec<u8>,
        iterations: u32,
        hashed_password: Vec<u8>,
        /// Auth message for final verification.
        auth_message: String,
    },
    Authenticated,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secure_string_debug_hides_content() {
        let secret = SecureString::new("super_secret_password".to_string());
        let debug_output = format!("{:?}", secret);

        // Debug output should NOT contain the actual password
        assert!(!debug_output.contains("super_secret_password"));
        // Should show SecureString struct name
        assert!(debug_output.contains("SecureString"));
        // Should show length field
        assert!(debug_output.contains("len"));
        assert!(debug_output.contains("21")); // Length of "super_secret_password"
    }

    #[test]
    fn test_secure_string_as_str_returns_content() {
        let secret = SecureString::new("my_password".to_string());
        assert_eq!(secret.as_str(), "my_password");
    }

    #[test]
    fn test_secure_string_empty() {
        let secret = SecureString::new(String::new());
        assert_eq!(secret.as_str(), "");
        let debug_output = format!("{:?}", secret);
        assert!(debug_output.contains("len"));
        assert!(debug_output.contains("0"));
    }

    #[test]
    fn test_sasl_state_default_is_none() {
        assert_eq!(SaslState::default(), SaslState::None);
    }

    #[test]
    fn test_sasl_state_variants_equality() {
        assert_eq!(SaslState::None, SaslState::None);
        assert_eq!(SaslState::WaitingForData, SaslState::WaitingForData);
        assert_eq!(SaslState::WaitingForExternal, SaslState::WaitingForExternal);
        assert_eq!(SaslState::Authenticated, SaslState::Authenticated);

        assert_ne!(SaslState::None, SaslState::Authenticated);
        assert_ne!(SaslState::WaitingForData, SaslState::WaitingForExternal);
    }

    #[test]
    fn test_multiline_constants() {
        // Verify multiline limits are reasonable values
        assert_eq!(MULTILINE_MAX_BYTES, 40000);
        assert_eq!(MULTILINE_MAX_LINES, 100);

        // Sanity check: bytes should be much larger than lines
        #[allow(clippy::assertions_on_constants)]
        {
            assert!(MULTILINE_MAX_BYTES > MULTILINE_MAX_LINES);
        }
    }

    #[test]
    fn test_supported_caps_not_empty() {
        #[allow(clippy::const_is_empty)]
        {
            assert!(!SUPPORTED_CAPS.is_empty());
        }
        // Should include common caps
        assert!(SUPPORTED_CAPS.contains(&Capability::Sasl));
        assert!(SUPPORTED_CAPS.contains(&Capability::MultiPrefix));
        assert!(SUPPORTED_CAPS.contains(&Capability::ServerTime));
    }
}
