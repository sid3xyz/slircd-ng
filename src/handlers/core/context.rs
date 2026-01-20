//! Command handler context and core types (Innovation 1).
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
use crate::caps::CapabilityAuthority;
use crate::db::Database;
use crate::state::actor::ChannelEvent;
use crate::state::{Matrix, RegisteredState, SessionState};
use slirc_proto::Prefix;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

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
    /// Active batch ID for grouping labeled responses.
    pub active_batch_id: Option<String>,
    /// Command registry (for STATS m command usage tracking).
    pub registry: &'a Arc<Registry>,
}

impl<'a, S> Context<'a, S> {
    /// Get the server name for prefixing replies.
    ///
    /// Use this instead of `ctx.matrix.server_info.name` for cleaner code.
    #[inline]
    pub fn server_name(&self) -> &str {
        &self.matrix.server_info.name
    }

    /// Get a server prefix for building messages.
    ///
    /// Use this instead of `Prefix::ServerName(ctx.server_name().to_string())`.
    #[inline]
    pub fn server_prefix(&self) -> Prefix {
        Prefix::ServerName(self.matrix.server_info.name.clone())
    }

    /// Get a capability authority for this context.
    ///
    /// Use this instead of `CapabilityAuthority::new(ctx.matrix.clone())`.
    #[inline]
    pub fn authority(&self) -> CapabilityAuthority {
        CapabilityAuthority::new(self.matrix.clone())
    }

    /// Build and send a server reply in one call.
    #[inline]
    pub async fn send_reply(
        &self,
        response: slirc_proto::Response,
        params: Vec<String>,
    ) -> Result<(), HandlerError> {
        use crate::handlers::util::helpers::{server_reply, with_label};
        let reply = server_reply(&self.matrix.server_info.name, response, params);
        let reply = with_label(reply, self.label.as_deref());
        self.sender.send(reply).await?;
        Ok(())
    }

    /// Send an error response and record the error metric in one call.
    ///
    /// Combines `ctx.sender.send(err)` + `metrics::record_command_error()`.
    /// Use this for all error responses to ensure metrics are always recorded.
    ///
    /// # Example
    /// ```ignore
    /// ctx.send_error("PRIVMSG", Response::err_nosuchnick(nick, target)).await?;
    /// ```
    #[inline]
    pub async fn send_error(
        &self,
        command: &str,
        error_name: &str,
        message: slirc_proto::Message,
    ) -> Result<(), HandlerError> {
        self.sender.send(message).await?;
        crate::metrics::record_command_error(command, error_name);
        Ok(())
    }
}

// ============================================================================
// SessionState convenience methods
// ============================================================================

impl<'a, S: SessionState> Context<'a, S> {
    /// Get the nick or "*" for error messages.
    ///
    /// Works for both RegisteredState (returns nick) and UnregisteredState
    /// (returns nick or "*").
    #[inline]
    pub fn nick(&self) -> &str {
        self.state.nick_or_star()
    }
}

// ============================================================================
// RegisteredState convenience methods
// ============================================================================

/// Convenience methods available only for registered connections.
///
/// These provide the same API as the old `TypedContext<Registered>` but without
/// the wrapper - the type system guarantees nick/user are present.
impl<'a> Context<'a, RegisteredState> {
    /// Get the username (guaranteed present for registered connections).
    #[inline]
    pub fn user(&self) -> &str {
        &self.state.user
    }

    /// Get both nick and user.
    #[inline]
    pub fn nick_user(&self) -> (&str, &str) {
        (&self.state.nick, &self.state.user)
    }

    /// Ensure the channel exists and return its sender.
    #[allow(clippy::result_large_err)]
    pub fn require_channel_exists(
        &self,
        channel: &str,
    ) -> Result<mpsc::Sender<ChannelEvent>, HandlerError> {
        let channel_lower = slirc_proto::irc_to_lower(channel);
        if let Some(sender) = self.matrix.channel_manager.channels.get(&channel_lower) {
            Ok(sender.value().clone())
        } else {
            Err(HandlerError::NoSuchChannel(channel.to_string()))
        }
    }

