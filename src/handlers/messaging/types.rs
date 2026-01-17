//! Types for message routing and handling.

use crate::handlers::core::Context;
use crate::security::UserContext;
use tracing::debug;

// ============================================================================
// Sender Snapshot
// ============================================================================

/// Pre-captured sender information to eliminate redundant user lookups.
///
/// InspIRCd pattern: Build complete sender context once at handler entry,
/// then pass by reference to all routing functions.
#[derive(Debug, Clone)]
pub struct SenderSnapshot {
    /// Sender's nickname.
    pub nick: String,
    /// Sender's username (ident).
    pub user: String,
    /// Sender's real hostname.
    pub host: String,
    /// Sender's visible (possibly cloaked) hostname.
    pub visible_host: String,
    /// Sender's realname (GECOS).
    pub realname: String,
    /// Sender's IP address.
    pub ip: String,
    /// Account name if identified.
    pub account: Option<String>,
    /// Whether sender is identified (+r).
    pub is_registered: bool,
    /// Whether sender is an IRC operator.
    pub is_oper: bool,
    /// Whether sender is marked as a bot (+B).
    pub is_bot: bool,
    /// Whether sender is on a TLS connection.
    pub is_tls: bool,
}

impl SenderSnapshot {
    /// Build a snapshot from context with a single user read.
    ///
    /// Returns None if the user is not found (shouldn't happen for registered users).
    pub async fn build<S>(ctx: &Context<'_, S>) -> Option<Self> {
        let user_arc = ctx
            .matrix
            .user_manager
            .users
            .get(ctx.uid)
            .map(|u| u.value().clone())?;
        let user = user_arc.read().await;
        Some(Self {
            nick: user.nick.clone(),
            user: user.user.clone(),
            host: user.host.clone(),
            visible_host: user.visible_host.clone(),
            realname: user.realname.clone(),
            ip: user.ip.clone(),
            account: user.account.clone(),
            is_registered: user.modes.registered,
            is_oper: user.modes.oper,
            is_bot: user.modes.bot,
            is_tls: user.modes.secure,
        })
    }

    /// Get the hostmask for shun checking (user@host).
    pub fn shun_mask(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    /// Get the full hostmask (nick!user@visible_host).
    pub fn full_mask(&self) -> String {
        format!("{}!{}@{}", self.nick, self.user, self.visible_host)
    }

    /// Build UserContext for channel routing (extended ban checks, etc.).
    pub fn to_user_context(&self, server_name: &str) -> UserContext {
        UserContext::for_registration(crate::security::RegistrationParams {
            hostname: self.host.clone(),
            nickname: self.nick.clone(),
            username: self.user.clone(),
            realname: self.realname.clone(),
            server: server_name.to_string(),
            account: self.account.clone(),
            is_tls: self.is_tls,
            is_oper: self.is_oper,
            oper_type: None, // oper_type not yet tracked
        })
    }
}

// ============================================================================
// Message Routing Types
// ============================================================================

pub use crate::state::actor::ChannelRouteResult;

/// Result of attempting to route a message to a user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Some variants used for internal flow control
pub enum UserRouteResult {
    /// Message was successfully sent (or queued).
    Sent,
    /// Target user does not exist.
    NoSuchNick,
    /// Blocked by +R (registered-only PMs).
    BlockedRegisteredOnly,
    /// Blocked by SILENCE list.
    BlockedSilence,
    /// Blocked by +T (no CTCP).
    BlockedCtcp,
}

/// Options for message routing behavior.
pub struct RouteOptions {
    /// Whether to send RPL_AWAY for user targets (only PRIVMSG).
    pub send_away_reply: bool,
    /// Status prefix for channel messages (e.g. @#chan).
    pub status_prefix: Option<char>,
}

/// Extra per-message metadata for routing.
///
/// Grouped into a struct to keep routing helpers readable and clippy-clean.
pub struct RouteMeta {
    pub timestamp: Option<String>,
    pub msgid: Option<String>,
    pub override_nick: Option<String>,
    /// For RELAYMSG: the nick of the user who issued the RELAYMSG command.
    /// If Some, adds `draft/relaymsg=<nick>` tag for recipients with that cap.
    pub relaymsg_sender_nick: Option<String>,
}

// ============================================================================
// Shun Checking
// ============================================================================

/// Check if a user is shunned using pre-fetched snapshot.
///
/// Returns true if the user is shunned and their command should be silently ignored.
pub async fn is_shunned_with_snapshot<S>(ctx: &Context<'_, S>, snapshot: &SenderSnapshot) -> bool {
    // Check database for shuns using pre-fetched hostmask
    match ctx.db.bans().matches_shun(&snapshot.shun_mask()).await {
        Ok(Some(shun)) => {
            debug!(
                uid = %ctx.uid,
                mask = %shun.mask,
                reason = ?shun.reason,
                "Shunned user attempted to send message"
            );
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_snapshot() -> SenderSnapshot {
        SenderSnapshot {
            nick: "testnick".to_string(),
            user: "testuser".to_string(),
            host: "real.host.com".to_string(),
            visible_host: "visible.host.com".to_string(),
            realname: "Real Name".to_string(),
            ip: "127.0.0.1".to_string(),
            account: Some("testaccount".to_string()),
            is_registered: true,
            is_oper: false,
            is_bot: false,
            is_tls: true,
        }
    }

    #[test]
    fn test_sender_snapshot_shun_mask() {
        let snapshot = create_test_snapshot();
        assert_eq!(snapshot.shun_mask(), "testuser@real.host.com");
    }

    #[test]
    fn test_sender_snapshot_full_mask() {
        let snapshot = create_test_snapshot();
        assert_eq!(snapshot.full_mask(), "testnick!testuser@visible.host.com");
    }
}
