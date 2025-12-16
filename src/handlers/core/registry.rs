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
use super::traits::{DynUniversalHandler, PostRegHandler, PreRegHandler};
use crate::state::{RegisteredState, UnregisteredState};
use slirc_proto::{Prefix, Response};
use crate::handlers::{
    account::RegisterHandler,
    admin::{SajoinHandler, SamodeHandler, SanickHandler, SapartHandler},
    bans::{
        DlineHandler, GlineHandler, KlineHandler, RlineHandler, ShunHandler, UndlineHandler,
        UnglineHandler, UnklineHandler, UnrlineHandler, UnshunHandler, UnzlineHandler,
        ZlineHandler,
    },
    batch::BatchHandler,
    cap::{AuthenticateHandler, CapHandler},
    channel::{
        CycleHandler, InviteHandler, JoinHandler, KickHandler, KnockHandler, ListHandler,
        NamesHandler, PartHandler, TopicHandler,
    },
    chathistory::ChatHistoryHandler,
    connection::{
        NickHandler, PassHandler, PingHandler, PongHandler, QuitHandler, UserHandler, WebircHandler,
    },
    helpers::with_label,
    messaging::{NoticeHandler, PrivmsgHandler, TagmsgHandler},
    mode::ModeHandler,
    monitor::MonitorHandler,
    oper::{
        ChghostHandler, ChgIdentHandler, DieHandler, GlobOpsHandler, KillHandler, OperHandler,
        RehashHandler, RestartHandler, TraceHandler, VhostHandler, WallopsHandler,
    },
    server_query::{
        AdminHandler, HelpHandler, InfoHandler, LinksHandler, LusersHandler, MapHandler,
        MotdHandler, RulesHandler, ServiceHandler, ServlistHandler, SqueryHandler, StatsHandler,
        TimeHandler, UseripHandler, VersionHandler, SummonHandler, UsersHandler,
    },
    service_aliases::{CsHandler, NsHandler},
    user_query::{IsonHandler, UserhostHandler, WhoHandler, WhoisHandler, WhowasHandler},
    user_status::{AwayHandler, SetnameHandler, SilenceHandler},
};
use crate::telemetry::CommandTimer;
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
    /// Handlers valid in any state (QUIT, PING, PONG, NICK, CAP, REGISTER)
    universal_handlers: HashMap<&'static str, Box<dyn DynUniversalHandler>>,
    /// Command usage counters for STATS m
    command_counts: HashMap<&'static str, Arc<AtomicU64>>,
}

