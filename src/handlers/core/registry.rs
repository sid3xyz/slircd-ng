//! Command handler registry and dispatch.
//!
//! The `Registry` manages command handlers and provides command usage statistics.
//! Includes IRC-aware instrumentation for observability (Innovation 3).
//!
//! ## Typestate Dispatch (Innovation 1)
//!
//! Handlers are stored in phase-specific maps based on registration requirements:
//! - **`pre_reg_handlers`**: USER, PASS, WEBIRC, AUTHENTICATE (valid only before registration)
//! - **`post_reg_handlers`**: PRIVMSG, JOIN, etc. (require registration)
//! - **`universal_handlers`**: QUIT, PING, PONG, NICK, CAP (valid in any state)
//!
//! The registry dispatches based on connection state using the type system.
//! Post-registration handlers are *not in the map* for unregistered connections,
//! making invalid dispatch a structural impossibility.

use super::context::{Context, HandlerResult};
use super::traits::{DynUniversalHandler, PostRegHandler, PreRegHandler, ServerHandler};
use crate::handlers::{
    admin::{SajoinHandler, SamodeHandler, SanickHandler, SapartHandler},
    batch::BatchHandler,
    cap::{AuthenticateHandler, CapHandler},
    chathistory::ChatHistoryHandler,
    helpers::with_label,
    messaging::{
        AcceptHandler, MetadataHandler, NoticeHandler, NpcHandler, PrivmsgHandler, RelayMsgHandler,
        SceneHandler, TagmsgHandler,
    },
    mode::ModeHandler,
    server::{
        base::{ServerHandshakeHandler, ServerPropagationHandler},
        capab::CapabHandler,
        encap::EncapHandler,
        kick::KickHandler as ServerKickHandler,
        kill::KillHandler as ServerKillHandler,
        routing::RoutedMessageHandler,
        sid::SidHandler,
        sjoin::SJoinHandler,
        svinfo::SvinfoHandler,
        tmode::TModeHandler,
        topic::TopicHandler as ServerTopicHandler,
        uid::UidHandler,
    },
    s2s::encap::EncapHandler,
    s2s::kline::{KlineHandler, UnklineHandler},
    services::account::RegisterHandler,
    services::aliases::{CsHandler, NsHandler},
    user::monitor::MonitorHandler,
    user::status::{AwayHandler, SetnameHandler, SilenceHandler},
};
use crate::state::{RegisteredState, ServerState, UnregisteredState};
use crate::telemetry::CommandTimer;
use slirc_proto::Response;
use slirc_proto::{ChannelExt, MessageRef};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{Instrument, Level, debug, span};

/// Registry of command handlers.
///
/// Handlers are organized into three maps by registration phase (Innovation 1):
/// - `pre_reg_handlers`: Commands valid only before registration
/// - `post_reg_handlers`: Commands requiring registration
/// - `universal_handlers`: Commands valid in any state (generic over state type)
pub struct Registry {
    /// Handlers for pre-registration commands (USER, PASS, WEBIRC, AUTHENTICATE)
    pre_reg_handlers: HashMap<&'static str, Box<dyn PreRegHandler>>,
    /// Handlers for post-registration commands (PRIVMSG, JOIN, etc.)
    post_reg_handlers: HashMap<&'static str, Box<dyn PostRegHandler>>,
    /// Handlers for server-to-server commands (BURST, DELTA, etc.)
    server_handlers: HashMap<&'static str, Box<dyn ServerHandler>>,
    /// Handlers valid in any state (QUIT, PING, PONG, NICK, CAP, REGISTER)
    universal_handlers: HashMap<&'static str, Box<dyn DynUniversalHandler>>,
    /// Command usage counters for STATS m
    command_counts: HashMap<&'static str, Arc<AtomicU64>>,
    /// Total command execution time in microseconds
    command_timings: HashMap<&'static str, Arc<AtomicU64>>,
}

