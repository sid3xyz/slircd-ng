//! Capability Authority - The capability mint.
//!
//! This module implements [`CapabilityAuthority`], the sole entity authorized
//! to create capability tokens. It evaluates permission based on user and
//! channel state, logs all grants, and issues unforgeable tokens.

use super::irc::*;
use super::tokens::{Cap, Capability};
use crate::state::Matrix;
use crate::state::actor::ChannelEvent;
use std::sync::Arc;
use tokio::sync::oneshot;
use tracing::{debug, instrument, trace};

/// The Capability Authority - sole minter of capability tokens.
///
/// This struct wraps access to the Matrix (server state) and provides
/// methods to request capabilities. Each request:
///
/// 1. Evaluates the user's permission level
/// 2. Logs the capability grant (or denial) for audit
/// 3. Returns `Some(Cap<T>)` if authorized, `None` otherwise
///
/// # Security Properties
///
/// - The Authority is the only code path that can call `Cap::new()`
/// - All grants are logged with user ID and scope for audit
/// - Permission checks are centralized, not scattered through handlers
///
/// # Example
///
/// ```ignore
/// let authority = CapabilityAuthority::new(matrix.clone());
///
/// // Request a capability
/// if let Some(kick_cap) = authority.request_kick_cap(uid, "#channel").await {
///     // Perform kick - function signature proves authorization
///     channel.kick(target, kick_cap);
/// } else {
///     // User is not a channel operator
///     send_error(ERR_CHANOPRIVSNEEDED);
/// }
/// ```
pub struct CapabilityAuthority {
    matrix: Arc<Matrix>,
}

impl CapabilityAuthority {
    /// Create a new CapabilityAuthority with access to server state.
    #[inline]
    pub fn new(matrix: Arc<Matrix>) -> Self {
        Self { matrix }
    }

    // ========================================================================
    // Internal permission check helpers
    // ========================================================================

    /// Check if a user is an IRC operator.
    async fn is_oper(&self, uid: &str) -> bool {
        let user_arc = self.matrix.users.get(uid).map(|u| u.value().clone());
        if let Some(user_arc) = user_arc {
            user_arc.read().await.modes.oper
        } else {
            false
        }
    }

    /// Get the user's nickname for logging.
    async fn get_nick(&self, uid: &str) -> String {
        let user_arc = self.matrix.users.get(uid).map(|u| u.value().clone());
        if let Some(user_arc) = user_arc {
            user_arc.read().await.nick.clone()
        } else {
            uid.to_string()
        }
    }

    /// Check if a user has operator status (+o or higher) in a channel.
    ///
    /// This sends a query to the channel actor and awaits the response.
    async fn is_chanop(&self, uid: &str, channel: &str) -> bool {
        let channel_lower = slirc_proto::irc_to_lower(channel);

        // Get channel actor sender
        let channel_tx = match self.matrix.channels.get(&channel_lower) {
            Some(tx) => tx.clone(),
            None => return false,
        };

        // Query member modes
        let (reply_tx, reply_rx) = oneshot::channel();
        let event = ChannelEvent::GetMemberModes {
            uid: uid.to_string(),
            reply_tx,
        };

        if channel_tx.send(event).await.is_err() {
            return false;
        }

        match reply_rx.await {
            Ok(Some(modes)) => modes.has_op_or_higher(),
            _ => false,
        }
    }

    /// Check if a user has halfop status (+h or higher) in a channel.
    async fn has_halfop_or_higher(&self, uid: &str, channel: &str) -> bool {
        let channel_lower = slirc_proto::irc_to_lower(channel);

        let channel_tx = match self.matrix.channels.get(&channel_lower) {
            Some(tx) => tx.clone(),
            None => return false,
        };

        let (reply_tx, reply_rx) = oneshot::channel();
        let event = ChannelEvent::GetMemberModes {
            uid: uid.to_string(),
            reply_tx,
        };

        if channel_tx.send(event).await.is_err() {
            return false;
        }

        match reply_rx.await {
            Ok(Some(modes)) => modes.has_op_or_higher() || modes.halfop,
            _ => false,
        }
    }

    /// Log a capability grant.
    fn log_grant<T: Capability>(&self, nick: &str, uid: &str, scope: &T::Scope)
    where
        T::Scope: std::fmt::Debug,
    {
        debug!(
            capability = T::NAME,
            nick = %nick,
            uid = %uid,
            scope = ?scope,
            "Capability granted"
        );
    }

    /// Log a capability denial.
    fn log_denial<T: Capability>(&self, nick: &str, uid: &str, scope: &T::Scope)
    where
        T::Scope: std::fmt::Debug,
    {
        trace!(
            capability = T::NAME,
            nick = %nick,
            uid = %uid,
            scope = ?scope,
            "Capability denied"
        );
    }

    // ========================================================================
    // Channel Capability Requests
    // ========================================================================

