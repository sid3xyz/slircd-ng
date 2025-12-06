//! Command handler context, state, and core types.
//!
//! Defines the `Context` struct passed to all handlers, `HandshakeState`
//! for connection registration tracking, and the `Handler` trait.

use super::middleware::ResponseMiddleware;
use super::registry::Registry;
use crate::db::Database;
use crate::handlers::batch::BatchState;
use crate::handlers::cap::SaslState;
use crate::state::Matrix;
use async_trait::async_trait;
use slirc_proto::{Message, MessageRef};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;

/// Handler context passed to each command handler.
pub struct Context<'a> {
    /// The user's unique ID.
    pub uid: &'a str,
    /// Shared server state.
    pub matrix: &'a Arc<Matrix>,
    /// Sender for outgoing messages to this client (can capture for labeled-response).
    pub sender: ResponseMiddleware<'a>,
    /// Current handshake state.
    pub handshake: &'a mut HandshakeState,
    /// Database for services.
    pub db: &'a Database,
    /// Remote address of the client.
    pub remote_addr: SocketAddr,
    /// Label from incoming message for labeled-response (IRCv3).
    /// If present, should be echoed back on all responses.
    pub label: Option<String>,
    /// Suppress automatic labeled-response ACK/BATCH wrapping.
    /// Set to true by handlers that manually apply labels (e.g., multiline BATCH).
    pub suppress_labeled_ack: bool,
    /// Command registry (for STATS m command usage tracking).
    pub registry: &'a Arc<Registry>,
}

/// State tracked during client registration handshake.
#[derive(Debug, Default)]
pub struct HandshakeState {
    /// Nick provided by NICK command.
    pub nick: Option<String>,
    /// Username provided by USER command.
    pub user: Option<String>,
    /// Realname provided by USER command.
    pub realname: Option<String>,
    /// Whether registration is complete.
    pub registered: bool,
    /// Whether CAP negotiation is in progress.
    pub cap_negotiating: bool,
    /// CAP protocol version (301 or 302).
    pub cap_version: u32,
    /// Capabilities enabled by this client.
    pub capabilities: HashSet<String>,
    /// SASL authentication state.
    pub sasl_state: SaslState,
    /// Buffer for accumulating chunked SASL data (for large payloads).
    pub sasl_buffer: String,
    /// Account name if SASL authenticated.
    pub account: Option<String>,
    /// Whether this is a TLS connection.
    pub is_tls: bool,
    /// TLS client certificate fingerprint (SHA-256, hex-encoded).
    /// Set by the network layer when a client presents a certificate.
    pub certfp: Option<String>,
    /// Failed OPER attempts counter (brute-force protection).
    pub failed_oper_attempts: u8,
    /// Timestamp of last OPER attempt (for rate limiting).
    pub last_oper_attempt: Option<std::time::Instant>,
    /// Whether WEBIRC was used to set client info.
    pub webirc_used: bool,
    /// Real IP address from WEBIRC (overrides connection IP).
    pub webirc_ip: Option<String>,
    /// Real hostname from WEBIRC (overrides reverse DNS).
    pub webirc_host: Option<String>,
    /// Password received via PASS command.
    pub pass_received: Option<String>,
    /// Active batch state for client-to-server batches (e.g., draft/multiline).
    pub active_batch: Option<BatchState>,
    /// Reference tag for the active batch.
    pub active_batch_ref: Option<String>,
}

impl HandshakeState {
    /// Check if we have both NICK and USER and can complete registration.
    /// Also requires CAP negotiation to be finished if it was started.
    pub fn can_register(&self) -> bool {
        self.nick.is_some() && self.user.is_some() && !self.registered && !self.cap_negotiating
    }
}

