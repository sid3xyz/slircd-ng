//! IRC command handlers.
//!
//! This module contains the Handler trait and command registry for dispatching
//! incoming IRC messages to appropriate handlers.
//!
//! ## Zero-Copy Architecture
//!
//! Handlers receive `MessageRef<'_>` which borrows directly from the transport
//! buffer, avoiding allocations in the hot loop. Use `msg.arg(n)` to access
//! arguments as `&str` slices.

mod admin;
mod account;
mod bans;
mod batch;
mod cap;
mod channel;
mod chathistory;
mod connection;
mod helpers;
mod messaging;
mod mode;
mod monitor;
mod oper;
mod server_query;
mod service_aliases;
mod user_query;
mod user_status;

// Re-export helper functions for use by handlers
pub use helpers::{
    err_chanoprivsneeded, err_needmoreparams, err_noprivileges, err_nosuchchannel, err_nosuchnick,
    err_notonchannel, err_notregistered, err_unknowncommand, err_usernotinchannel, labeled_ack,
    matches_ban_or_except, matches_hostmask, server_notice, server_reply, user_prefix, with_label,
};

pub use admin::{SajoinHandler, SamodeHandler, SanickHandler, SapartHandler};
pub use account::RegisterHandler;
pub use bans::{
    DlineHandler, GlineHandler, KlineHandler, RlineHandler, ShunHandler, UndlineHandler,
    UnglineHandler, UnklineHandler, UnrlineHandler, UnshunHandler, UnzlineHandler, ZlineHandler,
};
pub use batch::{BatchHandler, BatchState, process_batch_message};
pub use cap::{AuthenticateHandler, CapHandler, SaslState};
pub use channel::{CycleHandler, InviteHandler, JoinHandler, KickHandler, KnockHandler, ListHandler, NamesHandler, PartHandler, TopicHandler, force_join_channel, force_part_channel, TargetUser};
pub use chathistory::ChatHistoryHandler;
pub use connection::{
    NickHandler, PassHandler, PingHandler, PongHandler, QuitHandler, UserHandler, WebircHandler,
};
pub use messaging::{NoticeHandler, PrivmsgHandler, TagmsgHandler};
pub use mode::{ModeHandler, apply_channel_modes_typed, format_modes_for_log};
pub use monitor::{MonitorHandler, cleanup_monitors, notify_monitors_offline, notify_monitors_online};
pub use oper::{ChghostHandler, DieHandler, KillHandler, OperHandler, RehashHandler, RestartHandler, TraceHandler, VhostHandler, WallopsHandler};
pub use server_query::{
    AdminHandler, HelpHandler, InfoHandler, LinksHandler, LusersHandler, MapHandler, MotdHandler,
    RulesHandler, ServiceHandler, ServlistHandler, SqueryHandler, StatsHandler, TimeHandler, UseripHandler, VersionHandler,
};
pub use service_aliases::{CsHandler, NsHandler};
pub use user_query::{IsonHandler, UserhostHandler, WhoHandler, WhoisHandler, WhowasHandler};
pub use user_status::{AwayHandler, SetnameHandler, SilenceHandler};

use crate::db::Database;
use crate::state::Matrix;
use async_trait::async_trait;
use slirc_proto::{Message, MessageRef};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;
use tokio::sync::{mpsc, Mutex};

/// Middleware for routing handler responses.
/// Direct forwards to the connection sender; Capturing buffers for labeled-response batching.
#[derive(Clone)]
pub enum ResponseMiddleware<'a> {
    Direct(&'a mpsc::Sender<Message>),
    Capturing(&'a Mutex<Vec<Message>>),
}

impl<'a> ResponseMiddleware<'a> {
    /// Send or buffer a message depending on middleware mode.
    pub async fn send(&self, msg: Message) -> Result<(), mpsc::error::SendError<Message>> {
        match self {
            Self::Direct(tx) => tx.send(msg).await,
            Self::Capturing(buf) => {
                let mut guard = buf.lock().await;
                guard.push(msg);
                Ok(())
            }
        }
    }
}

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
    pub capabilities: std::collections::HashSet<String>,
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
}

