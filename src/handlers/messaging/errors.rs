//! Error message constants for messaging handlers.
//!
//! Centralizes error strings to ensure consistency and enable easy updates.
//! These constants are used by `send_cannot_send` and related error functions.

// ============================================================================
// ERR_CANNOTSENDTOCHAN (404) reasons
// ============================================================================

/// Cannot send to channel - user is not in the channel (+n mode)
pub const CANNOT_SEND_NOT_IN_CHANNEL: &str = "Cannot send to channel (+n)";

/// Cannot send to channel - user is not voiced/op in moderated channel (+m mode)
pub const CANNOT_SEND_MODERATED: &str = "Cannot send to channel (+m)";

/// Cannot send to channel - user is not identified/registered (+r mode)
pub const CANNOT_SEND_REGISTERED_ONLY: &str = "Cannot send to channel (+r)";

/// Cannot send to channel - user is not identified/registered (+M mode)
pub const CANNOT_SEND_REGISTERED_SPEAK: &str = "Cannot send to channel (+M)";

/// Cannot send to channel - user is banned (+b mode)
pub const CANNOT_SEND_BANNED: &str = "Cannot send to channel (+b)";

/// Cannot send CTCP to channel - CTCP blocked (+C mode)
pub const CANNOT_SEND_CTCP: &str = "Cannot send CTCP to channel (+C)";

/// Cannot send NOTICE to channel - NOTICE blocked (+T mode)
pub const CANNOT_SEND_NOTICE: &str = "Cannot send NOTICE to channel (+T)";

/// Cannot send to channel - too many caps (+B mode)
pub const CANNOT_SEND_ANTI_CAPS: &str = "Your message contains too many capital letters (+B)";

/// Cannot send to channel - censored word (+G mode)
pub const CANNOT_SEND_CENSORED: &str = "Your message contains censored words (+G)";

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Error message constant validation tests
    // ========================================================================

    #[test]
    fn error_messages_are_not_empty() {
        assert!(!CANNOT_SEND_NOT_IN_CHANNEL.is_empty());
        assert!(!CANNOT_SEND_MODERATED.is_empty());
        assert!(!CANNOT_SEND_REGISTERED_ONLY.is_empty());
        assert!(!CANNOT_SEND_REGISTERED_SPEAK.is_empty());
        assert!(!CANNOT_SEND_BANNED.is_empty());
        assert!(!CANNOT_SEND_CTCP.is_empty());
        assert!(!CANNOT_SEND_NOTICE.is_empty());
        assert!(!CANNOT_SEND_ANTI_CAPS.is_empty());
        assert!(!CANNOT_SEND_CENSORED.is_empty());
    }

    #[test]
    fn error_messages_contain_mode_indicators() {
        // Each error should indicate which mode caused the restriction
        assert!(CANNOT_SEND_NOT_IN_CHANNEL.contains("+n"));
        assert!(CANNOT_SEND_MODERATED.contains("+m"));
        assert!(CANNOT_SEND_REGISTERED_ONLY.contains("+r"));
        assert!(CANNOT_SEND_REGISTERED_SPEAK.contains("+M"));
        assert!(CANNOT_SEND_BANNED.contains("+b"));
        assert!(CANNOT_SEND_CTCP.contains("+C"));
        assert!(CANNOT_SEND_NOTICE.contains("+T"));
    }

    #[test]
    fn error_messages_start_with_cannot() {
        // Consistent user-facing messaging
        assert!(CANNOT_SEND_NOT_IN_CHANNEL.starts_with("Cannot send"));
        assert!(CANNOT_SEND_MODERATED.starts_with("Cannot send"));
        assert!(CANNOT_SEND_REGISTERED_ONLY.starts_with("Cannot send"));
        assert!(CANNOT_SEND_REGISTERED_SPEAK.starts_with("Cannot send"));
        assert!(CANNOT_SEND_BANNED.starts_with("Cannot send"));
        assert!(CANNOT_SEND_CTCP.starts_with("Cannot send"));
        assert!(CANNOT_SEND_NOTICE.starts_with("Cannot send"));
    }
}
