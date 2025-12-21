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
    Capability::CapNotify,
    Capability::AccountTag,
    Capability::Multiline,
    Capability::AccountRegistration,
    Capability::ChatHistory,
    Capability::EventPlayback,
    Capability::Tls, // STARTTLS - only useful on plaintext connections
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
    Authenticated,
}