impl<'a> Context<'a> {
    /// Build and send a server reply in one call.
    ///
    /// This is a convenience method that combines `server_reply()` + `sender.send().await?`.
    /// Reduces the common two-line pattern to a single call.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Before:
    /// let reply = server_reply(server_name, Response::RPL_VERSION, vec![nick, version]);
    /// ctx.sender.send(reply).await?;
    ///
    /// // After:
    /// ctx.send_reply(Response::RPL_VERSION, vec![nick, version]).await?;
    /// ```
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

/// Errors that can occur during command handling.
#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)] // Send variant is large but rarely constructed
pub enum HandlerError {
    #[error("not enough parameters")]
    NeedMoreParams,
    #[error("no text to send")]
    NoTextToSend,
    #[allow(dead_code)] // TODO: Return from NickHandler instead of sending reply directly
    #[error("nickname in use: {0}")]
    NicknameInUse(String),
    #[allow(dead_code)] // TODO: Return from NickHandler for invalid nicks
    #[error("erroneous nickname: {0}")]
    ErroneousNickname(String),
    #[error("not registered")]
    NotRegistered,
    /// Disconnect the client silently (error message already sent)
    #[error("access denied")]
    AccessDenied,
    #[allow(dead_code)] // TODO: Return from USER handler for re-registration attempts
    #[error("already registered")]
    AlreadyRegistered,
    #[error("internal error: nick or user missing after registration")]
    NickOrUserMissing,
    #[error("send error: {0}")]
    Send(#[from] mpsc::error::SendError<Message>),
    #[error("client quit: {0:?}")]
    Quit(Option<String>),
    #[error("internal error: {0}")]
    Internal(String),
}

/// Result type for command handlers.
pub type HandlerResult = Result<(), HandlerError>;

/// Trait implemented by all command handlers.
///
/// Handlers receive a borrowed `MessageRef` that references the transport buffer
/// directly. Use `msg.arg(n)` to access arguments as `&str` slices.
#[async_trait]
pub trait Handler: Send + Sync {
    /// Handle an incoming message.
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult;
}

// ============================================================================
// User lookup helpers (Phase 1.1: DRY refactoring)
// ============================================================================

/// Resolve a nickname to UID. Returns None if not found.
///
/// Uses IRC case-folding for comparison.
pub fn resolve_nick_to_uid(ctx: &Context<'_>, nick: &str) -> Option<String> {
    let lower = slirc_proto::irc_to_lower(nick);
    ctx.matrix.nicks.get(&lower).map(|r| r.value().clone())
}

/// Get the current user's nick, falling back to "*" if not found.
pub async fn get_nick_or_star(ctx: &Context<'_>) -> String {
    if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        user_ref.read().await.nick.clone()
    } else {
        "*".to_string()
    }
}

/// Get nick and user from handshake state.
///
/// Returns `Ok((nick, user))` if both are set, `Err(HandlerError::NickOrUserMissing)` otherwise.
/// Use this in handlers that require registration to be complete.
#[inline]
#[allow(clippy::result_large_err)]
pub fn get_nick_user<'a>(ctx: &'a Context<'_>) -> Result<(&'a str, &'a str), HandlerError> {
    let nick = ctx
        .handshake
        .nick
        .as_deref()
        .ok_or(HandlerError::NickOrUserMissing)?;
    let user = ctx
        .handshake
        .user
        .as_deref()
        .ok_or(HandlerError::NickOrUserMissing)?;
    Ok((nick, user))
}

/// Check registration and get nick/user in one call.
///
/// Returns `Err(HandlerError::NotRegistered)` if not registered,
/// `Err(HandlerError::NickOrUserMissing)` if nick/user missing (bug),
/// or `Ok((nick, user))` on success.
///
/// This is the recommended way to start post-registration handlers:
/// ```ignore
/// let (nick, user) = require_registered(ctx)?;
/// ```
#[inline]
#[allow(clippy::result_large_err)]
pub fn require_registered<'a>(ctx: &'a Context<'_>) -> Result<(&'a str, &'a str), HandlerError> {
    if !ctx.handshake.registered {
        return Err(HandlerError::NotRegistered);
    }
    get_nick_user(ctx)
}

/// Fetch the current nick, user, and visible host for a given UID from Matrix.
pub async fn user_mask_from_state(
    ctx: &Context<'_>,
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
pub async fn get_oper_info(ctx: &Context<'_>) -> Option<(String, bool)> {
    let user_ref = ctx.matrix.users.get(ctx.uid)?;
    let user = user_ref.read().await;
    Some((user.nick.clone(), user.modes.oper))
}

/// Check if the current user is an IRC operator.
///
/// Returns `Ok(nick)` if they are an oper, or sends `ERR_NOPRIVILEGES` and returns `Err(())`.
pub async fn require_oper(ctx: &mut Context<'_>) -> Result<String, ()> {
    let server_name = &ctx.matrix.server_info.name;

    let Some((nick, is_oper)) = get_oper_info(ctx).await else {
        return Err(());
    };

    if !is_oper {
        use crate::handlers::helpers::err_noprivileges;
        let _ = ctx.sender.send(err_noprivileges(server_name, &nick)).await;
        return Err(());
    }

    Ok(nick)
}

/// Check if a user is in a specific channel.
///
/// Returns true if the user (identified by uid) is a member of the channel.
pub async fn is_user_in_channel(ctx: &Context<'_>, uid: &str, channel_lower: &str) -> bool {
    if let Some(user_ref) = ctx.matrix.users.get(uid) {
        let user = user_ref.read().await;
        user.channels.contains(channel_lower)
    } else {
        false
    }
}

