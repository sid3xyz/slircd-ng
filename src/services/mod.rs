//! IRC services module.
//!
//! Provides virtual services like NickServ and ChanServ.

pub mod chanserv;
pub mod enforce;
pub mod nickserv;

use slirc_proto::Message;

/// Unified effect type returned by all service commands.
///
/// Services produce effects; callers (handlers) apply them to Matrix state.
/// This decouples service logic from state mutation, improving testability
/// and preparing for server-linking (effects can be forwarded).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Variants used by different services (NickServ, ChanServ, Enforce)
pub enum ServiceEffect {
    /// Send a message to a specific user (e.g., NOTICE reply).
    Reply { target_uid: String, msg: Message },

    /// Set user's account and +r mode (successful IDENTIFY/REGISTER).
    AccountIdentify { target_uid: String, account: String },

    /// Clear user's account and -r mode (DROP).
    AccountClear { target_uid: String },

    /// Clear enforcement timer for a user.
    ClearEnforceTimer { target_uid: String },

    /// Disconnect a user (GHOST, AKICK, KILL).
    Kill {
        target_uid: String,
        killer: String,
        reason: String,
    },

    /// Apply channel mode change (ChanServ OP/DEOP/VOICE).
    ChannelMode {
        channel: String,
        target_uid: String,
        mode_char: char,
        adding: bool,
    },

    /// Force nick change (enforcement).
    ForceNick {
        target_uid: String,
        old_nick: String,
        new_nick: String,
    },
}