    /// Send a server notice to the user.
    ///
    /// Wraps `server_notice` and sends it.
    #[inline]
    pub async fn send_notice(&self, text: impl Into<String>) -> Result<(), HandlerError> {
        let msg =
            crate::handlers::util::helpers::server_notice(self.server_name(), self.nick(), text);
        self.sender.send(msg).await?;
        Ok(())
    }
}

// ============================================================================
// User lookup helpers
// ============================================================================

/// Resolve a nickname to UID. Returns None if not found.
///
/// Uses IRC case-folding for comparison. For bouncer multiclient, returns first UID.
pub fn resolve_nick_to_uid<S>(ctx: &Context<'_, S>, nick: &str) -> Option<String> {
    let lower = slirc_proto::irc_to_lower(nick);
    ctx.matrix.user_manager.get_first_uid(&lower)
}

/// Resolve a nickname or send ERR_NOSUCHNICK.
///
/// Returns `Ok(Some(uid))` when found, or `Ok(None)` after sending ERR_NOSUCHNICK
/// with metrics + labeling.
pub async fn resolve_nick_or_nosuchnick<S: crate::state::SessionState>(
    ctx: &mut Context<'_, S>,
    cmd: &str,
    nick: &str,
) -> Result<Option<String>, HandlerError> {
    Ok(match resolve_nick_to_uid(ctx, nick) {
        Some(uid) => Some(uid),
        None => {
            crate::handlers::send_no_such_nick(ctx, cmd, nick).await?;
            None
        }
    })
}

/// Get the current user's nick, falling back to "*" if not found.
pub async fn get_nick_or_star<S>(ctx: &Context<'_, S>) -> String {
    let user_arc = ctx
        .matrix
        .user_manager
        .users
        .get(ctx.uid)
        .map(|u| u.value().clone());
    if let Some(user_arc) = user_arc {
        user_arc.read().await.nick.clone()
    } else {
        "*".to_string()
    }
}

/// Fetch the current nick, user, and visible host for a given UID from Matrix.
pub async fn user_mask_from_state<S>(
    ctx: &Context<'_, S>,
    uid: &str,
) -> Option<(String, String, String)> {
    let user_arc = ctx
        .matrix
        .user_manager
        .users
        .get(uid)
        .map(|u| u.value().clone())?;
    let user = user_arc.read().await;
    Some((
        user.nick.clone(),
        user.user.clone(),
        user.visible_host.clone(),
    ))
}

/// Get the current user's nick and oper status. Returns None if user not found.
pub async fn get_oper_info<S>(ctx: &Context<'_, S>) -> Option<(String, bool)> {
    let user_arc = ctx
        .matrix
        .user_manager
        .users
        .get(ctx.uid)
        .map(|u| u.value().clone())?;
    let user = user_arc.read().await;
    Some((user.nick.clone(), user.modes.oper))
}

/// Check if a user is in a specific channel.
///
/// Returns true if the user (identified by uid) is a member of the channel.
pub async fn is_user_in_channel<S>(ctx: &Context<'_, S>, uid: &str, channel_lower: &str) -> bool {
    let user_arc = ctx
        .matrix
        .user_manager
        .users
        .get(uid)
        .map(|u| u.value().clone());
    if let Some(user_arc) = user_arc {
        let user = user_arc.read().await;
        user.channels.contains(channel_lower)
    } else {
        false
    }
}
/// Check if a channel has a specific mode set.
///
/// Returns true if the channel has the mode, false otherwise (including if channel doesn't exist).
pub async fn channel_has_mode<S>(
    ctx: &Context<'_, S>,
    channel_lower: &str,
    mode: crate::state::actor::ChannelMode,
) -> bool {
    use crate::state::actor::ChannelEvent;
    use tokio::sync::oneshot;

    let tx = match ctx.matrix.channel_manager.channels.get(channel_lower) {
        Some(tx_ref) => tx_ref.value().clone(),
        None => return false,
    };

    let (reply_tx, reply_rx) = oneshot::channel();
    if tx.send(ChannelEvent::GetModes { reply_tx }).await.is_err() {
        return false;
    }

    match reply_rx.await {
        Ok(modes) => modes.contains(&mode),
        Err(_) => false,
    }
}