impl Registry {
    /// Create a new registry with all handlers registered.
    ///
    /// Handlers are placed into the appropriate phase map based on IRC protocol rules.
    /// `webirc_blocks` is passed from config for WEBIRC authorization.
    pub fn new(webirc_blocks: Vec<crate::config::WebircBlock>) -> Self {
        let mut pre_reg_handlers: HashMap<&'static str, Box<dyn PreRegHandler>> = HashMap::new();
        let mut post_reg_handlers: HashMap<&'static str, Box<dyn PostRegHandler>> = HashMap::new();
        let mut server_handlers: HashMap<&'static str, Box<dyn ServerHandler>> = HashMap::new();
        let mut universal_handlers: HashMap<&'static str, Box<dyn DynUniversalHandler>> =
            HashMap::new();

        // ====================================================================
        // Universal handlers (valid in any state)
        // These implement DynUniversalHandler for dual-dispatch capability.
        // ====================================================================
        universal_handlers.insert("CAP", Box::new(CapHandler));
        universal_handlers.insert("REGISTER", Box::new(RegisterHandler));
        universal_handlers.insert("AUTHENTICATE", Box::new(AuthenticateHandler));

        // ====================================================================
        // Pre-registration handlers (valid only before registration completes)
        // These require UnregisteredState and cannot be used after registration.
        // ====================================================================

        // Connection handlers (Universal + Pre-reg)
        crate::handlers::connection::register(
            &mut pre_reg_handlers,
            &mut universal_handlers,
            webirc_blocks,
        );

        // Server-to-server handshake
        pre_reg_handlers.insert("SERVER", Box::new(ServerHandshakeHandler));
        pre_reg_handlers.insert("CAPAB", Box::new(CapabHandler));
        pre_reg_handlers.insert("SVINFO", Box::new(SvinfoHandler));

        // ====================================================================
        // Server-to-server handlers (valid for registered servers)
        // ====================================================================
        server_handlers.insert("SERVER", Box::new(ServerPropagationHandler));
        server_handlers.insert("PRIVMSG", Box::new(RoutedMessageHandler));
        server_handlers.insert("NOTICE", Box::new(RoutedMessageHandler));
        server_handlers.insert("SJOIN", Box::new(SJoinHandler));
        server_handlers.insert("TMODE", Box::new(TModeHandler));
        server_handlers.insert("UID", Box::new(UidHandler));
        server_handlers.insert("SID", Box::new(SidHandler));
        server_handlers.insert("ENCAP", Box::new(EncapHandler));
        server_handlers.insert("TOPIC", Box::new(ServerTopicHandler));
        server_handlers.insert("TB", Box::new(crate::handlers::server::tb::TbHandler));
        server_handlers.insert("KICK", Box::new(ServerKickHandler));
        server_handlers.insert("KILL", Box::new(ServerKillHandler));
        server_handlers.insert(
            "BATCH",
            Box::new(crate::handlers::batch::server::ServerBatchHandler),
        );
        server_handlers.insert("KLINE", Box::new(KlineHandler::default()));
        server_handlers.insert("KLN", Box::new(KlineHandler::default())); // Alias
        server_handlers.insert("UNKLINE", Box::new(UnklineHandler::default()));
        server_handlers.insert("UNKLN", Box::new(UnklineHandler::default())); // Alias

        // ====================================================================
        // Post-registration handlers (require completed registration)
        // ====================================================================

        // Channel handlers
        crate::handlers::channel::register(&mut post_reg_handlers);
        post_reg_handlers.insert("MODE", Box::new(ModeHandler));

        // Messaging handlers
        post_reg_handlers.insert("PRIVMSG", Box::new(PrivmsgHandler));
        post_reg_handlers.insert("NOTICE", Box::new(NoticeHandler));
        post_reg_handlers.insert("TAGMSG", Box::new(TagmsgHandler));
        post_reg_handlers.insert("ACCEPT", Box::new(AcceptHandler));
        post_reg_handlers.insert("METADATA", Box::new(MetadataHandler));
        post_reg_handlers.insert("NPC", Box::new(NpcHandler));
        post_reg_handlers.insert("SCENE", Box::new(SceneHandler));
        post_reg_handlers.insert("RELAYMSG", Box::new(RelayMsgHandler));

        // User query handlers
        crate::handlers::user::query::register(&mut post_reg_handlers);

        // Server query handlers
        crate::handlers::server_query::register(&mut post_reg_handlers);

        // Misc handlers
        post_reg_handlers.insert("AWAY", Box::new(AwayHandler));
        post_reg_handlers.insert("SETNAME", Box::new(SetnameHandler));
        post_reg_handlers.insert("SILENCE", Box::new(SilenceHandler));
        post_reg_handlers.insert("MONITOR", Box::new(MonitorHandler));
        post_reg_handlers.insert("CHATHISTORY", Box::new(ChatHistoryHandler));

        // Batch handler for IRCv3 message batching (draft/multiline)
        post_reg_handlers.insert("BATCH", Box::new(BatchHandler));

        // Service aliases
        post_reg_handlers.insert("NICKSERV", Box::new(NsHandler));
        post_reg_handlers.insert("NS", Box::new(NsHandler)); // Shortcut for NickServ
        post_reg_handlers.insert("CHANSERV", Box::new(CsHandler));
        post_reg_handlers.insert("CS", Box::new(CsHandler)); // Shortcut for ChanServ

        // Operator handlers
        crate::handlers::oper::register(&mut post_reg_handlers);

        // Ban handlers
        crate::handlers::bans::register(&mut post_reg_handlers);

        // Admin SA* handlers
        post_reg_handlers.insert("SAJOIN", Box::new(SajoinHandler));
        post_reg_handlers.insert("SAPART", Box::new(SapartHandler));
        post_reg_handlers.insert("SANICK", Box::new(SanickHandler));
        post_reg_handlers.insert("SAMODE", Box::new(SamodeHandler));

        // Initialize command counters and timings for all registered commands
        let mut command_counts = HashMap::new();
        let mut command_timings = HashMap::new();

        macro_rules! init_stats {
            ($map:expr) => {
                for &cmd in $map.keys() {
                    command_counts.insert(cmd, Arc::new(AtomicU64::new(0)));
                    command_timings.insert(cmd, Arc::new(AtomicU64::new(0)));
                }
            };
        }

        init_stats!(pre_reg_handlers);
        init_stats!(post_reg_handlers);
        init_stats!(server_handlers);
        init_stats!(universal_handlers);

        Self {
            pre_reg_handlers,
            post_reg_handlers,
            server_handlers,
            universal_handlers,
            command_counts,
            command_timings,
        }
    }