/// Fetch the current nick, user, and visible host for a given UID from Matrix.
pub async fn user_mask_from_state(ctx: &Context<'_>, uid: &str) -> Option<(String, String, String)> {
    let user_ref = ctx.matrix.users.get(uid)?;
    let user = user_ref.read().await;
    Some((user.nick.clone(), user.user.clone(), user.visible_host.clone()))
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

/// Registry of command handlers.
pub struct Registry {
    handlers: HashMap<&'static str, Box<dyn Handler>>,
    /// Command usage counters for STATS m
    command_counts: HashMap<&'static str, Arc<AtomicU64>>,
}

impl Registry {
    /// Create a new registry with all handlers registered.
    ///
    /// `webirc_blocks` is passed from config for WEBIRC authorization.
    pub fn new(webirc_blocks: Vec<crate::config::WebircBlock>) -> Self {
        let mut handlers: HashMap<&'static str, Box<dyn Handler>> = HashMap::new();

        // WEBIRC must be first to process before NICK/USER
        handlers.insert("WEBIRC", Box::new(WebircHandler::new(webirc_blocks)));

        // Connection/registration handlers
        handlers.insert("NICK", Box::new(NickHandler));
        handlers.insert("USER", Box::new(UserHandler));
        handlers.insert("PASS", Box::new(PassHandler));
        handlers.insert("PING", Box::new(PingHandler));
        handlers.insert("PONG", Box::new(PongHandler));
        handlers.insert("QUIT", Box::new(QuitHandler));
        handlers.insert("CAP", Box::new(CapHandler));
        handlers.insert("AUTHENTICATE", Box::new(AuthenticateHandler));
        handlers.insert("REGISTER", Box::new(RegisterHandler));

        // Channel handlers
        handlers.insert("JOIN", Box::new(JoinHandler));
        handlers.insert("PART", Box::new(PartHandler));
        handlers.insert("CYCLE", Box::new(CycleHandler));
        handlers.insert("TOPIC", Box::new(TopicHandler));
        handlers.insert("NAMES", Box::new(NamesHandler));
        handlers.insert("MODE", Box::new(ModeHandler));
        handlers.insert("KICK", Box::new(KickHandler));
        handlers.insert("LIST", Box::new(ListHandler));
        handlers.insert("INVITE", Box::new(InviteHandler));

        // Messaging handlers
        handlers.insert("PRIVMSG", Box::new(PrivmsgHandler));
        handlers.insert("NOTICE", Box::new(NoticeHandler));
        handlers.insert("TAGMSG", Box::new(TagmsgHandler));

        // User query handlers
        handlers.insert("WHO", Box::new(WhoHandler));
        handlers.insert("WHOIS", Box::new(WhoisHandler));
        handlers.insert("WHOWAS", Box::new(WhowasHandler));

        // Server query handlers
        handlers.insert("VERSION", Box::new(VersionHandler));
        handlers.insert("TIME", Box::new(TimeHandler));
        handlers.insert("ADMIN", Box::new(AdminHandler));
        handlers.insert("INFO", Box::new(InfoHandler));
        handlers.insert("LUSERS", Box::new(LusersHandler));
        handlers.insert("STATS", Box::new(StatsHandler));
        handlers.insert("MOTD", Box::new(MotdHandler));
        handlers.insert("MAP", Box::new(MapHandler));
        handlers.insert("RULES", Box::new(RulesHandler));
        handlers.insert("USERIP", Box::new(UseripHandler));
        handlers.insert("LINKS", Box::new(LinksHandler));
        handlers.insert("HELP", Box::new(HelpHandler));

        // Service query handlers (RFC 2812 §3.5)
        handlers.insert("SERVICE", Box::new(ServiceHandler));
        handlers.insert("SERVLIST", Box::new(ServlistHandler));
        handlers.insert("SQUERY", Box::new(SqueryHandler));

        // Misc handlers
        handlers.insert("AWAY", Box::new(AwayHandler));
        handlers.insert("USERHOST", Box::new(UserhostHandler));
        handlers.insert("ISON", Box::new(IsonHandler));
        handlers.insert("KNOCK", Box::new(KnockHandler));
        handlers.insert("SETNAME", Box::new(SetnameHandler));
        handlers.insert("SILENCE", Box::new(SilenceHandler));
        handlers.insert("MONITOR", Box::new(MonitorHandler));
        handlers.insert("CHATHISTORY", Box::new(ChatHistoryHandler));

        // Batch handler for IRCv3 message batching (draft/multiline)
        handlers.insert("BATCH", Box::new(BatchHandler));

        // Service aliases
        handlers.insert("NICKSERV", Box::new(NsHandler));
        handlers.insert("NS", Box::new(NsHandler)); // Shortcut for NickServ
        handlers.insert("CHANSERV", Box::new(CsHandler));
        handlers.insert("CS", Box::new(CsHandler)); // Shortcut for ChanServ

        // Operator handlers
        handlers.insert("OPER", Box::new(OperHandler));
        handlers.insert("KILL", Box::new(KillHandler));
        handlers.insert("WALLOPS", Box::new(WallopsHandler));
        handlers.insert("DIE", Box::new(DieHandler));
        handlers.insert("REHASH", Box::new(RehashHandler));
        handlers.insert("RESTART", Box::new(RestartHandler));
        handlers.insert("CHGHOST", Box::new(ChghostHandler));
        handlers.insert("VHOST", Box::new(VhostHandler));
        handlers.insert("TRACE", Box::new(TraceHandler));

        // Ban handlers
        handlers.insert("KLINE", Box::new(KlineHandler::kline()));
        handlers.insert("DLINE", Box::new(DlineHandler::dline()));
        handlers.insert("GLINE", Box::new(GlineHandler::gline()));
        handlers.insert("ZLINE", Box::new(ZlineHandler::zline()));
        handlers.insert("RLINE", Box::new(RlineHandler::rline()));
        handlers.insert("SHUN", Box::new(ShunHandler));
        handlers.insert("UNKLINE", Box::new(UnklineHandler::unkline()));
        handlers.insert("UNDLINE", Box::new(UndlineHandler::undline()));
        handlers.insert("UNGLINE", Box::new(UnglineHandler::ungline()));
        handlers.insert("UNZLINE", Box::new(UnzlineHandler::unzline()));
        handlers.insert("UNRLINE", Box::new(UnrlineHandler::unrline()));
        handlers.insert("UNSHUN", Box::new(UnshunHandler));

        // Admin SA* handlers
        handlers.insert("SAJOIN", Box::new(SajoinHandler));
        handlers.insert("SAPART", Box::new(SapartHandler));
        handlers.insert("SANICK", Box::new(SanickHandler));
        handlers.insert("SAMODE", Box::new(SamodeHandler));

        // Initialize command counters for all registered commands
        let mut command_counts = HashMap::new();
        for &cmd in handlers.keys() {
            command_counts.insert(cmd, Arc::new(AtomicU64::new(0)));
        }

        Self { handlers, command_counts }
    }

    /// Get command usage statistics for STATS m.
    pub fn get_command_stats(&self) -> Vec<(&'static str, u64)> {
        let mut stats: Vec<_> = self.command_counts
            .iter()
            .map(|(cmd, count)| (*cmd, count.load(Ordering::Relaxed)))
            .filter(|(_, count)| *count > 0) // Only include used commands
            .collect();

        // Sort by usage count (descending)
        stats.sort_by(|a, b| b.1.cmp(&a.1));
        stats
    }

    /// Dispatch a message to the appropriate handler.
    ///
    /// Uses `msg.command_name()` to get the command name directly from the
    /// zero-copy `MessageRef`.
    pub async fn dispatch(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let cmd_name = msg.command_name().to_ascii_uppercase();

        if let Some(handler) = self.handlers.get(cmd_name.as_str()) {
            // Increment command counter (counters are created for all handlers in new())
            // We use expect() here because the invariant is that all handlers have counters.
            // If this fails, it indicates a logic error in Registry::new().
            let counter = self.command_counts.get(cmd_name.as_str())
                .expect("Command counter missing for registered handler");
            counter.fetch_add(1, Ordering::Relaxed);

            handler.handle(ctx, msg).await
        } else {
            // Send ERR_UNKNOWNCOMMAND for unrecognized commands
            let nick = get_nick_or_star(ctx).await;
            let reply = err_unknowncommand(&ctx.matrix.server_info.name, &nick, &cmd_name);
            // Attach label for labeled-response capability
            let reply = with_label(reply, ctx.label.as_deref());
            ctx.sender.send(reply).await?;
            Ok(())
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new(Vec::new())
    }
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
    let nick = ctx.handshake.nick.as_deref().ok_or(HandlerError::NickOrUserMissing)?;
    let user = ctx.handshake.user.as_deref().ok_or(HandlerError::NickOrUserMissing)?;
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
        let _ = ctx.sender.send(err_noprivileges(server_name, &nick)).await;
        return Err(());
    }

    Ok(nick)
}

