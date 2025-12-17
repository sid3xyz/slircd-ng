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

/// Cannot send to channel - user is banned (+b mode)
pub const CANNOT_SEND_BANNED: &str = "Cannot send to channel (+b)";

/// Cannot send CTCP to channel - CTCP blocked (+C mode)
pub const CANNOT_SEND_CTCP: &str = "Cannot send CTCP to channel (+C)";

/// Cannot send NOTICE to channel - NOTICE blocked (+T mode)
pub const CANNOT_SEND_NOTICE: &str = "Cannot send NOTICE to channel (+T)";
