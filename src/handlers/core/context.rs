//! Command handler context and core types (Innovation 1 Phase 3).
//!
//! Defines the `Context<'a, S>` struct passed to all handlers. The type parameter
//! `S` is the session state type:
//! - `UnregisteredState` — for pre-registration handlers
//! - `RegisteredState` — for post-registration handlers
//!
//! Universal handlers are generic over `S: SessionState`, allowing them to work
//! in both phases.

use super::middleware::ResponseMiddleware;
use super::registry::Registry;
use crate::db::Database;
use crate::state::{Matrix, RegisteredState};
use std::net::SocketAddr;
use std::sync::Arc;

// Re-export error types from central module
pub use crate::error::{HandlerError, HandlerResult};

/// Handler context passed to each command handler.
///
/// Generic over session state type `S`:
/// - `UnregisteredState` for pre-registration commands
/// - `RegisteredState` for post-registration commands
/// - `S: SessionState` for universal handlers that work in both phases
pub struct Context<'a, S> {
    /// The user's unique ID.
    pub uid: &'a str,
    /// Shared server state.
    pub matrix: &'a Arc<Matrix>,
    /// Sender for outgoing messages to this client.
    pub sender: ResponseMiddleware<'a>,
    /// Session state (type varies by registration phase).
    pub state: &'a mut S,
    /// Database for services.
    pub db: &'a Database,
    /// Remote address of the client.
    pub remote_addr: SocketAddr,
    /// Label from incoming message for labeled-response (IRCv3).
    pub label: Option<String>,
    /// Suppress automatic labeled-response ACK/BATCH wrapping.
    pub suppress_labeled_ack: bool,
    /// Command registry (for STATS m command usage tracking).
    pub registry: &'a Arc<Registry>,
}

impl<'a, S> Context<'a, S> {
    /// Create a new context.
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)] // Phase 3: Will be used when connection loop switches to ConnectionState
    pub fn new(
        uid: &'a str,
        matrix: &'a Arc<Matrix>,
        sender: ResponseMiddleware<'a>,
        state: &'a mut S,
        db: &'a Database,
        remote_addr: SocketAddr,
        label: Option<String>,
        registry: &'a Arc<Registry>,
    ) -> Self {
        Self {
            uid,
            matrix,
            sender,
            state,
            db,
            remote_addr,
            label,
            suppress_labeled_ack: false,
            registry,
        }
    }

    /// Build and send a server reply in one call.
    #[inline]
    pub async fn send_reply(
        &self,
        response: slirc_proto::Response,
        params: Vec<String>,
    ) -> Result<(), HandlerError> {
        use crate::handlers::helpers::server_reply;
        let reply = server_reply(&self.matrix.server_info.name, response, params);
        self.sender.send(reply).await?;
        Ok(())
    }
}

// ============================================================================
// RegisteredState convenience methods (Phase 3)
// ============================================================================

/// Convenience methods available only for registered connections.
///
/// These provide the same API as the old `TypedContext<Registered>` but without
/// the wrapper - the type system guarantees nick/user are present.
impl<'a> Context<'a, RegisteredState> {
    /// Get the user's nickname (guaranteed present for registered connections).
    #[inline]
    pub fn nick(&self) -> &str {
        &self.state.nick
    }

    /// Get the username (guaranteed present for registered connections).
    #[inline]
    pub fn user(&self) -> &str {
        &self.state.user
    }

    /// Get both nick and user.
    #[inline]
    #[allow(dead_code)]
    pub fn nick_user(&self) -> (&str, &str) {
        (&self.state.nick, &self.state.user)
    }

    /// Check if a capability is enabled.
    #[inline]
    #[allow(dead_code)]
    pub fn has_cap(&self, cap: &str) -> bool {
        self.state.capabilities.contains(cap)
    }

    /// Get account name for message tags.
    #[inline]
    #[allow(dead_code)]
    pub fn account_tag(&self) -> Option<&str> {
        self.state.account.as_deref()
    }
}

// ============================================================================
// User lookup helpers (Phase 1.1: DRY refactoring)
// ============================================================================

/// Resolve a nickname to UID. Returns None if not found.
///
/// Uses IRC case-folding for comparison.
pub fn resolve_nick_to_uid<S>(ctx: &Context<'_, S>, nick: &str) -> Option<String> {
    let lower = slirc_proto::irc_to_lower(nick);
    ctx.matrix.nicks.get(&lower).map(|r| r.value().clone())
}

/// Get the current user's nick, falling back to "*" if not found.
pub async fn get_nick_or_star<S>(ctx: &Context<'_, S>) -> String {
    if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        user_ref.read().await.nick.clone()
    } else {
        "*".to_string()
    }
}

/// Fetch the current nick, user, and visible host for a given UID from Matrix.
pub async fn user_mask_from_state<S>(
    ctx: &Context<'_, S>,
    uid: &str,
) -> Option<(String, String, String)> {
    let user_ref = ctx.matrix.users.get(uid)?;
    let user = user_ref.read().await;
    Some((
        user.nick.clone(),
        user.user.clone(),
        user.visible_host.clone(),
    ))
}

/// Get the current user's nick and oper status. Returns None if user not found.
pub async fn get_oper_info<S>(ctx: &Context<'_, S>) -> Option<(String, bool)> {
    let user_ref = ctx.matrix.users.get(ctx.uid)?;
    let user = user_ref.read().await;
    Some((user.nick.clone(), user.modes.oper))
}

/// Check if a user is in a specific channel.
///
/// Returns true if the user (identified by uid) is a member of the channel.
pub async fn is_user_in_channel<S>(ctx: &Context<'_, S>, uid: &str, channel_lower: &str) -> bool {
    if let Some(user_ref) = ctx.matrix.users.get(uid) {
        let user = user_ref.read().await;
        user.channels.contains(channel_lower)
    } else {
        false
    }
}
