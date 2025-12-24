//! State observer trait for distributed synchronization (Innovation 2).
//!
//! This module defines the `StateObserver` trait, which allows the `SyncManager`
//! to hook into local state changes and broadcast them as `DELTA` updates.

use slirc_crdt::channel::ChannelCrdt;
use slirc_crdt::clock::ServerId;
use slirc_crdt::user::UserCrdt;

/// Type of global ban for S2S propagation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalBanType {
    /// G-line: Global user@host ban.
    Gline,
    /// Z-line: Global IP ban (skips DNS).
    Zline,
    /// R-line: Global realname/GECOS ban.
    Rline,
    /// Shun: Silent ignore (global).
    Shun,
}

impl GlobalBanType {
    /// Get the S2S command name for this ban type.
    pub fn command_name(&self) -> &'static str {
        match self {
            GlobalBanType::Gline => "GLINE",
            GlobalBanType::Zline => "ZLINE",
            GlobalBanType::Rline => "RLINE",
            GlobalBanType::Shun => "SHUN",
        }
    }

    /// Get the S2S unset command name for this ban type.
    pub fn unset_command_name(&self) -> &'static str {
        match self {
            GlobalBanType::Gline => "UNGLINE",
            GlobalBanType::Zline => "UNZLINE",
            GlobalBanType::Rline => "UNRLINE",
            GlobalBanType::Shun => "UNSHUN",
        }
    }
}

/// Trait for observing local state changes.
///
/// Methods are called by managers (UserManager, ChannelManager) whenever
/// a local state change occurs.
#[allow(dead_code)]
pub trait StateObserver: Send + Sync {
    /// Called when a user is created or updated locally.
    /// `source` is the ServerId that originated the change, or None if local.
    fn on_user_update(&self, user: &UserCrdt, source: Option<ServerId>);

    /// Called when a user is removed locally.
    fn on_user_quit(&self, uid: &str, reason: &str, source: Option<ServerId>);

    /// Called when a channel is created or updated locally.
    /// `source` is the ServerId that originated the change, or None if local.
    fn on_channel_update(&self, channel: &ChannelCrdt, source: Option<ServerId>);

    /// Called when a channel is destroyed locally.
    fn on_channel_destroy(&self, name: &str, source: Option<ServerId>);

    /// Called when a global ban is added locally.
    ///
    /// Global bans (G-line, Z-line, R-line, Shun) are propagated to all peers.
    /// `source` is the ServerId that originated the change, or None if local.
    fn on_ban_add(
        &self,
        ban_type: GlobalBanType,
        mask: &str,
        reason: &str,
        setter: &str,
        duration: Option<i64>,
        source: Option<ServerId>,
    );

    /// Called when a global ban is removed locally.
    ///
    /// `source` is the ServerId that originated the change, or None if local.
    fn on_ban_remove(&self, ban_type: GlobalBanType, mask: &str, source: Option<ServerId>);

    /// Called when a user's account status changes.
    ///
    /// Propagates account login/logout to peers so they can enforce ACLs.
    /// `account` is the account name, or None for logout.
    fn on_account_change(&self, uid: &str, account: Option<&str>, source: Option<ServerId>);
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // GlobalBanType::command_name tests
    // ========================================================================

    #[test]
    fn gline_command_name() {
        assert_eq!(GlobalBanType::Gline.command_name(), "GLINE");
    }

    #[test]
    fn zline_command_name() {
        assert_eq!(GlobalBanType::Zline.command_name(), "ZLINE");
    }

    #[test]
    fn rline_command_name() {
        assert_eq!(GlobalBanType::Rline.command_name(), "RLINE");
    }

    #[test]
    fn shun_command_name() {
        assert_eq!(GlobalBanType::Shun.command_name(), "SHUN");
    }

    // ========================================================================
    // GlobalBanType::unset_command_name tests
    // ========================================================================

    #[test]
    fn gline_unset_command_name() {
        assert_eq!(GlobalBanType::Gline.unset_command_name(), "UNGLINE");
    }

    #[test]
    fn zline_unset_command_name() {
        assert_eq!(GlobalBanType::Zline.unset_command_name(), "UNZLINE");
    }

    #[test]
    fn rline_unset_command_name() {
        assert_eq!(GlobalBanType::Rline.unset_command_name(), "UNRLINE");
    }

    #[test]
    fn shun_unset_command_name() {
        assert_eq!(GlobalBanType::Shun.unset_command_name(), "UNSHUN");
    }

    // ========================================================================
    // GlobalBanType equality tests
    // ========================================================================

    #[test]
    fn ban_types_are_equal() {
        assert_eq!(GlobalBanType::Gline, GlobalBanType::Gline);
        assert_eq!(GlobalBanType::Zline, GlobalBanType::Zline);
    }

    #[test]
    fn ban_types_are_not_equal() {
        assert_ne!(GlobalBanType::Gline, GlobalBanType::Zline);
        assert_ne!(GlobalBanType::Rline, GlobalBanType::Shun);
    }

    #[test]
    fn ban_types_are_copy() {
        let bt = GlobalBanType::Gline;
        let bt2 = bt; // Copy
        assert_eq!(bt, bt2); // Both should still be valid
    }
}