    /// Get command usage statistics for STATS m.
    /// Returns: (Command, Count, TotalTimeMicros)
    pub fn get_command_stats(&self) -> Vec<(&'static str, u64, u64)> {
        let mut stats: Vec<_> = self
            .command_counts
            .iter()
            .map(|(cmd, count)| {
                let timing = self
                    .command_timings
                    .get(cmd)
                    .map(|t| t.load(Ordering::Relaxed))
                    .unwrap_or(0);
                (*cmd, count.load(Ordering::Relaxed), timing)
            })
            .filter(|(_, count, _)| *count > 0) // Only include used commands
            .collect();

        // Sort by usage count (descending)
        stats.sort_by(|a, b| b.1.cmp(&a.1));
        stats
    }

    /// Dispatch a message to the appropriate handler for unregistered connections.
    ///
    /// Uses `msg.command_name()` to get the command name directly from the
    /// zero-copy `MessageRef`. Includes IRC-aware instrumentation for tracing
    /// and metrics (Innovation 3).
    ///
    /// ## Typestate Dispatch (Innovation 1)
    ///
    /// Handler lookup for unregistered connections:
    /// - **Universal handlers**: Always checked first (QUIT, PING, PONG, NICK, CAP)
    /// - **Pre-reg handlers**: Checked for unregistered connections
    /// - **Post-reg handlers**: Inaccessible - returns NotRegistered error
    pub async fn dispatch_pre_reg(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let cmd_name = msg.command_name().to_ascii_uppercase();
        let cmd_str = cmd_name.as_str();

        // Increment command counter
        if let Some(counter) = self.command_counts.get(cmd_str) {
            counter.fetch_add(1, Ordering::Relaxed);
        }

        // Extract IRC context for tracing
        let source_nick = ctx.state.nick.as_deref();
        let channel = msg.arg(0).filter(|a| a.is_channel_name());
        let msgid = crate::telemetry::extract_msgid(msg);

        // Create instrumented span
        let irc_span = span!(
            Level::DEBUG,
            "irc.command",
            command = %cmd_name,
            uid = %ctx.uid,
            source_nick = source_nick,
            channel = channel,
            msgid = msgid.as_deref(),
            remote_addr = %ctx.remote_addr,
        );

        // Start timing for metrics
        let start = std::time::Instant::now();
        let _timer = CommandTimer::new(&cmd_name);

        // Execute handler within the span
        let result = if let Some(handler) = self.universal_handlers.get(cmd_str) {
            handler.handle_unreg(ctx, msg).instrument(irc_span).await
        } else if let Some(handler) = self.pre_reg_handlers.get(cmd_str) {
            handler.handle(ctx, msg).instrument(irc_span).await
        } else if self.post_reg_handlers.contains_key(cmd_str) {
            // Unregistered client trying to access post-reg command
            debug!(
                command = %cmd_name,
                uid = %ctx.uid,
                "Command rejected: not registered (typestate)"
            );
            crate::metrics::record_command_error(&cmd_name, "not_registered");
            Err(super::context::HandlerError::NotRegistered)
        } else {
            Err(super::context::HandlerError::UnknownCommand(
                cmd_name.clone(),
            ))
        };

        // Record timing
        let duration = start.elapsed().as_micros() as u64;
        if let Some(timing) = self.command_timings.get(cmd_str) {
            timing.fetch_add(duration, Ordering::Relaxed);
        }

        // Copy values needed for error handling before passing ctx as mutable
        let uid = ctx.uid.to_string();
        let nick = ctx.state.nick.clone();

        // Record errors for metrics
        self.handle_dispatch_result(&uid, nick.as_deref(), &cmd_name, result, ctx)
            .await
    }