impl Registry {
    /// Create a new registry with all handlers registered.
    ///
    /// Handlers are placed into the appropriate phase map based on IRC protocol rules.
    /// `webirc_blocks` is passed from config for WEBIRC authorization.
    pub fn new(webirc_blocks: Vec<crate::config::WebircBlock>) -> Self {
        let mut pre_reg_handlers: HashMap<&'static str, Box<dyn PreRegHandler>> = HashMap::new();
        let mut post_reg_handlers: HashMap<&'static str, Box<dyn PostRegHandler>> = HashMap::new();
        let mut universal_handlers: HashMap<&'static str, Box<dyn DynUniversalHandler>> =
            HashMap::new();

        // ====================================================================
        // Universal handlers (valid in any state)
        // These implement DynUniversalHandler for dual-dispatch capability.
        // ====================================================================
        universal_handlers.insert("QUIT", Box::new(QuitHandler));
        universal_handlers.insert("PING", Box::new(PingHandler));
        universal_handlers.insert("PONG", Box::new(PongHandler));
        universal_handlers.insert("NICK", Box::new(NickHandler));
        universal_handlers.insert("CAP", Box::new(CapHandler));
        universal_handlers.insert("REGISTER", Box::new(RegisterHandler));

        // ====================================================================
        // Pre-registration handlers (valid only before registration completes)
        // These require UnregisteredState and cannot be used after registration.
        // ====================================================================
        pre_reg_handlers.insert("WEBIRC", Box::new(WebircHandler::new(webirc_blocks)));
        pre_reg_handlers.insert("USER", Box::new(UserHandler));
        pre_reg_handlers.insert("PASS", Box::new(PassHandler));
        pre_reg_handlers.insert("AUTHENTICATE", Box::new(AuthenticateHandler));

        // ====================================================================
        // Post-registration handlers (require completed registration)
        // ====================================================================

        // Channel handlers
        post_reg_handlers.insert("JOIN", Box::new(JoinHandler));
        post_reg_handlers.insert("PART", Box::new(PartHandler));
        post_reg_handlers.insert("CYCLE", Box::new(CycleHandler));
        post_reg_handlers.insert("TOPIC", Box::new(TopicHandler));
        post_reg_handlers.insert("NAMES", Box::new(NamesHandler));
        post_reg_handlers.insert("MODE", Box::new(ModeHandler));
        post_reg_handlers.insert("KICK", Box::new(KickHandler));
        post_reg_handlers.insert("LIST", Box::new(ListHandler));
        post_reg_handlers.insert("INVITE", Box::new(InviteHandler));

        // Messaging handlers
        post_reg_handlers.insert("PRIVMSG", Box::new(PrivmsgHandler));
        post_reg_handlers.insert("NOTICE", Box::new(NoticeHandler));
        post_reg_handlers.insert("TAGMSG", Box::new(TagmsgHandler));

        // User query handlers
        post_reg_handlers.insert("WHO", Box::new(WhoHandler));
        post_reg_handlers.insert("WHOIS", Box::new(WhoisHandler));
        post_reg_handlers.insert("WHOWAS", Box::new(WhowasHandler));

        // Server query handlers
        post_reg_handlers.insert("VERSION", Box::new(VersionHandler));
        post_reg_handlers.insert("TIME", Box::new(TimeHandler));
        post_reg_handlers.insert("ADMIN", Box::new(AdminHandler));
        post_reg_handlers.insert("INFO", Box::new(InfoHandler));
        post_reg_handlers.insert("LUSERS", Box::new(LusersHandler));
        post_reg_handlers.insert("STATS", Box::new(StatsHandler));
        post_reg_handlers.insert("MOTD", Box::new(MotdHandler));
        post_reg_handlers.insert("MAP", Box::new(MapHandler));
        post_reg_handlers.insert("RULES", Box::new(RulesHandler));
        post_reg_handlers.insert("USERIP", Box::new(UseripHandler));
        post_reg_handlers.insert("LINKS", Box::new(LinksHandler));
        post_reg_handlers.insert("HELP", Box::new(HelpHandler));
        post_reg_handlers.insert("SUMMON", Box::new(SummonHandler));
        post_reg_handlers.insert("USERS", Box::new(UsersHandler));

        // Service query handlers (RFC 2812 §3.5)
        post_reg_handlers.insert("SERVICE", Box::new(ServiceHandler));
        post_reg_handlers.insert("SERVLIST", Box::new(ServlistHandler));
        post_reg_handlers.insert("SQUERY", Box::new(SqueryHandler));

        // Misc handlers
        post_reg_handlers.insert("AWAY", Box::new(AwayHandler));
        post_reg_handlers.insert("USERHOST", Box::new(UserhostHandler));
        post_reg_handlers.insert("ISON", Box::new(IsonHandler));
        post_reg_handlers.insert("KNOCK", Box::new(KnockHandler));
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
        post_reg_handlers.insert("OPER", Box::new(OperHandler));
        post_reg_handlers.insert("KILL", Box::new(KillHandler));
        post_reg_handlers.insert("WALLOPS", Box::new(WallopsHandler));
        post_reg_handlers.insert("GLOBOPS", Box::new(GlobOpsHandler));
        post_reg_handlers.insert("DIE", Box::new(DieHandler));
        post_reg_handlers.insert("REHASH", Box::new(RehashHandler));
        post_reg_handlers.insert("RESTART", Box::new(RestartHandler));
        post_reg_handlers.insert("CHGHOST", Box::new(ChghostHandler));
        post_reg_handlers.insert("CHGIDENT", Box::new(ChgIdentHandler));
        post_reg_handlers.insert("VHOST", Box::new(VhostHandler));
        post_reg_handlers.insert("TRACE", Box::new(TraceHandler));

        // Ban handlers
        post_reg_handlers.insert("KLINE", Box::new(KlineHandler::kline()));
        post_reg_handlers.insert("DLINE", Box::new(DlineHandler::dline()));
        post_reg_handlers.insert("GLINE", Box::new(GlineHandler::gline()));
        post_reg_handlers.insert("ZLINE", Box::new(ZlineHandler::zline()));
        post_reg_handlers.insert("RLINE", Box::new(RlineHandler::rline()));
        post_reg_handlers.insert("SHUN", Box::new(ShunHandler));
        post_reg_handlers.insert("UNKLINE", Box::new(UnklineHandler::unkline()));
        post_reg_handlers.insert("UNDLINE", Box::new(UndlineHandler::undline()));
        post_reg_handlers.insert("UNGLINE", Box::new(UnglineHandler::ungline()));
        post_reg_handlers.insert("UNZLINE", Box::new(UnzlineHandler::unzline()));
        post_reg_handlers.insert("UNRLINE", Box::new(UnrlineHandler::unrline()));
        post_reg_handlers.insert("UNSHUN", Box::new(UnshunHandler));

        // Admin SA* handlers
        post_reg_handlers.insert("SAJOIN", Box::new(SajoinHandler));
        post_reg_handlers.insert("SAPART", Box::new(SapartHandler));
        post_reg_handlers.insert("SANICK", Box::new(SanickHandler));
        post_reg_handlers.insert("SAMODE", Box::new(SamodeHandler));

        // Initialize command counters for all registered commands
        let mut command_counts = HashMap::new();
        for &cmd in pre_reg_handlers.keys() {
            command_counts.insert(cmd, Arc::new(AtomicU64::new(0)));
        }
        for &cmd in post_reg_handlers.keys() {
            command_counts.insert(cmd, Arc::new(AtomicU64::new(0)));
        }
        for &cmd in universal_handlers.keys() {
            command_counts.insert(cmd, Arc::new(AtomicU64::new(0)));
        }

        Self {
            pre_reg_handlers,
            post_reg_handlers,
            universal_handlers,
            command_counts,
        }
    }

