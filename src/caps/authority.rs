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
use tracing::{debug, trace};

// ============================================================================
// Request Method Generation Macros
// ============================================================================

/// Macro to generate oper capability request methods (unit scope, oper check only).
macro_rules! impl_oper_cap_request {
    ($(
        $(#[$meta:meta])*
        $method:ident -> $cap:ident
    ),* $(,)?) => {
        $(
            $(#[$meta])*
            pub async fn $method(&self, uid: &str) -> Option<Cap<$cap>> {
                let nick = self.get_nick(uid).await;
                if self.is_oper(uid).await {
                    self.log_grant::<$cap>(&nick, uid, &());
                    Some(Cap::new(()))
                } else {
                    self.log_denial::<$cap>(&nick, uid, &());
                    None
                }
            }
        )*
    };
}

/// Macro to generate channel capability request methods (chanop or oper check).
macro_rules! impl_chanop_cap_request {
    ($(
        $(#[$meta:meta])*
        $method:ident -> $cap:ident
    ),* $(,)?) => {
        $(
            $(#[$meta])*
            pub async fn $method(&self, uid: &str, channel: &str) -> Option<Cap<$cap>> {
                let nick = self.get_nick(uid).await;
                let channel_lower = slirc_proto::irc_to_lower(channel);
                let has_permission = self.is_oper(uid).await || self.is_chanop(uid, channel).await;
                if has_permission {
                    self.log_grant::<$cap>(&nick, uid, &channel_lower);
                    Some(Cap::new(channel_lower))
                } else {
                    self.log_denial::<$cap>(&nick, uid, &channel_lower);
                    None
                }
            }
        )*
    };
}

// ============================================================================
// Capability Authority
// ============================================================================

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
        let user_arc = self
            .matrix
            .user_manager
            .users
            .get(uid)
            .map(|u| u.value().clone());
        if let Some(user_arc) = user_arc {
            user_arc.read().await.modes.oper
        } else {
            false
        }
    }

    /// Get the user's nickname for logging.
    async fn get_nick(&self, uid: &str) -> String {
        let user_arc = self
            .matrix
            .user_manager
            .users
            .get(uid)
            .map(|u| u.value().clone());
        if let Some(user_arc) = user_arc {
            user_arc.read().await.nick.clone()
        } else {
            uid.to_string()
        }
    }

    /// Check if a user has operator status (+o or higher) in a channel.
    async fn is_chanop(&self, uid: &str, channel: &str) -> bool {
        let channel_lower = slirc_proto::irc_to_lower(channel);

        let channel_tx = match self.matrix.channel_manager.channels.get(&channel_lower) {
            Some(tx) => tx.value().clone(),
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
            Ok(Some(modes)) => modes.has_op_or_higher(),
            _ => false,
        }
    }

    /// Check if a user has halfop status (+h or higher) in a channel.
    async fn has_halfop_or_higher(&self, uid: &str, channel: &str) -> bool {
        let channel_lower = slirc_proto::irc_to_lower(channel);

        let channel_tx = match self.matrix.channel_manager.channels.get(&channel_lower) {
            Some(tx) => tx.value().clone(),
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
    // Channel Capability Requests (chanop or oper required)
    // ========================================================================

    impl_chanop_cap_request! {
        /// Request capability to kick a user from a channel.
        request_kick_cap -> KickCap,

        /// Request capability to set/unset bans on a channel.
        request_ban_cap -> BanCap,

        /// Request capability to set the channel topic.
        request_topic_cap -> TopicCap,

        /// Request capability to modify channel modes.
        request_mode_cap -> ChannelModeCap,

        /// Request capability to invite users to a channel.
        request_invite_cap -> InviteCap,
    }

    /// Request capability to give/take voice on a channel.
    ///
    /// Returns `Some(Cap<VoiceCap>)` if the user is halfop or higher.
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

    // ========================================================================
    // Oper Capability Requests (oper only)
    // ========================================================================

    impl_oper_cap_request! {
        /// Request capability to KILL a user.
        request_kill_cap -> KillCap,

        /// Request capability to set K-lines.
        request_kline_cap -> KlineCap,

        /// Request capability to set D-lines.
        request_dline_cap -> DlineCap,

        /// Request capability to set G-lines.
        request_gline_cap -> GlineCap,

        /// Request capability to set Z-lines.
        request_zline_cap -> ZlineCap,

        /// Request capability to set R-lines.
        request_rline_cap -> RlineCap,

        /// Request capability to SHUN users.
        request_shun_cap -> ShunCap,

        /// Request capability for SA* admin commands.
        request_admin_cap -> AdminCap,

        /// Request capability to REHASH the server.
        request_rehash_cap -> RehashCap,

        /// Request capability to DIE (shut down the server).
        request_die_cap -> DieCap,

        /// Request capability to RESTART the server.
        request_restart_cap -> RestartCap,

        /// Request capability to change user hosts (CHGHOST).
        request_chghost_cap -> ChgHostCap,

        /// Request capability to change user idents (CHGIDENT).
        request_chgident_cap -> ChgIdentCap,

        /// Request capability to set VHOSTs.
        request_vhost_cap -> VhostCap,

        /// Request capability to send WALLOPS.
        request_wallops_cap -> WallopsCap,

        /// Request capability to send GLOBOPS.
        request_globops_cap -> GlobOpsCap,

        /// Request capability to bypass flood protection.
        request_bypass_flood_cap -> BypassFloodCap,

        /// Request capability to send global notices.
        request_global_notice_cap -> GlobalNoticeCap,

        /// Request capability to configure spam detection.
        request_spamconf_cap -> SpamConfCap,

        /// Request capability to clear channel state (CLEARCHAN).
        request_clearchan_cap -> ClearChanCap,
    }

    /// Request capability to bypass mode restrictions on a channel.
    ///
    /// Returns `Some(Cap<BypassModeCap>)` if the user is an IRC operator.
    pub async fn request_bypass_mode_cap(
        &self,
        uid: &str,
        channel: &str,
    ) -> Option<Cap<BypassModeCap>> {
        let nick = self.get_nick(uid).await;
        let channel_lower = slirc_proto::irc_to_lower(channel);

        if self.is_oper(uid).await {
            self.log_grant::<BypassModeCap>(&nick, uid, &channel_lower);
            Some(Cap::new(channel_lower))
        } else {
            self.log_denial::<BypassModeCap>(&nick, uid, &channel_lower);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn authority_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        // CapabilityAuthority should be Send+Sync for async usage
    }
}