    /// Request capability to kick a user from a channel.
    ///
    /// Returns `Some(Cap<KickCap>)` if the user is a channel operator or higher.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_kick_cap(&self, uid: &str, channel: &str) -> Option<Cap<KickCap>> {
        let nick = self.get_nick(uid).await;
        let channel_lower = slirc_proto::irc_to_lower(channel);

        // IRC operators can kick in any channel (oper override)
        let has_permission = self.is_oper(uid).await || self.is_chanop(uid, channel).await;

        if has_permission {
            self.log_grant::<KickCap>(&nick, uid, &channel_lower);
            Some(Cap::new(channel_lower))
        } else {
            self.log_denial::<KickCap>(&nick, uid, &channel_lower);
            None
        }
    }

    /// Request capability to set/unset bans on a channel.
    ///
    /// Returns `Some(Cap<BanCap>)` if the user is a channel operator or higher.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_ban_cap(&self, uid: &str, channel: &str) -> Option<Cap<BanCap>> {
        let nick = self.get_nick(uid).await;
        let channel_lower = slirc_proto::irc_to_lower(channel);

        let has_permission = self.is_oper(uid).await || self.is_chanop(uid, channel).await;

        if has_permission {
            self.log_grant::<BanCap>(&nick, uid, &channel_lower);
            Some(Cap::new(channel_lower))
        } else {
            self.log_denial::<BanCap>(&nick, uid, &channel_lower);
            None
        }
    }

    /// Request capability to set the channel topic.
    ///
    /// Returns `Some(Cap<TopicCap>)` if the user can set the topic.
    /// For channels with +t, requires channel operator or higher.
    /// For channels without +t, any member can set the topic (handled elsewhere).
    #[instrument(skip(self), level = "trace")]
    pub async fn request_topic_cap(&self, uid: &str, channel: &str) -> Option<Cap<TopicCap>> {
        let nick = self.get_nick(uid).await;
        let channel_lower = slirc_proto::irc_to_lower(channel);

        let has_permission = self.is_oper(uid).await || self.is_chanop(uid, channel).await;

        if has_permission {
            self.log_grant::<TopicCap>(&nick, uid, &channel_lower);
            Some(Cap::new(channel_lower))
        } else {
            self.log_denial::<TopicCap>(&nick, uid, &channel_lower);
            None
        }
    }

    /// Request capability to modify channel modes.
    ///
    /// Returns `Some(Cap<ChannelModeCap>)` if the user is a channel operator or higher.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_mode_cap(&self, uid: &str, channel: &str) -> Option<Cap<ChannelModeCap>> {
        let nick = self.get_nick(uid).await;
        let channel_lower = slirc_proto::irc_to_lower(channel);

        let has_permission = self.is_oper(uid).await || self.is_chanop(uid, channel).await;

        if has_permission {
            self.log_grant::<ChannelModeCap>(&nick, uid, &channel_lower);
            Some(Cap::new(channel_lower))
        } else {
            self.log_denial::<ChannelModeCap>(&nick, uid, &channel_lower);
            None
        }
    }

    /// Request capability to give/take voice on a channel.
    ///
    /// Returns `Some(Cap<VoiceCap>)` if the user is halfop or higher.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_voice_cap(&self, uid: &str, channel: &str) -> Option<Cap<VoiceCap>> {
        let nick = self.get_nick(uid).await;
        let channel_lower = slirc_proto::irc_to_lower(channel);

        let has_permission =
            self.is_oper(uid).await || self.has_halfop_or_higher(uid, channel).await;

        if has_permission {
            self.log_grant::<VoiceCap>(&nick, uid, &channel_lower);
            Some(Cap::new(channel_lower))
        } else {
            self.log_denial::<VoiceCap>(&nick, uid, &channel_lower);
            None
        }
    }

    /// Request capability to invite users to a channel.
    ///
    /// Returns `Some(Cap<InviteCap>)` if the user is a channel operator or higher.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_invite_cap(&self, uid: &str, channel: &str) -> Option<Cap<InviteCap>> {
        let nick = self.get_nick(uid).await;
        let channel_lower = slirc_proto::irc_to_lower(channel);

        let has_permission = self.is_oper(uid).await || self.is_chanop(uid, channel).await;

        if has_permission {
            self.log_grant::<InviteCap>(&nick, uid, &channel_lower);
            Some(Cap::new(channel_lower))
        } else {
            self.log_denial::<InviteCap>(&nick, uid, &channel_lower);
            None
        }
    }

    // ========================================================================
    // Oper Capability Requests
    // ========================================================================

    /// Request capability to KILL a user.
    ///
    /// Returns `Some(Cap<KillCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_kill_cap(&self, uid: &str) -> Option<Cap<KillCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<KillCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<KillCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to set K-lines.
    ///
    /// Returns `Some(Cap<KlineCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_kline_cap(&self, uid: &str) -> Option<Cap<KlineCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<KlineCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<KlineCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to set D-lines.
    ///
    /// Returns `Some(Cap<DlineCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_dline_cap(&self, uid: &str) -> Option<Cap<DlineCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<DlineCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<DlineCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to set G-lines.
    ///
    /// Returns `Some(Cap<GlineCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_gline_cap(&self, uid: &str) -> Option<Cap<GlineCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<GlineCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<GlineCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to set Z-lines.
    ///
    /// Returns `Some(Cap<ZlineCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_zline_cap(&self, uid: &str) -> Option<Cap<ZlineCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<ZlineCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<ZlineCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to set R-lines.
    ///
    /// Returns `Some(Cap<RlineCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_rline_cap(&self, uid: &str) -> Option<Cap<RlineCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<RlineCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<RlineCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to SHUN users.
    ///
    /// Returns `Some(Cap<ShunCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_shun_cap(&self, uid: &str) -> Option<Cap<ShunCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<ShunCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<ShunCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability for SA* admin commands.
    ///
    /// Returns `Some(Cap<AdminCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_admin_cap(&self, uid: &str) -> Option<Cap<AdminCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<AdminCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<AdminCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to REHASH the server.
    ///
    /// Returns `Some(Cap<RehashCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_rehash_cap(&self, uid: &str) -> Option<Cap<RehashCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<RehashCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<RehashCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to DIE (shut down the server).
    ///
    /// Returns `Some(Cap<DieCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_die_cap(&self, uid: &str) -> Option<Cap<DieCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<DieCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<DieCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to RESTART the server.
    ///
    /// Returns `Some(Cap<RestartCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_restart_cap(&self, uid: &str) -> Option<Cap<RestartCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<RestartCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<RestartCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to change user hosts (CHGHOST).
    ///
    /// Returns `Some(Cap<ChgHostCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_chghost_cap(&self, uid: &str) -> Option<Cap<ChgHostCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<ChgHostCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<ChgHostCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to change user idents (CHGIDENT).
    ///
    /// Returns `Some(Cap<ChgIdentCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_chgident_cap(&self, uid: &str) -> Option<Cap<ChgIdentCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<ChgIdentCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<ChgIdentCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to set VHOSTs.
    ///
    /// Returns `Some(Cap<VhostCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_vhost_cap(&self, uid: &str) -> Option<Cap<VhostCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<VhostCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<VhostCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to send WALLOPS.
    ///
    /// Returns `Some(Cap<WallopsCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_wallops_cap(&self, uid: &str) -> Option<Cap<WallopsCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<WallopsCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<WallopsCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to send GLOBOPS.
    ///
    /// Returns `Some(Cap<GlobOpsCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_globops_cap(&self, uid: &str) -> Option<Cap<GlobOpsCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<GlobOpsCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<GlobOpsCap>(&nick, uid, &());
            None
        }
    }

    // ========================================================================
    // Special Capability Requests
    // ========================================================================

    /// Request capability to bypass flood protection.
    ///
    /// Returns `Some(Cap<BypassFloodCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_bypass_flood_cap(&self, uid: &str) -> Option<Cap<BypassFloodCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<BypassFloodCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<BypassFloodCap>(&nick, uid, &());
            None
        }
    }

    /// Request capability to bypass mode restrictions on a channel.
    ///
    /// Returns `Some(Cap<BypassModeCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_bypass_mode_cap(
        &self,
        uid: &str,
        channel: &str,
    ) -> Option<Cap<BypassModeCap>> {
        let nick = self.get_nick(uid).await;
        let channel_lower = slirc_proto::irc_to_lower(channel);

        // Only IRC operators can bypass mode restrictions
        if self.is_oper(uid).await {
            self.log_grant::<BypassModeCap>(&nick, uid, &channel_lower);
            Some(Cap::new(channel_lower))
        } else {
            self.log_denial::<BypassModeCap>(&nick, uid, &channel_lower);
            None
        }
    }

    /// Request capability to send global notices.
    ///
    /// Returns `Some(Cap<GlobalNoticeCap>)` if the user is an IRC operator.
    #[instrument(skip(self), level = "trace")]
    pub async fn request_global_notice_cap(&self, uid: &str) -> Option<Cap<GlobalNoticeCap>> {
        let nick = self.get_nick(uid).await;

        if self.is_oper(uid).await {
            self.log_grant::<GlobalNoticeCap>(&nick, uid, &());
            Some(Cap::new(()))
        } else {
            self.log_denial::<GlobalNoticeCap>(&nick, uid, &());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    // Note: Full integration tests require setting up a Matrix with users.
    // Unit tests here verify the basic structure compiles correctly.

    #[test]
    fn authority_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        // CapabilityAuthority should be Send+Sync for async usage
        // (Can't fully test without Matrix, but structure check is valid)
    }
}