    /// Dispatch a message to a post-registration handler.
    ///
    /// Receives `Context<RegisteredState>` with compile-time guarantees.
    /// The connection loop calls this directly for registered connections.
    ///
    /// ## Typestate Guarantee
    ///
    /// The caller has already transitioned the connection to `RegisteredState`,
    /// so we know nick/user are guaranteed present. No runtime checks needed.
    pub async fn dispatch_post_reg(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let cmd_name = msg.command_name().to_ascii_uppercase();
        let cmd_str = cmd_name.as_str();

        // Increment command counter
        if let Some(counter) = self.command_counts.get(cmd_str) {
            counter.fetch_add(1, Ordering::Relaxed);
        }

        // Extract IRC context for tracing
        let source_nick = Some(ctx.state.nick.as_str());
        let channel = msg.arg(0).filter(|a| a.is_channel_name());
        let msgid = crate::telemetry::extract_msgid(msg);

        // Create instrumented span
        let irc_span = span!(
            Level::DEBUG,
            "irc.command",
            command = %cmd_name,
            uid = %ctx.uid,
            source_nick = source_nick,
            channel = channel,
            msgid = msgid.as_deref(),
            remote_addr = %ctx.remote_addr,
        );

        // Start timing for metrics
        let start = std::time::Instant::now();
        let _timer = CommandTimer::new(&cmd_name);

        // Execute handler within the span
        // For registered connections, check universal handlers first, then post-reg
        let result = if let Some(handler) = self.universal_handlers.get(cmd_str) {
            handler.handle_reg(ctx, msg).instrument(irc_span).await
        } else if let Some(handler) = self.post_reg_handlers.get(cmd_str) {
            handler.handle(ctx, msg).instrument(irc_span).await
        } else if self.pre_reg_handlers.contains_key(cmd_str) {
            // Registered client trying to use pre-reg-only command
            debug!(
                command = %cmd_name,
                uid = %ctx.uid,
                "Command rejected: already registered"
            );
            crate::metrics::record_command_error(&cmd_name, "already_registered");
            Err(super::context::HandlerError::AlreadyRegistered)
        } else {
            Err(super::context::HandlerError::UnknownCommand(
                cmd_name.clone(),
            ))
        };

        // Record timing
        let duration = start.elapsed().as_micros() as u64;
        if let Some(timing) = self.command_timings.get(cmd_str) {
            timing.fetch_add(duration, Ordering::Relaxed);
        }

        // Copy values needed for error handling before passing ctx as mutable
        let uid = ctx.uid.to_string();
        let nick = ctx.state.nick.clone();

        // Record errors for metrics
        self.handle_dispatch_result(&uid, Some(&nick), &cmd_name, result, ctx)
            .await
    }