    /// Get command usage statistics for STATS m.
    pub fn get_command_stats(&self) -> Vec<(&'static str, u64)> {
        let mut stats: Vec<_> = self
            .command_counts
            .iter()
            .map(|(cmd, count)| (*cmd, count.load(Ordering::Relaxed)))
            .filter(|(_, count)| *count > 0) // Only include used commands
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
            Err(super::context::HandlerError::UnknownCommand(cmd_name.clone()))
        };

        // Copy values needed for error handling before passing ctx as mutable
        let uid = ctx.uid.to_string();
        let server_name = ctx.matrix.server_info.name.clone();
        let nick = ctx.state.nick.clone();

        // Record errors for metrics
        self.handle_dispatch_result(&uid, &server_name, nick.as_deref(), &cmd_name, result, ctx).await
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
            Err(super::context::HandlerError::UnknownCommand(cmd_name.clone()))
        };

        // Copy values needed for error handling before passing ctx as mutable
        let uid = ctx.uid.to_string();
        let server_name = ctx.matrix.server_info.name.clone();
        let nick = ctx.state.nick.clone();

        // Record errors for metrics
        self.handle_dispatch_result(&uid, &server_name, Some(&nick), &cmd_name, result, ctx).await
    }

    /// Common result handling for both dispatch methods.
    async fn handle_dispatch_result<S>(
        &self,
        uid: &str,
        server_name: &str,
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
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
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
