//! State observer trait for distributed synchronization (Innovation 2).
//!
//! This module defines the `StateObserver` trait, which allows the `SyncManager`
//! to hook into local state changes and broadcast them as `DELTA` updates.

use slirc_crdt::user::UserCrdt;
use slirc_crdt::channel::ChannelCrdt;
use slirc_crdt::clock::ServerId;

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
}