    /// Dispatch a message to the appropriate handler for server-to-server connections.
    pub async fn dispatch_server(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let cmd_name = msg.command_name().to_ascii_uppercase();
        let cmd_str = cmd_name.as_str();

        // Increment command counter
        if let Some(counter) = self.command_counts.get(cmd_str) {
            counter.fetch_add(1, Ordering::Relaxed);
        }

        // Create instrumented span
        let span = span!(
            Level::INFO,
            "server_command",
            command = %cmd_name,
            sid = %ctx.state.sid,
            server = %ctx.state.name
        );

        let start = std::time::Instant::now();
        let result = async {
            // 1. Check universal handlers first
            if let Some(handler) = self.universal_handlers.get(cmd_str) {
                return handler.handle_server(ctx, msg).await;
            }

            // 2. Check server-specific handlers
            if let Some(handler) = self.server_handlers.get(cmd_str) {
                return handler.handle(ctx, msg).await;
            }

            // 3. Unknown command for servers
            debug!(
                command = %cmd_name,
                sid = %ctx.state.sid,
                "Unknown server command"
            );
            Ok(()) // Servers usually ignore unknown commands silently or log them
        }
        .instrument(span)
        .await;

        // Record timing
        let duration = start.elapsed().as_micros() as u64;
        if let Some(timing) = self.command_timings.get(cmd_str) {
            timing.fetch_add(duration, Ordering::Relaxed);
        }

        // Record errors for metrics
        self.handle_dispatch_result(ctx.uid, None, &cmd_name, result, ctx)
            .await
    }

    /// Common result handling for both dispatch methods.
    async fn handle_dispatch_result<S>(
        &self,
        uid: &str,
        nick: Option<&str>,
        cmd_name: &str,
        result: HandlerResult,
        ctx: &mut Context<'_, S>,
    ) -> HandlerResult
    where
        S: Send,
    {
        match result {
            Ok(_) => Ok(()),
            Err(super::context::HandlerError::UnknownCommand(_)) => {
                // Unknown command
                let nick_str = nick.unwrap_or("*");
                let reply = Response::err_unknowncommand(nick_str, cmd_name)
                    .with_prefix(ctx.server_prefix());
                let reply = with_label(reply, ctx.label.as_deref());
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error(cmd_name, "unknown_command");
                Ok(())
            }
            Err(e) => {
                crate::metrics::record_command_error(cmd_name, e.error_code());
                debug!(command = %cmd_name, uid = %uid, error = %e, "Command error");
                Err(e)
            }
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}
